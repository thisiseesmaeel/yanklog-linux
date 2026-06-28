use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use std::fs;

use crate::profile::Profile;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keybindings {
    #[serde(default = "default_quick_pick")]
    pub quick_pick: String,
    #[serde(default = "default_quick_pick_items")]
    pub quick_pick_items: usize,
    #[serde(default = "default_quick_pick_opacity")]
    pub quick_pick_opacity: f64,
}

impl Default for Keybindings {
    fn default() -> Self {
        Self {
            quick_pick: default_quick_pick(),
            quick_pick_items: default_quick_pick_items(),
            quick_pick_opacity: default_quick_pick_opacity(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Retention {
    #[serde(default)]
    pub max_age_days: u32,
}

impl Default for Retention {
    fn default() -> Self {
        Self { max_age_days: 0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Privacy {
    #[serde(default = "default_ignore_secret_like")]
    pub ignore_secret_like: bool,
    #[serde(default)]
    pub ignore_one_time_codes: bool,
    #[serde(default = "default_min_text_length")]
    pub min_text_length: usize,
    #[serde(default)]
    pub max_text_length: usize,
    #[serde(default)]
    pub ignored_patterns: Vec<String>,
}

impl Default for Privacy {
    fn default() -> Self {
        Self {
            ignore_secret_like: default_ignore_secret_like(),
            ignore_one_time_codes: false,
            min_text_length: default_min_text_length(),
            max_text_length: 0,
            ignored_patterns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    System,
    Light,
    Dark,
}

impl Default for ThemePreference {
    fn default() -> Self {
        Self::System
    }
}

impl ThemePreference {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

impl From<&str> for ThemePreference {
    fn from(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_max_history_size")]
    pub max_history_size: usize,
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_max_preview_length")]
    pub max_preview_length: usize,
    #[serde(default = "default_window_width")]
    pub window_width: i32,
    #[serde(default = "default_window_height")]
    pub window_height: i32,
    #[serde(default)]
    pub launch_at_startup: bool,
    #[serde(default)]
    pub keybindings: Keybindings,
    #[serde(default)]
    pub retention: Retention,
    #[serde(default)]
    pub privacy: Privacy,
    #[serde(default)]
    pub theme: ThemePreference,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_history_size: default_max_history_size(),
            poll_interval_ms: default_poll_interval_ms(),
            max_preview_length: default_max_preview_length(),
            window_width: default_window_width(),
            window_height: default_window_height(),
            launch_at_startup: false,
            keybindings: Keybindings::default(),
            retention: Retention::default(),
            privacy: Privacy::default(),
            theme: ThemePreference::default(),
        }
    }
}

impl Config {
    pub fn load(profile: &Profile) -> Option<Self> {
        let path = profile.config_path();
        if !path.exists() {
            return None;
        }

        fs::read_to_string(&path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
    }

    pub fn save(&self, profile: &Profile) -> Result<(), Box<dyn std::error::Error>> {
        let path = profile.config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn save_linux_app(&self, profile: &Profile) -> Result<(), Box<dyn std::error::Error>> {
        let path = profile.config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, linux_app_toml(self)?)?;
        Ok(())
    }

    pub fn truncate_for_preview(&self, text: &str) -> String {
        let text = text.replace('\n', " ").replace('\r', "");
        let chars: Vec<char> = text.chars().collect();
        if chars.len() > self.max_preview_length {
            let truncated: String = chars.into_iter().take(self.max_preview_length).collect();
            format!("{truncated}...")
        } else {
            text
        }
    }

    pub fn should_ignore_clipboard(&self, text: &str) -> bool {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return true;
        }

        let char_count = trimmed.chars().count();
        if self.privacy.min_text_length > 0 && char_count < self.privacy.min_text_length {
            return true;
        }
        if self.privacy.max_text_length > 0 && char_count > self.privacy.max_text_length {
            return true;
        }

        let lowercase = trimmed.to_lowercase();
        if self.privacy.ignored_patterns.iter().any(|pattern| {
            !pattern.trim().is_empty() && lowercase.contains(&pattern.to_lowercase())
        }) {
            return true;
        }

        if self.privacy.ignore_one_time_codes && looks_like_one_time_code(trimmed) {
            return true;
        }

        self.privacy.ignore_secret_like && looks_like_secret(trimmed)
    }

    pub fn ignored_patterns_text(&self) -> String {
        self.privacy.ignored_patterns.join(", ")
    }

    pub fn set_ignored_patterns_text(&mut self, patterns: &str) {
        self.privacy.ignored_patterns = patterns
            .split(|c| c == ',' || c == '\n')
            .map(str::trim)
            .filter(|pattern| !pattern.is_empty())
            .map(ToString::to_string)
            .collect();
    }
}

#[derive(Serialize)]
struct LinuxAppConfig<'a> {
    max_history_size: usize,
    max_preview_length: usize,
    launch_at_startup: bool,
    keybindings: LinuxAppKeybindings,
    retention: &'a Retention,
    privacy: &'a Privacy,
    theme: ThemePreference,
}

#[derive(Serialize)]
struct LinuxAppKeybindings {
    quick_pick_items: usize,
    quick_pick_opacity: f64,
}

fn linux_app_toml(config: &Config) -> Result<String, toml::ser::Error> {
    toml::to_string_pretty(&LinuxAppConfig {
        max_history_size: config.max_history_size,
        max_preview_length: config.max_preview_length,
        launch_at_startup: config.launch_at_startup,
        keybindings: LinuxAppKeybindings {
            quick_pick_items: config.keybindings.quick_pick_items,
            quick_pick_opacity: config.keybindings.quick_pick_opacity,
        },
        retention: &config.retention,
        privacy: &config.privacy,
        theme: config.theme,
    })
}

fn default_quick_pick() -> String {
    "Ctrl+Shift+V".to_string()
}

fn default_quick_pick_items() -> usize {
    10
}

fn default_quick_pick_opacity() -> f64 {
    0.95
}

fn default_ignore_secret_like() -> bool {
    true
}

fn default_min_text_length() -> usize {
    1
}

fn default_max_history_size() -> usize {
    1000
}

fn default_poll_interval_ms() -> u64 {
    500
}

fn default_max_preview_length() -> usize {
    300
}

fn default_window_width() -> i32 {
    500
}

fn default_window_height() -> i32 {
    600
}

fn looks_like_one_time_code(text: &str) -> bool {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    (compact.len() == 6 || compact.len() == 8) && compact.chars().all(|c| c.is_ascii_digit())
}

fn looks_like_secret(text: &str) -> bool {
    let lowered = text.to_lowercase();
    let secret_markers = [
        "password=",
        "passwd=",
        "token=",
        "secret=",
        "api_key=",
        "apikey=",
        "access_key=",
        "private_key",
        "bearer ",
    ];
    if secret_markers.iter().any(|marker| lowered.contains(marker)) {
        return true;
    }

    if text.lines().count() > 3 || text.chars().count() < 16 || text.contains(' ') {
        return false;
    }

    let has_upper = text.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = text.chars().any(|c| c.is_ascii_lowercase());
    let has_digit = text.chars().any(|c| c.is_ascii_digit());
    let has_symbol = text
        .chars()
        .any(|c| c.is_ascii_punctuation() && c != '-' && c != '_');

    [has_upper, has_lower, has_digit, has_symbol]
        .iter()
        .filter(|&&value| value)
        .count()
        >= 3
}

pub fn format_timestamp(timestamp: &str) -> String {
    if let Ok(dt) = timestamp.parse::<DateTime<Utc>>() {
        let local: DateTime<Local> = dt.into();
        let now = Local::now();
        let duration = now.signed_duration_since(local);

        if duration.num_seconds() < 60 {
            "Just now".to_string()
        } else if duration.num_minutes() < 60 {
            let mins = duration.num_minutes();
            format!("{} min{} ago", mins, if mins == 1 { "" } else { "s" })
        } else if duration.num_hours() < 24 {
            let hours = duration.num_hours();
            format!("{} hour{} ago", hours, if hours == 1 { "" } else { "s" })
        } else if duration.num_days() < 7 {
            let days = duration.num_days();
            format!("{} day{} ago", days, if days == 1 { "" } else { "s" })
        } else {
            local.format("%b %d, %Y").to_string()
        }
    } else {
        timestamp.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_removes_newlines() {
        let config = Config::default();
        let result = config.truncate_for_preview("line1\nline2\rline3");
        assert!(!result.contains('\n'));
        assert!(!result.contains('\r'));
    }

    #[test]
    fn privacy_filters_secret_like_values() {
        let config = Config::default();
        assert!(config.should_ignore_clipboard("token=abc123"));
        assert!(config.should_ignore_clipboard("A8fj29DKLm!!s0pq"));
        assert!(!config.should_ignore_clipboard("a normal clipboard sentence"));
    }

    #[test]
    fn ignored_patterns_round_trip() {
        let mut config = Config::default();
        config.set_ignored_patterns_text("bank, private\nscratch");
        assert_eq!(
            config.privacy.ignored_patterns,
            vec!["bank", "private", "scratch"]
        );
        assert!(config.should_ignore_clipboard("open private notes"));
    }

    #[test]
    fn linux_app_toml_omits_desktop_managed_shortcut_fields() {
        let mut config = Config::default();
        config.keybindings.quick_pick = "Ctrl+Alt+Y".to_string();
        config.poll_interval_ms = 250;
        config.window_width = 900;
        config.window_height = 700;

        let toml = linux_app_toml(&config).unwrap();

        assert!(!toml.contains("quick_pick ="));
        assert!(!toml.contains("poll_interval_ms"));
        assert!(!toml.contains("window_width"));
        assert!(!toml.contains("window_height"));
        assert!(toml.contains("quick_pick_items"));
        assert!(toml.contains("quick_pick_opacity"));

        let loaded: Config = toml::from_str(&toml).unwrap();
        assert_eq!(loaded.keybindings.quick_pick, "Ctrl+Shift+V");
        assert_eq!(loaded.poll_interval_ms, 500);
        assert_eq!(loaded.window_width, 500);
        assert_eq!(loaded.window_height, 600);
    }
}
