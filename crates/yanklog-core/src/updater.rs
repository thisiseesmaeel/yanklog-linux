use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::profile::{Platform, Profile};

const DEFAULT_DOWNLOADS_ROOT_URL: &str = "https://downloads.yanklog.com";
const DEFAULT_LINUX_INSTALL_SCRIPT_URL: &str = "https://downloads.yanklog.com/install.sh";
const DEFAULT_MACOS_INSTALL_SCRIPT_URL: &str = "https://downloads.yanklog.com/install-macos.sh";

pub fn check_for_update(
    profile: &Profile,
    current_version: &str,
) -> Result<Option<String>, String> {
    if profile.dev || env::var("YANKLOG_DISABLE_UPDATE_CHECK").as_deref() == Ok("1") {
        return Ok(None);
    }

    let latest_version_url = match profile.platform {
        Platform::Linux => format!(
            "{}/latest-version.txt",
            platform_downloads_base_url(profile.platform)
        ),
        Platform::MacOS => format!(
            "{}/latest-macos-version.txt",
            platform_downloads_base_url(profile.platform)
        ),
    };
    let latest_version = sanitize_version(&download_text(&latest_version_url)?)?;

    if is_newer_version(&latest_version, current_version) {
        Ok(Some(latest_version))
    } else {
        Ok(None)
    }
}

pub fn install_update(profile: &Profile, version: &str) -> Result<String, String> {
    if profile.dev {
        return Err("Updates are disabled for yanklog dev builds.".to_string());
    }

    let version = sanitize_version(version)?;
    let installer_url = match profile.platform {
        Platform::Linux => env::var("YANKLOG_INSTALLER_URL")
            .unwrap_or_else(|_| DEFAULT_LINUX_INSTALL_SCRIPT_URL.to_string()),
        Platform::MacOS => env::var("YANKLOG_MACOS_INSTALLER_URL")
            .unwrap_or_else(|_| DEFAULT_MACOS_INSTALL_SCRIPT_URL.to_string()),
    };
    let script = download_text(&installer_url)?;
    let install_dir = detect_install_dir(profile.platform);

    let mut command = Command::new("sh");
    command
        .arg("-s")
        .arg("--")
        .arg("--version")
        .arg(&version)
        .arg("--dir")
        .arg(&install_dir);

    match profile.platform {
        Platform::Linux => {
            command.arg("--no-path-update");
        }
        Platform::MacOS => {
            command.arg("--no-launch");
        }
    }

    let mut child = command
        .env("YANKLOG_INSTALL_DIR", &install_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Failed to start installer shell: {err}"))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Failed to open installer stdin".to_string())?;
    stdin
        .write_all(script.as_bytes())
        .map_err(|err| format!("Failed to write installer script to stdin: {err}"))?;
    drop(stdin);

    let output = child
        .wait_with_output()
        .map_err(|err| format!("Failed to wait for installer process: {err}"))?;

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if output.status.success() {
        return Ok(stdout);
    }

    Err(if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("Installer exited with status {}", output.status)
    })
}

fn platform_downloads_base_url(platform: Platform) -> String {
    let platform_base = match platform {
        Platform::Linux => env::var("YANKLOG_BASE_URL").ok(),
        Platform::MacOS => env::var("YANKLOG_MACOS_BASE_URL").ok(),
    };

    if let Some(base_url) = platform_base {
        return base_url.trim_end_matches('/').to_string();
    }

    let root = env::var("YANKLOG_DOWNLOADS_ROOT_URL")
        .unwrap_or_else(|_| DEFAULT_DOWNLOADS_ROOT_URL.to_string());
    let platform_path = match platform {
        Platform::Linux => "linux",
        Platform::MacOS => "macos",
    };
    format!("{}/{}", root.trim_end_matches('/'), platform_path)
}

fn sanitize_version(version: &str) -> Result<String, String> {
    let version = version.trim().trim_start_matches('v');
    if version.is_empty() {
        return Err("Update version was empty.".to_string());
    }

    if version.len() > 64
        || version.contains('<')
        || version.contains('>')
        || version.to_ascii_lowercase().contains("doctype")
        || !version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(format!("Invalid update version: {version}"));
    }

    Ok(version.to_string())
}

fn download_text(url: &str) -> Result<String, String> {
    let output = if command_exists("curl") {
        Command::new("curl")
            .args(["-fsSL", url])
            .output()
            .map_err(|err| format!("Failed to run curl: {err}"))?
    } else if command_exists("wget") {
        Command::new("wget")
            .args(["-qO-", url])
            .output()
            .map_err(|err| format!("Failed to run wget: {err}"))?
    } else {
        return Err("Neither curl nor wget is installed".to_string());
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(format!("Failed to download {url}"))
        } else {
            Err(format!("Failed to download {url}: {stderr}"))
        }
    } else {
        String::from_utf8(output.stdout)
            .map_err(|err| format!("Invalid UTF-8 response from {url}: {err}"))
    }
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    let latest = latest.trim().trim_start_matches('v');
    let current = current.trim().trim_start_matches('v');

    match (
        parse_numeric_version(latest),
        parse_numeric_version(current),
    ) {
        (Some(latest_parts), Some(current_parts)) => {
            compare_version_parts(&latest_parts, &current_parts)
        }
        _ => latest != current,
    }
}

fn parse_numeric_version(version: &str) -> Option<Vec<u64>> {
    let numeric_prefix = version
        .split_once('-')
        .map(|(prefix, _)| prefix)
        .unwrap_or(version);
    if numeric_prefix.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    for part in numeric_prefix.split('.') {
        if part.is_empty() {
            return None;
        }
        parts.push(part.parse::<u64>().ok()?);
    }
    Some(parts)
}

fn compare_version_parts(latest: &[u64], current: &[u64]) -> bool {
    let max_len = latest.len().max(current.len());
    for index in 0..max_len {
        let latest_part = latest.get(index).copied().unwrap_or(0);
        let current_part = current.get(index).copied().unwrap_or(0);
        if latest_part > current_part {
            return true;
        }
        if latest_part < current_part {
            return false;
        }
    }
    false
}

fn command_exists(cmd: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {cmd} >/dev/null 2>&1")])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn detect_install_dir(platform: Platform) -> String {
    if let Ok(appimage_path) = env::var("APPIMAGE") {
        let appimage = PathBuf::from(appimage_path);
        if let Some(parent) = appimage.parent() {
            return parent.to_string_lossy().to_string();
        }
    }

    if platform == Platform::MacOS {
        if let Some(app_parent) = env::current_exe().ok().and_then(|path| {
            path.ancestors()
                .find(|ancestor| ancestor.extension().and_then(|ext| ext.to_str()) == Some("app"))
                .and_then(|app_bundle| app_bundle.parent().map(|parent| parent.to_path_buf()))
        }) {
            return app_parent.to_string_lossy().to_string();
        }

        return dirs::home_dir()
            .map(|home| home.join("Applications").to_string_lossy().to_string())
            .unwrap_or_else(|| "~/Applications".to_string());
    }

    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/bin")))
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| "~/.local/bin".to_string())
}

#[cfg(test)]
mod tests {
    use super::{is_newer_version, sanitize_version};

    #[test]
    fn detects_newer_semver() {
        assert!(is_newer_version("1.0.51", "1.0.50"));
        assert!(!is_newer_version("1.0.50", "1.0.50"));
        assert!(!is_newer_version("1.0.49", "1.0.50"));
    }

    #[test]
    fn supports_prefixed_versions() {
        assert!(is_newer_version("v2.0.0", "1.9.9"));
    }

    #[test]
    fn rejects_invalid_update_versions() {
        assert_eq!(sanitize_version("v1.0.0").unwrap(), "1.0.0");
        assert!(sanitize_version("<!DOCTYPE html>").is_err());
        assert!(sanitize_version("").is_err());
    }
}
