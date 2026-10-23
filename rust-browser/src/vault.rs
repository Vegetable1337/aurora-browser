//! Encrypted storage for Aurora-owned profile data.
//!
//! This does not encrypt Chromium/WebView2 profile files.

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::{RngCore, rngs::OsRng};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

const KEY_BYTES: usize = 32;
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 12;
const NONCE_PREFIX_BYTES: usize = 8;
const KDF_MEMORY_KIB: u32 = 64 * 1024;
const KDF_ITERATIONS: u32 = 3;
const BACKUP_MAGIC: &[u8] = b"AURORABK1";
const BACKUP_AAD: &[u8] = b"Aurora encrypted profile backup v1";
const MAX_BACKUP_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("Профиль заблокирован или пароль неверный")]
    Unlock,
    #[error("Повреждены данные защищённого профиля")]
    InvalidData,
    #[error("Ошибка хранилища профиля: {0}")]
    Io(#[from] std::io::Error),
    #[error("Ошибка базы данных профиля: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Ошибка шифрования профиля")]
    Crypto,
    #[error("Неверный пароль, формат или содержимое резервной копии")]
    InvalidBackup,
}

#[derive(Clone, Serialize, Deserialize)]
struct Envelope {
    nonce: String,
    ciphertext: String,
    #[serde(default)]
    salt: String,
    #[serde(default)]
    kdf: String,
    #[serde(default)]
    memory_kib: u32,
    #[serde(default)]
    iterations: u32,
    #[serde(default)]
    parallelism: u32,
}

#[derive(Serialize, Deserialize)]
struct KeyWraps {
    password: Envelope,
    recovery: Envelope,
    #[serde(skip_serializing_if = "Option::is_none")]
    windows_dpapi: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Metadata {
    version: u32,
    profile_id: String,
    display_name: String,
    key_wraps: KeyWraps,
}

pub struct CreatedVault {
    pub vault: ProfileVault,
    pub recovery_key: String,
}

pub struct ProfileVault {
    directory: PathBuf,
    profile_id: String,
    display_name: String,
    master_key: Option<[u8; KEY_BYTES]>,
}

impl ProfileVault {
    pub fn create(root: &Path, name: &str, password: &str) -> Result<CreatedVault, VaultError> {
        if password.chars().count() < 12 || name.trim().is_empty() || name.chars().count() > 64 {
            return Err(VaultError::InvalidData);
        }
        fs::create_dir_all(root)?;
        let profile_id = uuid::Uuid::new_v4().simple().to_string();
        let directory = root.join(&profile_id);
        fs::create_dir(&directory)?;
        let mut master_key = [0u8; KEY_BYTES];
        let mut recovery = [0u8; KEY_BYTES];
        OsRng.fill_bytes(&mut master_key);
        OsRng.fill_bytes(&mut recovery);
        let aad = format!("Aurora/{profile_id}/pmk/v1");
        let password_wrap = wrap_password(password, &master_key, aad.as_bytes())?;
        let recovery_wrap = wrap_recovery(&profile_id, &recovery, &master_key, aad.as_bytes())?;
        #[cfg(target_os = "windows")]
        let dpapi_wrap = Some(STANDARD.encode(dpapi_protect(&master_key, profile_id.as_bytes())?));
        #[cfg(not(target_os = "windows"))]
        let dpapi_wrap = None;
        let metadata = Metadata {
            version: 1,
            profile_id: profile_id.clone(),
            display_name: name.trim().to_owned(),
            key_wraps: KeyWraps {
                password: password_wrap,
                recovery: recovery_wrap,
                windows_dpapi: dpapi_wrap,
            },
        };
        if let Err(error) = write_metadata(&directory, &metadata).and_then(|_| {
            let connection = Connection::open(directory.join("records.sqlite3"))?;
            init_store(&connection)?;
            Ok(())
        }) {
            let _ = fs::remove_dir_all(&directory);
            master_key.zeroize();
            recovery.zeroize();
            return Err(error);
        }
        let recovery_key = hex::encode(recovery);
        recovery.zeroize();
        Ok(CreatedVault {
            vault: Self {
                directory,
                profile_id,
                display_name: name.trim().to_owned(),
                master_key: Some(master_key),
            },
            recovery_key,
        })
    }

    pub fn discover(root: &Path) -> Vec<(String, String, PathBuf)> {
        let Ok(entries) = fs::read_dir(root) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for entry in entries.flatten() {
            let directory = entry.path();
            if !directory.is_dir() {
                continue;
            }
            if let Ok(metadata) = read_metadata(&directory) {
                found.push((metadata.profile_id, metadata.display_name, directory));
            }
        }
        found.sort_by_key(|left| left.1.to_lowercase());
        found
    }

    pub fn unlock_password(directory: &Path, password: &str) -> Result<Self, VaultError> {
        let metadata = read_metadata(directory)?;
        let aad = format!("Aurora/{}/pmk/v1", metadata.profile_id);
        let key = unwrap_password(password, &metadata.key_wraps.password, aad.as_bytes())
            .map_err(|_| VaultError::Unlock)?;
        Ok(Self {
            directory: directory.to_owned(),
            profile_id: metadata.profile_id,
            display_name: metadata.display_name,
            master_key: Some(key),
        })
    }

    pub fn unlock_recovery(directory: &Path, recovery_key: &str) -> Result<Self, VaultError> {
        let metadata = read_metadata(directory)?;
        let aad = format!("Aurora/{}/pmk/v1", metadata.profile_id);
        let key = unwrap_recovery(
            &metadata.profile_id,
            recovery_key,
            &metadata.key_wraps.recovery,
            aad.as_bytes(),
        )
        .map_err(|_| VaultError::Unlock)?;
        Ok(Self {
            directory: directory.to_owned(),
            profile_id: metadata.profile_id,
            display_name: metadata.display_name,
            master_key: Some(key),
        })
    }

    #[cfg(target_os = "windows")]
    pub fn unlock_windows(directory: &Path) -> Result<Self, VaultError> {
        let metadata = read_metadata(directory)?;
        let wrapped = STANDARD
            .decode(metadata.key_wraps.windows_dpapi.ok_or(VaultError::Unlock)?)
            .map_err(|_| VaultError::InvalidData)?;
        let key = dpapi_unprotect(&wrapped, metadata.profile_id.as_bytes())?
            .try_into()
            .map_err(|_| VaultError::InvalidData)?;
        Ok(Self {
            directory: directory.to_owned(),
            profile_id: metadata.profile_id,
            display_name: metadata.display_name,
            master_key: Some(key),
        })
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    pub fn delete_locked(directory: &Path) -> Result<(), VaultError> {
        let _ = read_metadata(directory)?;
        fs::remove_dir_all(directory)?;
        Ok(())
    }

    pub fn export_backup(
        &self,
        destination: &Path,
        backup_password: &str,
    ) -> Result<(), VaultError> {
        self.key()?;
        let profile_json = fs::read(self.directory.join("profile.json"))?;
        let records_path = self.directory.join("records.sqlite3");
        let records = if records_path.exists() {
            fs::read(&records_path)?
        } else {
            Vec::new()
        };
        if profile_json.len() as u64 + records.len() as u64 > MAX_BACKUP_BYTES {
            return Err(VaultError::InvalidBackup);
        }
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        archive
            .start_file("profile.json", options)
            .map_err(|_| VaultError::InvalidBackup)?;
        archive.write_all(&profile_json)?;
        if !records.is_empty() {
            archive
                .start_file("records.sqlite3", options)
                .map_err(|_| VaultError::InvalidBackup)?;
            archive.write_all(&records)?;
        }
        let payload = archive
            .finish()
            .map_err(|_| VaultError::InvalidBackup)?
            .into_inner();
        if payload.len() as u64 > MAX_BACKUP_BYTES {
            return Err(VaultError::InvalidBackup);
        }
        let mut salt = [0u8; SALT_BYTES];
        OsRng.fill_bytes(&mut salt);
        let mut key = Zeroizing::new(password_key(backup_password, &salt)?);
        let (nonce, encrypted) = seal(&key, &payload, BACKUP_AAD)?;
        key.zeroize();
        let nonce = STANDARD.decode(nonce).map_err(|_| VaultError::Crypto)?;
        let encrypted = STANDARD.decode(encrypted).map_err(|_| VaultError::Crypto)?;
        let mut output =
            Vec::with_capacity(BACKUP_MAGIC.len() + salt.len() + nonce.len() + encrypted.len());
        output.extend_from_slice(BACKUP_MAGIC);
        output.extend_from_slice(&salt);
        output.extend_from_slice(&nonce);
        output.extend_from_slice(&encrypted);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut temp_name = destination.as_os_str().to_owned();
        temp_name.push(".tmp");
        let temporary = PathBuf::from(temp_name);
        fs::write(&temporary, output)?;
        fs::rename(temporary, destination)?;
        Ok(())
    }

    pub fn restore_backup(
        backup_path: &Path,
        root: &Path,
        backup_password: &str,
        profile_password: &str,
    ) -> Result<Self, VaultError> {
        let raw = fs::read(backup_path)?;
        let header_len = BACKUP_MAGIC.len() + SALT_BYTES + NONCE_BYTES;
        if raw.len() as u64 > MAX_BACKUP_BYTES
            || raw.len() <= header_len
            || !raw.starts_with(BACKUP_MAGIC)
        {
            return Err(VaultError::InvalidBackup);
        }
        let salt_at = BACKUP_MAGIC.len();
        let nonce_at = salt_at + SALT_BYTES;
        let mut backup_key = password_key(backup_password, &raw[salt_at..nonce_at])
            .map_err(|_| VaultError::InvalidBackup)?;
        let backup_nonce: [u8; NONCE_BYTES] = raw[nonce_at..header_len]
            .try_into()
            .map_err(|_| VaultError::InvalidBackup)?;
        let payload = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&backup_key))
            .decrypt(
                Nonce::from_slice(&backup_nonce),
                aes_gcm::aead::Payload {
                    msg: &raw[header_len..],
                    aad: BACKUP_AAD,
                },
            )
            .map_err(|_| VaultError::InvalidBackup);
        backup_key.zeroize();
        let payload = payload?;
        if payload.len() as u64 > MAX_BACKUP_BYTES {
            return Err(VaultError::InvalidBackup);
        }
        let mut zip =
            zip::ZipArchive::new(Cursor::new(payload)).map_err(|_| VaultError::InvalidBackup)?;
        let mut files = std::collections::HashMap::new();
        let mut expanded = 0u64;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(|_| VaultError::InvalidBackup)?;
            let name = entry
                .name()
                .map_err(|_| VaultError::InvalidBackup)?
                .into_owned();
            if !matches!(name.as_str(), "profile.json" | "records.sqlite3")
                || files.contains_key(&name)
            {
                return Err(VaultError::InvalidBackup);
            }
            expanded = expanded
                .checked_add(entry.size())
                .ok_or(VaultError::InvalidBackup)?;
            if expanded > MAX_BACKUP_BYTES {
                return Err(VaultError::InvalidBackup);
            }
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut bytes)?;
            files.insert(name, bytes);
        }
        let profile_json = files
            .remove("profile.json")
            .ok_or(VaultError::InvalidBackup)?;
        let mut metadata: Metadata =
            serde_json::from_slice(&profile_json).map_err(|_| VaultError::InvalidBackup)?;
        if metadata.version != 1
            || uuid::Uuid::parse_str(&metadata.profile_id).is_err()
            || metadata.display_name.is_empty()
            || metadata.display_name.chars().count() > 64
        {
            return Err(VaultError::InvalidBackup);
        }
        let profile_id = uuid::Uuid::parse_str(&metadata.profile_id)
            .map_err(|_| VaultError::InvalidBackup)?
            .simple()
            .to_string();
        metadata.profile_id = profile_id.clone();
        let aad = format!("Aurora/{profile_id}/pmk/v1");
        let master_key = unwrap_password(
            profile_password,
            &metadata.key_wraps.password,
            aad.as_bytes(),
        )
        .map_err(|_| VaultError::InvalidBackup)?;
        fs::create_dir_all(root)?;
        let destination = root.join(&profile_id);
        if destination.exists() {
            return Err(VaultError::InvalidData);
        }
        let staging = root.join(format!(
            "{profile_id}.restore-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&staging)?;
        let result = (|| -> Result<(), VaultError> {
            fs::write(staging.join("profile.json"), &profile_json)?;
            let db_path = staging.join("records.sqlite3");
            let has_records_file = if let Some(records) = files.remove("records.sqlite3") {
                fs::write(&db_path, records)?;
                true
            } else {
                false
            };
            let connection = Connection::open(&db_path)?;
            if !has_records_file {
                init_store(&connection)?;
            }
            let integrity: String =
                connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
            if integrity != "ok" {
                return Err(VaultError::InvalidBackup);
            }
            let has_store: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='store_meta')",
                [],
                |row| row.get(0),
            )?;
            if !has_store {
                return Err(VaultError::InvalidBackup);
            }
            let (old_prefix, counter): (Vec<u8>, i64) = connection.query_row(
                "SELECT value,(SELECT value FROM store_meta WHERE key='next_nonce_counter') FROM store_meta WHERE key='nonce_prefix'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if old_prefix.len() != NONCE_PREFIX_BYTES || !(0..(1i64 << 32)).contains(&counter) {
                return Err(VaultError::InvalidBackup);
            }
            let mut prefix = [0u8; NONCE_PREFIX_BYTES];
            OsRng.fill_bytes(&mut prefix);
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "UPDATE store_meta SET value=? WHERE key='nonce_prefix'",
                [prefix.as_slice()],
            )?;
            transaction.execute(
                "UPDATE store_meta SET value=0 WHERE key='next_nonce_counter'",
                [],
            )?;
            transaction.commit()?;
            #[cfg(target_os = "windows")]
            {
                metadata.key_wraps.windows_dpapi =
                    Some(STANDARD.encode(dpapi_protect(&master_key, profile_id.as_bytes())?));
                write_metadata(&staging, &metadata)?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result?;
        fs::rename(&staging, &destination)?;
        Ok(Self {
            directory: destination,
            profile_id,
            display_name: metadata.display_name,
            master_key: Some(master_key),
        })
    }

    pub fn change_password(&mut self, password: &str) -> Result<(), VaultError> {
        if password.chars().count() < 12 {
            return Err(VaultError::InvalidData);
        }
        let master_key = self.key()?;
        let mut metadata = read_metadata(&self.directory)?;
        let aad = format!("Aurora/{}/pmk/v1", self.profile_id);
        metadata.key_wraps.password = wrap_password(password, master_key, aad.as_bytes())?;
        write_metadata(&self.directory, &metadata)
    }

    pub fn lock(&mut self) {
        if let Some(key) = self.master_key.as_mut() {
            key.zeroize();
        }
        self.master_key = None;
    }

    pub fn opaque_record_id(&self, domain: &str, identity: &str) -> Result<String, VaultError> {
        validate_domain(domain)?;
        let info = format!("Aurora/{}/record-index/{domain}/v1", self.profile_id);
        let key = derive_hkdf(self.key()?.as_slice(), &[], info.as_bytes())?;
        let mut mac =
            <Hmac<Sha256> as Mac>::new_from_slice(&key).map_err(|_| VaultError::Crypto)?;
        mac.update(identity.as_bytes());
        Ok(hex::encode(mac.finalize().into_bytes()))
    }

    pub fn put(&self, domain: &str, record_id: &str, plaintext: &[u8]) -> Result<(), VaultError> {
        validate_domain(domain)?;
        if record_id.len() > 1024 {
            return Err(VaultError::InvalidData);
        }
        let key = Zeroizing::new(self.record_key(domain)?);
        let aad = format!("Aurora/{}/{domain}/{record_id}/v1", self.profile_id);
        let mut connection = self.connect_store()?;
        let transaction = connection.transaction()?;
        let (prefix, counter): (Vec<u8>, i64) = transaction.query_row(
            "SELECT value,(SELECT value FROM store_meta WHERE key='next_nonce_counter') FROM store_meta WHERE key='nonce_prefix'",
            [], |row| Ok((row.get(0)?, row.get(1)?)))?;
        if prefix.len() != NONCE_PREFIX_BYTES || !(0..(1i64 << 32)).contains(&counter) {
            return Err(VaultError::InvalidData);
        }
        let mut nonce = prefix;
        nonce.extend_from_slice(&(counter as u32).to_be_bytes());
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_slice()));
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                aes_gcm::aead::Payload {
                    msg: plaintext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| VaultError::Crypto)?;
        transaction.execute(
            "UPDATE store_meta SET value=? WHERE key='next_nonce_counter'",
            params![counter + 1],
        )?;
        transaction.execute("INSERT INTO encrypted_records(domain,record_id,nonce,ciphertext) VALUES(?,?,?,?)
            ON CONFLICT(domain,record_id) DO UPDATE SET nonce=excluded.nonce,ciphertext=excluded.ciphertext",
            params![domain, record_id, nonce, ciphertext])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn replace_records(
        &self,
        domain: &str,
        records: &[(String, Vec<u8>)],
    ) -> Result<(), VaultError> {
        validate_domain(domain)?;
        let key = Zeroizing::new(self.record_key(domain)?);
        let mut connection = self.connect_store()?;
        let transaction = connection.transaction()?;
        let (prefix, mut counter): (Vec<u8>, i64) = transaction.query_row(
            "SELECT value,(SELECT value FROM store_meta WHERE key='next_nonce_counter') FROM store_meta WHERE key='nonce_prefix'",
            [], |row| Ok((row.get(0)?, row.get(1)?)))?;
        if prefix.len() != NONCE_PREFIX_BYTES
            || !(0..(1i64 << 32)).contains(&counter)
            || counter.saturating_add(records.len() as i64) > (1i64 << 32)
        {
            return Err(VaultError::InvalidData);
        }
        transaction.execute("DELETE FROM encrypted_records WHERE domain=?", [domain])?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_slice()));
        for (record_id, plaintext) in records {
            if record_id.len() > 1024 {
                return Err(VaultError::InvalidData);
            }
            let mut nonce = prefix.clone();
            nonce.extend_from_slice(&(counter as u32).to_be_bytes());
            counter += 1;
            let aad = format!("Aurora/{}/{domain}/{record_id}/v1", self.profile_id);
            let ciphertext = cipher
                .encrypt(
                    Nonce::from_slice(&nonce),
                    aes_gcm::aead::Payload {
                        msg: plaintext,
                        aad: aad.as_bytes(),
                    },
                )
                .map_err(|_| VaultError::Crypto)?;
            transaction.execute(
                "INSERT INTO encrypted_records(domain,record_id,nonce,ciphertext) VALUES(?,?,?,?)",
                params![domain, record_id, nonce, ciphertext],
            )?;
        }
        transaction.execute(
            "UPDATE store_meta SET value=? WHERE key='next_nonce_counter'",
            [counter],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn get(&self, domain: &str, record_id: &str) -> Result<Option<Vec<u8>>, VaultError> {
        validate_domain(domain)?;
        let key = Zeroizing::new(self.record_key(domain)?);
        let connection = self.connect_store()?;
        let row: Option<(Vec<u8>, Vec<u8>)> = connection
            .query_row(
                "SELECT nonce,ciphertext FROM encrypted_records WHERE domain=? AND record_id=?",
                params![domain, record_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(nonce, ciphertext)| {
            decrypt_record(
                &key,
                &nonce,
                &ciphertext,
                format!("Aurora/{}/{domain}/{record_id}/v1", self.profile_id).as_bytes(),
            )
        })
        .transpose()
    }

    pub fn list(&self, domain: &str) -> Result<Vec<(String, Vec<u8>)>, VaultError> {
        validate_domain(domain)?;
        let key = Zeroizing::new(self.record_key(domain)?);
        let connection = self.connect_store()?;
        let mut statement = connection.prepare("SELECT record_id,nonce,ciphertext FROM encrypted_records WHERE domain=? ORDER BY record_id")?;
        let rows = statement.query_map([domain], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        let mut records = Vec::new();
        for row in rows {
            let (id, nonce, ciphertext) = row?;
            let plaintext = decrypt_record(
                &key,
                &nonce,
                &ciphertext,
                format!("Aurora/{}/{domain}/{id}/v1", self.profile_id).as_bytes(),
            )?;
            records.push((id, plaintext));
        }
        Ok(records)
    }

    pub fn delete(&self, domain: &str, record_id: &str) -> Result<(), VaultError> {
        validate_domain(domain)?;
        self.record_key(domain)?;
        self.connect_store()?.execute(
            "DELETE FROM encrypted_records WHERE domain=? AND record_id=?",
            params![domain, record_id],
        )?;
        Ok(())
    }

    fn key(&self) -> Result<&[u8; KEY_BYTES], VaultError> {
        self.master_key.as_ref().ok_or(VaultError::Unlock)
    }
    fn record_key(&self, domain: &str) -> Result<[u8; KEY_BYTES], VaultError> {
        validate_domain(domain)?;
        derive_hkdf(
            self.key()?,
            &[],
            format!("Aurora/{}/records/{domain}/v1", self.profile_id).as_bytes(),
        )
    }
    fn connect_store(&self) -> Result<Connection, VaultError> {
        // Never recreate nonce state in an existing vault: resetting its counter
        // could reuse an AES-GCM nonce under the same profile key.
        let connection = Connection::open_with_flags(
            self.directory.join("records.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?;
        Ok(connection)
    }
}

impl Drop for ProfileVault {
    fn drop(&mut self) {
        self.lock();
    }
}

fn validate_domain(domain: &str) -> Result<(), VaultError> {
    if domain.is_empty()
        || domain.len() > 64
        || !domain.bytes().enumerate().all(|(i, b)| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || (i > 0 && b"._-".contains(&b))
        })
    {
        return Err(VaultError::InvalidData);
    }
    Ok(())
}

fn derive_hkdf(key: &[u8], salt: &[u8], info: &[u8]) -> Result<[u8; KEY_BYTES], VaultError> {
    let hkdf = Hkdf::<Sha256>::new(if salt.is_empty() { None } else { Some(salt) }, key);
    let mut output = [0u8; KEY_BYTES];
    hkdf.expand(info, &mut output)
        .map_err(|_| VaultError::Crypto)?;
    Ok(output)
}

fn password_key(password: &str, salt: &[u8]) -> Result<[u8; KEY_BYTES], VaultError> {
    if password.chars().count() < 12 || salt.len() != SALT_BYTES {
        return Err(VaultError::Unlock);
    }
    let params = Params::new(KDF_MEMORY_KIB, KDF_ITERATIONS, 1, Some(KEY_BYTES))
        .map_err(|_| VaultError::Crypto)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; KEY_BYTES];
    argon
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|_| VaultError::Crypto)?;
    Ok(key)
}

fn seal(
    key: &[u8; KEY_BYTES],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<(String, String), VaultError> {
    let mut nonce = [0u8; NONCE_BYTES];
    OsRng.fill_bytes(&mut nonce);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            aes_gcm::aead::Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| VaultError::Crypto)?;
    Ok((STANDARD.encode(nonce), STANDARD.encode(ciphertext)))
}

fn open(key: &[u8; KEY_BYTES], envelope: &Envelope, aad: &[u8]) -> Result<Vec<u8>, VaultError> {
    let nonce = STANDARD
        .decode(&envelope.nonce)
        .map_err(|_| VaultError::InvalidData)?;
    let ciphertext = STANDARD
        .decode(&envelope.ciphertext)
        .map_err(|_| VaultError::InvalidData)?;
    if nonce.len() != NONCE_BYTES {
        return Err(VaultError::InvalidData);
    }
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key))
        .decrypt(
            Nonce::from_slice(&nonce),
            aes_gcm::aead::Payload {
                msg: &ciphertext,
                aad,
            },
        )
        .map_err(|_| VaultError::Crypto)
}

fn wrap_password(
    password: &str,
    master: &[u8; KEY_BYTES],
    aad: &[u8],
) -> Result<Envelope, VaultError> {
    let mut salt = [0u8; SALT_BYTES];
    OsRng.fill_bytes(&mut salt);
    let mut key = Zeroizing::new(password_key(password, &salt)?);
    let (nonce, ciphertext) = seal(&key, master, aad)?;
    key.zeroize();
    Ok(Envelope {
        nonce,
        ciphertext,
        salt: STANDARD.encode(salt),
        kdf: "argon2id".into(),
        memory_kib: KDF_MEMORY_KIB,
        iterations: KDF_ITERATIONS,
        parallelism: 1,
    })
}

fn unwrap_password(
    password: &str,
    envelope: &Envelope,
    aad: &[u8],
) -> Result<[u8; KEY_BYTES], VaultError> {
    if envelope.kdf != "argon2id"
        || envelope.memory_kib < 32 * 1024
        || envelope.memory_kib > 256 * 1024
        || !(2..=8).contains(&envelope.iterations)
        || !(1..=4).contains(&envelope.parallelism)
    {
        return Err(VaultError::InvalidData);
    }
    let salt = STANDARD
        .decode(&envelope.salt)
        .map_err(|_| VaultError::InvalidData)?;
    if salt.len() != SALT_BYTES {
        return Err(VaultError::InvalidData);
    }
    let params = Params::new(
        envelope.memory_kib,
        envelope.iterations,
        envelope.parallelism,
        Some(KEY_BYTES),
    )
    .map_err(|_| VaultError::InvalidData)?;
    let mut key = [0u8; KEY_BYTES];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .map_err(|_| VaultError::Unlock)?;
    let plaintext = open(&key, envelope, aad);
    key.zeroize();
    plaintext?.try_into().map_err(|_| VaultError::InvalidData)
}

fn wrap_recovery(
    profile_id: &str,
    recovery: &[u8; KEY_BYTES],
    master: &[u8; KEY_BYTES],
    aad: &[u8],
) -> Result<Envelope, VaultError> {
    let mut salt = [0u8; SALT_BYTES];
    OsRng.fill_bytes(&mut salt);
    let key = derive_hkdf(
        recovery,
        &salt,
        format!("Aurora/{profile_id}/recovery-wrap/v1").as_bytes(),
    )?;
    let (nonce, ciphertext) = seal(&key, master, aad)?;
    Ok(Envelope {
        nonce,
        ciphertext,
        salt: STANDARD.encode(salt),
        kdf: "hkdf-sha256".into(),
        memory_kib: 0,
        iterations: 0,
        parallelism: 0,
    })
}

fn unwrap_recovery(
    profile_id: &str,
    recovery_hex: &str,
    envelope: &Envelope,
    aad: &[u8],
) -> Result<[u8; KEY_BYTES], VaultError> {
    if envelope.kdf != "hkdf-sha256" {
        return Err(VaultError::InvalidData);
    }
    let recovery = hex::decode(recovery_hex).map_err(|_| VaultError::Unlock)?;
    if recovery.len() != KEY_BYTES {
        return Err(VaultError::Unlock);
    }
    let salt = STANDARD
        .decode(&envelope.salt)
        .map_err(|_| VaultError::InvalidData)?;
    if salt.len() != SALT_BYTES {
        return Err(VaultError::InvalidData);
    }
    let key = derive_hkdf(
        &recovery,
        &salt,
        format!("Aurora/{profile_id}/recovery-wrap/v1").as_bytes(),
    )?;
    open(&key, envelope, aad)?
        .try_into()
        .map_err(|_| VaultError::InvalidData)
}

fn decrypt_record(
    key: &[u8; KEY_BYTES],
    nonce: &[u8],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, VaultError> {
    if nonce.len() != NONCE_BYTES {
        return Err(VaultError::InvalidData);
    }
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key))
        .decrypt(
            Nonce::from_slice(nonce),
            aes_gcm::aead::Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| VaultError::Crypto)
}

fn read_metadata(directory: &Path) -> Result<Metadata, VaultError> {
    let path = directory.join("profile.json");
    if fs::metadata(&path)?.len() > 64 * 1024 {
        return Err(VaultError::InvalidData);
    }
    let metadata: Metadata =
        serde_json::from_slice(&fs::read(path)?).map_err(|_| VaultError::InvalidData)?;
    if metadata.version != 1
        || metadata.profile_id
            != directory
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
        || metadata.display_name.is_empty()
        || metadata.display_name.chars().count() > 64
    {
        return Err(VaultError::InvalidData);
    }
    Ok(metadata)
}

fn write_metadata(directory: &Path, metadata: &Metadata) -> Result<(), VaultError> {
    let path = directory.join("profile.json");
    let temporary = directory.join("profile.json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec(metadata).map_err(|_| VaultError::InvalidData)?,
    )?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn init_store(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;
        CREATE TABLE IF NOT EXISTS store_meta(key TEXT PRIMARY KEY, value BLOB NOT NULL);
        CREATE TABLE IF NOT EXISTS encrypted_records(domain TEXT NOT NULL, record_id TEXT NOT NULL,
        nonce BLOB NOT NULL UNIQUE, ciphertext BLOB NOT NULL, PRIMARY KEY(domain,record_id));",
    )?;
    let mut prefix = [0u8; NONCE_PREFIX_BYTES];
    OsRng.fill_bytes(&mut prefix);
    connection.execute(
        "INSERT OR IGNORE INTO store_meta(key,value) VALUES('nonce_prefix',?)",
        [prefix.as_slice()],
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO store_meta(key,value) VALUES('next_nonce_counter',0)",
        [],
    )?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn dpapi_protect(data: &[u8], entropy: &[u8]) -> Result<Vec<u8>, VaultError> {
    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData},
        },
        core::PCWSTR,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len().try_into().map_err(|_| VaultError::InvalidData)?,
        pbData: data.as_ptr() as *mut u8,
    };
    let entropy_blob = CRYPT_INTEGER_BLOB {
        cbData: entropy
            .len()
            .try_into()
            .map_err(|_| VaultError::InvalidData)?,
        pbData: entropy.as_ptr() as *mut u8,
    };
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            Some(&entropy_blob),
            None,
            None,
            0,
            &mut output,
        )
        .map_err(|_| VaultError::Crypto)?;
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut _)));
    }
    Ok(result)
}

#[cfg(target_os = "windows")]
fn dpapi_unprotect(data: &[u8], entropy: &[u8]) -> Result<Vec<u8>, VaultError> {
    use windows::Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData},
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len().try_into().map_err(|_| VaultError::InvalidData)?,
        pbData: data.as_ptr() as *mut u8,
    };
    let entropy_blob = CRYPT_INTEGER_BLOB {
        cbData: entropy
            .len()
            .try_into()
            .map_err(|_| VaultError::InvalidData)?,
        pbData: entropy.as_ptr() as *mut u8,
    };
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            Some(&entropy_blob),
            None,
            None,
            0,
            &mut output,
        )
        .map_err(|_| VaultError::Crypto)?;
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut _)));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("aurora-test-{}", uuid::Uuid::new_v4())))
        }
        fn create(&self) -> CreatedVault {
            ProfileVault::create(&self.0, "Test", "test password alpha").unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn missing_nonce_state_fails_closed() {
        let root = Fixture::new();
        let created = root.create();
        let vault = created.vault;
        vault.put("settings", "secret", b"private value").unwrap();
        vault.delete("settings", "secret").unwrap();
        let db = Connection::open(vault.directory().join("records.sqlite3")).unwrap();
        db.execute("DELETE FROM store_meta WHERE key='next_nonce_counter'", [])
            .unwrap();
        assert!(vault.put("settings", "secret", b"changed value").is_err());
    }
    #[test]
    fn independent_keys_lock_and_authenticated_records() {
        let root = Fixture::new();
        let mut first = root.create().vault;
        let second = root.create().vault;
        assert_ne!(first.key().unwrap(), second.key().unwrap());
        first.put("settings", "secret", b"private value").unwrap();
        assert_eq!(
            first.get("settings", "secret").unwrap().unwrap(),
            b"private value"
        );
        assert!(second.get("settings", "secret").unwrap().is_none());
        let db = Connection::open(first.directory().join("records.sqlite3")).unwrap();
        let (nonce, ciphertext): (Vec<u8>, Vec<u8>) = db
            .query_row(
                "SELECT nonce,ciphertext FROM encrypted_records WHERE record_id='secret'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let other_db = Connection::open(second.directory().join("records.sqlite3")).unwrap();
        other_db
            .execute(
                "INSERT INTO encrypted_records VALUES('settings','secret',?,?)",
                params![nonce, ciphertext],
            )
            .unwrap();
        assert!(second.get("settings", "secret").is_err());
        db.execute("UPDATE encrypted_records SET ciphertext=zeroblob(32)", [])
            .unwrap();
        assert!(first.get("settings", "secret").is_err());
        first.lock();
        assert!(first.put("settings", "secret", b"x").is_err());
        assert!(first.get("settings", "secret").is_err());
        second.put("settings", "still-open", b"yes").unwrap();
    }
    #[test]
    fn nonce_progress_survives_reopen_and_deleted_database_is_not_recreated() {
        let root = Fixture::new();
        let mut vault = root.create().vault;
        vault.put("settings", "one", b"first").unwrap();
        let directory = vault.directory().to_owned();
        let path = directory.join("records.sqlite3");
        let nonce_before: Vec<u8> = Connection::open(&path)
            .unwrap()
            .query_row("SELECT nonce FROM encrypted_records", [], |row| row.get(0))
            .unwrap();
        vault.lock();
        let reopened = ProfileVault::unlock_password(&directory, "test password alpha").unwrap();
        reopened.put("settings", "one", b"second").unwrap();
        let nonce_after: Vec<u8> = Connection::open(&path)
            .unwrap()
            .query_row("SELECT nonce FROM encrypted_records", [], |row| row.get(0))
            .unwrap();
        assert_ne!(nonce_before, nonce_after);
        fs::remove_file(&path).unwrap();
        assert!(reopened.put("settings", "one", b"third").is_err());
        assert!(!path.exists());
    }
    #[test]
    fn recovery_and_backup_round_trip() {
        let root = Fixture::new();
        let created = root.create();
        let mut vault = created.vault;
        vault
            .put("bookmarks", "one", b"https://example.invalid")
            .unwrap();
        let directory = vault.directory().to_owned();
        let backup = root.0.join("backup.aurorabk");
        vault
            .export_backup(&backup, "backup password alpha")
            .unwrap();
        vault.lock();
        assert!(ProfileVault::unlock_password(&directory, "wrong password").is_err());
        let recovered = ProfileVault::unlock_recovery(&directory, &created.recovery_key).unwrap();
        assert_eq!(
            recovered.get("bookmarks", "one").unwrap().unwrap(),
            b"https://example.invalid"
        );
        let destination = root.0.join("restored");
        let restored = ProfileVault::restore_backup(
            &backup,
            &destination,
            "backup password alpha",
            "test password alpha",
        )
        .unwrap();
        assert_eq!(
            restored.get("bookmarks", "one").unwrap().unwrap(),
            b"https://example.invalid"
        );
        restored.put("bookmarks", "two", b"new record").unwrap();
        let mut bytes = fs::read(&backup).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&backup, bytes).unwrap();
        assert!(
            ProfileVault::restore_backup(
                &backup,
                &root.0.join("tampered"),
                "backup password alpha",
                "test password alpha"
            )
            .is_err()
        );
    }

    #[test]
    fn restores_profile_backup_payload_written_by_zip_261() {
        // This ZIP payload was produced by zip 2.6.1, the version used before
        // Aurora upgraded its archive reader/writer to zip 9.0.5.
        let root = Fixture::new();
        fs::create_dir_all(&root.0).unwrap();
        let backup_password = "legacy archive password";
        let salt = [0x3c; SALT_BYTES];
        let nonce = [0x7d; NONCE_BYTES];
        let key = Zeroizing::new(password_key(backup_password, &salt).unwrap());
        let payload = include_bytes!("../fixtures/legacy-v1-backup-payload.zip");
        let encrypted = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key[..]))
            .encrypt(
                Nonce::from_slice(&nonce),
                aes_gcm::aead::Payload {
                    msg: payload,
                    aad: BACKUP_AAD,
                },
            )
            .unwrap();
        let mut backup_bytes = Vec::new();
        backup_bytes.extend_from_slice(BACKUP_MAGIC);
        backup_bytes.extend_from_slice(&salt);
        backup_bytes.extend_from_slice(&nonce);
        backup_bytes.extend_from_slice(&encrypted);
        let backup_path = root.0.join("legacy-v1.aurorabk");
        fs::write(&backup_path, backup_bytes).unwrap();

        let restored = ProfileVault::restore_backup(
            &backup_path,
            &root.0.join("restored-legacy"),
            backup_password,
            "fixture profile password 2026",
        )
        .unwrap();
        assert_eq!(
            restored.get("bookmarks", "fixture").unwrap().unwrap(),
            br#"{"title":"Legacy ZIP 2.6.1 fixture","url":"https://example.invalid/"}"#
        );
    }
}
