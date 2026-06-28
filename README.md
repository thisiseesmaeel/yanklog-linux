# yanklog Linux

Open-source Linux app and shared Rust core for yanklog.

This repository contains:

- `crates/yanklog-core`: shared Rust backend
- `apps/linux`: native Linux app built with Rust, GTK4, and libadwaita
- Linux installer, AppImage build workflow, reproducible build notes, and Flatpak starter packaging

## License

The Linux app and shared Rust core are licensed under the MIT License. See `LICENSES/MIT.txt`.

## Install

Recommended install path:

```sh
curl -fsSLO https://downloads.yanklog.com/install.sh
less install.sh
sh install.sh --no-path-update
