use std::fs;
use std::path::Path;

use argon2::Argon2;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use chrono::Utc;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::{config::Config, database::ClipboardEntry};

const MAGIC: &[u8; 8] = b"YNKLOGB1";
const SALT_LENGTH: usize = 16;
const NONCE_LENGTH: usize = 24;

#[derive(Debug, Serialize, Deserialize)]
struct BackupDocument {
    format_version: u8,
    created_at: String,
    entries: Vec<BackupEntry>,
    #[serde(default)]
    config: Option<Config>,
}

pub struct BackupContents {
    pub entries: Vec<ClipboardEntry>,
    pub config: Option<Config>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BackupEntry {
    content: String,
    content_type: String,
    timestamp: String,
    is_favorite: bool,
}

impl From<ClipboardEntry> for BackupEntry {
    fn from(entry: ClipboardEntry) -> Self {
        Self {
            content: entry.content,
            content_type: entry.content_type,
            timestamp: entry.timestamp,
            is_favorite: entry.is_favorite,
        }
    }
}

impl From<BackupEntry> for ClipboardEntry {
    fn from(entry: BackupEntry) -> Self {
        Self {
            id: 0,
            content: entry.content,
            content_type: entry.content_type,
            timestamp: entry.timestamp,
            is_favorite: entry.is_favorite,
        }
    }
}

pub fn write_encrypted(
    path: &Path,
    password: &str,
    entries: Vec<ClipboardEntry>,
    config: Option<Config>,
) -> Result<(), String> {
    validate_password(password)?;
    let document = BackupDocument {
        format_version: 1,
        created_at: Utc::now().to_rfc3339(),
        entries: entries.into_iter().map(BackupEntry::from).collect(),
        config,
    };
    let plaintext = serde_json::to_vec(&document).map_err(|error| error.to_string())?;

    let mut salt = [0_u8; SALT_LENGTH];
    let mut nonce = [0_u8; NONCE_LENGTH];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);
    let key = derive_key(password, &salt)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|_| "Could not encrypt the backup.".to_string())?;

    let mut encoded =
        Vec::with_capacity(MAGIC.len() + SALT_LENGTH + NONCE_LENGTH + ciphertext.len());
    encoded.extend_from_slice(MAGIC);
    encoded.extend_from_slice(&salt);
    encoded.extend_from_slice(&nonce);
    encoded.extend_from_slice(&ciphertext);
    fs::write(path, encoded).map_err(|error| error.to_string())
}

pub fn read_encrypted(path: &Path, password: &str) -> Result<BackupContents, String> {
    validate_password(password)?;
    let encoded = fs::read(path).map_err(|error| error.to_string())?;
    let header_length = MAGIC.len() + SALT_LENGTH + NONCE_LENGTH;
    if encoded.len() <= header_length || &encoded[..MAGIC.len()] != MAGIC {
        return Err("This is not a supported YankLog backup.".to_string());
    }

    let salt_start = MAGIC.len();
    let nonce_start = salt_start + SALT_LENGTH;
    let ciphertext_start = nonce_start + NONCE_LENGTH;
    let key = derive_key(password, &encoded[salt_start..nonce_start])?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let plaintext = cipher
        .decrypt(
            XNonce::from_slice(&encoded[nonce_start..ciphertext_start]),
            &encoded[ciphertext_start..],
        )
        .map_err(|_| "The password is incorrect or the backup is damaged.".to_string())?;
    let document: BackupDocument = serde_json::from_slice(&plaintext)
        .map_err(|_| "The backup data is invalid.".to_string())?;
    if document.format_version != 1 {
        return Err("This backup was created by an unsupported YankLog version.".to_string());
    }
    Ok(BackupContents {
        entries: document
            .entries
            .into_iter()
            .map(ClipboardEntry::from)
            .collect(),
        config: document.config,
    })
}

fn validate_password(password: &str) -> Result<(), String> {
    if password.chars().count() < 8 {
        return Err("Backup passwords must contain at least 8 characters.".to_string());
    }
    Ok(())
}

fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut key = [0_u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|_| "Could not derive the backup encryption key.".to_string())?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_backup_round_trip_and_wrong_password() {
        let path =
            std::env::temp_dir().join(format!("yanklog-backup-{}.yanklog", std::process::id()));
        let entry = ClipboardEntry {
            id: 1,
            content: "private text".to_string(),
            content_type: "text".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            is_favorite: true,
        };
        write_encrypted(&path, "correct horse", vec![entry], Some(Config::default())).unwrap();
        assert!(!fs::read(&path)
            .unwrap()
            .windows(12)
            .any(|bytes| bytes == b"private text"));
        assert_eq!(
            read_encrypted(&path, "correct horse").unwrap().entries[0].content,
            "private text"
        );
        assert!(read_encrypted(&path, "wrong password").is_err());
        assert!(read_encrypted(&path, "correct horse")
            .unwrap()
            .config
            .is_some());
        let _ = fs::remove_file(path);
    }
}
