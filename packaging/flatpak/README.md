# Flatpak Packaging

This directory contains starter Flatpak packaging for the open-source Linux app.

It is not a submitted Flathub manifest yet. Flathub submission still needs:

- a public Git repository URL for the open-source Linux/core source;
- a signed or immutable release tag;
- generated Cargo source metadata, usually produced with `flatpak-cargo-generator.py`;
- final sandbox review for clipboard, tray/status-notifier, and desktop integration behavior;
- published screenshots and app metadata that match Flathub requirements.

## Local Test Build

From this directory:

```sh
flatpak-builder --force-clean build-dir com.yanklog.app.yml
flatpak-builder --user --install --force-clean build-dir com.yanklog.app.yml
flatpak run com.yanklog.app
```

The local manifest builds from the repository checkout with:

```yaml
sources:
  - type: dir
    path: ../..
```

For Flathub, replace that source with a public tagged Git source and add generated Cargo sources.

## Flathub Conversion Sketch

After publishing the Linux/core source:

```yaml
sources:
  - type: git
    url: https://github.com/<owner>/<repo>.git
    tag: v1.2.0
    commit: <release-commit>
  - generated-sources.json
```

Generate Cargo sources from the public `Cargo.lock` before submitting.
