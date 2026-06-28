#!/bin/sh
set -eu

APP_NAME="yanklog"
INSTALL_DIR="${YANKLOG_INSTALL_DIR:-$HOME/.local/bin}"
REMOVE_DATA=0

usage() {
    cat <<'EOF'
Uninstall yanklog files installed by install.sh.

Usage:
  sh uninstall.sh [options]

Options:
  --dir <path>        Install directory used during install (default: ~/.local/bin)
  --remove-data       Also remove app data and config directories
  -h, --help          Show this help message

Environment variables:
  YANKLOG_INSTALL_DIR Same as --dir
EOF
}

log() {
    printf '%s\n' "$*"
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --dir)
            [ "$#" -ge 2 ] || {
                printf 'Error: --dir requires a value\n' >&2
                exit 1
            }
            INSTALL_DIR="$2"
            shift 2
            ;;
        --remove-data)
            REMOVE_DATA=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            printf 'Error: Unknown option: %s\n' "$1" >&2
            exit 1
            ;;
    esac
done

TARGET_BIN="${INSTALL_DIR}/${APP_NAME}"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
APPLICATIONS_DIR="${DATA_HOME}/applications"
ICONS_DIR="${DATA_HOME}/icons/hicolor/scalable/apps"
DESKTOP_FILE="${APPLICATIONS_DIR}/com.yanklog.app.desktop"
QUICK_PICK_DESKTOP_FILE="${APPLICATIONS_DIR}/com.yanklog.app.quickpick.desktop"
ICON_FILE="${ICONS_DIR}/yanklog.svg"
DATA_DIR="${DATA_HOME}/yanklog"
CONFIG_DIR="${CONFIG_HOME}/yanklog"

if [ -f "$TARGET_BIN" ]; then
    rm -f "$TARGET_BIN"
    log "Removed ${TARGET_BIN}"
else
    log "Binary not found: ${TARGET_BIN}"
fi

if [ -f "$DESKTOP_FILE" ]; then
    rm -f "$DESKTOP_FILE"
    log "Removed ${DESKTOP_FILE}"
fi

if [ -f "$QUICK_PICK_DESKTOP_FILE" ]; then
    rm -f "$QUICK_PICK_DESKTOP_FILE"
    log "Removed ${QUICK_PICK_DESKTOP_FILE}"
fi

if [ -f "$ICON_FILE" ]; then
    rm -f "$ICON_FILE"
    log "Removed ${ICON_FILE}"
fi

if command -v update-desktop-database >/dev/null 2>&1 && [ -d "$APPLICATIONS_DIR" ]; then
    update-desktop-database "$APPLICATIONS_DIR" >/dev/null 2>&1 || true
fi

if [ "$REMOVE_DATA" -eq 1 ]; then
    rm -rf "$DATA_DIR"
    rm -rf "$CONFIG_DIR"
    log "Removed data: ${DATA_DIR}"
    log "Removed config: ${CONFIG_DIR}"
fi

if [ -d "$APPLICATIONS_DIR" ]; then
    rmdir "$APPLICATIONS_DIR" 2>/dev/null || true
fi

if [ -d "$ICONS_DIR" ]; then
    rmdir "$ICONS_DIR" 2>/dev/null || true
fi

log "Uninstall complete"
