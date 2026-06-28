use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOS,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub platform: Platform,
    pub dev: bool,
}

impl Profile {
    pub fn new(platform: Platform, dev: bool) -> Self {
        Self { platform, dev }
    }

    pub fn platform_name(&self) -> &'static str {
        match self.platform {
            Platform::Linux => "linux",
            Platform::MacOS => "macos",
        }
    }

    pub fn display_name(&self) -> &'static str {
        if self.dev {
            "yanklog dev"
        } else {
            "yanklog"
        }
    }

    pub fn app_dir_name(&self) -> &'static str {
        match (self.platform, self.dev) {
            (Platform::Linux, false) => "yanklog",
            (Platform::Linux, true) => "yanklog-dev",
            (Platform::MacOS, false) => "YankLog",
            (Platform::MacOS, true) => "YankLog Dev",
        }
    }

    pub fn config_path(&self) -> PathBuf {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(self.app_dir_name());
        config_dir.join("config.toml")
    }

    pub fn data_dir(&self) -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(self.app_dir_name())
    }

    pub fn lock_file_name(&self) -> &'static str {
        if self.dev {
            "yanklog-dev.lock"
        } else {
            "yanklog.lock"
        }
    }
}
