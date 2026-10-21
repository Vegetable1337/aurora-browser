# Profile crypto milestone

## What is implemented

`app/crypto_profiles.py` is a Windows-first key-vault primitive for Aurora-owned application records. It generates a fresh 256-bit PMK per vault, never stores that PMK unwrapped, wraps it independently with an Argon2id password and a random 256-bit recovery key, and adds a Windows user-scoped DPAPI wrapper when running on Windows. Password and recovery wrappers use AES-256-GCM with profile-specific associated data. Each data category receives a separate HKDF-SHA-256 derived key.

The encrypted SQLite record store encrypts values with AES-256-GCM. A per-store random 64-bit prefix and an atomically advanced 32-bit counter form each 96-bit nonce; SQLite serializes writers and rejects duplicate nonce values. A restored backup starts a new nonce-prefix branch before new writes. Record category and record ID remain visible in SQLite as metadata; record values are authenticated ciphertext.

Encrypted exports use a separate backup password, Argon2id and AES-256-GCM around the archive. Restore checks authentication, archive members, profile metadata, SQLite integrity and the profile password before making the profile directory visible. Windows restore refreshes the DPAPI wrapper for the current user.

The browser now has a Profiles menu to create, unlock, recover, switch, lock, change the password for, and delete password-protected profiles. Each protected profile gets its own Qt WebEngine profile directory. Protected-profile bookmarks use the encrypted record store; URL-derived record IDs use a profile-keyed HMAC so bookmark URLs are not exposed by SQLite row IDs. Profile display names remain plaintext in `profile.json` so Aurora can show them while locked.

The vault API's `lock()` zeroes the module's mutable PMK buffer and blocks later API access through that vault object. When switching or locking, Aurora stops and destroys its tabs before disposing the Qt profile object and locking the vault. Python, the crypto library, Chromium, the OS, and other copies may retain key material in memory; this is not secure erasure.

## What is not implemented

- The `Default` profile and its bookmarks remain unprotected. Existing default-profile bookmarks are not migrated automatically.
- Password-protected profiles isolate Chromium cookies, cache, history, local storage, and related engine files in separate directories, but those files remain plaintext at rest. This includes saved website credentials and permissions managed by Chromium.
- Only bookmarks in a password-protected profile are encrypted through the vault. Profile display names, record categories, and random-looking HMAC identifiers remain visible as metadata.
- A profile lock closes its tabs and revokes this application's vault API access. It does not encrypt Chromium files on disk, guarantee renderer-process termination, or erase plaintext from RAM, swap, or system caches.
- Export/import is available in the vault API only; the browser UI does not yet expose backup or restore.
- macOS Keychain and Linux Secret Service integration are not implemented. On those platforms password and recovery wrappers can be used, but automatic OS credential unlock is unavailable.
- No biometric flow, password change, secure erasure, background-profile process stop, or cross-device sync is implemented.

Therefore this milestone is **not** complete per the brief's whole-profile encryption acceptance criterion, and Aurora profiles must not be described as fully encrypted. See [threat-model.md](threat-model.md).

## Run the crypto checks

```powershell
python -m unittest discover -s tests -v
```

