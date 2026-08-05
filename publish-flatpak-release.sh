#!/bin/sh
# Publish a flatpak repository to Cloudflare R2.
# Designed to run in CI after flatpak-builder has produced a repo directory.
set -eu

REPO_DIR=""
R2_BUCKET="${YANKLOG_R2_BUCKET:-yanklog-downloads}"
R2_PREFIX="linux/flatpak"
RCLONE_REMOTE="r2"
GPG_KEY=""
GPG_HOMEDIR=""
PRUNE_DAYS=30
GENERATE_DELTAS=1
DRY_RUN=0

usage() {
    cat <<'EOF'
Publish a flatpak repository to Cloudflare R2.

Usage:
  ./publish-flatpak-release.sh --repo-dir <path> [options]

Required:
  --repo-dir <path>        Path to the flatpak repository directory

Options:
  --r2-bucket <name>       Cloudflare R2 bucket (default: yanklog-downloads)
  --r2-prefix <prefix>     R2 key prefix (default: linux/flatpak)
  --rclone-remote <name>   rclone remote name for R2 (default: r2)
  --gpg-key <key-id>       GPG key ID to sign the repository
  --gpg-homedir <path>     GPG home directory for the signing key
  --prune-days <n>         Prune refs not updated in N days (default: 30, 0=skip)
  --no-prune               Skip pruning
  --no-deltas              Skip generating static deltas
  --dry-run                Show what would be uploaded without writing to R2
  -h, --help               Show this help message

The repository should be created using:
  flatpak build-repo <repo-dir>
  flatpak build-import-bundle <repo-dir> *.flatpak  # one bundle per call

Or: flatpak build-export <repo-dir> <app-id>

Before running, configure rclone for R2 (or pass config via env vars):
  rclone config create r2 s3 \\
      provider=Other s3_provider=Other \\
      endpoint=<account-id>.r2.cloudflarestorage.com \\
      region=auto \\
      access_key_id=<r2-access-key-id> \\
      secret_access_key=<r2-secret-access-key>

In CI, rclone auto-discovers the "r2" remote from:
  RCLONE_CONFIG_R2_TYPE=s3
  RCLONE_CONFIG_R2_PROVIDER=Other
  RCLONE_CONFIG_R2_S3_PROVIDER=Other
  RCLONE_CONFIG_R2_ENDPOINT=...
  RCLONE_CONFIG_R2_ACCESS_KEY_ID=...
  RCLONE_CONFIG_R2_SECRET_ACCESS_KEY=...

Cache headers applied:
  objects/    -> public, max-age=31536000, immutable  (1 year)
  deltas/     -> public, max-age=86400                (1 day)
  refs/       -> public, max-age=60                   (1 minute)
  config      -> public, max-age=60                   (1 minute)
  summary     -> no-cache                              (always fresh)
  summary.sig -> no-cache                              (always fresh)

Upload order is atomic: objects and deltas are uploaded first,
then summary/summary.sig are written last so clients never see
a summary that references missing objects.
EOF
}

log()  { printf '%s\n' "$*"; }
die()  { printf 'Error: %s\n' "$*" >&2; exit 1; }
have_cmd() { command -v "$1" >/dev/null 2>&1; }

resolve_dir() {
    target="$1"
    if [ -d "$target" ]; then
        (CDPATH= cd -- "$target" && pwd)
    else
        return 1
    fi
}

rclone_sync_cmd() {
    src="$1" dst="$2" cache="$3" extra="$4"
    if [ "$DRY_RUN" -eq 1 ]; then
        log "  [dry-run] rclone sync $src -> $dst (cache-control: $cache)"
    else
        rclone sync "$src" "$dst" \
            --cache-control "$cache" \
            --s3-no-check-bucket \
            --transfers 16 \
            --tpslimit 0 \
            $extra
    fi
}

rclone_copy_cmd() {
    src="$1" dst="$2" cache="$3" extra="$4"
    if [ "$DRY_RUN" -eq 1 ]; then
        log "  [dry-run] rclone copy $src -> $dst (cache-control: $cache)"
    else
        rclone copy "$src" "$dst" \
            --cache-control "$cache" \
            --s3-no-check-bucket \
            --transfers 16 \
            --tpslimit 0 \
            $extra
    fi
}

rclone_put_cmd() {
    src="$1" dst="$2" cache="$3"
    if [ "$DRY_RUN" -eq 1 ]; then
        log "  [dry-run] rclone copyto $src -> $dst (cache-control: $cache)"
    else
        rclone copyto "$src" "$dst" \
            --cache-control "$cache" \
            --s3-no-check-bucket
    fi
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --repo-dir)
            [ "$#" -ge 2 ] || die "--repo-dir requires a value"
            REPO_DIR="$(resolve_dir "$2")" || die "Repository directory not found: $2"
            shift 2
            ;;
        --r2-bucket)
            [ "$#" -ge 2 ] || die "--r2-bucket requires a value"
            R2_BUCKET="$2"; shift 2 ;;
        --r2-prefix)
            [ "$#" -ge 2 ] || die "--r2-prefix requires a value"
            R2_PREFIX="$2"; shift 2 ;;
        --rclone-remote)
            [ "$#" -ge 2 ] || die "--rclone-remote requires a value"
            RCLONE_REMOTE="$2"; shift 2 ;;
        --gpg-key)
            [ "$#" -ge 2 ] || die "--gpg-key requires a value"
            GPG_KEY="$2"; shift 2 ;;
        --gpg-homedir)
            [ "$#" -ge 2 ] || die "--gpg-homedir requires a value"
            GPG_HOMEDIR="$2"; shift 2 ;;
        --prune-days)
            [ "$#" -ge 2 ] || die "--prune-days requires a value"
            PRUNE_DAYS="$2"; shift 2 ;;
        --no-prune)
            PRUNE_DAYS=0; shift ;;
        --no-deltas)
            GENERATE_DELTAS=0; shift ;;
        --dry-run)
            DRY_RUN=1; shift ;;
        -h|--help)
            usage; exit 0 ;;
        *)
            die "Unknown option: $1" ;;
    esac
done

# Validate required inputs
[ -n "$REPO_DIR" ] || die "--repo-dir is required"
[ -d "$REPO_DIR/objects" ] || die "Not a flatpak repo: $REPO_DIR/objects missing"
[ -f "$REPO_DIR/summary" ] || die "Not a flatpak repo: $REPO_DIR/summary missing"

have_cmd rclone || die "rclone is required for R2 sync (install from https://rclone.org)"
have_cmd flatpak || die "flatpak CLI is required (flatpak build-export, build-sign, build-update-repo)"

R2_PATH="${RCLONE_REMOTE}:${R2_BUCKET}/${R2_PREFIX}"

# --- Optional repository maintenance (local) ---

if [ -n "$GPG_KEY" ]; then
    log "Signing repository objects with GPG key: $GPG_KEY"
    SIGN_ARGS="--gpg-sign=${GPG_KEY}"
    [ -z "$GPG_HOMEDIR" ] || SIGN_ARGS="$SIGN_ARGS --gpg-homedir=${GPG_HOMEDIR}"
    flatpak build-sign $SIGN_ARGS "$REPO_DIR"
    flatpak build-update-repo --sign $SIGN_ARGS "$REPO_DIR"
fi

if [ "$PRUNE_DAYS" -gt 0 ]; then
    log "Pruning references older than ${PRUNE_DAYS} days..."
    flatpak build-update-repo --prune --prune-from-days "$PRUNE_DAYS" "$REPO_DIR"
fi

if [ "$GENERATE_DELTAS" -eq 1 ]; then
    log "Generating static deltas..."
    flatpak build-update-repo --generate-delta-repo "$REPO_DIR" || true
fi

# --- Upload to R2 (atomic ordering: objects first, summary last) ---

log "Publishing flatpak repository to R2..."
log "  Bucket: $R2_BUCKET"
log "  Prefix: $R2_PREFIX"
log ""

# Step 1 — immutable OSTree objects (longest cache)
log "Uploading OSTree objects..."
rclone_sync_cmd \
    "${REPO_DIR}/objects/" "${R2_PATH}/objects/" \
    "public,max-age=31536000,immutable" \
    "--checksum"

# Step 2 — static deltas (if present)
if [ -d "${REPO_DIR}/deltas" ]; then
    log "Uploading static deltas..."
    rclone_sync_cmd \
        "${REPO_DIR}/deltas/" "${R2_PATH}/deltas/" \
        "public,max-age=86400" \
        "--checksum --create-empty-src-dirs"
fi

# Step 3 — refs (sync so deleted refs are removed on R2)
log "Uploading refs..."
rclone_sync_cmd \
    "${REPO_DIR}/refs/" "${R2_PATH}/refs/" \
    "public,max-age=60" \
    "--checksum"

# Step 4 — config
if [ -f "${REPO_DIR}/config" ]; then
    log "Uploading config..."
    rclone_put_cmd "${REPO_DIR}/config" "${R2_PATH}/config" "public,max-age=60"
fi

# Step 5 — summary + signature (uploaded LAST so the repo is atomic)
log "Uploading summary (atomic publish - last step)..."
rclone_put_cmd "${REPO_DIR}/summary" "${R2_PATH}/summary" "no-cache"

if [ -f "${REPO_DIR}/summary.sig" ]; then
    rclone_put_cmd "${REPO_DIR}/summary.sig" "${R2_PATH}/summary.sig" "no-cache"
fi

log ""
log "Published repository:"
log "  flatpak remote-add --if-not-exists yanklog https://<your-domain>/${R2_PREFIX}/yanklog.flatpakrepo"
