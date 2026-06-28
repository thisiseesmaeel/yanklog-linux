mod clipboard;
mod config;
mod database;
mod profile;
mod updater;

use std::sync::{Arc, Mutex};

pub use clipboard::{copy_to_clipboard, ClipboardMonitor};
pub use config::{format_timestamp, Config, Keybindings, Privacy, Retention, ThemePreference};
pub use database::{ClipboardEntry, Database};
pub use profile::{Platform, Profile};
pub use updater::{check_for_update, install_update as install_profile_update};

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum YanklogError {
    #[error("{message}")]
    Message { message: String },
}

impl From<rusqlite::Error> for YanklogError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Message {
            message: value.to_string(),
        }
    }
}

impl From<Box<dyn std::error::Error>> for YanklogError {
    fn from(value: Box<dyn std::error::Error>) -> Self {
        Self::Message {
            message: value.to_string(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ClipboardEntryRecord {
    pub id: i64,
    pub content: String,
    pub content_type: String,
    pub timestamp: String,
    pub relative_timestamp: String,
    pub is_favorite: bool,
}

impl From<ClipboardEntry> for ClipboardEntryRecord {
    fn from(entry: ClipboardEntry) -> Self {
        Self {
            relative_timestamp: format_timestamp(&entry.timestamp),
            id: entry.id,
            content: entry.content,
            content_type: entry.content_type,
            timestamp: entry.timestamp,
            is_favorite: entry.is_favorite,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ConfigRecord {
    pub max_history_size: u64,
    pub max_preview_length: u64,
    pub poll_interval_ms: u64,
    pub quick_pick_shortcut: String,
    pub quick_pick_items: u64,
    pub quick_pick_opacity: f64,
    pub retention_days: u32,
    pub ignore_secret_like: bool,
    pub ignore_one_time_codes: bool,
    pub min_text_length: u64,
    pub max_text_length: u64,
    pub ignored_patterns: Vec<String>,
    pub theme: String,
    pub launch_at_startup: bool,
}

impl From<Config> for ConfigRecord {
    fn from(config: Config) -> Self {
        Self {
            max_history_size: config.max_history_size as u64,
            max_preview_length: config.max_preview_length as u64,
            poll_interval_ms: config.poll_interval_ms,
            quick_pick_shortcut: config.keybindings.quick_pick,
            quick_pick_items: config.keybindings.quick_pick_items as u64,
            quick_pick_opacity: config.keybindings.quick_pick_opacity,
            retention_days: config.retention.max_age_days,
            ignore_secret_like: config.privacy.ignore_secret_like,
            ignore_one_time_codes: config.privacy.ignore_one_time_codes,
            min_text_length: config.privacy.min_text_length as u64,
            max_text_length: config.privacy.max_text_length as u64,
            ignored_patterns: config.privacy.ignored_patterns,
            theme: config.theme.as_str().to_string(),
            launch_at_startup: config.launch_at_startup,
        }
    }
}

impl ConfigRecord {
    fn into_config(self, platform: &str, dev: bool) -> Result<Config, YanklogError> {
        let mut config = Config::load(&profile_from_parts(platform, dev)?).unwrap_or_default();
        config.max_history_size = self.max_history_size as usize;
        config.max_preview_length = self.max_preview_length as usize;
        config.poll_interval_ms = self.poll_interval_ms;
        config.keybindings.quick_pick = self.quick_pick_shortcut;
        config.keybindings.quick_pick_items = self.quick_pick_items as usize;
        config.keybindings.quick_pick_opacity = self.quick_pick_opacity;
        config.retention.max_age_days = self.retention_days;
        config.privacy.ignore_secret_like = self.ignore_secret_like;
        config.privacy.ignore_one_time_codes = self.ignore_one_time_codes;
        config.privacy.min_text_length = self.min_text_length as usize;
        config.privacy.max_text_length = self.max_text_length as usize;
        config.privacy.ignored_patterns = self.ignored_patterns;
        config.theme = ThemePreference::from(self.theme.as_str());
        config.launch_at_startup = self.launch_at_startup;
        Ok(config)
    }
}

#[derive(uniffi::Object)]
pub struct YanklogStore {
    profile: Profile,
    database: Arc<Mutex<Database>>,
    monitor: ClipboardMonitor,
}

#[uniffi::export]
impl YanklogStore {
    #[uniffi::constructor]
    pub fn new(platform: String, dev: bool) -> Result<Arc<Self>, YanklogError> {
        let profile = profile_from_parts(&platform, dev)?;
        let database = Database::open(profile.clone())?;
        let config = Config::load(&profile).unwrap_or_default();
        Ok(Arc::new(Self {
            profile,
            database: Arc::new(Mutex::new(database)),
            monitor: ClipboardMonitor::new(config.poll_interval_ms),
        }))
    }

    pub fn list_entries(
        &self,
        limit: Option<u64>,
    ) -> Result<Vec<ClipboardEntryRecord>, YanklogError> {
        let limit = limit.map(|value| value as usize);
        let entries = self
            .database
            .lock()
            .map_err(|_| YanklogError::Message {
                message: "Database lock was poisoned.".to_string(),
            })?
            .get_history(limit)?;
        Ok(entries
            .into_iter()
            .map(ClipboardEntryRecord::from)
            .collect())
    }

    pub fn search_entries(
        &self,
        query: String,
        limit: Option<u64>,
    ) -> Result<Vec<ClipboardEntryRecord>, YanklogError> {
        let limit = limit.map(|value| value as usize);
        let entries = self
            .database
            .lock()
            .map_err(|_| YanklogError::Message {
                message: "Database lock was poisoned.".to_string(),
            })?
            .search_history(&query, limit)?;
        Ok(entries
            .into_iter()
            .map(ClipboardEntryRecord::from)
            .collect())
    }

    pub fn get_entry(&self, id: i64) -> Result<Option<ClipboardEntryRecord>, YanklogError> {
        Ok(self
            .database
            .lock()
            .map_err(|_| YanklogError::Message {
                message: "Database lock was poisoned.".to_string(),
            })?
            .get_entry(id)?
            .map(ClipboardEntryRecord::from))
    }

    pub fn insert_entry(&self, content: String, content_type: String) -> Result<i64, YanklogError> {
        Ok(self
            .database
            .lock()
            .map_err(|_| YanklogError::Message {
                message: "Database lock was poisoned.".to_string(),
            })?
            .insert_entry(&content, &content_type)?)
    }

    pub fn delete_entry(&self, id: i64) -> Result<(), YanklogError> {
        self.database
            .lock()
            .map_err(|_| YanklogError::Message {
                message: "Database lock was poisoned.".to_string(),
            })?
            .delete_entry(id)?;
        Ok(())
    }

    pub fn clear_history(&self) -> Result<(), YanklogError> {
        self.database
            .lock()
            .map_err(|_| YanklogError::Message {
                message: "Database lock was poisoned.".to_string(),
            })?
            .clear_history()?;
        Ok(())
    }

    pub fn toggle_favorite(&self, id: i64) -> Result<(), YanklogError> {
        self.database
            .lock()
            .map_err(|_| YanklogError::Message {
                message: "Database lock was poisoned.".to_string(),
            })?
            .toggle_favorite(id)?;
        Ok(())
    }

    pub fn copy_text(&self, content: String) -> Result<(), YanklogError> {
        copy_to_clipboard(&content).map_err(|err| YanklogError::Message {
            message: err.to_string(),
        })?;
        self.monitor.update_last_content(&content);
        Ok(())
    }

    pub fn check_clipboard_once(&self) -> Result<Option<String>, YanklogError> {
        Ok(self.monitor.check_for_changes())
    }

    pub fn load_config(&self) -> ConfigRecord {
        Config::load(&self.profile).unwrap_or_default().into()
    }

    pub fn save_config(&self, config: ConfigRecord) -> Result<(), YanklogError> {
        config
            .into_config(self.profile.platform_name(), self.profile.dev)?
            .save(&self.profile)?;
        Ok(())
    }

    pub fn count_entries(&self) -> Result<u64, YanklogError> {
        Ok(self
            .database
            .lock()
            .map_err(|_| YanklogError::Message {
                message: "Database lock was poisoned.".to_string(),
            })?
            .count_entries()? as u64)
    }

    pub fn data_dir(&self) -> String {
        self.profile.data_dir().to_string_lossy().to_string()
    }

    pub fn config_path(&self) -> String {
        self.profile.config_path().to_string_lossy().to_string()
    }
}

#[uniffi::export]
pub fn check_update(
    platform: String,
    current_version: String,
) -> Result<Option<String>, YanklogError> {
    let profile = profile_from_parts(&platform, false)?;
    updater::check_for_update(&profile, &current_version)
        .map_err(|message| YanklogError::Message { message })
}

#[uniffi::export]
pub fn install_update(platform: String, version: String) -> Result<String, YanklogError> {
    let profile = profile_from_parts(&platform, false)?;
    updater::install_update(&profile, &version).map_err(|message| YanklogError::Message { message })
}

fn profile_from_parts(platform: &str, dev: bool) -> Result<Profile, YanklogError> {
    let platform = match platform {
        "linux" => Platform::Linux,
        "macos" => Platform::MacOS,
        other => {
            return Err(YanklogError::Message {
                message: format!("Unsupported platform: {other}"),
            })
        }
    };
    Ok(Profile::new(platform, dev))
}

uniffi::setup_scaffolding!();
