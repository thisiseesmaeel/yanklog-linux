# yanklog Linux

Open-source Linux app and shared Rust core for yanklog.

YankLog 2.0 includes complete-history paging, a redesigned keyboard-first Quick Pick,
password-encrypted backups, Secret Service key protection, sensitive-value filters,
timed pauses, safer clearing with keep-pinned support, deletion undo, onboarding,
automatic settings saving, and native tray integration.

This repository contains:

- `crates/yanklog-core`: shared Rust backend
- `apps/linux`: native Linux app built with Rust, GTK4, and libadwaita
- Linux installer, AppImage and Flatpak build workflows, reproducible build notes, and Flathub-ready packaging

## License

The Linux app and shared Rust core are licensed under the MIT License. See `LICENSES/MIT.txt`.

## Install

Recommended install path:

```sh
curl -fsSLO https://downloads.yanklog.com/install.sh
less install.sh
sh install.sh --no-path-update
```

## Development

On Ubuntu or another supported Linux desktop:

```sh
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libdbus-1-dev
cargo run -p yanklog-linux-native
``
```
