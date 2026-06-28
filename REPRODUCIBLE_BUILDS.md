# Reproducible Builds And Provenance

This document describes how Linux AppImage releases are built and how users can compare a local rebuild with a published artifact.

## Release Build Environment

Official Linux AppImages are built by `.github/workflows/build-appimages.yml`.

- GitHub-hosted runner: `ubuntu-24.04` for x86_64, `ubuntu-24.04-arm` for aarch64.
- Build container: `ubuntu:22.04`.
- Rust toolchain: stable, installed by `dtolnay/rust-toolchain`.
- GTK/libadwaita baseline: Ubuntu 22.04 packages.
- AppImage bundling: `linuxdeploy` continuous AppImage for the target architecture.

The Ubuntu 22.04 container is intentional. It keeps the public AppImage baseline around `glibc 2.35`, which matches Ubuntu 22.04, Debian 12, Linux Mint 21, Fedora 36, and newer distributions.

## Published Files

For each Linux release and architecture, publish:

```text
linux/yanklog-<version>-linux-<arch>.AppImage
linux/yanklog-<version>-linux-<arch>.AppImage.sha256
linux/yanklog-<version>-linux-<arch>.AppImage.buildinfo
```

The `.buildinfo` file records:

- release version
- artifact name and SHA-256
- target architecture
- source commit
- dirty-file count at build time
- `Cargo.lock` SHA-256
- Rust and Cargo versions
- build OS
- `linuxdeploy` SHA-256
- build command

## Local Rebuild

On a matching Linux host or Ubuntu 22.04 container:

```sh
sudo apt update
sudo apt install -y \
  build-essential \
  ca-certificates \
  curl \
  desktop-file-utils \
  file \
  fuse \
  git \
  libadwaita-1-dev \
  libayatana-appindicator3-dev \
  libgtk-4-dev \
  librsvg2-bin \
  patchelf \
  pkg-config

./build-appimage.sh --version 1.2.0 --arch x86_64
```

Then compare:

```sh
cat dist/appimage/yanklog-1.2.0-linux-x86_64.AppImage.sha256
cat dist/appimage/yanklog-1.2.0-linux-x86_64.AppImage.buildinfo
```

## Current Limitations

The AppImage pipeline is provenance-backed, but not guaranteed bit-for-bit reproducible yet. Known sources of variation include:

- `linuxdeploy` continuous builds can change over time.
- Rust stable can change unless pinned to a specific toolchain version.
- Ubuntu package versions can change unless the build container and apt repository snapshots are pinned.
- AppImage metadata can include timestamps unless every tool in the chain honors deterministic settings.

To tighten this further, pin the Rust toolchain, pin `linuxdeploy` by version and checksum, use an apt snapshot, set `SOURCE_DATE_EPOCH` from the release commit, and compare rebuilds in CI before publishing.
