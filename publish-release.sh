#!/bin/sh
# Publish both AppImage and Flatpak releases to Cloudflare R2.
# This script wraps publish-web-release.sh (AppImages via wrangler)
# and publish-flatpak-release.sh (Flatpak repo via rclone).
set -eu

VERSION=""
X86_64_INPUT=""
AARCH64_INPUT=""
FLATPAK_REPO=""
RELEASE_NOTES_PATH=""
R2_BUCKET="${YANKLOG_R2_BUCKET:-yanklog-downloads}"
SKIP_APPIMAGE=0
SKIP_FLATPAK=0

usage() {
    cat <<'EOF'
Publish yanklog Linux release (AppImage + Flatpak) to Cloudflare R2.

Usage:
  ./publish-release.sh --version <version> [options]

Required:
  --version <version>           Release version (example: 2.0.0)
  --x86_64 <file>               x86_64 AppImage or GitHub Actions ZIP
  --aarch64 <file>              aarch64 AppImage or GitHub Actions ZIP
  --flatpak-repo <dir>          Path to the flatpak repository directory

Options:
  --release-notes <file>        Release notes for the AppImage updater
  --r2-bucket <name>            Cloudflare R2 bucket (default: yanklog-downloads)
  --skip-appimage               Skip AppImage publishing (Flatpak only)
  --skip-flatpak                Skip Flatpak publishing (AppImage only)
  -h, --help                    Show this help message

This script calls:
  1. publish-web-release.sh  — uploads AppImages, checksums, buildinfo,
     install.sh, uninstall.sh, release notes to R2 (via wrangler)
  2. publish-flatpak-release.sh — syncs the flatpak repo to R2 (via rclone)
  3. Generates and uploads the .flatpakrepo descriptor file

For local use, authenticate wrangler first:
  npx wrangler login
And configure rclone for R2:
  rclone config create r2 s3 provider=Other ...
EOF
}

log()  { printf '%s\n' "$*"; }
die()  { printf 'Error: %s\n' "$*" >&2; exit 1; }
have_cmd() { command -v "$1" >/dev/null 2>&1; }

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            [ "$#" -ge 2 ] || die "--version requires a value"
            VERSION="$2"; shift 2 ;;
        --x86_64)
            [ "$#" -ge 2 ] || die "--x86_64 requires a file"
            X86_64_INPUT="$2"; shift 2 ;;
        --aarch64)
            [ "$#" -ge 2 ] || die "--aarch64 requires a file"
            AARCH64_INPUT="$2"; shift 2 ;;
        --flatpak-repo)
            [ "$#" -ge 2 ] || die "--flatpak-repo requires a value"
            FLATPAK_REPO="$2"; shift 2 ;;
        --release-notes)
            [ "$#" -ge 2 ] || die "--release-notes requires a value"
            RELEASE_NOTES_PATH="$2"; shift 2 ;;
        --r2-bucket)
            [ "$#" -ge 2 ] || die "--r2-bucket requires a value"
            R2_BUCKET="$2"; shift 2 ;;
        --skip-appimage)
            SKIP_APPIMAGE=1; shift ;;
        --skip-flatpak)
            SKIP_FLATPAK=1; shift ;;
        -h|--help)
            usage; exit 0 ;;
        *)
            die "Unknown option: $1" ;;
    esac
done

# --- Validate ---

if [ "$SKIP_APPIMAGE" -eq 0 ]; then
    [ -n "$VERSION" ] || die "--version is required (or use --skip-appimage)"
    [ -n "$X86_64_INPUT" ] || die "--x86_64 is required (or use --skip-appimage)"
    [ -n "$AARCH64_INPUT" ] || die "--aarch64 is required (or use --skip-appimage)"
    have_cmd npx || die "npx is required for AppImage publishing (install Node.js)"
fi

if [ "$SKIP_FLATPAK" -eq 0 ]; then
    [ -n "$FLATPAK_REPO" ] || die "--flatpak-repo is required (or use --skip-flatpak)"
    [ -d "$FLATPAK_REPO/objects" ] || die "Not a flatpak repo: $FLATPAK_REPO/objects missing"
    have_cmd rclone || die "rclone is required for Flatpak publishing (install from https://rclone.org)"
    have_cmd flatpak || die "flatpak CLI is required (flatpak build-sign, build-update-repo)"
fi

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"

# --- 1. Publish AppImages ---

if [ "$SKIP_APPIMAGE" -eq 0 ]; then
    log "=== Publishing AppImages ==="
    APPIMAGE_ARGS="--version $VERSION --x86_64 $X86_64_INPUT --aarch64 $AARCH64_INPUT --r2-bucket $R2_BUCKET"
    [ -n "$RELEASE_NOTES_PATH" ] && APPIMAGE_ARGS="$APPIMAGE_ARGS --release-notes $RELEASE_NOTES_PATH"
    sh "$SCRIPT_DIR/publish-web-release.sh" $APPIMAGE_ARGS
fi

# --- 2. Publish Flatpak repository ---

if [ "$SKIP_FLATPAK" -eq 0 ]; then
    log "=== Publishing Flatpak repository ==="
    sh "$SCRIPT_DIR/publish-flatpak-release.sh" --repo-dir "$FLATPAK_REPO" --r2-bucket "$R2_BUCKET"

    # Generate and upload the .flatpakrepo descriptor
    log "Generating .flatpakrepo descriptor..."
    R2_PREFIX="linux/flatpak"
    R2_PATH="r2:${R2_BUCKET}/${R2_PREFIX}"
    cat > /tmp/yanklog.flatpakrepo << 'EOF'
[Flatpak Remote]
Name=yanklog
Url=https://downloads.yanklog.com/linux/flatpak
EOF

    rclone copyto /tmp/yanklog.flatpakrepo \
        "${R2_PATH}/yanklog.flatpakrepo" \
        --cache-control "public,max-age=3600"
    log "Uploaded: yanklog.flatpakrepo"
fi

log ""
log "=== Publish complete ==="
if [ "$SKIP_APPIMAGE" -eq 0 ]; then
    log "  AppImages:  https://downloads.yanklog.com/linux/yanklog-${VERSION}-linux-<arch>.AppImage"
fi
if [ "$SKIP_FLATPAK" -eq 0 ]; then
    log "  Flatpak:    https://downloads.yanklog.com/linux/flatpak/"
    log "  Install:    flatpak remote-add --if-not-exists yanklog https://downloads.yanklog.com/linux/flatpak/yanklog.flatpakrepo"
fi
