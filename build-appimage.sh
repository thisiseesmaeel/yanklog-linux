#!/bin/sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$SCRIPT_DIR"

VERSION="${YANKLOG_VERSION:-}"
TARGET_ARCH=""
SKIP_BUILD=0
OUTPUT_DIR="$REPO_ROOT/dist/appimage"
TOOLS_DIR="$REPO_ROOT/dist/tools"

usage() {
    cat <<'EOF'
Build yanklog AppImage on Linux (Ubuntu VM friendly).

Usage:
  ./build-appimage.sh [options]

Options:
  --version <version>      Version to embed in output name (default: Cargo.toml version)
  YANKLOG_VERSION          Environment alternative to --version
  --arch <arch>            Target arch name for artifact naming (x86_64 or aarch64)
  --output-dir <path>      Output directory (default: ./dist/appimage)
  --tools-dir <path>       Tools directory for linuxdeploy (default: ./dist/tools)
  --skip-build             Skip `cargo build --release`
  -h, --help               Show this help message

Output files:
  yanklog-<VERSION>-linux-<ARCH>.AppImage
  yanklog-<VERSION>-linux-<ARCH>.AppImage.sha256
EOF
}

log() {
    printf '%s\n' "$*"
}

die() {
    printf 'Error: %s\n' "$*" >&2
    exit 1
}

have_cmd() {
    command -v "$1" >/dev/null 2>&1
}

cargo_toml_version() {
    awk -F'"' '/^version = "/ { print $2; exit }' "$REPO_ROOT/apps/linux/Cargo.toml"
}

update_cargo_toml_version() {
    current_version="$(cargo_toml_version)"
    [ "$current_version" != "$VERSION" ] || return 0

    for manifest in \
        "$REPO_ROOT/apps/linux/Cargo.toml" \
        "$REPO_ROOT/crates/yanklog-core/Cargo.toml" \
        "$REPO_ROOT/tools/uniffi-bindgen-swift/Cargo.toml"
    do
        [ -f "$manifest" ] || continue
        tmp_file="$(mktemp)"
        if ! awk -v version="$VERSION" '
            BEGIN { in_package = 0; updated = 0 }
            /^\[package\][[:space:]]*$/ { in_package = 1; print; next }
            /^\[/ { in_package = 0 }
            in_package && /^version[[:space:]]*=/ && updated == 0 {
                print "version = \"" version "\""
                updated = 1
                next
            }
            { print }
            END { if (updated == 0) exit 1 }
        ' "$manifest" > "$tmp_file"; then
            rm -f "$tmp_file"
            die "Failed to update version in $manifest"
        fi
        mv "$tmp_file" "$manifest"
    done
    log "Updated Cargo.toml version: $current_version -> $VERSION"
}

resolve_dir() {
    target="$1"
    if [ -d "$target" ]; then
        (CDPATH= cd -- "$target" && pwd)
    else
        return 1
    fi
}

resolve_file() {
    target="$1"
    if [ -f "$target" ]; then
        dir_part="$(dirname -- "$target")"
        base_part="$(basename -- "$target")"
        printf '%s/%s\n' "$(resolve_dir "$dir_part")" "$base_part"
    else
        return 1
    fi
}

write_sha256() {
    file_path="$1"
    out_path="$2"
    if have_cmd sha256sum; then
        sha="$(sha256sum "$file_path" | awk '{print $1}')"
    elif have_cmd shasum; then
        sha="$(shasum -a 256 "$file_path" | awk '{print $1}')"
    else
        die "Neither sha256sum nor shasum is installed"
    fi
    printf '%s\n' "$sha" > "$out_path"
}

sha256_of_file() {
    file_path="$1"
    if have_cmd sha256sum; then
        sha256sum "$file_path" | awk '{print $1}'
    elif have_cmd shasum; then
        shasum -a 256 "$file_path" | awk '{print $1}'
    else
        printf 'unavailable\n'
    fi
}

write_buildinfo() {
    artifact_path="$1"
    checksum_path="$2"
    buildinfo_path="$3"

    source_commit="$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || printf 'unknown')"
    dirty_count="$(git -C "$REPO_ROOT" status --porcelain 2>/dev/null | wc -l | tr -d ' ' || printf 'unknown')"
    rustc_version="$(rustc --version 2>/dev/null || printf 'unavailable')"
    cargo_version="$(cargo --version 2>/dev/null || printf 'unavailable')"
    os_release="$(. /etc/os-release 2>/dev/null && printf '%s %s' "${NAME:-unknown}" "${VERSION_ID:-unknown}" || printf 'unknown')"
    cargo_lock_sha="$(sha256_of_file "$REPO_ROOT/Cargo.lock")"
    linuxdeploy_sha="$(sha256_of_file "$LINUXDEPLOY_BIN")"
    appimage_sha="$(tr -d ' \t\r\n' < "$checksum_path")"

    cat > "$buildinfo_path" <<EOF
name: yanklog
version: $VERSION
artifact: $(basename -- "$artifact_path")
artifact_sha256: $appimage_sha
target_arch: $TARGET_ARCH
source_commit: $source_commit
source_dirty_files: $dirty_count
cargo_lock_sha256: $cargo_lock_sha
rustc: $rustc_version
cargo: $cargo_version
build_os: $os_release
linuxdeploy_sha256: $linuxdeploy_sha
build_command: ./build-appimage.sh --version $VERSION --arch $TARGET_ARCH
EOF
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
            TARGET_ARCH="$2"
            shift 2
            ;;
        --output-dir)
            [ "$#" -ge 2 ] || die "--output-dir requires a value"
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --tools-dir)
            [ "$#" -ge 2 ] || die "--tools-dir requires a value"
            TOOLS_DIR="$2"
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD=1
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

[ "$(uname -s)" = "Linux" ] || die "This script must run on Linux"

HOST_ARCH_RAW="$(uname -m)"
case "$HOST_ARCH_RAW" in
    x86_64|amd64)
        HOST_ARCH="x86_64"
        ;;
    aarch64|arm64)
        HOST_ARCH="aarch64"
        ;;
    *)
        die "Unsupported host architecture: $HOST_ARCH_RAW"
        ;;
esac

if [ -z "$TARGET_ARCH" ]; then
    TARGET_ARCH="$HOST_ARCH"
fi

case "$TARGET_ARCH" in
    x86_64|aarch64)
        ;;
    *)
        die "Unsupported --arch: $TARGET_ARCH (expected x86_64 or aarch64)"
        ;;
esac

[ "$TARGET_ARCH" = "$HOST_ARCH" ] || die "--arch ($TARGET_ARCH) must match host architecture ($HOST_ARCH) for this build"

if [ -z "$VERSION" ]; then
    VERSION="$(cargo_toml_version)"
fi
[ -n "$VERSION" ] || die "Could not resolve version"
case "$VERSION" in
    *[!0-9A-Za-z._-]*)
        die "Invalid version: $VERSION"
        ;;
esac

update_cargo_toml_version

if [ -d "$OUTPUT_DIR" ]; then
    OUTPUT_DIR_ABS="$(resolve_dir "$OUTPUT_DIR")"
else
    mkdir -p "$OUTPUT_DIR"
    OUTPUT_DIR_ABS="$(resolve_dir "$OUTPUT_DIR")"
fi

if [ -d "$TOOLS_DIR" ]; then
    TOOLS_DIR_ABS="$(resolve_dir "$TOOLS_DIR")"
else
    mkdir -p "$TOOLS_DIR"
    TOOLS_DIR_ABS="$(resolve_dir "$TOOLS_DIR")"
fi

BIN_PATH="$REPO_ROOT/target/release/yanklog"
DESKTOP_FILE="$REPO_ROOT/com.yanklog.app.desktop"
ICON_SOURCE_FILE="$REPO_ROOT/assets/logo.svg"

[ -f "$DESKTOP_FILE" ] || die "Missing desktop file: $DESKTOP_FILE"
[ -f "$ICON_SOURCE_FILE" ] || die "Missing icon file: $ICON_SOURCE_FILE"

if [ "$SKIP_BUILD" -eq 0 ]; then
    log "Building release binary with cargo"
    (
        cd "$REPO_ROOT"
        YANKLOG_BUILD_VERSION="$VERSION" cargo build --release -p yanklog-linux-native --bin yanklog
    )
fi
[ -x "$BIN_PATH" ] || die "Release binary not found: $BIN_PATH"

LINUXDEPLOY_BIN="$TOOLS_DIR_ABS/linuxdeploy-${TARGET_ARCH}.AppImage"
if [ ! -x "$LINUXDEPLOY_BIN" ]; then
    case "$TARGET_ARCH" in
        x86_64)
            LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
            ;;
        aarch64)
            LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-aarch64.AppImage"
            ;;
    esac
    log "Downloading linuxdeploy: $LINUXDEPLOY_URL"
    if have_cmd curl; then
        curl -fsSL "$LINUXDEPLOY_URL" -o "$LINUXDEPLOY_BIN"
    elif have_cmd wget; then
        wget -qO "$LINUXDEPLOY_BIN" "$LINUXDEPLOY_URL"
    else
        die "Install curl or wget to download linuxdeploy"
    fi
    chmod 0755 "$LINUXDEPLOY_BIN"
fi

APPDIR="$REPO_ROOT/AppDir"
rm -rf "$APPDIR"

TMP_DIFF_DIR="$(mktemp -d)"
cleanup() {
    rm -rf "$TMP_DIFF_DIR"
}
trap cleanup EXIT INT TERM

# linuxdeploy maps icon name from filename; our desktop file uses Icon=yanklog.
# Create a temporary alias with that filename to avoid icon lookup errors.
ICON_ALIAS_FILE="$TMP_DIFF_DIR/yanklog.svg"
cp "$ICON_SOURCE_FILE" "$ICON_ALIAS_FILE"

find "$REPO_ROOT" -maxdepth 1 -type f -name '*.AppImage' -print | sort > "$TMP_DIFF_DIR/before.txt"

log "Packaging AppImage with linuxdeploy"
(
    cd "$REPO_ROOT"
    export APPIMAGE_EXTRACT_AND_RUN=1
    ARCH="$TARGET_ARCH" "$LINUXDEPLOY_BIN" \
        --appdir "$APPDIR" \
        --executable "$BIN_PATH" \
        --desktop-file "$DESKTOP_FILE" \
        --icon-file "$ICON_ALIAS_FILE" \
        --output appimage
)

find "$REPO_ROOT" -maxdepth 1 -type f -name '*.AppImage' -print | sort > "$TMP_DIFF_DIR/after.txt"
NEW_APPIMAGE="$(comm -13 "$TMP_DIFF_DIR/before.txt" "$TMP_DIFF_DIR/after.txt" | head -n 1 || true)"

if [ -z "$NEW_APPIMAGE" ]; then
    NEW_APPIMAGE="$(ls -1t "$REPO_ROOT"/*.AppImage 2>/dev/null | head -n 1 || true)"
fi
[ -n "$NEW_APPIMAGE" ] || die "linuxdeploy did not produce an AppImage"

NEW_APPIMAGE_ABS="$(resolve_file "$NEW_APPIMAGE")"
TARGET_NAME="yanklog-${VERSION}-linux-${TARGET_ARCH}.AppImage"
TARGET_PATH="$OUTPUT_DIR_ABS/$TARGET_NAME"
CHECKSUM_PATH="${TARGET_PATH}.sha256"
BUILDINFO_PATH="${TARGET_PATH}.buildinfo"

mv "$NEW_APPIMAGE_ABS" "$TARGET_PATH"
chmod 0755 "$TARGET_PATH"
write_sha256 "$TARGET_PATH" "$CHECKSUM_PATH"
write_buildinfo "$TARGET_PATH" "$CHECKSUM_PATH" "$BUILDINFO_PATH"

log "Built AppImage:"
log "  $TARGET_PATH"
log "Checksum:"
log "  $CHECKSUM_PATH"
log "Build info:"
log "  $BUILDINFO_PATH"
