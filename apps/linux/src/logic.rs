use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, NaiveDate};

/// Whether clipboard capture is paused, and until when. The state is written to
/// `path` so a pause survives a restart.
#[derive(Debug, Default)]
pub struct PauseState {
    paused: AtomicBool,
    until: Mutex<Option<SystemTime>>,
    path: Option<PathBuf>,
}

impl PauseState {
    /// Restores a pause saved at `path`, unless it has already expired.
    pub fn load(path: PathBuf) -> Self {
        let saved = std::fs::read_to_string(&path).unwrap_or_default();
        let state = Self {
            paused: AtomicBool::new(false),
            until: Mutex::new(None),
            path: Some(path),
        };
        match saved.trim() {
            "indefinite" => state.paused.store(true, Ordering::SeqCst),
            value => {
                if let Some(until) = value
                    .parse::<u64>()
                    .ok()
                    .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
                {
                    if until > SystemTime::now() {
                        if let Ok(mut slot) = state.until.lock() {
                            *slot = Some(until);
                        }
                        state.paused.store(true, Ordering::SeqCst);
                    }
                }
            }
        }
        state
    }

    pub fn pause(&self, duration: Option<Duration>) {
        let until = duration.and_then(|duration| SystemTime::now().checked_add(duration));
        if let Ok(mut slot) = self.until.lock() {
            *slot = until;
        }
        self.paused.store(true, Ordering::SeqCst);
        self.persist(Some(until));
    }

    pub fn resume(&self) {
        if let Ok(mut until) = self.until.lock() {
            *until = None;
        }
        self.paused.store(false, Ordering::SeqCst);
        self.persist(None);
    }

    pub fn is_paused(&self) -> bool {
        if !self.paused.load(Ordering::SeqCst) {
            return false;
        }
        let expired = self
            .until
            .lock()
            .ok()
            .and_then(|until| *until)
            .is_some_and(|until| SystemTime::now() >= until);
        if expired {
            self.resume();
            return false;
        }
        true
    }

    pub fn monitoring_label(&self) -> String {
        if !self.is_paused() {
            return "Monitoring".to_string();
        }
        let remaining = self
            .until
            .lock()
            .ok()
            .and_then(|until| *until)
            .and_then(|until| until.duration_since(SystemTime::now()).ok());
        match remaining {
            Some(duration) => {
                let minutes = duration.as_secs().div_ceil(60).max(1);
                format!("Paused · {minutes} min left")
            }
            None => "Paused".to_string(),
        }
    }

    /// `None` clears the saved pause; `Some(None)` saves an indefinite one.
    fn persist(&self, pause: Option<Option<SystemTime>>) {
        let Some(path) = &self.path else {
            return;
        };
        let result = match pause {
            None => match std::fs::remove_file(path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            },
            Some(until) => {
                let text = until
                    .and_then(|until| until.duration_since(UNIX_EPOCH).ok())
                    .map(|since_epoch| since_epoch.as_secs().to_string())
                    .unwrap_or_else(|| "indefinite".to_string());
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(path, text)
            }
        };
        if let Err(error) = result {
            eprintln!("Could not save the pause state: {error}");
        }
    }
}

/// Remembers for a moment that the clipboard holds content its owner marked as
/// secret, so the capture thread can skip it.
#[derive(Debug, Default)]
pub struct SecretHint {
    seen_at: Mutex<Option<Instant>>,
}

impl SecretHint {
    const WINDOW: Duration = Duration::from_secs(2);

    pub fn mark(&self) {
        if let Ok(mut seen_at) = self.seen_at.lock() {
            *seen_at = Some(Instant::now());
        }
    }

    pub fn is_recent(&self) -> bool {
        self.seen_at
            .lock()
            .ok()
            .and_then(|seen_at| *seen_at)
            .is_some_and(|seen_at| seen_at.elapsed() < Self::WINDOW)
    }
}

/// The history section an entry belongs to.
pub fn section_title(is_pinned: bool, timestamp: &str, today: NaiveDate) -> &'static str {
    if is_pinned {
        return "Pinned";
    }
    let Ok(parsed) = DateTime::parse_from_rfc3339(timestamp) else {
        return "Earlier";
    };
    let date = parsed.with_timezone(&Local).date_naive();
    if date == today {
        "Today"
    } else if today.pred_opt() == Some(date) {
        "Yesterday"
    } else {
        "Earlier"
    }
}

pub fn truncate_preview(text: &str, max_chars: usize) -> String {
    let text = text.replace('\n', " ").replace('\r', "");
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text;
    }
    let truncated: String = chars.into_iter().take(max_chars).collect();
    format!("{truncated}...")
}

/// "39 characters · 1 line", as shown under a previewed entry.
pub fn content_summary(content: &str) -> String {
    let characters = content.chars().count();
    let lines = content.lines().count().max(1);
    format!(
        "{characters} character{} · {lines} line{}",
        if characters == 1 { "" } else { "s" },
        if lines == 1 { "" } else { "s" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("yanklog-linux-{name}-{}", std::process::id()))
    }

    #[test]
    fn indefinite_pause_survives_a_restart() {
        let path = temp_path("pause-indefinite");
        let _ = std::fs::remove_file(&path);
        PauseState::load(path.clone()).pause(None);
        assert!(PauseState::load(path.clone()).is_paused());

        PauseState::load(path.clone()).resume();
        assert!(!PauseState::load(path.clone()).is_paused());
        assert!(!path.exists());
    }

    #[test]
    fn timed_pause_is_restored_until_it_expires() {
        let path = temp_path("pause-timed");
        PauseState::load(path.clone()).pause(Some(Duration::from_secs(600)));
        let restored = PauseState::load(path.clone());
        assert!(restored.is_paused());
        assert!(restored.monitoring_label().starts_with("Paused · "));

        std::fs::write(&path, "1").unwrap();
        assert!(!PauseState::load(path.clone()).is_paused());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn sections_follow_pin_state_then_day() {
        let today = Local::now().date_naive();
        let now = Local::now().to_rfc3339();
        let yesterday = (Local::now() - chrono::Duration::days(1)).to_rfc3339();
        assert_eq!(section_title(true, &now, today), "Pinned");
        assert_eq!(section_title(false, &now, today), "Today");
        assert_eq!(section_title(false, &yesterday, today), "Yesterday");
        assert_eq!(
            section_title(false, "2020-01-01T00:00:00+00:00", today),
            "Earlier"
        );
        assert_eq!(section_title(false, "not a date", today), "Earlier");
    }

    #[test]
    fn previews_and_summaries() {
        assert_eq!(truncate_preview("a\nb", 10), "a b");
        assert_eq!(truncate_preview("abcdef", 3), "abc...");
        assert_eq!(content_summary("x"), "1 character · 1 line");
        assert_eq!(content_summary("ab\ncd"), "5 characters · 2 lines");
    }

    #[test]
    fn secret_hint_expires() {
        let hint = SecretHint::default();
        assert!(!hint.is_recent());
        hint.mark();
        assert!(hint.is_recent());
    }
}
