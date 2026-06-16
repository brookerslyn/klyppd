# Changelog

## 0.3.2 - 2026-06-16

### Features

- Added configurable clipping sounds with a built-in chime, custom WAV path, and preview control.

### Fixes

- Fixed replay saves falsely reporting success after the recorder process exited.
- Added a Hyprland monitor capture fallback when the desktop portal is unavailable.
- Kept recorder state in sync when replay or recording children exit unexpectedly.

## 0.3.1 - 2026-06-06

### Performance

- Stream editor preview video from disk with HTTP range support instead of loading entire clips into memory.
- Cap and prune generated preview transcodes in `/tmp/klyppd-preview`.
- Reduce startup latency by making the boot animation non-blocking.

### Fixes

- Remove the unused full-video byte read command from the Tauri API.
- Remove stale hotkey parser dead code.

## Session Changelog

## UI

- Reworked the library filter area into a compact terminal-style panel.
- Replaced the native sort select with a custom in-app dropdown.
- Restyled the settings codec/container/audio codec dropdowns to match the app UI.
- Kept filter controls shared across library, uploads, and permanent tabs.

## Library Search and Sorting

- Added clip searching by filename and path.
- Added app/window filtering using stored window names with filename fallback.
- Replaced the old date filter with newest/oldest sorting.
- Added window name suggestions sourced from recorded clips.

## Backend and Data

- Stored window/app names for clips in the database.
- Added a backend query for distinct window names.
- Updated clip scanning and metadata updates to preserve window names.
- Tightened SQL column selection for clip queries.

## Fixes

- Fixed the library view freeze during tab switching.
- Fixed the library filters so uploads and permanent tabs use the same filtered data.
- Fixed the filter UI layout so controls fit the terminal theme.
- Fixed the dropdown popup styling so open menus match the rest of the app.
