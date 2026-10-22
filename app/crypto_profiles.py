"""Per-profile key vault for Aurora-owned records.

This module does not encrypt Qt WebEngine/Chromium storage.  Its encrypted SQLite
store is an app-owned building block, not evidence that a whole browser profile is
encrypted.
"""

from __future__ import annotations

import base64
import ctypes
import hmac
import json
import os
import re
import secrets
import shutil
import sqlite3
import uuid
import zipfile
from contextlib import closing
from dataclasses import dataclass
from io import BytesIO
from pathlib import Path
from typing import Any

from argon2.low_level import Type, hash_secret_raw
from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.hkdf import HKDF


PMK_BYTES = 32
SALT_BYTES = 16
NONCE_BYTES = 12
NONCE_PREFIX_BYTES = 8
ARGON_MEMORY_KIB = 64 * 1024
ARGON_ITERATIONS = 3
ARGON_PARALLELISM = 1
ARGON_MAX_MEMORY_KIB = 256 * 1024
ARGON_MAX_ITERATIONS = 8
BACKUP_MAGIC = b"AURORABK1"
BACKUP_AAD = b"Aurora encrypted profile backup v1"
MAX_BACKUP_BYTES = 256 * 1024 * 1024
_DOMAIN_RE = re.compile(r"^[a-z0-9][a-z0-9_.-]{0,63}$")


class ProfileError(Exception):
    """Base error for invalid or unavailable profile operations."""


class ProfileUnlockError(ProfileError):
    """The profile could not be unlocked with the supplied credential."""


class InvalidBackupError(ProfileError):
    """A backup is malformed, corrupted, or has an incorrect password."""


class UnsupportedPlatformError(ProfileError):
    """An operating-system credential feature is unavailable here."""


def _b64(data: bytes) -> str:
    return base64.b64encode(data).decode("ascii")


def _unb64(value: str, expected: int | None = None) -> bytes:
    try:
        decoded = base64.b64decode(value, validate=True)
    except (ValueError, TypeError) as exc:
        raise ProfileError("Invalid profile key metadata") from exc
    if expected is not None and len(decoded) != expected:
        raise ProfileError("Invalid profile key metadata")
    return decoded


def _derive_password_key(password: str, salt: bytes, memory_kib: int = ARGON_MEMORY_KIB,
                         iterations: int = ARGON_ITERATIONS,
                         parallelism: int = ARGON_PARALLELISM) -> bytes:
    if not isinstance(password, str) or not password:
        raise ProfileUnlockError("Password is empty")
    if len(password) < 12:
        raise ProfileUnlockError("Passwords must contain at least 12 characters")
    if (not 32 * 1024 <= memory_kib <= ARGON_MAX_MEMORY_KIB
            or not 2 <= iterations <= ARGON_MAX_ITERATIONS
            or not 1 <= parallelism <= 4):
        raise ProfileError("Unsafe or unsupported password KDF parameters")
    try:
        return hash_secret_raw(
            secret=password.encode("utf-8"), salt=salt, time_cost=iterations,
            memory_cost=memory_kib, parallelism=parallelism, hash_len=32,
            type=Type.ID,
        )
    except (ValueError, MemoryError) as exc:
        raise ProfileError("Password key derivation failed") from exc


def _derive_recovery_key(recovery_key: str, salt: bytes, profile_id: str) -> bytes:
    try:
        raw = bytes.fromhex(recovery_key)
    except (ValueError, TypeError) as exc:
        raise ProfileUnlockError("Invalid recovery key") from exc
    if len(raw) != 32:
        raise ProfileUnlockError("Invalid recovery key")
    return HKDF(
        algorithm=hashes.SHA256(), length=32, salt=salt,
        info=f"Aurora/{profile_id}/recovery-wrap/v1".encode("ascii"),
    ).derive(raw)


def _wrap_aes(key: bytes, plaintext: bytes, aad: bytes) -> dict[str, str]:
    nonce = secrets.token_bytes(NONCE_BYTES)
    return {"nonce": _b64(nonce), "ciphertext": _b64(AESGCM(key).encrypt(nonce, plaintext, aad))}


def _unwrap_aes(key: bytes, envelope: dict[str, str], aad: bytes) -> bytes:
    nonce = _unb64(envelope["nonce"], NONCE_BYTES)
    ciphertext = _unb64(envelope["ciphertext"])
    return AESGCM(key).decrypt(nonce, ciphertext, aad)


def _data_blob(data: bytes):
    class DATA_BLOB(ctypes.Structure):
        _fields_ = [("cbData", ctypes.c_ulong), ("pbData", ctypes.POINTER(ctypes.c_ubyte))]

    backing = (ctypes.c_ubyte * len(data)).from_buffer_copy(data)
    return DATA_BLOB(len(data), ctypes.cast(backing, ctypes.POINTER(ctypes.c_ubyte))), backing, DATA_BLOB


def _dpapi_protect(data: bytes, entropy: bytes) -> bytes:
    if os.name != "nt":
        raise UnsupportedPlatformError("DPAPI is available only on Windows")
    crypt32 = ctypes.WinDLL("crypt32", use_last_error=True)
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    in_blob, in_buffer, DATA_BLOB = _data_blob(data)
    entropy_blob, entropy_buffer, _ = _data_blob(entropy)
    out_blob = DATA_BLOB()
    if not crypt32.CryptProtectData(ctypes.byref(in_blob), "Aurora profile key", ctypes.byref(entropy_blob),
                                    None, None, 0, ctypes.byref(out_blob)):
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        return ctypes.string_at(out_blob.pbData, out_blob.cbData)
    finally:
        kernel32.LocalFree(out_blob.pbData)


def _dpapi_unprotect(data: bytes, entropy: bytes) -> bytes:
    if os.name != "nt":
        raise UnsupportedPlatformError("DPAPI is available only on Windows")
    crypt32 = ctypes.WinDLL("crypt32", use_last_error=True)
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    in_blob, in_buffer, DATA_BLOB = _data_blob(data)
    entropy_blob, entropy_buffer, _ = _data_blob(entropy)
    out_blob = DATA_BLOB()
    if not crypt32.CryptUnprotectData(ctypes.byref(in_blob), None, ctypes.byref(entropy_blob),
                                      None, None, 0, ctypes.byref(out_blob)):
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        return ctypes.string_at(out_blob.pbData, out_blob.cbData)
    finally:
        kernel32.LocalFree(out_blob.pbData)


@dataclass(frozen=True)
class CreatedProfile:
    profile: "ProfileVault"
    recovery_key: str


class ProfileVault:
    """A profile-specific PMK protected by password, recovery key and Windows DPAPI."""

    def __init__(self, profile_dir: Path, profile_id: str, master_key: bytes,
                 display_name: str = "Profile"):
        self.profile_dir = profile_dir
        self.profile_id = profile_id
        self.display_name = display_name
        if len(master_key) != PMK_BYTES:
            raise ProfileError("Invalid profile master key")
        self._master_key: bytearray | None = bytearray(master_key)

    @property
    def is_unlocked(self) -> bool:
        return self._master_key is not None

    @classmethod
    def create(cls, profiles_root: Path | str, password: str,
               display_name: str = "Profile") -> CreatedProfile:
        if not isinstance(password, str) or len(password) < 12:
            raise ProfileUnlockError("Choose a profile password of at least 12 characters")
        display_name = display_name.strip()
        if not display_name or len(display_name) > 64:
            raise ProfileError("Profile name must contain 1–64 characters")
        root = Path(profiles_root)
        root.mkdir(parents=True, exist_ok=True)
        profile_id = uuid.uuid4().hex
        profile_dir = root / profile_id
        profile_dir.mkdir(mode=0o700)
        pmk = secrets.token_bytes(PMK_BYTES)
        recovery = secrets.token_bytes(PMK_BYTES).hex()
        aad = f"Aurora/{profile_id}/pmk/v1".encode("ascii")

        salt = secrets.token_bytes(SALT_BYTES)
        password_key = _derive_password_key(password, salt)
        password_wrap = _wrap_aes(password_key, pmk, aad)
        password_wrap.update({
            "salt": _b64(salt), "kdf": "argon2id",
            "memory_kib": ARGON_MEMORY_KIB, "iterations": ARGON_ITERATIONS,
            "parallelism": ARGON_PARALLELISM,
        })

        recovery_salt = secrets.token_bytes(SALT_BYTES)
        recovery_key = _derive_recovery_key(recovery, recovery_salt, profile_id)
        recovery_wrap = _wrap_aes(recovery_key, pmk, aad)
        recovery_wrap.update({"salt": _b64(recovery_salt), "kdf": "hkdf-sha256"})

        metadata: dict[str, Any] = {
            "version": 1,
            "profile_id": profile_id,
            "display_name": display_name,
            "key_wraps": {"password": password_wrap, "recovery": recovery_wrap},
        }
        if os.name == "nt":
            metadata["key_wraps"]["windows_dpapi"] = _b64(_dpapi_protect(pmk, profile_id.encode("ascii")))
        try:
            cls._write_metadata(profile_dir, metadata)
            (profile_dir / "records.sqlite3").touch(mode=0o600)
        except Exception:
            shutil.rmtree(profile_dir, ignore_errors=True)
            raise
        return CreatedProfile(cls(profile_dir, profile_id, pmk, display_name), recovery)

    @classmethod
    def discover(cls, profiles_root: Path | str) -> list[tuple[str, str, Path]]:
        root = Path(profiles_root)
        if not root.exists():
            return []
        found: list[tuple[str, str, Path]] = []
        for candidate in root.iterdir():
            if not candidate.is_dir():
                continue
            try:
                metadata = cls._read_metadata(candidate)
                name = metadata.get("display_name", "Profile")
                if not isinstance(name, str) or not name or len(name) > 64:
                    name = "Profile"
                found.append((metadata["profile_id"], name, candidate))
            except (OSError, ProfileError):
                continue
        return sorted(found, key=lambda item: item[1].casefold())

    @staticmethod
    def _read_metadata(profile_dir: Path) -> dict[str, Any]:
        try:
            if (profile_dir / "profile.json").stat().st_size > 64 * 1024:
                raise ProfileError("Profile metadata is too large")
            metadata = json.loads((profile_dir / "profile.json").read_text("utf-8"))
            profile_id = uuid.UUID(hex=metadata["profile_id"]).hex
            if profile_id != profile_dir.name or metadata.get("version") != 1:
                raise ProfileError("Profile metadata does not match its directory")
            if not isinstance(metadata.get("key_wraps"), dict):
                raise ProfileError("Profile key protection metadata is missing")
            display_name = metadata.get("display_name", "Profile")
            if not isinstance(display_name, str) or not display_name or len(display_name) > 64:
                raise ProfileError("Invalid profile name metadata")
            return metadata
        except ProfileError:
            raise
        except (OSError, ValueError, KeyError, TypeError, json.JSONDecodeError) as exc:
            raise ProfileError("Invalid profile metadata") from exc

    @staticmethod
    def _write_metadata(profile_dir: Path, metadata: dict[str, Any]) -> None:
        path = profile_dir / "profile.json"
        temp = profile_dir / "profile.json.tmp"
        encoded = json.dumps(metadata, sort_keys=True, separators=(",", ":")).encode("utf-8")
        with temp.open("wb") as stream:
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temp, path)

    @classmethod
    def unlock_password(cls, profile_dir: Path | str, password: str) -> "ProfileVault":
        directory = Path(profile_dir)
        metadata = cls._read_metadata(directory)
        pmk = cls._unlock_with_password_metadata(metadata, password)
        return cls(directory, metadata["profile_id"], pmk, metadata.get("display_name", "Profile"))

    @classmethod
    def _unlock_with_password_metadata(cls, metadata: dict[str, Any], password: str) -> bytes:
        try:
            envelope = metadata["key_wraps"]["password"]
            if envelope.get("kdf") != "argon2id":
                raise ProfileError("Unsupported profile password KDF")
            salt = _unb64(envelope["salt"], SALT_BYTES)
            key = _derive_password_key(password, salt, int(envelope["memory_kib"]),
                                       int(envelope["iterations"]), int(envelope["parallelism"]))
            return _unwrap_aes(key, envelope, f"Aurora/{metadata['profile_id']}/pmk/v1".encode("ascii"))
        except InvalidTag as exc:
            raise ProfileUnlockError("Incorrect profile password") from exc
        except (KeyError, TypeError, ValueError) as exc:
            raise ProfileError("Invalid password key protection metadata") from exc

    @classmethod
    def unlock_recovery(cls, profile_dir: Path | str, recovery_key: str) -> "ProfileVault":
        directory = Path(profile_dir)
        metadata = cls._read_metadata(directory)
        try:
            envelope = metadata["key_wraps"]["recovery"]
            if envelope.get("kdf") != "hkdf-sha256":
                raise ProfileError("Unsupported recovery key derivation")
            salt = _unb64(envelope["salt"], SALT_BYTES)
            key = _derive_recovery_key(recovery_key, salt, metadata["profile_id"])
            pmk = _unwrap_aes(key, envelope, f"Aurora/{metadata['profile_id']}/pmk/v1".encode("ascii"))
            return cls(directory, metadata["profile_id"], pmk, metadata.get("display_name", "Profile"))
        except InvalidTag as exc:
            raise ProfileUnlockError("Incorrect recovery key") from exc
        except (KeyError, TypeError, ValueError) as exc:
            raise ProfileError("Invalid recovery key protection metadata") from exc

    @classmethod
    def unlock_windows(cls, profile_dir: Path | str) -> "ProfileVault":
        if os.name != "nt":
            raise UnsupportedPlatformError("DPAPI unlock is available only on Windows")
        directory = Path(profile_dir)
        metadata = cls._read_metadata(directory)
        try:
            wrapped = _unb64(metadata["key_wraps"]["windows_dpapi"])
            pmk = _dpapi_unprotect(wrapped, metadata["profile_id"].encode("ascii"))
            return cls(directory, metadata["profile_id"], pmk, metadata.get("display_name", "Profile"))
        except (InvalidTag, KeyError, TypeError, ValueError, OSError) as exc:
            raise ProfileUnlockError("Windows could not unlock this profile") from exc

    def lock(self) -> None:
        if self._master_key is not None:
            for index in range(len(self._master_key)):
                self._master_key[index] = 0
            self._master_key = None

    def change_password(self, password: str) -> None:
        self._require_key()
        if not isinstance(password, str) or len(password) < 12:
            raise ProfileUnlockError("Choose a profile password of at least 12 characters")
        metadata = self._read_metadata(self.profile_dir)
        salt = secrets.token_bytes(SALT_BYTES)
        password_key = _derive_password_key(password, salt)
        wrap = _wrap_aes(password_key, self._require_key(),
                         f"Aurora/{self.profile_id}/pmk/v1".encode("ascii"))
        wrap.update({
            "salt": _b64(salt), "kdf": "argon2id",
            "memory_kib": ARGON_MEMORY_KIB, "iterations": ARGON_ITERATIONS,
            "parallelism": ARGON_PARALLELISM,
        })
        metadata["key_wraps"]["password"] = wrap
        self._write_metadata(self.profile_dir, metadata)

    def _require_key(self) -> bytes:
        if self._master_key is None:
            raise ProfileUnlockError("Profile is locked")
        return bytes(self._master_key)

    def _record_key(self, domain: str) -> bytes:
        if not _DOMAIN_RE.fullmatch(domain):
            raise ValueError("Domain must be 1–64 lowercase ASCII letters, digits, '.', '_' or '-'")
        return HKDF(
            algorithm=hashes.SHA256(), length=32, salt=None,
            info=f"Aurora/{self.profile_id}/records/{domain}/v1".encode("ascii"),
        ).derive(self._require_key())

    def opaque_record_id(self, domain: str, identity: str) -> str:
        if not isinstance(identity, str):
            raise TypeError("Record identity must be text")
        if not _DOMAIN_RE.fullmatch(domain):
            raise ValueError("Invalid record domain")
        index_key = HKDF(
            algorithm=hashes.SHA256(), length=32, salt=None,
            info=f"Aurora/{self.profile_id}/record-index/{domain}/v1".encode("ascii"),
        ).derive(self._require_key())
        return hmac.new(index_key, identity.encode("utf-8"), "sha256").hexdigest()

    def _connect_store(self) -> sqlite3.Connection:
        connection = sqlite3.connect(self.profile_dir / "records.sqlite3", timeout=10, isolation_level="IMMEDIATE")
        connection.execute("PRAGMA journal_mode=DELETE")
        connection.execute("PRAGMA synchronous=FULL")
        connection.execute("""CREATE TABLE IF NOT EXISTS store_meta
            (key TEXT PRIMARY KEY, value BLOB NOT NULL)""")
        connection.execute("""CREATE TABLE IF NOT EXISTS encrypted_records
            (domain TEXT NOT NULL, record_id TEXT NOT NULL, nonce BLOB NOT NULL UNIQUE,
             ciphertext BLOB NOT NULL, PRIMARY KEY(domain, record_id))""")
        connection.execute("INSERT OR IGNORE INTO store_meta(key,value) VALUES('nonce_prefix',?)",
                           (secrets.token_bytes(NONCE_PREFIX_BYTES),))
        connection.execute("INSERT OR IGNORE INTO store_meta(key,value) VALUES('next_nonce_counter',?)", (0,))
        return connection

    def put(self, domain: str, record_id: str, plaintext: bytes) -> None:
        self._require_key()
        if not isinstance(plaintext, bytes):
            raise TypeError("Encrypted record values must be bytes")
        if len(record_id.encode("utf-8")) > 1024:
            raise ValueError("Record id is too long")
        key = self._record_key(domain)
        aad = f"Aurora/{self.profile_id}/{domain}/{record_id}/v1".encode("utf-8")
        with closing(self._connect_store()) as db, db:
            prefix = db.execute("SELECT value FROM store_meta WHERE key='nonce_prefix'").fetchone()[0]
            counter = int(db.execute("SELECT value FROM store_meta WHERE key='next_nonce_counter'").fetchone()[0])
            if counter >= 2**32:
                raise ProfileError("Profile encryption nonce space exhausted")
            nonce = prefix + counter.to_bytes(4, "big")
            db.execute("UPDATE store_meta SET value=? WHERE key='next_nonce_counter'", (counter + 1,))
            ciphertext = AESGCM(key).encrypt(nonce, plaintext, aad)
            db.execute("""INSERT INTO encrypted_records(domain,record_id,nonce,ciphertext)
                VALUES(?,?,?,?) ON CONFLICT(domain,record_id) DO UPDATE SET
                nonce=excluded.nonce,ciphertext=excluded.ciphertext""",
                       (domain, record_id, nonce, ciphertext))

    def get(self, domain: str, record_id: str) -> bytes | None:
        self._require_key()
        key = self._record_key(domain)
        with closing(self._connect_store()) as db, db:
            row = db.execute("SELECT nonce,ciphertext FROM encrypted_records WHERE domain=? AND record_id=?",
                             (domain, record_id)).fetchone()
        if row is None:
            return None
        return self._decrypt_record(domain, record_id, row[0], row[1], key)

    def list_records(self, domain: str) -> list[tuple[str, bytes]]:
        self._require_key()
        key = self._record_key(domain)
        with closing(self._connect_store()) as db, db:
            rows = db.execute("SELECT record_id,nonce,ciphertext FROM encrypted_records WHERE domain=? ORDER BY record_id",
                              (domain,)).fetchall()
        return [(record_id, self._decrypt_record(domain, record_id, nonce, ciphertext, key))
                for record_id, nonce, ciphertext in rows]

    def delete_record(self, domain: str, record_id: str) -> None:
        self._require_key()
        self._record_key(domain)
        with closing(self._connect_store()) as db, db:
            db.execute("DELETE FROM encrypted_records WHERE domain=? AND record_id=?", (domain, record_id))

    def _decrypt_record(self, domain: str, record_id: str, nonce: bytes,
                        ciphertext: bytes, key: bytes) -> bytes:
        aad = f"Aurora/{self.profile_id}/{domain}/{record_id}/v1".encode("utf-8")
        try:
            return AESGCM(key).decrypt(nonce, ciphertext, aad)
        except InvalidTag as exc:
            raise ProfileError("Encrypted profile record failed authentication") from exc

    def export_backup(self, destination: Path | str, backup_password: str) -> None:
        self._require_key()
        payload = BytesIO()
        with zipfile.ZipFile(payload, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            archive.writestr("profile.json", (self.profile_dir / "profile.json").read_bytes())
            records = self.profile_dir / "records.sqlite3"
            if records.exists() and records.stat().st_size:
                if records.stat().st_size > MAX_BACKUP_BYTES:
                    raise ProfileError("App data is too large for this alpha backup format")
                archive.writestr("records.sqlite3", records.read_bytes())
        if payload.tell() > MAX_BACKUP_BYTES:
            raise ProfileError("App data is too large for this alpha backup format")
        salt = secrets.token_bytes(SALT_BYTES)
        key = _derive_password_key(backup_password, salt)
        nonce = secrets.token_bytes(NONCE_BYTES)
        ciphertext = AESGCM(key).encrypt(nonce, payload.getvalue(), BACKUP_AAD)
        path = Path(destination)
        path.parent.mkdir(parents=True, exist_ok=True)
        temp = path.with_name(path.name + ".tmp")
        with temp.open("wb") as stream:
            stream.write(BACKUP_MAGIC + salt + nonce + ciphertext)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temp, path)

    @classmethod
    def restore_backup(cls, backup_path: Path | str, profiles_root: Path | str,
                       backup_password: str, profile_password: str) -> "ProfileVault":
        backup_file = Path(backup_path)
        if backup_file.stat().st_size > MAX_BACKUP_BYTES:
            raise InvalidBackupError("Backup exceeds the size limit for this alpha")
        raw = backup_file.read_bytes()
        header_length = len(BACKUP_MAGIC) + SALT_BYTES + NONCE_BYTES
        if len(raw) <= header_length or not raw.startswith(BACKUP_MAGIC):
            raise InvalidBackupError("Unknown or truncated backup format")
        offset = len(BACKUP_MAGIC)
        salt = raw[offset:offset + SALT_BYTES]
        nonce = raw[offset + SALT_BYTES:header_length]
        try:
            key = _derive_password_key(backup_password, salt)
            plaintext = AESGCM(key).decrypt(nonce, raw[header_length:], BACKUP_AAD)
        except (InvalidTag, ProfileUnlockError, ProfileError) as exc:
            raise InvalidBackupError("Incorrect backup password or damaged backup") from exc

        root = Path(profiles_root)
        root.mkdir(parents=True, exist_ok=True)
        staging: Path | None = None
        try:
            with zipfile.ZipFile(BytesIO(plaintext), "r") as archive:
                archive_names = archive.namelist()
                names = set(archive_names)
                if len(archive_names) != len(names):
                    raise InvalidBackupError("Backup contains duplicate file entries")
                if "profile.json" not in names or names - {"profile.json", "records.sqlite3"}:
                    raise InvalidBackupError("Backup contains unexpected files")
                files: dict[str, bytes] = {}
                expanded_size = 0
                for name in archive.namelist():
                    if name.startswith("/") or ".." in Path(name).parts:
                        raise InvalidBackupError("Backup contains an unsafe path")
                    info = archive.getinfo(name)
                    expanded_size += info.file_size
                    if expanded_size > MAX_BACKUP_BYTES:
                        raise InvalidBackupError("Backup member exceeds the size limit")
                    files[name] = archive.read(name)
            try:
                metadata = json.loads(files["profile.json"].decode("utf-8"))
                profile_id = uuid.UUID(hex=metadata["profile_id"]).hex
            except (ValueError, KeyError, TypeError, json.JSONDecodeError) as exc:
                raise InvalidBackupError("Backup profile metadata is invalid") from exc
            if (metadata.get("version") != 1 or not isinstance(metadata.get("key_wraps"), dict)
                    or not isinstance(metadata.get("display_name", "Profile"), str)
                    or not metadata.get("display_name", "Profile")
                    or len(metadata.get("display_name", "Profile")) > 64):
                raise InvalidBackupError("Backup profile metadata is invalid")
            destination = root / profile_id
            if destination.exists():
                raise ProfileError("A profile with this id already exists")
            staging = root / f"{profile_id}.restore-{secrets.token_hex(6)}"
            staging.mkdir(mode=0o700)
            for name, data in files.items():
                (staging / name).write_bytes(data)
            if not (staging / "records.sqlite3").exists():
                sqlite3.connect(staging / "records.sqlite3").close()
            else:
                # Start a restored branch with a fresh nonce prefix so future
                # writes cannot reuse nonces from the source profile's timeline.
                with closing(sqlite3.connect(staging / "records.sqlite3")) as db, db:
                    integrity = db.execute("PRAGMA quick_check").fetchone()
                    if not integrity or integrity[0] != "ok":
                        raise InvalidBackupError("Profile database integrity check failed")
                    table = db.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name='store_meta'").fetchone()
                    if not table:
                        raise InvalidBackupError("Encrypted profile store metadata is missing")
                    prefix = db.execute("SELECT value FROM store_meta WHERE key='nonce_prefix'").fetchone()
                    counter = db.execute("SELECT value FROM store_meta WHERE key='next_nonce_counter'").fetchone()
                    if not prefix or len(prefix[0]) != NONCE_PREFIX_BYTES or not counter:
                        raise InvalidBackupError("Encrypted profile nonce state is invalid")
                    next_counter = int(counter[0])
                    if not 0 <= next_counter < 2**32:
                        raise InvalidBackupError("Encrypted profile nonce state is invalid")
                    db.execute("UPDATE store_meta SET value=? WHERE key='nonce_prefix'",
                               (secrets.token_bytes(NONCE_PREFIX_BYTES),))
                    db.execute("UPDATE store_meta SET value=0 WHERE key='next_nonce_counter'")
            # Credential verification is done before the imported files become visible.
            verified_key = cls._unlock_with_password_metadata(metadata, profile_password)
            restored = cls(staging, profile_id, verified_key, metadata.get("display_name", "Profile"))
            if os.name == "nt":
                metadata["key_wraps"]["windows_dpapi"] = _b64(
                    _dpapi_protect(verified_key, metadata["profile_id"].encode("ascii")))
                cls._write_metadata(staging, metadata)
            os.replace(staging, destination)
            restored.profile_dir = destination
            staging = None
            return restored
        except (zipfile.BadZipFile, OSError, KeyError, ValueError, sqlite3.Error) as exc:
            raise InvalidBackupError("Malformed encrypted profile backup") from exc
        finally:
            if staging is not None and staging.exists():
                shutil.rmtree(staging, ignore_errors=True)

    def delete_profile(self) -> None:
        self.lock()
        self.delete_profile_directory(self.profile_dir)

    @classmethod
    def delete_profile_directory(cls, profile_dir: Path | str) -> None:
        directory = Path(profile_dir)
        cls._read_metadata(directory)
        shutil.rmtree(directory)

