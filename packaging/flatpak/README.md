# Flatpak Packaging

This directory contains the upstream Flatpak packaging for YankLog. The Flatpak ID is
`com.yanklog.YankLog`; the older starter ID `com.yanklog.app` must not be submitted.

## Build

From this directory:

```sh
flatpak-builder --force-clean build-dir com.yanklog.YankLog.yml
flatpak-builder --user --install --force-clean build-dir com.yanklog.YankLog.yml
flatpak run com.yanklog.YankLog
```

Install the matching runtime, SDK, and Rust extension first:

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user flathub org.gnome.Platform//50 org.gnome.Sdk//50 org.freedesktop.Sdk.Extension.rust-stable//25.08
```

The upstream manifest builds from this checkout and uses `generated-sources.json` for
offline Cargo dependencies. Flathub's submission copy must replace the `dir` source
with the public release tag and its exact commit.

## Sandbox Permissions

- `--socket=x11` and `--share=ipc` are required because the current clipboard backend
  uses X11. On Wayland desktops, YankLog runs through XWayland.
- `--device=dri` enables accelerated GTK rendering.
- `org.freedesktop.secrets` stores the encrypted database key in Secret Service.
- `org.kde.StatusNotifierWatcher` exposes the tray/status-notifier menu.

The Flatpak has no network or host-filesystem permission. Backup import and export use
the GTK file chooser portal. Self-updates and host autostart-file writes are disabled;
updates are delivered by Flatpak.

## Regenerate Cargo Sources

After `Cargo.lock` changes, regenerate `generated-sources.json`:

```sh
curl -fsSLO https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
python3 -m pip install --user tomlkit aiohttp
python3 flatpak-cargo-generator.py ../../Cargo.lock -o generated-sources.json
rm flatpak-cargo-generator.py
```

## Flathub Submission Source

After the release is committed, pushed, and tagged, copy the manifest and generated
sources into a clean directory named after the Flatpak ID. Replace the main source with:

```yaml
sources:
  - type: git
    url: https://github.com/thisiseesmaeel/yanklog-linux.git
    tag: v1.3.0
    commit: <exact-tag-commit>
    x-checker-data:
      type: git
      tag-pattern: ^v([\d.]+)$
  - generated-sources.json
```

Only the manifest and generated dependency manifest belong in the Flathub submission
repository. Metadata, desktop files, icons, source code, and binaries remain upstream.
