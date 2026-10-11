# Outpost build log

Session of 9–10 October 2026. Built Outpost from an empty folder to a working,
packaged app store for AppImages, following the phases in [CLAUDE.md](CLAUDE.md).

## Where things stand

| Phase | Status |
|---|---|
| 1. Local manager | Done, committed |
| 2. Index repo | Done, live at https://cmumby.github.io/outpost-index/index.json |
| 3. Store UI | Done, committed |
| 4. Updates | Done, committed |
| 5. Ship it | Done. v0.1.0 released at https://github.com/cmumby/outpost/releases |

**Tested on this machine:** the AppImage's layout after the theme fix, the
welcome screen, and **Move**, which put Outpost at `~/Applications/Outpost.AppImage`.

## Decisions made

- **App ID:** `io.github.cmumby.Outpost` (GitHub user `cmumby`).
- **Reinstalling replaces** the existing copy. No side-by-side versions.
- **UI:** plain gtk4-rs + libadwaita, no relm4. Screens built in Rust code.
- **Index:** separate public repo `cmumby/outpost-index`. Curated apps are TOML
  files in `curated/`. The build script is stdlib-only Python, run hourly by a
  GitHub Action and published to GitHub Pages.
- **Featured = curated** for now. A separate `featured = true` flag may be needed
  once there are dozens of curated apps.
- **Icon:** option A, the watchtower with a blue download flag (chosen from three
  sketches). Files are in `data/icons/`.
- **Release builds** target Ubuntu 24.04 (libadwaita 1.5, GTK 4.14), so the
  AppImage runs on 24.04 and newer.
- **Background checks** only run while the store is open (v1).
- **No dock badge**; the update count shows on the Installed tab only.

## Dependencies added

`gtk4`, `libadwaita`, `serde`, `serde_json`, `directories`, `reqwest` (blocking,
built-in TLS), `gio-unix` (needed to launch apps through their desktop entry),
`sha2`. Packaging tools (linuxdeploy and its GTK plugin) are downloaded into
`target/` by the build script and are not dependencies.

## What was built

### Phase 1: local manager (`6fb866d`, `8b922a6`)
- `outpost install <file|url>`, `outpost list`, `outpost uninstall <id>`.
- Installs into `~/Applications/<id>.AppImage`. Pulls only the `.desktop` file
  and icon out of the AppImage (`--appimage-extract <pattern>`, no FUSE needed),
  following symlinks inside the image. Writes `outpost-<id>.desktop` and the icon
  into hicolor. Tracks everything in `~/.local/share/outpost/installed.json`.
- Downloads show progress. A libfuse2 check prints the right install command
  for the distro.

### Phase 2: index (`outpost-index` repo)
- `scripts/build_index.py` merges AppImageHub's feed (about 3,000 apps) with
  `curated/`. It looks up latest releases through GitHub GraphQL, 50 repos per
  request, and picks one AppImage per architecture.
- It reuses GitHub's own SHA-256 digests. Otherwise it hashes files itself, up to
  2 GB per run, remembering earlier results via the previously published index.
- Result: about 2,300 installable apps, a 1.8 MB index (380 KB compressed),
  about 6 minutes per full run.
- Curated so far: LocalSend, Photocraft, Upscayl, Zen Browser (plus Outpost,
  unpushed).

### Phase 3: store UI (`e410cca`)
- Explore tab (Featured, Categories, search), app pages, Installed tab.
- Install runs on a background thread with a progress bar. Uninstall asks first.
  Open launches through the app's desktop entry.
- The index is cached with ETags, so the store works offline.

### Phase 4: updates (`0a630ba`)
- Re-checks the index every 15 minutes while open. `OUTPOST_CHECK_INTERVAL`
  (seconds) shortens this for testing.
- Installed tab shows an update count badge, an Updates section and Update All.
- Desktop notification when a new update appears while the window is in the
  background, once per update.
- Store downloads are checked against the index's SHA-256 and refused on mismatch.
- Outpost writes its own launcher entry, which GNOME needs before it will show
  notifications.

### Phase 5: ship it
- `packaging/build-appimage.sh` builds `target/appimage/Outpost-<arch>.AppImage`
  (35 MB) with GTK 4 and libadwaita bundled.
- `.github/workflows/release.yml` builds x86_64 and ARM on Ubuntu 24.04 when a
  `v*` tag is pushed, and publishes a GitHub release.
- Self-update: when running as an AppImage and the index has a newer Outpost, a
  banner offers Update. It downloads, verifies, swaps the file, then offers Restart.
- Welcome screen on first run. It can move Outpost from Downloads into
  `~/Applications/Outpost.AppImage` and restart from there. If the welcome screen
  is skipped, a banner offers the same move.

## Problems found and fixed

- **Launcher icon didn't appear after install.** GTK only re-scans an icon theme
  when the theme folder's own timestamp changes. Outpost now bumps it after
  adding or removing icons.
- **Catalogue failed to load.** 112 AppImageHub entries have `null` categories.
  The index script now drops them, and the store skips any single broken entry
  instead of rejecting the whole catalogue.
- **GitHub "403" during index builds.** These were GitHub's secondary rate
  limits. The script now waits and retries.
- **Concurrent installs could collide.** Scratch files now get unique names, and
  `installed.json` changes take turns behind a lock.
- **Window froze after the update notification.** VSCodium (Electron) sets
  `GDK_BACKEND=x11` for programs started from its terminal, and GTK 4 windows can
  stop redrawing under XWayland. Outpost now removes that setting when Wayland
  is available.
- **AppImage looked unstyled** (no spacing around icons, squashed rows). The
  linuxdeploy GTK plugin's startup script forces `GTK_THEME` and
  `GDK_BACKEND=x11`. The build script now deletes both lines before packaging.

## Next steps

1. Check that installed v0.1.0 offers v0.2.0 as a self-update.
2. Find a usable screenshot for LocalSend. Its official ones are WebP, which GTK
   can't always open.
3. Consider a separate `featured = true` flag once there are many curated apps.

## Testing notes

- Network tests run only on request: `cargo test -- --ignored`.
- To fake an available update: edit the app's `"version"` in
  `~/.local/share/outpost/installed.json`.
- To see the welcome screen again: `rm ~/.local/share/outpost/welcomed`.
- Only one copy of Outpost runs at a time. A second launch hands over to the
  open window.
- Apps installed with the command line (Upscayl, LocalSend on this machine)
  don't get store updates, because they aren't linked to a store entry.
  Reinstalling them from the store fixes that.

### After Phase 5: screenshots (v0.2.0)
- The index carries AppImageHub's screenshots (3,017 of about 3,050 apps have
  one) as `screenshot_urls`. Curated files can list `screenshots`, falling back
  to AppImageHub's. Photocraft has its official demo screenshot.
- App pages show screenshots in a carousel under the Install button, with arrow
  buttons and dots when there's more than one. They load on demand and are
  cached on disk.
- About one AppImageHub screenshot link in eight is broken (404). The screenshot
  area stays hidden until an image actually loads, and failed ones are dropped,
  so those apps show no empty frame.
- Upscayl has two screenshots and Zen Browser one, added in their curated files.
