#!/bin/sh
set -eu

APP_NAME="yanklog"
BASE_URL="${YANKLOG_BASE_URL:-https://downloads.yanklog.com/linux}"
ICON_URL="${YANKLOG_ICON_URL:-https://yanklog.com/logo.svg}"
INSTALL_DIR="${YANKLOG_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${YANKLOG_VERSION:-}"
ARCH_OVERRIDE="${YANKLOG_ARCH:-}"
MIN_GLIBC_VERSION="2.35"
SKIP_DESKTOP=0
PATH_UPDATE=1

if [ "${YANKLOG_NO_PATH_UPDATE:-}" = "1" ]; then
    PATH_UPDATE=0
fi

usage() {
    cat <<'EOF'
Install yanklog on Linux from prebuilt AppImage artifacts.

Usage:
  sh install.sh [options]

Options:
  --version <version>      Install a specific version (default: latest-version.txt)
  --arch <arch>            Force architecture (x86_64 or aarch64)
  --x86_64                 Shortcut for --arch x86_64
  --aarch64                Shortcut for --arch aarch64
  --aarch                  Shortcut for --arch aarch64
  --dir <path>             Install directory (default: ~/.local/bin)
  --no-path-update         Do not modify shell startup files for PATH
  --skip-desktop           Skip desktop entry and icon installation
  -h, --help               Show this help message

Environment variables:
  YANKLOG_VERSION          Same as --version
  YANKLOG_ARCH             Same as --arch
  YANKLOG_INSTALL_DIR      Same as --dir
  YANKLOG_NO_PATH_UPDATE   Set to 1 to disable automatic PATH update
  YANKLOG_BASE_URL         Base URL for Linux release artifacts and metadata (default: https://downloads.yanklog.com/linux)
  YANKLOG_ICON_URL         Icon URL (default: https://yanklog.com/logo.svg)
EOF
}

log() {
    printf '%s\n' "$*"
}

print_header() {
    cat <<'EOF'
+----------------------------------------------------------+
| yanklog                                                  |
| private clipboard history, stored locally on this device |
+----------------------------------------------------------+
EOF
}

die() {
    printf 'Error: %s\n' "$*" >&2
    exit 1
}

have_cmd() {
    command -v "$1" >/dev/null 2>&1
}

normalize_arch() {
    case "$1" in
        x86_64|amd64)
            printf 'x86_64\n'
            ;;
        aarch64|arm64|aarch)
            printf 'aarch64\n'
            ;;
        *)
            return 1
            ;;
    esac
}

glibc_version() {
    if have_cmd getconf; then
        getconf GNU_LIBC_VERSION 2>/dev/null | awk '{print $2; exit}'
        return
    fi

    if have_cmd ldd; then
        ldd --version 2>/dev/null | awk 'NR == 1 {
            for (i = 1; i <= NF; i++) {
                if ($i ~ /^[0-9]+\.[0-9]+/) {
                    print $i
                    exit
                }
            }
        }'
        return
    fi
}

version_at_least() {
    found="$1"
    minimum="$2"
    awk -v found="$found" -v minimum="$minimum" '
        BEGIN {
            split(found, f, ".")
            split(minimum, m, ".")
            for (i = 1; i <= 3; i++) {
                fv = (f[i] == "" ? 0 : f[i]) + 0
                mv = (m[i] == "" ? 0 : m[i]) + 0
                if (fv > mv) exit 0
                if (fv < mv) exit 1
            }
            exit 0
        }
    '
}

check_glibc_baseline() {
    found="$(glibc_version || true)"
    [ -n "$found" ] || return 0

    if ! version_at_least "$found" "$MIN_GLIBC_VERSION"; then
        die "This AppImage requires glibc ${MIN_GLIBC_VERSION} or newer, roughly Ubuntu 22.04+ / Debian 12+ era systems. This system has glibc ${found}. Use a newer Linux distribution, or wait for a future Flatpak build for older distro support."
    fi
}

append_path_line_if_missing() {
    rc_file="$1"
    path_line="$2"

    if [ -f "$rc_file" ] && grep -Fqs "$path_line" "$rc_file"; then
        return
    fi

    if [ -f "$rc_file" ] && [ -s "$rc_file" ]; then
        printf '\n%s\n' "$path_line" >> "$rc_file"
    else
        printf '%s\n' "$path_line" >> "$rc_file"
    fi

    if [ -z "${UPDATED_PATH_FILES:-}" ]; then
        UPDATED_PATH_FILES="$rc_file"
    else
        UPDATED_PATH_FILES="${UPDATED_PATH_FILES}, $rc_file"
    fi
}

persist_path_for_future_shells() {
    path_line="$1"
    append_path_line_if_missing "$HOME/.profile" "$path_line"

    shell_name="$(basename "${SHELL:-}")"
    case "$shell_name" in
        bash)
            append_path_line_if_missing "$HOME/.bashrc" "$path_line"
            ;;
        zsh)
            append_path_line_if_missing "$HOME/.zshrc" "$path_line"
            ;;
    esac
}

download_file() {
    url="$1"
    output="$2"

    if have_cmd curl; then
        curl -fsSL "$url" -o "$output"
        return
    fi

    if have_cmd wget; then
        wget -qO "$output" "$url"
        return
    fi

    die "Neither curl nor wget is installed"
}

desktop_exec_quote() {
    printf '"%s"' "$(printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g; s/\$/\\$/g; s/`/\\`/g')"
}

print_success_summary() {
    data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
    config_home="${XDG_CONFIG_HOME:-$HOME/.config}"

    log ""
    log "Installed to: ${TARGET_BIN}"
    log "Launch:       ${APP_NAME}"
    log "Quick Pick:   ${TARGET_BIN} --pick"
    log "Update:       ${APP_NAME} --update"
    log "Manual update:"
    log "  curl -fsSLO https://downloads.yanklog.com/install.sh"
    log "  sh install.sh --no-path-update"
    log "Manual uninstall:"
    log "  curl -fsSLO https://downloads.yanklog.com/uninstall.sh"
    log "  sh uninstall.sh"
    log "Data:         ${data_home}/${APP_NAME}"
    log "Database:     ${data_home}/${APP_NAME}/history.db"
    log "Config:       ${config_home}/${APP_NAME}"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            [ "$#" -ge 2 ] || die "--version requires a value"
            VERSION="$2"
            shift 2
            ;;
        --arch)
            [ "$#" -ge 2 ] || die "--arch requires a value"
            ARCH_OVERRIDE="$2"
            shift 2
            ;;
        --x86_64)
            ARCH_OVERRIDE="x86_64"
            shift
            ;;
        --aarch64|--aarch)
            ARCH_OVERRIDE="aarch64"
            shift
            ;;
        --dir)
            [ "$#" -ge 2 ] || die "--dir requires a value"
            INSTALL_DIR="$2"
            shift 2
            ;;
        --no-path-update)
            PATH_UPDATE=0
            shift
            ;;
        --skip-desktop)
            SKIP_DESKTOP=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "Unknown option: $1"
            ;;
    esac
done

OS_NAME="$(uname -s)"
[ "$OS_NAME" = "Linux" ] || die "This installer only supports Linux (found: $OS_NAME)"
check_glibc_baseline

if [ -n "$ARCH_OVERRIDE" ]; then
    ARCH="$(normalize_arch "$ARCH_OVERRIDE")" || die "Unsupported --arch: $ARCH_OVERRIDE (supported: x86_64, aarch64)"
else
    ARCH_RAW="$(uname -m)"
    ARCH="$(normalize_arch "$ARCH_RAW")" || die "Unsupported architecture: $ARCH_RAW (supported: x86_64, aarch64)"
fi

TMP_DIR="$(mktemp -d)"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

if [ -z "$VERSION" ]; then
    VERSION_FILE="$TMP_DIR/latest-version.txt"
    download_file "$BASE_URL/latest-version.txt" "$VERSION_FILE" || die "Failed to download latest version metadata"
    VERSION="$(tr -d ' \t\r\n' < "$VERSION_FILE")"
fi

[ -n "$VERSION" ] || die "Resolved version is empty"
case "$VERSION" in
    *[!0-9A-Za-z._-]*|*html*|*HTML*|*DOCTYPE*|*doctype*|*\<*|*\>*)
        die "Invalid version metadata from ${BASE_URL}/latest-version.txt. Publish latest-version.txt and the Linux AppImage artifacts first."
        ;;
esac

ARTIFACT_NAME="yanklog-${VERSION}-linux-${ARCH}.AppImage"
ARTIFACT_URL="${BASE_URL}/${ARTIFACT_NAME}"
CHECKSUM_URL="${ARTIFACT_URL}.sha256"
ARTIFACT_PATH="${TMP_DIR}/${ARTIFACT_NAME}"
CHECKSUM_PATH="${TMP_DIR}/${ARTIFACT_NAME}.sha256"

print_header
log ""
log "${APP_NAME} ${VERSION} for Linux ${ARCH}"
log ""
log "Downloading release artifact and checksum..."
download_file "$ARTIFACT_URL" "$ARTIFACT_PATH" || die "Failed to download ${ARTIFACT_URL}"
download_file "$CHECKSUM_URL" "$CHECKSUM_PATH" || die "Failed to download ${CHECKSUM_URL}"

EXPECTED_SHA="$(awk '{print $1; exit}' "$CHECKSUM_PATH")"
[ -n "$EXPECTED_SHA" ] || die "Checksum file is empty or invalid: $CHECKSUM_URL"

if have_cmd sha256sum; then
    ACTUAL_SHA="$(sha256sum "$ARTIFACT_PATH" | awk '{print $1}')"
elif have_cmd shasum; then
    ACTUAL_SHA="$(shasum -a 256 "$ARTIFACT_PATH" | awk '{print $1}')"
else
    die "Neither sha256sum nor shasum is installed"
fi

[ "$ACTUAL_SHA" = "$EXPECTED_SHA" ] || die "Checksum verification failed for ${ARTIFACT_NAME}"

mkdir -p "$INSTALL_DIR"
TARGET_BIN="${INSTALL_DIR}/${APP_NAME}"

log "Installing AppImage..."
if have_cmd install; then
    install -m 0755 "$ARTIFACT_PATH" "$TARGET_BIN"
else
    cp "$ARTIFACT_PATH" "$TARGET_BIN"
    chmod 0755 "$TARGET_BIN"
fi

if [ "$SKIP_DESKTOP" -eq 0 ]; then
    DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
    APPLICATIONS_DIR="${DATA_HOME}/applications"
    ICONS_DIR="${DATA_HOME}/icons/hicolor/scalable/apps"
    DESKTOP_FILE="${APPLICATIONS_DIR}/com.yanklog.app.desktop"
    QUICK_PICK_DESKTOP_FILE="${APPLICATIONS_DIR}/com.yanklog.app.quickpick.desktop"
    ICON_FILE="${ICONS_DIR}/yanklog.svg"
    DESKTOP_EXEC="$(desktop_exec_quote "$TARGET_BIN")"

    mkdir -p "$APPLICATIONS_DIR"
    mkdir -p "$ICONS_DIR"

    cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=YankLog
GenericName=Clipboard Manager
Comment=Clipboard manager for Linux
Exec=${DESKTOP_EXEC}
Icon=yanklog
Terminal=false
Categories=Utility;
Keywords=clipboard;history;copy;paste;yanklog;
StartupNotify=true
Actions=QuickPick;

[Desktop Action QuickPick]
Name=Quick Pick
Exec=${DESKTOP_EXEC} --pick
Icon=yanklog
EOF

    cat > "$QUICK_PICK_DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=yanklog Quick Pick
Comment=Open the yanklog quick picker
Exec=${DESKTOP_EXEC} --pick
Icon=yanklog
Terminal=false
Categories=Utility;
StartupNotify=false
NoDisplay=true
EOF

    if ! download_file "$ICON_URL" "$ICON_FILE"; then
        log "Warning: could not download icon from ${ICON_URL}"
    fi

    if have_cmd update-desktop-database; then
        update-desktop-database "$APPLICATIONS_DIR" >/dev/null 2>&1 || true
    fi
fi

log "Install complete."

case ":$PATH:" in
    *":$INSTALL_DIR:"*)
        ;;
    *)
        PATH_LINE="export PATH=\"${INSTALL_DIR}:\$PATH\""
        if [ "$PATH_UPDATE" -eq 1 ]; then
            UPDATED_PATH_FILES=""
            persist_path_for_future_shells "$PATH_LINE"
            if [ -n "$UPDATED_PATH_FILES" ]; then
                log "Added PATH entry to: $UPDATED_PATH_FILES"
                log "Open a new terminal, or run:"
                log "  export PATH=\"${INSTALL_DIR}:\$PATH\""
            fi
        else
            log "Add this to your shell profile:"
            log "  $PATH_LINE"
        fi
        ;;
esac

print_success_summary
