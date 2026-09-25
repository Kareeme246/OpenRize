# Changelog

All notable changes are documented here. Entries are generated from commit history.

## [unreleased]

### Features
- **timers:** Rust timer store with persistence and tests
- **tray:** Macos menu-bar icon reflecting active timers
- **ui:** App shell with header and sidebar
- **trackers:** Concurrent stopwatch widgets
- **ui:** Settings tab, idle-on-create trackers, hour-dial widgets
- **ui:** Confirm reset and delete with native OS alerts
- **ui:** Replace the native alert with an in-app confirm dialog
- **activity:** Capture active windows; wire dashboard, sessions, focus, breaks
- Implement P0 foundations and P1 core tracking with Calendar day view (#11)
- **timers:** Bring back manual multi-timer page and stopwatch widgets (#15)
- **calendar:** Add drag sessions and zoomable five-to-five day (#28)
- **apps:** Add interactive daily timeline view (#27)
- **entries:** Make time entries read-only and handle empty project state (#29)

### Bug Fixes
- **ui:** Scroll sidebar on its own, standardise nav rows, retarget dial
- **ui:** Preselect neither confirm button, and colour Cancel
- **icons:** Correct macOS app-icon margin and regenerate menu-bar glyphs

### Documentation
- Rewrite README for the shipped P0-P4 core (#16)

### Maintenance
- Add generated changelog and release version guard (#26)

### Other Changes
- Init create tauri app
- Add some agent files
- Add CLAUDE md
- Apply rustfmt to rust sources
- Add proof of concept
- Remove generation script
- Update src-tauri/src/timers.rs
- Update src/components/ConfirmDialog.tsx
- Update src/components/Sidebar.tsx
- Update src/pages/Settings.tsx
- Add new app icon (Chrono Bloom) and regenerate full Tauri iconset
- Redesign stopwatch widget: ring doubles as play/pause, app-wide mono font
- Stub out sidebar nav and bento-style pages for unbuilt Rize feature areas
- Add basic readme information and license
- Add Biome + verify/dev-screenshot tooling for agents
- Bring newly-merged activity/topbar code up to pnpm verify
- Wire cargo clippy --fix into pnpm fix
- Push activity ticks and move timers into SQLite
- Add appearance, close behavior, storage, and retention settings
- Manually remove code bloat and legacy unneeded code and references
- Add a drive-app skill for clicking through the real app (#10)
- On-device AI categorization and review flow (#12)
- Learning loop and confidence calibration (#13)
- Calendar week and month, My Timesheet, Time Entries, Projects (#14)
- Remove extra files
- Refine README content and improve clarity
- Fix settings scrollbar, custom picker component, and entry aggregation (#17)
- Remove wrong files and symlink claude.md
- Group activity into sessions and stop minting ghost entries (#18)
- Remove unnecessary files
- Add battery and energy usage monitoring (#19)
- Release readiness: CI/CD, version tagging, repo templates, must-fix cleanup (#20)
- Collapse database schema to v1 (#21)
- Move timers into Manual sidebar section (#23)
- Add bulk timesheet entry deletion (#22)
- Add calendar manual time entry support (#24)
