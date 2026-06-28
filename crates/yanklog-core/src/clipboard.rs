use arboard::Clipboard;
use std::sync::{Arc, Mutex};

pub struct ClipboardMonitor {
    last_content: Arc<Mutex<String>>,
}

impl ClipboardMonitor {
    pub fn new(_poll_interval_ms: u64) -> Self {
        Self {
            last_content: Arc::new(Mutex::new(String::new())),
        }
    }

    pub fn check_for_changes(&self) -> Option<String> {
        let mut clipboard = Clipboard::new().ok()?;
        let current = clipboard.get_text().ok()?;
        if current.is_empty() {
            return None;
        }

        let mut last = self.last_content.lock().ok()?;
        if current != *last {
            *last = current.clone();
            Some(current)
        } else {
            None
        }
    }

    pub fn update_last_content(&self, content: &str) {
        if let Ok(mut last) = self.last_content.lock() {
            *last = content.to_string();
        }
    }
}

pub fn copy_to_clipboard(text: &str) -> Result<(), arboard::Error> {
    let mut clipboard = Clipboard::new()?;
    clipboard.set_text(text)
}
