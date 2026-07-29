use chrono::{Duration, Utc};
use rand::Rng;
use rusqlite::{params, Connection, Result};
use std::fs;
use std::path::PathBuf;

use crate::profile::Profile;

#[derive(Debug, Clone)]
pub struct ClipboardEntry {
    pub id: i64,
    pub content: String,
    pub content_type: String,
    pub timestamp: String,
    pub is_favorite: bool,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn generate_encryption_key() -> String {
        let mut rng = rand::thread_rng();
        let random_bytes: [u8; 32] = rng.gen();
        hex::encode(random_bytes)
    }

    pub fn is_valid_encryption_key(key: &str) -> bool {
        key.len() == 64 && key.chars().all(|character| character.is_ascii_hexdigit())
    }

    pub fn open(profile: Profile) -> Result<Self> {
        Self::open_at(profile.data_dir())
    }

    pub fn open_at(data_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&data_dir).ok();
        let key = Self::get_or_create_key(&data_dir)
            .expect("Failed to create or read yanklog encryption key");
        Self::open_at_with_key(data_dir, &key)
    }

    pub fn open_with_key(profile: Profile, key: &str) -> Result<Self> {
        Self::open_at_with_key(profile.data_dir(), key)
    }

    pub fn open_at_with_key(data_dir: PathBuf, key: &str) -> Result<Self> {
        std::fs::create_dir_all(&data_dir).ok();
        let db_path = data_dir.join("history.db");
        let conn = Connection::open(&db_path)?;
        conn.pragma_update(None, "key", key)?;

        if let Err(err) = conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(())) {
            eprintln!("Failed to unlock encrypted database: {err}");
            return Err(err);
        }

        conn.execute(
            "CREATE TABLE IF NOT EXISTS clipboard_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                content_type TEXT NOT NULL DEFAULT 'text',
                timestamp TEXT NOT NULL,
                is_favorite INTEGER NOT NULL DEFAULT 0,
                UNIQUE(content)
            )",
            [],
        )?;
        Self::ensure_column(
            &conn,
            "clipboard_history",
            "is_favorite",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_timestamp ON clipboard_history(timestamp DESC)",
            [],
        )?;

        Ok(Self { conn })
    }

    fn get_or_create_key(
        data_dir: &std::path::Path,
    ) -> std::result::Result<String, Box<dyn std::error::Error>> {
        let key_path = data_dir.join("secret.key");
        if key_path.exists() {
            return Ok(fs::read_to_string(&key_path)?.trim().to_string());
        }

        let key = Self::generate_encryption_key();
        fs::write(&key_path, &key)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&key_path)?.permissions();
            perms.set_mode(0o600);
            fs::set_permissions(&key_path, perms)?;
        }

        Ok(key)
    }

    pub fn insert_entry(&self, content: &str, content_type: &str) -> Result<i64> {
        let timestamp = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO clipboard_history (content, content_type, timestamp)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(content) DO UPDATE SET timestamp = ?3, content_type = ?2",
            params![content, content_type, timestamp],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn restore_entry(&self, entry: &ClipboardEntry) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO clipboard_history (content, content_type, timestamp, is_favorite)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(content) DO UPDATE SET
                 content_type = excluded.content_type,
                 timestamp = excluded.timestamp,
                 is_favorite = excluded.is_favorite",
            params![
                entry.content,
                entry.content_type,
                entry.timestamp,
                i64::from(entry.is_favorite)
            ],
        )?;
        self.conn.query_row(
            "SELECT id FROM clipboard_history WHERE content = ?1",
            [&entry.content],
            |row| row.get(0),
        )
    }

    pub fn get_history(&self, limit: Option<usize>) -> Result<Vec<ClipboardEntry>> {
        if let Some(limit) = limit {
            return self.get_history_page(limit, 0);
        }
        let mut stmt = self.conn.prepare(
            "SELECT id, content, content_type, timestamp, is_favorite
             FROM clipboard_history
             ORDER BY is_favorite DESC, timestamp DESC, id DESC",
        )?;
        let entries = stmt.query_map([], entry_from_row)?;
        entries.collect()
    }

    pub fn get_history_page(&self, limit: usize, offset: usize) -> Result<Vec<ClipboardEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, content_type, timestamp, is_favorite
             FROM clipboard_history
             ORDER BY is_favorite DESC, timestamp DESC, id DESC
             LIMIT ?1 OFFSET ?2",
        )?;
        let entries = stmt.query_map(params![limit as i64, offset as i64], entry_from_row)?;
        entries.collect()
    }

    pub fn search_history(&self, query: &str, limit: Option<usize>) -> Result<Vec<ClipboardEntry>> {
        if let Some(limit) = limit {
            return self.search_history_page(query, limit, 0);
        }
        let mut stmt = self.conn.prepare(
            "SELECT id, content, content_type, timestamp, is_favorite
             FROM clipboard_history
             WHERE content LIKE ?1
             ORDER BY is_favorite DESC, timestamp DESC, id DESC",
        )?;
        let pattern = format!("%{query}%");
        let entries = stmt.query_map([pattern], entry_from_row)?;
        entries.collect()
    }

    pub fn search_history_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ClipboardEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, content_type, timestamp, is_favorite
             FROM clipboard_history
             WHERE content LIKE ?1
             ORDER BY is_favorite DESC, timestamp DESC, id DESC
             LIMIT ?2 OFFSET ?3",
        )?;
        let pattern = format!("%{query}%");
        let entries = stmt.query_map(
            params![pattern, limit as i64, offset as i64],
            entry_from_row,
        )?;
        entries.collect()
    }

    pub fn count_search_history(&self, query: &str) -> Result<usize> {
        let pattern = format!("%{query}%");
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM clipboard_history WHERE content LIKE ?1",
            [pattern],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn get_entry(&self, id: i64) -> Result<Option<ClipboardEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, content_type, timestamp, is_favorite
             FROM clipboard_history
             WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map([id], entry_from_row)?;
        match rows.next() {
            Some(entry) => Ok(Some(entry?)),
            None => Ok(None),
        }
    }

    pub fn toggle_favorite(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE clipboard_history
             SET is_favorite = CASE is_favorite WHEN 0 THEN 1 ELSE 0 END
             WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn delete_entry(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM clipboard_history WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear_history(&self) -> Result<()> {
        self.conn.execute("DELETE FROM clipboard_history", [])?;
        Ok(())
    }

    pub fn clear_unpinned_history(&self) -> Result<usize> {
        self.conn
            .execute("DELETE FROM clipboard_history WHERE is_favorite = 0", [])
    }

    pub fn clear_older_than_days(&self, days: u32) -> Result<usize> {
        if days == 0 {
            return Ok(0);
        }
        let cutoff = Utc::now() - Duration::days(days as i64);
        let deleted = self.conn.execute(
            "DELETE FROM clipboard_history
             WHERE is_favorite = 0 AND timestamp < ?1",
            params![cutoff.to_rfc3339()],
        )?;
        Ok(deleted)
    }

    pub fn count_entries(&self) -> Result<usize> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM clipboard_history", [], |row| {
                    row.get(0)
                })?;
        Ok(count as usize)
    }

    pub fn count_unpinned_entries(&self) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM clipboard_history WHERE is_favorite = 0",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn prune_old_entries(&self, max_entries: usize) -> Result<()> {
        self.conn.execute(
            "DELETE FROM clipboard_history
             WHERE id NOT IN (
                 SELECT id FROM clipboard_history
                 ORDER BY is_favorite DESC, timestamp DESC, id DESC
                 LIMIT ?1
             )",
            params![max_entries as i64],
        )?;
        Ok(())
    }

    fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for existing in columns {
            if existing? == column {
                return Ok(());
            }
        }
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
        Ok(())
    }
}

fn entry_from_row(row: &rusqlite::Row<'_>) -> Result<ClipboardEntry> {
    Ok(ClipboardEntry {
        id: row.get(0)?,
        content: row.get(1)?,
        content_type: row.get(2)?,
        timestamp: row.get(3)?,
        is_favorite: row.get::<_, i64>(4)? != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_operations_use_encrypted_file() {
        let dir = std::env::temp_dir().join(format!("yanklog-core-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let db = Database::open_at(dir.clone()).unwrap();
        let id = db.insert_entry("test content", "text").unwrap();
        assert!(id > 0);
        assert_eq!(db.count_entries().unwrap(), 1);
        assert!(dir.join("history.db").exists());
        assert!(dir.join("secret.key").exists());

        let entries = db.search_history("test", None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "test content");

        db.toggle_favorite(entries[0].id).unwrap();
        assert!(db.get_entry(entries[0].id).unwrap().unwrap().is_favorite);

        db.clear_history().unwrap();
        assert_eq!(db.count_entries().unwrap(), 0);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn clear_unpinned_and_restore_preserve_entry_metadata() {
        let dir =
            std::env::temp_dir().join(format!("yanklog-core-recovery-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let key = "ab".repeat(32);
        let db = Database::open_at_with_key(dir.clone(), &key).unwrap();
        let pinned_id = db.insert_entry("keep me", "text").unwrap();
        db.toggle_favorite(pinned_id).unwrap();
        let deleted_id = db.insert_entry("restore me", "text").unwrap();
        let deleted = db.get_entry(deleted_id).unwrap().unwrap();

        assert_eq!(db.count_unpinned_entries().unwrap(), 1);
        assert_eq!(db.clear_unpinned_history().unwrap(), 1);
        assert_eq!(db.count_unpinned_entries().unwrap(), 0);
        assert_eq!(db.count_entries().unwrap(), 1);
        assert!(db.get_entry(pinned_id).unwrap().unwrap().is_favorite);

        db.restore_entry(&deleted).unwrap();
        let restored = db
            .search_history("restore me", None)
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(restored.timestamp, deleted.timestamp);
        assert_eq!(restored.is_favorite, deleted.is_favorite);
        assert!(!dir.join("secret.key").exists());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn paged_search_returns_complete_non_overlapping_results() {
        let dir =
            std::env::temp_dir().join(format!("yanklog-core-paging-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = Database::open_at_with_key(dir.clone(), &"cd".repeat(32)).unwrap();

        for index in 0..130 {
            db.insert_entry(&format!("needle item {index:03}"), "text")
                .unwrap();
        }
        for index in 0..7 {
            db.insert_entry(&format!("unrelated item {index:03}"), "text")
                .unwrap();
        }

        assert_eq!(db.count_search_history("needle").unwrap(), 130);
        let first = db.search_history_page("needle", 75, 0).unwrap();
        let second = db.search_history_page("needle", 75, 75).unwrap();
        assert_eq!(first.len(), 75);
        assert_eq!(second.len(), 55);
        let first_ids = first
            .iter()
            .map(|entry| entry.id)
            .collect::<std::collections::HashSet<_>>();
        assert!(second.iter().all(|entry| !first_ids.contains(&entry.id)));

        let _ = std::fs::remove_dir_all(dir);
    }
}
