# Profile crypto milestone

## What is implemented

`app/crypto_profiles.py` is a Windows-first key-vault primitive for Aurora-owned application records. It generates a fresh 256-bit PMK per vault, never stores that PMK unwrapped, wraps it independently with an Argon2id password and a random 256-bit recovery key, and adds a Windows user-scoped DPAPI wrapper when running on Windows. Password and recovery wrappers use AES-256-GCM with profile-specific associated data. Each data category receives a separate HKDF-SHA-256 derived key.

The encrypted SQLite record store encrypts values with AES-256-GCM. A per-store random 64-bit prefix and an atomically advanced 32-bit counter form each 96-bit nonce; SQLite serializes writers and rejects duplicate nonce values. A restored backup starts a new nonce-prefix branch before new writes. Record category and record ID remain visible in SQLite as metadata; record values are authenticated ciphertext.

Encrypted exports use a separate backup password, Argon2id and AES-256-GCM around the archive. Restore checks authentication, archive members, profile metadata, SQLite integrity and the profile password before making the profile directory visible. Windows restore refreshes the DPAPI wrapper for the current user.

The vault API's `lock()` zeroes the module's mutable PMK buffer and blocks later API access through that vault object. Python, the crypto library, the OS, and other copies may retain key material in memory; this is not secure erasure.

## What is not implemented

- The browser window does not create or unlock these vaults yet.
- The current Qt WebEngine profile and `aurora.sqlite3` bookmarks database are still stored as ordinary Chromium/SQLite files. Cookies, cache, history, IndexedDB, service workers and other Chromium data are not encrypted by this vault.
- Profile names and a profile manager UI are not implemented.
- macOS Keychain and Linux Secret Service integration are not implemented. On those platforms password and recovery wrappers can be used, but automatic OS credential unlock is unavailable.
- No biometric flow, password change, secure erasure, background-profile process stop, or cross-device sync is implemented.

Therefore this milestone is **not** complete per the brief's whole-profile encryption acceptance criterion, and Aurora profiles must not be described as fully encrypted. See [threat-model.md](threat-model.md).

## Run the crypto checks

```powershell
python -m unittest discover -s tests -v
```

