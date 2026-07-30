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
offline Cargo dependencies. The corresponding Flathub submission copy is in
[`../flathub/com.yanklog.YankLog.yml`](../flathub/com.yanklog.YankLog.yml); it replaces
the `dir` source with the public release tag and its exact commit.

## Sandbox Permissions

- `--socket=wayland` enables the native Wayland clipboard backend on compositors that
  expose the data-control protocol; `--socket=x11` remains as the automatic X11/XWayland
  fallback. `--share=ipc` is required by GTK.
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

Use the two files in [`../flathub/`](../flathub/) for a Flathub submission:

```text
com.yanklog.YankLog.yml
generated-sources.json
```

After the public release is committed, pushed, and tagged, update the Flathub manifest
to the immutable release source. For example, after creating `v2.0.0`:

```sh
git rev-list -n 1 v2.0.0
```

Set `tag` to `v2.0.0` and `commit` to the printed commit in
`../flathub/com.yanklog.YankLog.yml`. Regenerate and copy
`generated-sources.json` whenever `Cargo.lock` changes.

## Submit A New App To Flathub

This is a separate submission repository. Do not clone it inside
`yanklog-linux-public` and do not add it as a submodule.

1. Finish and publish the upstream release first. The source used by Flathub must be
   an immutable public tag, not `master` or an uncommitted local checkout:

   ```sh
   cd /path/to/yanklog-linux-public
   git status --short
   git tag -a v2.0.0 -m "YankLog 2.0.0"
   git push origin master v2.0.0
   git rev-list -n 1 v2.0.0
   ```

   Copy the last command's output. It is the exact commit that the Flathub manifest
   must use.

2. Build and test the upstream Flatpak manifest locally. Also regenerate
   `packaging/flatpak/generated-sources.json` if `Cargo.lock` changed.

3. In GitHub, fork [`flathub/flathub`](https://github.com/flathub/flathub). The fork
   may contain only `master`; the next step explicitly creates the required `new-pr`
   branch in it.

4. Create a checkout beside the Linux repository from Flathub's upstream `new-pr`
   branch, then push that branch to your fork and create the submission branch:

   ```sh
   cd /path/to/parent-directory
   git clone --branch=new-pr https://github.com/flathub/flathub.git flathub
   cd flathub
   git remote rename origin upstream
   git remote add origin git@github.com:<your-github-user>/flathub.git
   git push -u origin new-pr
   git checkout -b add-com.yanklog.YankLog new-pr
   ```

   The directory layout should now be:

   ```text
   parent-directory/
   ├── yanklog-linux-public/
   └── flathub/
   ```

5. Copy the two prepared submission files into the root of the `flathub` checkout:

   ```sh
   cp ../yanklog-linux-public/packaging/flathub/com.yanklog.YankLog.yml .
   cp ../yanklog-linux-public/packaging/flathub/generated-sources.json .
   ```

6. Edit `com.yanklog.YankLog.yml`. Under `sources`, replace the existing `tag` and
   `commit` together with the release values from step 1:

   ```yaml
   tag: v2.0.0
   commit: <exact output from git rev-list -n 1 v2.0.0>
   ```

   Do not use `master`, a branch name, or a commit from before the `v2.0.0` tag.

7. Check that `git status --short` lists only the two submission files as new changes
   (apart from files already tracked by Flathub's `new-pr` template), then commit and
   push:

   ```sh
   git status --short
   git add com.yanklog.YankLog.yml generated-sources.json
   git commit -m "Add com.yanklog.YankLog"
   git push origin add-com.yanklog.YankLog
   ```

8. Open the pull request in GitHub's web interface. Set its base branch to `new-pr`,
   not `master`, and use the title `Add com.yanklog.YankLog`. Address reviewer
   comments in the same branch. When everything is resolved, request a test build by
   commenting `bot, build` on the pull request.

Only the manifest and generated dependency manifest belong in the Flathub submission
repository. Metadata, desktop files, icons, source code, and binaries remain upstream.

Before submitting, make sure the application and submission comply with Flathub's
current requirements, including its generative-AI policy.
