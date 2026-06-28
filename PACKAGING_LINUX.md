# Linux Packaging And Publishing

This file is the release checklist for the native Linux app.

## What Gets Published

The Linux release artifact is an AppImage per architecture:

- `yanklog-<version>-linux-x86_64.AppImage`
- `yanklog-<version>-linux-x86_64.AppImage.sha256`
- `yanklog-<version>-linux-x86_64.AppImage.buildinfo`
- `yanklog-<version>-linux-aarch64.AppImage`
- `yanklog-<version>-linux-aarch64.AppImage.sha256`
- `yanklog-<version>-linux-aarch64.AppImage.buildinfo`

The public installer reads metadata and artifacts from Cloudflare R2:

- `https://downloads.yanklog.com/install.sh`
- `https://downloads.yanklog.com/uninstall.sh`
- `https://downloads.yanklog.com/linux/latest-version.txt`
- `https://downloads.yanklog.com/linux/yanklog-<version>-linux-<arch>.AppImage`
- `https://downloads.yanklog.com/linux/yanklog-<version>-linux-<arch>.AppImage.sha256`
- `https://downloads.yanklog.com/linux/yanklog-<version>-linux-<arch>.AppImage.buildinfo`

Recommended public install command:

```sh
curl -fsSLO https://downloads.yanklog.com/install.sh
less install.sh
sh install.sh --no-path-update
```

## GitHub Actions Build

Use GitHub Actions for public Linux builds. This avoids depending on a local Linux machine for release artifacts.

The AppImage jobs run inside an Ubuntu 22.04 container, even when the host runner is newer. This keeps the public AppImage baseline at roughly `glibc 2.35`, so it should run on Ubuntu 22.04 and newer distributions while still working on current distros.

Manual artifact build:

1. Push the commit you want GitHub to build.
2. Open the repository on GitHub.
3. Go to `Actions`.
4. Select `Build AppImages`.
5. Click `Run workflow`.
6. Leave `version` empty to use `apps/linux/Cargo.toml`, or enter a version such as `1.2.0`.
7. Download both workflow artifacts when the jobs finish:
   - `yanklog-appimage-x86_64.zip`
   - `yanklog-appimage-aarch64.zip`

GitHub downloads workflow artifacts as ZIP files. The publish script accepts those ZIPs directly and extracts the contained AppImage plus checksum.

Release build:

```sh
git tag v1.2.0
git push origin v1.2.0
```

Pushing a tag that starts with `v` creates a GitHub Release and uploads both AppImages automatically.

Normal branch pushes do not build AppImages. The workflow only runs manually or on `v*` tags.

## Local Linux Build

For local testing on Linux:

```sh
sudo apt update
sudo apt install -y build-essential pkg-config libgtk-4-dev libadwaita-1-dev libayatana-appindicator3-dev
./script/build_linux_dev.sh
```

For a local AppImage build on Linux:

```sh
./build-appimage.sh --version 1.2.0
```

Output:

```text
dist/appimage/yanklog-1.2.0-linux-<arch>.AppImage
dist/appimage/yanklog-1.2.0-linux-<arch>.AppImage.sha256
dist/appimage/yanklog-1.2.0-linux-<arch>.AppImage.buildinfo
```

The script updates Cargo package versions when `--version` differs from the current manifest version.

## R2 Publishing

After you have both architecture artifacts, publish them to R2. If you downloaded artifacts from GitHub Actions, pass the ZIP files directly:

```sh
./publish-web-release.sh \
  --version 1.2.0 \
  --x86_64 ~/Downloads/yanklog-appimage-x86_64.zip \
  --aarch64 ~/Downloads/yanklog-appimage-aarch64.zip
```

Raw local AppImage files are also supported:

```sh
./publish-web-release.sh \
  --version 1.2.0 \
  --x86_64 dist/yanklog-1.2.0-linux-x86_64.AppImage \
  --aarch64 dist/yanklog-1.2.0-linux-aarch64.AppImage
```

Use a non-default bucket only when needed:

```sh
./publish-web-release.sh \
  --version 1.2.0 \
  --x86_64 dist/yanklog-1.2.0-linux-x86_64.AppImage \
  --aarch64 dist/yanklog-1.2.0-linux-aarch64.AppImage \
  --r2-bucket yanklog-downloads
```

The publish script:

- accepts either GitHub Actions artifact ZIP files or raw local AppImage files;
- extracts ZIP inputs and validates that each contains one AppImage and one checksum;
- uploads the Linux AppImages, checksums, and build info files to `linux/` in Cloudflare R2;
- writes `linux/latest-version.txt` to Cloudflare R2;
- writes `install.sh` and `uninstall.sh` to the R2 root;
- deletes the previous published Linux version from R2 after the new upload succeeds;
- removes stale flat Linux objects from older publishing layouts;
- does not touch `yanklog-web`.

## User Updates

Users update by running the same installer again:

```sh
curl -fsSLO https://downloads.yanklog.com/install.sh
less install.sh
sh install.sh --no-path-update
```

Or from an installed app:

```sh
yanklog --update
```

The installer downloads the version from R2 `linux/latest-version.txt`, then downloads the AppImage and checksum from R2, verifies the checksum, and replaces the installed AppImage.

Build provenance is published next to each AppImage as `.buildinfo`. See [REPRODUCIBLE_BUILDS.md](REPRODUCIBLE_BUILDS.md).

## Flatpak / Flathub

Starter Flatpak packaging lives in `packaging/flatpak/`.

The current manifest is for local testing from this checkout. Before submitting to Flathub, publish the Linux/core source, switch the manifest source to a tagged public Git URL, generate Cargo source metadata, and complete a sandbox review.

## Linux Compatibility

The AppImage release target is:

- Ubuntu 22.04+
- Debian 12+
- Linux Mint 21+
- Fedora 36+
- other distributions with `glibc 2.35` or newer

Ubuntu 18.04 and Ubuntu 20.04 are not supported by the AppImage build because their system glibc is older than the release baseline. AppImages can bundle GTK/libadwaita libraries, but they should not bundle glibc itself.

## Quick Picker Shortcut

Linux desktop environments own global shortcut registration. The app exposes the picker as a command, but users must bind it in their desktop keyboard settings.

The installer writes:

- `~/.local/share/applications/com.yanklog.app.desktop`
- `~/.local/share/applications/com.yanklog.app.quickpick.desktop`

The main desktop entry appears in app launchers as `YankLog` and includes a `Quick Pick` action. The quick-pick desktop entry launches the picker directly and stays hidden from normal app menus. The command shown after install is the safest command to bind because it uses the absolute installed binary path:

```sh
~/.local/bin/yanklog --pick
```

If a custom install directory is used, bind the command printed by the installer instead. In the app, `Settings > Shortcut setup` also shows the resolved command and includes copy/test buttons.
