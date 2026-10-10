# Outpost — Project Brief

> Outpost: an AppImage store and manager for Linux. Name may still change.
> This file is the project brief for Claude Code.
>
> Theme: AppImages today are the "Wild West" (download anywhere, chmod by hand, update
> by memory). Outpost is the supply station on the frontier that brings order to it.

## What we're building

A GNOME-native "App Store" for AppImages on Linux, itself distributed as an AppImage.
Today, AppImages work like the old Windows model: download from wherever, chmod it,
run it from wherever, and remember to check for updates by hand. This app replaces
that with an app-store experience.

### The core user workflow

1. User opens the store and searches for an app (e.g. "Photocraft").
2. Clicks Install. The store downloads the AppImage into `~/Applications`.
3. The store makes it executable and owned by the user (equivalent of `chmod u+x`).
4. The store integrates it into the desktop: a `.desktop` entry and icon, so the app
   appears in the GNOME launcher like any installed app.
5. The store checks for updates, notifies the user, and updates in one click.
6. Uninstall removes the AppImage, its desktop entry and its icon cleanly.

### Why this exists

Existing tools each cover part of this: Gear Lever (GNOME manager for AppImages you
already have, updates often need manual URLs), AppImagePool (browser for AppImageHub),
AM/AppMan (CLI package manager). None combines discovery + install + desktop
integration + automatic updates in one polished, App Store-feeling GUI. That is the gap.
Expect a coming wave of AI-built ("vibe coded") Linux apps shipped as AppImages
(e.g. Photocraft, a Rust photo editor), which makes this more valuable.

## Architecture

Two separate pieces:

### 1. The index (separate repo, e.g. `outpost-index`)

- The **single source of truth** for the store app. Clients only ever read this.
- A GitHub repo containing:
  - `curated/` — one small file per app we curate/feature (name, repo URL, description,
    icon, categories, verified flag). Developers can submit via pull request.
  - A scheduled GitHub Action (roughly hourly) that:
    1. Pulls AppImageHub's feed: `https://appimage.github.io/feed.json`
    2. Merges it with `curated/` (curated entries win on conflict; dedupe by repo URL)
    3. Uses the GitHub Releases API (authenticated token) to look up each app's latest
       version and AppImage asset URL(s) per architecture
    4. Publishes one `index.json` to GitHub Pages
- Because the index carries latest versions, **client machines never call GitHub's API**.
  This avoids per-user rate limits entirely.

Draft `index.json` shape (refine as needed):

```json
{
  "schema_version": 1,
  "generated_at": "2026-10-09T12:00:00Z",
  "apps": [
    {
      "id": "photocraft",
      "name": "Photocraft",
      "summary": "Photo editor",
      "description": "...",
      "icon_url": "...",
      "categories": ["Graphics"],
      "source_repo": "https://github.com/owner/photocraft",
      "verified": true,
      "latest_version": "1.4.2",
      "assets": {
        "x86_64": { "url": "...", "sha256": "...", "size": 123456 },
        "aarch64": { "url": "...", "sha256": "...", "size": 123456 }
      }
    }
  ]
}
```

- `schema_version` lets the format evolve without breaking older clients.
- Checksums let the client verify downloads (the index is a trust point: keep the repo
  locked down).

### 2. The store app (this repo)

- **Language/UI:** Rust, GTK4 + libadwaita (gtk4-rs / libadwaita-rs). GNOME-first look.
  Libadwaita runs acceptably on KDE; a KDE-native (Qt/Kirigami) variant is a far-future
  maybe, not a goal now.
- Likely crates: `gtk4`, `libadwaita`, `reqwest`, `tokio`, `serde`/`serde_json`,
  `sha2`, `directories`. Consider `relm4` if it makes UI state simpler.
- **App ID:** `io.github.cmumby.Outpost` (GitHub username: `cmumby`).
- **Local state:** `~/.local/share/outpost/installed.json` tracking installed apps
  (id, version, path, install date, source).
- **Install locations:**
  - AppImages: `~/Applications/`
  - Desktop entries: `~/.local/share/applications/`
  - Icons: `~/.local/share/icons/hicolor/...`
  - Extract `.desktop` file and icon from the AppImage (e.g. `--appimage-extract`
    into a temp dir), rewrite `Exec=` to the installed path.
- **Update checks:** fetch `index.json` on launch and every ~15 minutes while running,
  using conditional requests (ETag / If-Modified-Since) so unchanged checks are tiny.
  Compare to `installed.json`, show updates, send desktop notifications.
- **Self-update:** the store is itself an AppImage and should be able to update itself.

## Known gotchas

- Many newer distros don't ship `libfuse2`, which type-2 AppImages need. Detect this
  and show a clear, friendly message with the fix.
- Pick the right asset for the machine's architecture (x86_64 vs aarch64).
- AppImageHub entries vary in quality; some are stale or broken. Curated entries
  should be preferred and flagged.
- Never require root. Everything lives in the user's home directory.

## Build plan (phases — finish and test each before the next)

1. **Local manager.** Install an AppImage from a local file or URL into
   `~/Applications`, make executable, create desktop entry + icon, list installed
   apps, uninstall cleanly. No store yet.
2. **Index repo.** Curated folder, sync Action, published `index.json`.
3. **Store UI.** Browse, search, app detail page, Install button, using the index.
4. **Updates.** Periodic index checks, update badges, notifications, one-click update,
   checksum verification.
5. **Ship it.** Package the store as an AppImage, self-update, onboarding screen.

## Open decisions (ask before assuming)

- Background mode: v1 checks for updates only while the store is open. Later option:
  start at login (minimized/background) so notifications arrive even when closed.
- Whether to keep multiple versions side-by-side or always replace.

## How to work with me

I'm building this by vibe coding, so: explain decisions in plain language, keep changes
small and testable, tell me exactly what to run to test each step, and ask before
making big architectural changes or adding major dependencies.
