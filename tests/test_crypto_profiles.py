import json
import sqlite3
import tempfile
import unittest
from contextlib import closing
from pathlib import Path

from app.crypto_profiles import (
    InvalidBackupError,
    ProfileError,
    ProfileUnlockError,
    ProfileVault,
)


PASSWORD_A = "profile password alpha"
PASSWORD_B = "profile password bravo"
BACKUP_PASSWORD = "archive password charlie"


class ProfileVaultTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name) / "profiles"
        created = ProfileVault.create(self.root, PASSWORD_A)
        self.profile = created.profile
        self.recovery_key = created.recovery_key

    def tearDown(self):
        self.profile.lock()
        self.temp.cleanup()

    def test_profiles_get_unique_random_master_keys(self):
        other = ProfileVault.create(self.root, PASSWORD_B).profile
        self.assertNotEqual(self.profile._require_key(), other._require_key())
        other.lock()

    def test_discovery_exposes_profile_names_without_unlocking_keys(self):
        ProfileVault.create(self.root, PASSWORD_B, "Work").profile.lock()
        names = [name for _profile_id, name, _path in ProfileVault.discover(self.root)]
        self.assertIn("Work", names)

    def test_opaque_record_ids_hide_plaintext_identity(self):
        url = "https://example.invalid/private/path"
        identifier = self.profile.opaque_record_id("bookmarks", url)
        self.profile.put("bookmarks", identifier, url.encode())
        raw_db = (self.profile.profile_dir / "records.sqlite3").read_bytes()
        self.assertNotIn(url.encode(), raw_db)
        self.assertEqual(self.profile.get("bookmarks", identifier), url.encode())

    def test_master_key_is_not_written_unwrapped_to_profile_files(self):
        master_key = self.profile._require_key()
        for path in self.profile.profile_dir.iterdir():
            if path.is_file():
                content = path.read_bytes()
                self.assertNotIn(master_key, content)
                self.assertNotIn(PASSWORD_A.encode(), content)

    def test_wrong_password_cannot_unlock(self):
        self.profile.lock()
        with self.assertRaises(ProfileUnlockError):
            ProfileVault.unlock_password(self.profile.profile_dir, "incorrect password")

    def test_recovery_key_unlocks_the_same_profile(self):
        key_before = self.profile._require_key()
        self.profile.lock()
        recovered = ProfileVault.unlock_recovery(self.profile.profile_dir, self.recovery_key)
        self.assertEqual(key_before, recovered._require_key())
        recovered.lock()

    def test_password_change_keeps_profile_key_and_recovery_wrapper(self):
        key = self.profile._require_key()
        self.profile.change_password("profile password delta")
        self.profile.lock()
        with self.assertRaises(ProfileUnlockError):
            ProfileVault.unlock_password(self.profile.profile_dir, PASSWORD_A)
        changed = ProfileVault.unlock_password(self.profile.profile_dir, "profile password delta")
        self.assertEqual(key, changed._require_key())
        changed.lock()
        recovered = ProfileVault.unlock_recovery(self.profile.profile_dir, self.recovery_key)
        self.assertEqual(key, recovered._require_key())
        recovered.lock()

    def test_other_profile_cannot_decrypt_ciphertext(self):
        other = ProfileVault.create(self.root, PASSWORD_B).profile
        self.profile.put("settings", "start-url", b"https://example.invalid/")
        with closing(sqlite3.connect(self.profile.profile_dir / "records.sqlite3")) as source, source:
            nonce, ciphertext = source.execute(
                "SELECT nonce,ciphertext FROM encrypted_records WHERE domain='settings' AND record_id='start-url'"
            ).fetchone()
        other.put("settings", "placeholder", b"initialized")
        with closing(sqlite3.connect(other.profile_dir / "records.sqlite3")) as target, target:
            target.execute("INSERT OR REPLACE INTO encrypted_records(domain,record_id,nonce,ciphertext) VALUES(?,?,?,?)",
                           ("settings", "start-url", nonce, ciphertext))
        with self.assertRaises(ProfileError):
            other.get("settings", "start-url")
        other.lock()

    def test_ciphertext_tampering_is_rejected(self):
        self.profile.put("bookmarks", "one", b"secret bookmark")
        with closing(sqlite3.connect(self.profile.profile_dir / "records.sqlite3")) as db, db:
            nonce, ciphertext = db.execute(
                "SELECT nonce,ciphertext FROM encrypted_records WHERE domain='bookmarks' AND record_id='one'"
            ).fetchone()
            corrupted = bytes([ciphertext[0] ^ 1]) + ciphertext[1:]
            db.execute("UPDATE encrypted_records SET ciphertext=? WHERE nonce=?", (corrupted, nonce))
        with self.assertRaises(ProfileError):
            self.profile.get("bookmarks", "one")

    def test_lock_revokes_store_access(self):
        self.profile.put("settings", "theme", b"dark")
        self.profile.lock()
        self.assertFalse(self.profile.is_unlocked)
        with self.assertRaises(ProfileUnlockError):
            self.profile.get("settings", "theme")

    def test_aes_gcm_nonces_remain_unique_after_reopen(self):
        for number in range(3):
            self.profile.put("settings", f"key-{number}", str(number).encode())
        self.profile.lock()
        self.profile = ProfileVault.unlock_password(self.profile.profile_dir, PASSWORD_A)
        self.profile.put("settings", "key-3", b"3")
        with closing(sqlite3.connect(self.profile.profile_dir / "records.sqlite3")) as db, db:
            nonces = [row[0] for row in db.execute("SELECT nonce FROM encrypted_records")]
        self.assertEqual(len(nonces), len(set(nonces)))

    def test_profile_deletion_removes_vault_files(self):
        self.profile.put("settings", "theme", b"dark")
        profile_dir = self.profile.profile_dir
        self.profile.delete_profile()
        self.assertFalse(profile_dir.exists())
        self.assertFalse(self.profile.is_unlocked)

    def test_backup_is_encrypted_and_restores_records(self):
        marker = b"private data absent from backup plaintext"
        self.profile.put("notes", "note-1", marker)
        backup = Path(self.temp.name) / "aurora.backup"
        self.profile.export_backup(backup, BACKUP_PASSWORD)
        self.assertNotIn(marker, backup.read_bytes())
        with closing(sqlite3.connect(self.profile.profile_dir / "records.sqlite3")) as db, db:
            old_nonces = {row[0] for row in db.execute("SELECT nonce FROM encrypted_records")}
        restored_root = Path(self.temp.name) / "restored"
        restored = ProfileVault.restore_backup(backup, restored_root, BACKUP_PASSWORD, PASSWORD_A)
        self.assertEqual(marker, restored.get("notes", "note-1"))
        with closing(sqlite3.connect(restored.profile_dir / "records.sqlite3")) as db, db:
            prior_nonces = {row[0] for row in db.execute("SELECT nonce FROM encrypted_records")}
        restored.put("notes", "after-restore", b"new branch")
        with closing(sqlite3.connect(restored.profile_dir / "records.sqlite3")) as db, db:
            restored_nonces = [row[0] for row in db.execute("SELECT nonce FROM encrypted_records")]
            new_nonce = db.execute(
                "SELECT nonce FROM encrypted_records WHERE domain='notes' AND record_id='after-restore'"
            ).fetchone()[0]
        self.assertEqual(len(restored_nonces), len(set(restored_nonces)))
        self.assertEqual(old_nonces, prior_nonces)
        self.assertNotIn(new_nonce, old_nonces)
        restored.lock()

    def test_tampered_backup_is_rejected(self):
        backup = Path(self.temp.name) / "aurora.backup"
        self.profile.export_backup(backup, BACKUP_PASSWORD)
        damaged = bytearray(backup.read_bytes())
        damaged[-1] ^= 1
        backup.write_bytes(damaged)
        with self.assertRaises(InvalidBackupError):
            ProfileVault.restore_backup(backup, Path(self.temp.name) / "restored", BACKUP_PASSWORD, PASSWORD_A)

    def test_unsafe_argon2_parameters_are_rejected_before_derivation(self):
        metadata_path = self.profile.profile_dir / "profile.json"
        metadata = json.loads(metadata_path.read_text("utf-8"))
        metadata["key_wraps"]["password"]["memory_kib"] = 2**31
        metadata_path.write_text(json.dumps(metadata), "utf-8")
        with self.assertRaises(ProfileError):
            ProfileVault.unlock_password(self.profile.profile_dir, PASSWORD_A)

    def test_windows_dpapi_unlock_when_available(self):
        import os
        if os.name != "nt":
            self.skipTest("DPAPI is Windows-only")
        self.profile.lock()
        by_os = ProfileVault.unlock_windows(self.profile.profile_dir)
        by_password = ProfileVault.unlock_password(self.profile.profile_dir, PASSWORD_A)
        self.assertEqual(by_os._require_key(), by_password._require_key())
        by_os.lock()
        by_password.lock()


if __name__ == "__main__":
    unittest.main()
