# Session Changelog

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

