# v0.2.0

## New Features

- **Global hotkeys via evdev** — works on any compositor (Hyprland, GNOME, KDE, Sway, X11) without per-WM config. Supports any key combo (Alt+R, Ctrl+Shift+F9, F9, Super+C, etc.)
- **Clip renaming** — pencil button on each card or double-click the clip name
- **Window transparency** — opacity slider in Settings controls compositor window opacity in real time
- **Smart filenames** — clips auto-named from the active window via .desktop file lookup (e.g. `Sober_2026-05-22.mp4` instead of `Replay_2026-05-22_22-08-02.mp4`)
- **R2 storage usage** — uploads/permanent tabs show total space used on R2
- **Minimize to system tray** — hide button sends to tray; click tray icon or re-run `klyppd` to restore
- **Single instance** — re-launching klyppd focuses the existing window instead of opening a new one
- **Upload grid view** — uploads and permanent tabs now show clip cards with thumbnails (same as library)
- **Copy link vs Share** — already-uploaded clips show "copy link" instead of re-uploading

## Improvements

- **Instant trimmer** — skips FFmpeg transcode for clips already in h264+aac mp4 (opens in <1s)
- **Better trim** — video seeks to frame during handle drag so you see exactly where you're cutting
- **Rich Discord embeds** — "Clipped with Klyppd · 11.4 MB" as site_name, "Sober (May 22, 2026)" as title, GitHub link in embed page footer
- **Short share URLs** — 6-char IDs (`cdn.example.com/t/abc123`)
- **AAC audio by default** — recordings use AAC so previews play instantly without transcode
- **Wayland crash fix baked in** — `WEBKIT_DISABLE_DMABUF_RENDERER=1` set automatically
- **Scrollbar hidden** — no ugly GTK scrollbar; scroll via mousewheel/trackpad
- **Debounced clip watcher** — no more duplicate notifications per saved clip
- **Debounced hotkey** — prevents double-fires from key repeat

## UI
- **Revamped UI** (Not fully AI made now)
- **Migrated to plain Svelte 5 + Vite** (removed SvelteKit, simpler build)
- **New sidebar icons** — Lucide-style SVGs (grid, upload, bookmark, sliders)
- **Fixed tab switching artifacts** — forced repaint on webkit2gtk transparent backgrounds
- **Opaque main content area** — prevents ghost pixels from previous tab bleeding through
- **Default theme updated** — #101418 base, #9accfa accent, JetBrains Mono, 0.8 opacity, 10px blur
- **K logo mark** — proper tight crop, centered in sidebar and titlebar

## Packaging & Distribution

- **AUR packages live** — `klyppd-bin` (prebuilt, seconds), `klyppd-git` (builds from main)
- **GitHub Actions** — CI build + release workflow producing .deb on tag push
- **.desktop file** — shows in rofi/wofi/app launchers with K icon
- **Faster builds** — mold linker config, reduced tokio features, dev profile optimizations

## Fixes

- Video playback in editor (asset protocol URL encoding fixed)
- R2 upload crash (`behavior-version-latest` feature flag)
- Hide/close buttons (added window permissions to capabilities)
- Removed stale SvelteKit files and routes


## Important

- Balls
