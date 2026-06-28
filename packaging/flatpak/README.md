# Flatpak Packaging

This directory contains Flatpak packaging for the open-source Linux app.

It is not submitted to Flathub yet. Flathub submission still needs:

- a pushed public Git commit matching the manifest `commit`;
- a release tag, if you want the manifest to use `tag` as well as `commit`;
- final sandbox review for clipboard, tray/status-notifier, and desktop integration behavior on Linux;
- published screenshots and app metadata that match Flathub requirements.

## Build

From this directory:

```sh
flatpak-builder --force-clean build-dir com.yanklog.app.yml
flatpak-builder --user --install --force-clean build-dir com.yanklog.app.yml
flatpak run com.yanklog.app
```

The manifest builds from the public Git source pinned in `com.yanklog.app.yml` and uses `generated-sources.json` for vendored Cargo dependencies.

## Regenerate Cargo Sources

After `Cargo.lock` changes, regenerate `generated-sources.json`:

```sh
curl -fsSLO https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
python3 -m pip install --user tomlkit aiohttp
python3 flatpak-cargo-generator.py ../../Cargo.lock -o generated-sources.json
rm flatpak-cargo-generator.py
```

## Release Pinning

The current manifest pins an exact commit:

```yaml
sources:
  - type: git
    url: https://github.com/thisiseesmaeel/yanklog-linux.git
    commit: 6ea60b642e99c95007fd29e924d8b53be933dd81
  - generated-sources.json
```

After creating a release tag, add it next to the commit:

```yaml
tag: v1.2.0
```
