#!/bin/sh
set -eu

VERSION=""
X86_64_INPUT=""
AARCH64_INPUT=""
RELEASE_NOTES_PATH=""
R2_BUCKET="${YANKLOG_R2_BUCKET:-yanklog-downloads}"
R2_PREFIX="linux"

usage() {
    cat <<'EOF'
Publish yanklog Linux AppImage release artifacts to Cloudflare R2.

Usage:
  ./publish-web-release.sh --version <version> --x86_64 <file> --aarch64 <file> [options]

Required:
  --version <version>      Release version (example: 2.0.2)
  --x86_64 <file>          GitHub Actions ZIP or x86_64 AppImage
  --aarch64 <file>         GitHub Actions ZIP or aarch64 AppImage

Options:
  --release-notes <file>  Plain-text notes shown by the in-app updater
  --r2-bucket <name>       Cloudflare R2 bucket (default: yanklog-downloads)
  -h, --help               Show this help message

Each input must contain these files for the supplied version and architecture:
  yanklog-<version>-linux-<arch>.AppImage
  yanklog-<version>-linux-<arch>.AppImage.sha256
  yanklog-<version>-linux-<arch>.AppImage.buildinfo
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

sha256_of_file() {
    file_path="$1"
    if have_cmd sha256sum; then
        sha256sum "$file_path" | awk '{print $1}'
    elif have_cmd shasum; then
        shasum -a 256 "$file_path" | awk '{print $1}'
    else
        die "Neither sha256sum nor shasum is installed"
    fi
}

validate_checksum() {
    artifact_path="$1"
    checksum_path="$2"
    expected="$(awk 'NF { print $1; exit }' "$checksum_path")"

    printf '%s\n' "$expected" | grep -Eq '^[0-9A-Fa-f]{64}$' || \
        die "Invalid SHA-256 checksum: $checksum_path"

    actual="$(sha256_of_file "$artifact_path")"
    [ "$(printf '%s' "$expected" | tr '[:upper:]' '[:lower:]')" = "$actual" ] || \
        die "Checksum does not match: $artifact_path"
}

r2_get_text() {
    bucket="$1"
    object_name="$2"
    output="$3"

    npx wrangler r2 object get "${bucket}/${object_name}" --remote --file "$output" >/dev/null 2>&1 || return 1
}

r2_put_file() {
    bucket="$1"
    object_name="$2"
    file_path="$3"
    content_type="$4"
    cache_control="$5"

    npx wrangler r2 object put "${bucket}/${object_name}" \
        --remote \
        --file "$file_path" \
        --content-type "$content_type" \
        --cache-control "$cache_control"
}

r2_delete_if_present() {
    bucket="$1"
    object_name="$2"

    npx wrangler r2 object delete "${bucket}/${object_name}" --remote >/dev/null 2>&1 || true
}

release_name() {
    arch="$1"
    printf 'yanklog-%s-linux-%s.AppImage\n' "$VERSION" "$arch"
}

find_single_file() {
    directory="$1"
    filename="$2"
    matches="$(find "$directory" -type f -name "$filename" -print)"
    match_count="$(printf '%s\n' "$matches" | awk 'NF { count++ } END { print count + 0 }')"

    [ "$match_count" -eq 1 ] || die "Expected exactly one ${filename} in $directory; found $match_count"
    printf '%s\n' "$matches"
}

prepare_architecture() {
    arch="$1"
    input="$2"
    artifact_name="$(release_name "$arch")"

    input_path="$(resolve_file "$input")" || die "Input not found: $input"
    input_base="$(basename -- "$input_path")"

    if [ "$input_base" = "$artifact_name" ]; then
        artifact_path="$input_path"
        checksum_path="${artifact_path}.sha256"
        buildinfo_path="${artifact_path}.buildinfo"
    elif case "$input_base" in *.zip) true ;; *) false ;; esac; then
        have_cmd unzip || die "unzip is required to read GitHub Actions ZIP artifacts"
        extract_dir="$TMP_DIR/$arch"
        mkdir -p "$extract_dir"
        unzip -q "$input_path" -d "$extract_dir" || die "Could not extract ZIP: $input_path"
        artifact_path="$(find_single_file "$extract_dir" "$artifact_name")"
        checksum_path="$(find_single_file "$extract_dir" "${artifact_name}.sha256")"
        buildinfo_path="$(find_single_file "$extract_dir" "${artifact_name}.buildinfo")"
    else
        die "--${arch} must be $artifact_name or a GitHub Actions ZIP containing it"
    fi

    [ -f "$checksum_path" ] || die "Checksum not found: $checksum_path"
    [ -f "$buildinfo_path" ] || die "Build info not found: $buildinfo_path"
    validate_checksum "$artifact_path" "$checksum_path"

    case "$arch" in
        x86_64)
            X86_64_ARTIFACT="$artifact_path"
            X86_64_CHECKSUM="$checksum_path"
            X86_64_BUILDINFO="$buildinfo_path"
            ;;
        aarch64)
            AARCH64_ARTIFACT="$artifact_path"
            AARCH64_CHECKSUM="$checksum_path"
            AARCH64_BUILDINFO="$buildinfo_path"
            ;;
    esac
}

delete_release_artifacts() {
    release_version="$1"
    prefix="$2"

    [ -n "$release_version" ] || return 0
    for arch in x86_64 aarch64; do
        artifact="yanklog-${release_version}-linux-${arch}.AppImage"
        r2_delete_if_present "$R2_BUCKET" "${prefix}${artifact}"
        r2_delete_if_present "$R2_BUCKET" "${prefix}${artifact}.sha256"
        r2_delete_if_present "$R2_BUCKET" "${prefix}${artifact}.buildinfo"
    done
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            [ "$#" -ge 2 ] || die "--version requires a value"
            VERSION="$2"
            shift 2
            ;;
        --x86_64)
            [ "$#" -ge 2 ] || die "--x86_64 requires a file"
            X86_64_INPUT="$2"
            shift 2
            ;;
        --aarch64)
            [ "$#" -ge 2 ] || die "--aarch64 requires a file"
            AARCH64_INPUT="$2"
            shift 2
            ;;
        --release-notes)
            [ "$#" -ge 2 ] || die "--release-notes requires a file"
            RELEASE_NOTES_PATH="$2"
            shift 2
            ;;
        --r2-bucket)
            [ "$#" -ge 2 ] || die "--r2-bucket requires a value"
            R2_BUCKET="$2"
            shift 2
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

[ -n "$VERSION" ] || die "--version is required"
[ -n "$X86_64_INPUT" ] || die "--x86_64 is required"
[ -n "$AARCH64_INPUT" ] || die "--aarch64 is required"
case "$VERSION" in
    *[!0-9A-Za-z.+_-]*|.*|-*|*/*|*..*) die "Invalid version: $VERSION" ;;
esac
have_cmd npx || die "npx is required (install Node.js and Wrangler first)"

if [ -n "$RELEASE_NOTES_PATH" ]; then
    RELEASE_NOTES_SRC="$(resolve_file "$RELEASE_NOTES_PATH")" || die "Release notes not found: $RELEASE_NOTES_PATH"
fi

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/yanklog-linux-release.XXXXXX")"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

prepare_architecture x86_64 "$X86_64_INPUT"
prepare_architecture aarch64 "$AARCH64_INPUT"

PREVIOUS_VERSION=""
if r2_get_text "$R2_BUCKET" "${R2_PREFIX}/latest-version.txt" "$TMP_DIR/previous-version.txt"; then
    PREVIOUS_VERSION="$(tr -d ' \t\r\n' < "$TMP_DIR/previous-version.txt")"
fi
printf '%s\n' "$VERSION" > "$TMP_DIR/latest-version.txt"

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
[ -f "$SCRIPT_DIR/install.sh" ] || die "install.sh not found next to this script"
[ -f "$SCRIPT_DIR/uninstall.sh" ] || die "uninstall.sh not found next to this script"

log "Uploading Linux release $VERSION to R2 bucket: $R2_BUCKET"
for arch in x86_64 aarch64; do
    artifact_name="$(release_name "$arch")"
    case "$arch" in
        x86_64)
            artifact="$X86_64_ARTIFACT"; checksum="$X86_64_CHECKSUM"; buildinfo="$X86_64_BUILDINFO"
            ;;
        aarch64)
            artifact="$AARCH64_ARTIFACT"; checksum="$AARCH64_CHECKSUM"; buildinfo="$AARCH64_BUILDINFO"
            ;;
    esac
    r2_put_file "$R2_BUCKET" "${R2_PREFIX}/${artifact_name}" "$artifact" "application/octet-stream" "public, max-age=31536000, immutable"
    r2_put_file "$R2_BUCKET" "${R2_PREFIX}/${artifact_name}.sha256" "$checksum" "text/plain" "public, max-age=31536000, immutable"
    r2_put_file "$R2_BUCKET" "${R2_PREFIX}/${artifact_name}.buildinfo" "$buildinfo" "text/plain" "public, max-age=31536000, immutable"
done
r2_put_file "$R2_BUCKET" "${R2_PREFIX}/latest-version.txt" "$TMP_DIR/latest-version.txt" "text/plain" "no-cache"
if [ -n "$RELEASE_NOTES_PATH" ]; then
    r2_put_file "$R2_BUCKET" "${R2_PREFIX}/release-notes-${VERSION}.txt" "$RELEASE_NOTES_SRC" "text/plain" "public, max-age=31536000, immutable"
fi
r2_put_file "$R2_BUCKET" "install.sh" "$SCRIPT_DIR/install.sh" "text/x-shellscript" "no-cache"
r2_put_file "$R2_BUCKET" "uninstall.sh" "$SCRIPT_DIR/uninstall.sh" "text/x-shellscript" "no-cache"

if [ -n "$PREVIOUS_VERSION" ] && [ "$PREVIOUS_VERSION" != "$VERSION" ]; then
    log "Deleting previous Linux R2 artifacts for $PREVIOUS_VERSION"
    delete_release_artifacts "$PREVIOUS_VERSION" "${R2_PREFIX}/"
    r2_delete_if_present "$R2_BUCKET" "${R2_PREFIX}/release-notes-${PREVIOUS_VERSION}.txt"
fi

# Older releases used the R2 root instead of the linux/ prefix.
delete_release_artifacts "$PREVIOUS_VERSION" ""
delete_release_artifacts "$VERSION" ""
if [ -n "$PREVIOUS_VERSION" ]; then
    r2_delete_if_present "$R2_BUCKET" "release-notes-${PREVIOUS_VERSION}.txt"
fi
r2_delete_if_present "$R2_BUCKET" "release-notes-${VERSION}.txt"
r2_delete_if_present "$R2_BUCKET" "latest-linux-version.txt"
r2_delete_if_present "$R2_BUCKET" "linux-artifact-base-url.txt"

log "Done. Published:"
for arch in x86_64 aarch64; do
    artifact_name="$(release_name "$arch")"
    log "  ${R2_PREFIX}/${artifact_name}"
    log "  ${R2_PREFIX}/${artifact_name}.sha256"
    log "  ${R2_PREFIX}/${artifact_name}.buildinfo"
done
log "  ${R2_PREFIX}/latest-version.txt"
if [ -n "$RELEASE_NOTES_PATH" ]; then
    log "  ${R2_PREFIX}/release-notes-${VERSION}.txt"
fi
log "  install.sh"
log "  uninstall.sh"
