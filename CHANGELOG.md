# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- Clicking a scene in the sidebar now only opens it for browsing/editing — it no longer immediately syncs skills to your agents. Use the new **Apply to Default** button at the top of My Skills to sync the viewed scene whenever you're ready. The first time you open a scene after upgrading, an inline tip explains the new flow.

### Added
- Show **Applied to ...** / **Not applied yet** status next to the scene title so it's clear which scene is currently live on disk vs. which one you're editing.
- Warn when no agent is enabled/installed so you can't accidentally trigger an apply with no target.

## [1.15.2] - 2026-04-29

### Changed
- Replaced the single-skill delete confirmation modal with an inline popover next to the trash button. Deletions now run in the background with a per-card spinner, so you can keep deleting other skills without waiting for each one to finish.

### Fixed
- Sped up scenario switching, especially for libraries with many skills.

## [1.15.1] - 2026-04-28

### Changed
- Use symlinks as the default sync mode for faster scenario switching and a single source of truth.

### Fixed
- Improved bulk skill deletion performance by processing selected skills in one operation.

## [1.15.0] - 2026-04-25

### Added
- Allow editing project skills path for custom agents
- Multi-device sync metadata support
- New cyan/teal S app icon design

### Changed
- Updated sidebar icon to match the new S design (transparent background)

### Fixed
- Wrap Dock icon in proper macOS squircle so corners render rounded
- Emit refresh event when polling rescan picks up new watch directories
- Stop watching empty skill dirs so users can delete agent folders
- Remove emptied skills-disabled directory after re-enabling last skill

## [1.14.3] - 2026-04-21

### Added
- 

### Changed
- 

### Fixed
- 

### Removed
- 
## [1.14.3] - 2026-04-21

### Changed
- Improved text size scaling to keep the Settings page scrollable at all zoom levels

### Fixed
- Fixed symlink skill uninstall failure on Windows
- Fixed Windows symlink sync issues when using agent directories
- Added logging for Windows symlink fallback to aid troubleshooting

## [1.14.2] - 2026-04-21

### Added
- 

### Changed
- 

### Fixed
- Avoid black screen when opening skill detail sheet on macOS
- Preserve update check settings when importing skills from archives
- Sync skill symlinks to agent directories on install

## [1.14.1] - 2026-04-18

### Added
- Command palette for quick navigation and actions
- Per-agent sync status indicators to see which agents need syncing
- Bulk tag editing for skills to organize skills faster
- Agent toggle in project detail panel for quick agent assignment
- Skill detail panel with local/diff/center tabs to compare skill versions
- Agent dots and tags displayed in skill detail panel

### Changed
- Improved project workspace skill management with better organization
- Skill detail panel now fully scrollable with a persistent close button

### Fixed
- Removed agent assignment count label from project skill cards for a cleaner look

### Removed
- No removals in this release
## [1.14.0] - 2026-04-18

### Added
- Bulk skill update actions to update multiple installed skills in one step
- Custom central repository path support for users who keep their managed skills outside the default location

### Changed
- Refined Settings form controls for a cleaner and more consistent configuration experience

### Fixed
- Deduplicated startup skill update notifications to avoid repeated alerts for the same update
- Updated Antigravity path defaults so installs and sync use the correct skills directory
- Tightened Claude Code skill discovery and import matching to avoid false positives from plugin marketplace caches and mismatched same-name skills

### Removed
- No removals in this release

## [1.13.3] - 2026-04-11

### Changed
- Linking an external workspace no longer asks for a disabled-skills directory. Skills Manager now creates and uses a sibling `*-disabled` folder automatically, and gracefully degrades to read-only mode when that folder cannot be created.

## [1.13.2] - 2026-04-11

### Fixed
- Quitting Skills Manager on Linux no longer terminates other running applications or the desktop session (#47)

## [1.13.1] - 2026-04-10

### Fixed
- Prevented symlink cycles from causing infinite loops when scanning project skills or computing timestamps
- Validated symlink targets in skill document reads to stay within allowed project roots
- Fixed import matching to stay consistent with the sync-status displayed in the UI

## [1.13.0] - 2026-04-10

### Added
- Improved agent assignment controls in project workspaces for clearer setup and management flows

### Changed
- Refined sidebar typography and alignment for a cleaner, more consistent app layout
- Refreshed in-app help content and guidance copy for a clearer user experience

### Fixed
- No user-facing bug fixes in this release

### Removed
- No removals in this release
## [1.12.0] - 2026-04-10

### Added
- Skill source diff viewer to compare source changes before updating local skills
- Richer skill detail metadata panel with source and update context
- Missing local skill source handling to keep installed skills manageable even when source files disappear
- Project improvements including empty project initialization, tag-filtered batch export, and sidebar sync health indicator
- Expanded agent support and refined agent settings management

### Changed
- Clarified project workspace wording and add-skill actions across project flows
- Improved routing for startup skill update notifications and refined parts of the settings and sidebar UI

### Fixed
- Prevent skill detail markdown refreshes from resetting the current view
- Avoid incorrect file swaps for monorepo no-op updates and show the correct update toast
- Improved project sync status accuracy, git sync error messages, and network error detection
- Fixed grid card height alignment, sidebar action button layout shift, larger text clipping, and scenario sync mode persistence
## [1.11.1] - 2026-03-28

### Changed
- Simplified custom agent form layout and copy
- Bilingual release notes (English + Chinese) in GitHub Releases
- Updated README with custom tools documentation

### Fixed
- Prevent action buttons clipping with larger text size in Settings

## [1.11.0] - 2026-03-27

### Added
- Custom agent support: add, configure, and remove user-defined agents with custom skills directories
- Path override for built-in agents: customize skills directory for any supported agent
- Inline path editing with native folder picker in Settings
- Legacy tool key migration (clawdbot → openclaw) with automatic data migration

### Fixed
- Fixed tool key remap logic that could incorrectly drop existing records during migration
## [1.10.0] - 2026-03-25

### Added
- Drag-and-drop skill reordering in project skill lists
- Clickable skill cards on dashboard for quick navigation
- Marketplace contributor quick filter
- Expand/collapse all groups button in marketplace view
- Auto-check skill updates on startup with notification badge
- Toast notification navigation (click to jump to relevant page)
- Text size setting for better readability
- zh-TW locale support

### Changed
- Simplified marketplace layout by removing source grouping
- Improved scan with plugin directory detection, rename support, and date display

### Fixed
- Missing dnd-kit dependencies causing build errors
- React hook violations and lint warnings
- Scenario deletion edge cases and sync error logging
- Git duplicate warning on skill scan
## [1.9.0] - 2026-03-23

### Added
- Multi-select batch operations for skills and project skills
- Per-scenario skill-agent toggles for fine-grained control
- Auto-create Default scenario when no scenarios exist

### Fixed
- Improved batch operation resilience and export selection handling
## [1.8.0] - 2026-03-23

### Added
- Drag-and-drop reordering for scenarios and projects in sidebar
- Git install preview dialog with backup sync
- Dynamic overflow for source filter tags with popover popup
- System tray menu improvements with scenario switcher

### Fixed
- Prevent skill install from overwriting existing skills; improved name collision detection
- Preserve Unix file permissions when extracting ZIP archives
- Security hardening: path traversal prevention, CSP improvements, input sanitization
- Temp directory cleanup in git preview/install lifecycle
- Source filter overflow robustness, accessibility, and layout fixes
## [1.7.0] - 2026-03-22

### Added
- Custom tray icon with full-color RGBA rendering on macOS
- Hide-to-tray on window close with configurable close action dialog
- Tray icon toggle in settings with lazy tray creation
- Proxy support for git clone and network requests
- Multi-select mode and batch delete for My Skills
- Enable/disable toggle for agents in Settings

### Fixed
- Improved tray close behavior with proper quit flow and UI polish
- Consolidated proxy handling and added URL validation
- Security hardening across frontend, backend, and CI
- Better error handling for batch delete and missing i18n keys
## [1.6.0] - 2026-03-19

### Added
- Show current snapshot version in git version history panel

### Changed
- Enlarged sidebar logo for better visibility
- Improved error handling and code structure

### Fixed
- Fixed snapshot tag display format in version history
- Fixed commit message placeholder text
## [1.5.0] - 2026-03-18

### Added
- Git snapshot versioning: create and restore point-in-time snapshots of your skills library
- Batch import skills from a local folder
- Snapshot tags are now automatically pushed to remote during sync

### Changed
- Redesigned skill detail panel header layout
- Sync button uses amber tone instead of red for better visual clarity
- Deeper directory scanning when reconciling skills index (supports nested folder structures)

### Fixed
- Snapshot restore now correctly handles file deletions with automatic rollback on failure
- Duplicate snapshot tags no longer created when retrying after a failed push
## [1.4.1] - 2026-03-15

### Added
- Skill installation can now be cancelled mid-progress
- Clone timeout to prevent installations from hanging indefinitely
- Duplicate install detection to prevent reinstalling the same skill
- Single instance restriction to prevent multiple app windows

### Changed
- Improved app responsiveness by making all backend operations async

### Fixed
- Skill directory not recognized when folder name differs from SKILL.md name
- Install button not showing "Cancel" label text
- Auto-update not working on Windows
- Release builds missing updater signature files
## [1.4.0] - 2026-03-14

### Added
- Install progress toasts and installed state indicators for skill cards

### Changed
- Browse commands now async with client-side search result caching for better performance

### Fixed
- Disable autocorrect and spellcheck on all search inputs

## [1.3.0] - 2026-03-12

### Added
- Project management: view and manage `.claude/skills/` in project directories
- Skill actions for project skills (import, export, toggle, delete)
- Skill tagging system with filter UI
- Sync status tracking and bidirectional update for project skills

### Changed
- Extracted SkillMarkdown component and improved tag UX
- Hardened project skill path traversal and use dir_name as stable key

## [1.2.0] - 2026-03-12

### Added
- Git backup and sync for skill library with multi-machine sync support
- Git sync controls (commit & push, pull) on My Skills page

### Changed
- Moved Git sync operations from Settings to My Skills page for easier access
- Simplified Git backup UI by removing custom commit message input
- Updated Git sync documentation to reflect new UI layout

## [1.1.3] - 2026-03-09

### Added
- In-app auto-update support via tauri-plugin-updater

### Fixed
- Improve update UX with semver comparison, fallback download, and i18n fixes

## [1.1.2] - 2026-03-09

### Added
- Check-for-updates button in Settings page

## [1.1.1] - 2026-03-09

### Added
- Sort market search results by download count

### Fixed
- Debounce market search input to reduce lag and prevent stale results
- Improve light/dark mode color contrast and simplify skill status badges
- Improve text readability across light and dark themes
- Increase font sizes for readability and add CJK font stack
- Increase font sizes and window dimensions for better readability

## [1.1.0] - 2026-03-08

### Added
- Windows and Linux support: cross-platform file manager opening, console window suppression
- Backend command `get_central_repo_path` to expose real repo path to frontend
- Tool adapter fallback strategy for `.config/` paths on Windows

### Changed
- UI text from macOS-specific ("Open in Finder", "Built for macOS") to cross-platform wording
- Settings page now displays dynamic repo path instead of hardcoded `~/.skills-manager/`
- CI Windows smoke check reduced to `cargo check` only (avoids duplicate frontend build)
- Renamed `open_central_repo_in_finder` to `open_central_repo_folder` across backend and frontend

### Fixed
- Windows `explorer.exe` false error due to non-zero exit code on success
- Missing Linux `/home/<user>` → `~` path abbreviation in Settings UI

## [1.0.1] - 2026-03-08

### Added
- GitHub Actions cross-platform build workflow (macOS, Linux, Windows)
- CHANGELOG and macOS troubleshooting guide

### Changed
- Moved sync/unsync buttons from skill card list into SkillDetailPanel
- Moved assets (icon, demo GIFs) from docs/ to assets/
- Set bundle targets to "all" for cross-platform builds

## [1.0.0] - 2025-03-08

### Added
- Initial release of Skills Manager v2 with Tauri backend
- Scenario management: create, rename, delete, and switch scenarios
- Scenario icons and sync engine improvements
- Light/dark theme support with system preference detection
- Global search dialog and help dialog
- Configurable sync mode and startup scenario sync
- External link button for market skill cards
- Market search/filter, error banners, and enhanced confirm dialog
- Skill update checking and updating for git-based skills
- Load-more pagination for market skill list
- Skill deduplication: check central path before installing

### Changed
- Redesigned MySkills card and list layout for compactness
- Unified UI styling with compact, consistent design system
- Paginate market skill list and flatten local scan UI
- Consolidated skill card metadata into a single priority-based status badge
- Compact skill card and list row layout with inline action buttons
- Compact market toolbar layout and redesigned skill cards
- Simplified local install section UI
- Improved skill detail panel rendering and market card layout
- Introduced shared app-page utility classes and standardized UI layout
- Removed global search and topbar; added help button to settings
- Updated app icons

### Fixed
- Replaced CSS `-webkit-app-region` drag with programmatic Tauri drag bar
- Replaced Hammer icon with custom app logo image in sidebar
