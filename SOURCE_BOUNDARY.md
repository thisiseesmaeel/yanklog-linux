# Source Boundary

yanklog uses a split source model.

## Open Source

These paths are licensed under the MIT License in `LICENSES/MIT.txt`:

- `crates/yanklog-core/`
- `apps/linux/`
- `assets/`
- `com.yanklog.app.desktop`
- `.github/workflows/build-appimages.yml`
- `build-appimage.sh`
- `install.sh`
- `uninstall.sh`
- `PACKAGING_LINUX.md`
- `REPRODUCIBLE_BUILDS.md`
- `packaging/flatpak/`

This covers the shared Rust core, native Linux app, Linux installer/uninstaller, AppImage build workflow, build provenance documentation, and starter Flatpak packaging.

## Proprietary

All other source, packaging, marketing, website, screenshot, release, and macOS code remains proprietary unless a file or directory states otherwise. This includes:

- `apps/macos/`
- `app-store-screenshots/`
- `tools/`
- macOS release and publishing scripts
- R2 publishing scripts
- website source in `../yanklog-web/`

## Publishing Notes

When publishing the Linux/core source publicly, use this file as the source-of-truth for what is open source. If the repo is split later, copy `LICENSES/MIT.txt` into the public Linux/core repository root and remove private paths from the public export.
