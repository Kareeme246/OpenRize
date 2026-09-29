# Changelog

All notable changes are documented here. Entries are generated from commit history.

## [unreleased]

### Features
- **sidebar:** Collapsible sidebar that auto-collapses in narrow windows (#48)
- **calendar:** Limit time by category to top three and group the rest under Other (#50)
- **calendar:** Enlarge day summary text and drop duplicate review label (#52)
- Add four UI size modes and responsive calendar review badge (#53)
- **settings:** Make sections scrollable on one page (#54)

### Bug Fixes
- **calendar:** Wrap category and project chips instead of truncating them (#47)
- **sidebar:** Use muted accent-soft for the selected nav item (#49)
- **calendar:** Move billable toggle below project in entry review sidebar (#51)

### Maintenance
- **release:** Bump version to 0.4.15 (#55)

### Other Changes
- Update OpenRize description for clarity
## [0.4.7] - 2026-09-28

### Features
- **updates:** Add in-app updater with sidebar notice and Settings changelog (#46)

### Bug Fixes
- **capture:** Allow idle sleep and stop recording while the Mac is locked or asleep (#45)

### Documentation
- Update changelog for v0.4.7

### Other Changes
- Update README.md
## [0.4.5] - 2026-09-28

### Features
- Allow deleting projects and unapproving timesheet entries (#42)
- **release:** Sign and notarize macOS builds, rename identifier to com.offlinestudios.openrize (#44)

### Bug Fixes
- **calendar:** Hide suggestion rationale (#43)

### Documentation
- Update changelog for v0.4.5
## [0.4.2] - 2026-09-28

### Features
- **calendar:** Live-updating recording session and 5-min drag snap (#32)
- **settings:** Add tracking hours schedule and capture gating (#33)
- **calendar:** Add panel tabs, a recording now line, and launch at login (#35)
- **tray:** Add the Pulse glance panel under the menu-bar icon (#36)
- Add project timesheets and client invoices (#37)
- **calendar:** Split the day view into Time Entries, Labels, and Project lanes (#38)

### Bug Fixes
- **capture:** Count foreground video as presence and keep sessions continuous (#34)
- Resolve invoice picker, project due date, timesheet flicker, app scroll, and session defaults (#39)
- Editable entry times, unapprove flow, pending drag sessions, untitled live sessions (#41)

### Documentation
- **readme:** Centered header with logo, tagline, nav links, and badges (#40)
- Update changelog for v0.4.2

### Other Changes
- Remove local skill
## [0.3.6] - 2026-09-25

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
- Update changelog for v0.3.6

### Maintenance
- Add generated changelog and release version guard (#26)
- Pin git-cliff to v2.14.2 in release workflow (#30)
- Bump git-cliff-action to v4.9.1 (#31)

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
