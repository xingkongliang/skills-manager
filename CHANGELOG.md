# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.25.0] - 2026-06-19

### Release Overview
- This release adds global tag rename and delete from the skill filter bar via a right-click context menu.

### User-facing
- **Right-click tag management from the filter bar** — Right-click any tag pill in the filter bar to open a context menu with Rename and Delete options.
- **Rename tags globally** — The Rename action opens a modal dialog, applies the new tag name to all skills, and updates any active filters automatically.
- **Delete tags globally with confirmation** — The Delete action asks for confirmation before removing the tag from all skills.
- **Cleaner filter pills** — The previous hover-icon design (✏️/🗑️ on each pill) has been removed; left-click still filters by tag as before.

### Developer & Governance
- Added `src/components/TagRenameDialog.tsx` for the modal rename dialog.
- The backend `rename_tag` / `delete_tag` SQLite commands were already shipped in v1.24.0; this release wires them into the redesigned filter-bar UX.
- Added `tagName` and `manageHint` i18n keys to English, Simplified Chinese, and Traditional Chinese.

## [1.24.0] - 2026-06-18

### Release Overview
- A new built-in agent: OMP Agent (oh-my-pi) now ships out of the box, with skills syncing to oh-my-pi's native user- and project-level skill paths.

### User-facing
- **OMP Agent (oh-my-pi) is now a built-in agent** — oh-my-pi ships out of the box with its own icon. User-level skills sync to `~/.omp/agent/skills`, and project-level skills to `<repo>/.omp/skills` — matching oh-my-pi's native skill discovery, whose project-scope path drops the `agent` segment. OMP Agent is listed under the "more agents" section in Settings and sits after the mainstream coding agents in the default agent order (#235).

### Developer & Governance
- Added the `omp_agent` tool adapter with asymmetric default paths (user `~/.omp/agent/skills`, project `<repo>/.omp/skills`) per oh-my-pi's `native` discovery provider; it is placed after `opencode` in `DEFAULT_PRIORITY_ORDER` and excluded from `MAINSTREAM_AGENT_KEYS`. Unit tests cover the adapter's default paths and the new-agent insertion order for existing users.
## [1.23.2] - 2026-06-17

### Release Overview
- A sync-accuracy fix: copy-mode skills that contain Python scripts no longer get falsely flagged "center changed" after their scripts run, because compiled-Python artifacts are now excluded from skill content hashing.

### User-facing
- **Running a skill's Python scripts no longer marks it as out-of-sync** — For skills deployed in copy mode, executing their Python scripts created `__pycache__/*.pyc` bytecode caches inside the agent's copy. Those cache files were folded into the skill's content hash, so the copy diverged from the central library and the skill stayed flagged "center changed" until a manual re-sync. `__pycache__` directories and `*.pyc` files are now excluded from content hashing (and from the source-diff view), so a skill keeps reading as in-sync after its scripts run. Symlink-mode skills were never affected, since the deployed link and the library are the same files.

### Developer & Governance
- `content_hash` now ignores `__pycache__` and `*.pyc` through the shared `list_content_files` enumeration, so the update badge and the source-diff stay consistent; added unit tests covering both a `__pycache__` directory and a loose `.pyc` file.

## [1.23.1] - 2026-06-10

### Release Overview
- A Windows link-mode rescue release: when Windows blocks symlink creation (no admin rights, Developer Mode off), skills now sync as directory junctions instead of silently degrading to full copies. Also adds a CI job that finally runs the Rust test suite on macOS and Windows.

### User-facing
- **Symlink mode now works on Windows without Developer Mode** — Creating real symlinks on Windows requires admin rights or Developer Mode, so for most users the "symlink" sync mode silently fell back to copying every skill into each agent's folder, ballooning disk usage for large skills. When a symlink cannot be created, Skills Manager now creates a directory junction instead — junctions need no privilege on local NTFS volumes and stay live-linked to the central library exactly like symlinks. Full copy remains only as the last resort, e.g. for WSL targets (`\\wsl.localhost\...`), which Windows cannot link to from user mode (#126, #38). Note: targets that already degraded to copies are not converted automatically — trigger a manual re-sync (or update the skill) to switch them to junctions.
- **Dangling directory links are now removed correctly on Windows** — Deleting a synced skill whose directory symlink/junction pointed at an already-removed source used to fail silently and leave a broken link behind; removal now classifies links by their own metadata instead of following them.

### Developer & Governance
- New `Test` CI workflow runs `cargo test` on macOS and Windows for every push/PR touching `src-tauri/`, so `cfg(windows)` code paths (symlink/junction sync, removal) are finally exercised automatically; a `taskkill vctip.exe` step keeps the Windows post-job cache save from flaking.
- Skill content hashes now use `/` path separators on Windows too, so identical skill content hashes identically across platforms; existing Windows hashes recompute once on next sync.
- Fixed the pull-conflict git test to pin the bare remote's initial branch to `main` — CI runners have no global `init.defaultBranch`, which left the cloned repo on an unborn branch and broke the test everywhere except dev machines.
## [1.23.0] - 2026-06-06

### Release Overview
- A release centered on cleaner skill/preset boundaries: installing a skill now only adds it to the central library instead of silently joining the active preset, and preset exports and agent ordering respect which agents are actually enabled. Also adds Grok as a built-in agent.

### User-facing
- **Grok is now a built-in agent** — Grok ships out of the box with skill paths at `~/.grok/skills` and `<repo>/.grok/skills`, slotted right after Codex in the default order and the Settings agent group, with its own icon.
- **Installing a skill no longer auto-adds it to the active preset** — Installs now only add the skill to the central library. Previously each install was silently added to whichever preset was active and synced to your agents; because the active preset drifts (creating a preset auto-activates it, deleting the active one picks a replacement, startup restores the default), skills leaked into unintended presets and had to be removed by hand. To enable an installed skill, add it to a preset (or install it to an agent) explicitly — matching the CLI, which already behaved this way (#213).
- **Preset exports target only enabled agents** — Exporting a preset to a project now writes to agents that are both installed and enabled, instead of also touching disabled ones, so a disabled agent no longer receives preset skills (#206).
- **Newly added agents keep their canonical order** — For users who already have a saved agent order, a newly registered priority agent (such as Grok) is now inserted right after its predecessor in the default order instead of being appended at the bottom.

### Developer & Governance
- All five desktop install paths now pass `None` to `store_installed_skill_unlocked` instead of the active scenario, and the batch-import "already exists" branch no longer re-adds skills to the active preset; the function's `Option` parameter is retained for the CLI's `--sync` / `--sync-preset` (#213, #214).
- Collapsed the duplicated `installed && enabled` agent-filter predicate (`getDefaultExportAgents`, `initialSheetAgents`, `presetBarAgentKeys`) into a single `enabledInstalledAgentKeys()` helper so the availability rule cannot drift between call sites (#206).
- `merge_order` now inserts a new priority agent right after its predecessor in `DEFAULT_PRIORITY_ORDER` (non-priority agents still append), with unit tests for fresh install, new-priority insertion, and non-priority append.
- Added video intro links (YouTube + Bilibili) to the README.
## [1.22.5] - 2026-06-01

### Release Overview
- A Git-sync reliability release: the very first backup to a fresh remote now actually uploads instead of silently reporting "Up to date", conflicting edits from two machines recover gracefully instead of wedging the library, and Git operations are now logged so sync problems can be diagnosed. Built-in agents also gain editable project skill paths.

### User-facing
- **First backup to a new remote now uploads** — Setting up backup against a freshly created empty repository used to commit everything locally but never push, so Sync reported "Up to date" while the remote stayed empty. The first sync now correctly performs the initial push (setting up upstream tracking), so a new remote is populated as expected (#162, #179, #116).
- **Sync conflicts recover instead of breaking the library** — When two machines edited the same skill and both synced, the merge conflict left the repository in a stuck state that blocked all future syncs (and could even prevent the app from loading). Sync now rolls back the failed merge automatically and offers a one-click "re-clone from remote" recovery, with skills that exist only locally preserved (#169).
- **Built-in agents get editable project skill paths** — The per-project skills path (and reset-to-default) that was previously only available for custom agents now works for built-in agents too. Each path row exposes edit/reset actions on hover for both the global and project paths.

### Developer & Governance
- Fixed the sync push gate in `handleGitSync`: a `no_upstream` repo reports `ahead = 0` (there is no `@{upstream}` to diff against), so the old `committed || ahead > 0` condition skipped the first push entirely. It now also pushes when `upstream_health === "no_upstream"`, relying on the backend `push -u` path to establish tracking.
- Added structured logging across the Git backup subsystem (`init`/`set_remote`/`commit`/`push`/`pull`/`snapshot`/`restore`/`clone`/`reclone`) at INFO, with a single WARN failure chokepoint in `run_git_checked`; remote URLs are redacted. Previously the subsystem emitted no logs, leaving "sync silently did nothing" reports undiagnosable.
- `pull_unlocked` now runs the merge via `run_git` and, on conflict, logs a warning, runs best-effort `git merge --abort` to clear the conflicted tree and `MERGE_HEAD`, then bails with a recognizable `SYNC_CONFLICT` error; the frontend routes that to the recovery dialog (re-clone only for conflicts). Regression tests cover first-push-to-empty-remote and the two-sided conflict abort.
- Built-in agent project-path overrides persist in a new `custom_tool_project_paths` setting (an empty value or one equal to the built-in default clears the override); the Settings path UI was unified so global and project rows share the same right-aligned hover actions.
## [1.22.4] - 2026-05-30

### Release Overview
- A fix release that restores the missing delete/manage button after uploading a global-workspace skill to the central library, and makes the "update available" badge agree with what the Diff tab actually shows.

### User-facing
- **Uploaded skills get their delete button back** — Uploading a skill from the Global Workspace to the central library used to leave the card with no actions at all: the skill was synced but unmanaged, so neither a delete nor a re-upload button appeared. Newly uploaded skills are now registered as managed targets, and a one-time startup repair restores the button for skills that were already stranded by the earlier behavior.
- **Update badge and diff now agree** — The "update available" badge hashed the whole skill directory, but the Diff tab only compared the main `SKILL.md`, so a change inside `references/`, `scripts/`, an added/removed file, or an exec-bit flip would flag an update yet show an empty diff. The Diff tab now reports per-file changes across the entire skill directory, so the badge and the diff always match.

### Developer & Governance
- Uploading a local agent skill to the center now reuses the regular `sync_single_skill_to_tool` path so the adopted skill becomes a managed target consistent with every other managed skill; the freshly inserted skill row is rolled back if target registration fails.
- Added a `backfill_stranded_agent_targets` startup repair that scans each installed, enabled agent for center skills whose `source_ref` points at an agent skills dir but lack a target. It matches strictly by `source_ref` (never content hash, to avoid adopting look-alikes) and only repairs skills the workspace classifies as `in_sync` (since the sync rewrites the agent artifact from central). The pass is idempotent and short-circuits on a cheap pre-check once everything is targeted.
- Shared one file-enumeration helper (`content_hash::list_content_files`) for both hashing and diffing so their scope can never drift, and added a `get_skill_source_diff` command returning per-file entries (added / removed / modified; text / binary / too_large / permission_only); the Diff tab renders `SkillSourceDiffViewer` per changed file, lazily loaded on open.
- Documented the macOS 15 Gatekeeper "could not verify this app is free of malware" dialog in the README, with a screenshot and the steps to open the app anyway.
- CI: skip the rust-cache save step to avoid false-positive failures on Windows release builds.
## [1.22.3] - 2026-05-30

### Release Overview
- A small fix release that keeps each project's agent buttons readable in the skill detail panel when a project targets many agents.

### User-facing
- **Agent buttons no longer overflow the card** — In a skill's detail panel, projects that target many agents used to push the per-agent add/installed buttons past the card edge, where they were clipped. The buttons now sit on their own line below the project name and wrap as needed, so both the project name and every agent button stay visible (#188, #189).

### Developer & Governance
- Restructured the project row in `SkillProjectsSection` from a single horizontal flex line to a two-line stack (name + wrapping chip row), dropped the `shrink-0` that prevented `flex-wrap` from triggering, left-aligned the chips under the project name, and cleaned up the leftover indentation in the chip map block.
## [1.22.2] - 2026-05-28

### Release Overview
- A maintenance release that fixes a startup crash and makes skills visible to the Codex CLI again.

### User-facing
- **Codex skills are visible again** — Skills now deploy to `~/.codex/skills/`, which is where the Codex CLI actually reads user-level skills. Earlier builds wrote them to `~/.agents/skills/`, so installed skills never showed up in Codex; that path is kept as a discovery fallback so existing installs still surface in the Codex tab (#182).
- **No more startup crash from stale presets** — Fixed a foreign-key panic that could crash the app on launch when a preset still referenced a skill that had been deleted. Stale memberships are now skipped (and logged) during reindex instead of aborting startup (#170).

### Developer & Governance
- Sync logging is quieter and more useful: dropped the spurious `package-lock` peer-marker noise and now warns when a stale preset membership is skipped, with a regression test covering memberships that point at a missing skill or preset.
- Reworked both CHANGELOG files and the release-notes template around three audience-aware sections (Release Overview / User-facing / Developer & Governance), replacing the old Added/Changed/Fixed/Removed split.
- Release notes are now assembled with auto-injected metadata — release date, the previous-tag→current-tag compare URL, and a verification block — and an awk pass strips any empty section so half-filled entries can't leak placeholder headings.

## [1.22.1] - 2026-05-22

### Release Overview
- This release cleans up two confusing status indicators so the Library cards and Settings agent toggle are readable at a glance.

### User-facing
- **Library card status indicator** — Removed the small circle in the top-left of each Library skill card. It conflated "synced to any agent" with preset membership, which the green left border already shows; per-agent sync status remains in the bottom-right agent dots.
- **Discoverable agent toggle in Settings** — The tiny status icon next to each agent has been replaced with a macOS-style switch (green = enabled, gray = disabled). The previous icon looked like a status badge, so users didn't realize they could click it to enable or disable an agent.

## [1.22.0] - 2026-05-21

### User-facing
- **Skill auto-update** — New **Settings → Skill Auto-Update** section. Pick a background check frequency (hourly / every 6 hours / daily) so the "update available" badge stays current while the app is open, and optionally enable **Apply updates automatically** to pull and apply detected upstream updates without a manual click — off by default; when off, updates are only flagged in the Library. The redundant in-Settings "Check Now" button was removed, since the Library toolbar already has "Check All".
- **Lobster Agents** now form their own group in the sidebar, separate from coding agents.
- Applying a preset from the tray menu is no longer blocked while a skill update check is running.
- **Presets are curation labels** — Adding or removing a skill from a preset no longer immediately changes what is deployed to your agents; deployment happens only when you explicitly apply a preset.

### Developer & governance
- Reworked the preset model around curation-label semantics: membership edits are decoupled from disk sync, with explicit batch apply modes and a workspace-scoped tray apply path.
- The background auto-update scheduler polls every 15 minutes to honor the shortest (hourly) interval and to pick up settings changes promptly.
- Tray preset-apply and update-check use independent locks so the two operations no longer block each other.

## [1.21.0] - 2026-05-18

### Added
- **Add from Library sheet** — In any workspace, click **+ Add Skills** to open a unified picker: search your central library, toggle target agents with always-visible chips (with select-all / clear shortcuts), and batch-add multiple skills in one click.
- **Untagged filter pill** in the Library tag-filter row to quickly surface skills that haven't been tagged yet.
- **Delete from agent cards** — In **Global Workspace**, skills that only live inside an agent's directory (not linked from the central library) can now be deleted right from the card. In **Project Workspaces** the per-card delete button is always visible instead of hover-only.
- **Activity log bundled with Export Logs** — Install / remove / update / sync operations are recorded locally, and **Settings → Export Logs** now packages them together with recent log files into a single zip — much easier to attach when filing an issue.
- **Startup timing diagnostics** added to logs to help track down slow Windows launches (#153).

### Changed
- **Dashboard refocused on library-wide state** — The hero replaces the old "Current Preset: …" framing with total library skills, sync coverage, and the actual count of installed-and-enabled agents. Recent activity now pulls from all managed skills.
- **Faster Copy-mode sync** — Skip the per-file rewrite when the source hash hasn't changed; large libraries (especially on Windows) now resync noticeably faster (#153).

### Fixed
- **Global Workspace agent reload could get stuck** — A stale "loaded agent" reference is now cleared on cleanup so switching agents always re-fetches.
- **Project Workspace skill toggles** behave more reliably after changing the target agent set.

## [1.20.0] - 2026-05-18

### Added
- **`skills-manager-cli` write commands** — the CLI now lets agents fully manage skills: `install` (local path / git URL / `owner/repo[@skill]` shorthand), `update`, `check`, `remove`, `sync`, `search` (skills.sh marketplace, no API key), `adopt` (pull existing skills from agent directories into the central library), and `tag add/remove/list`. Every command supports `--json`; `remove`, `sync`, and `adopt` support `--dry-run`. `remove` always requires `--yes`.
- **`presets add-skill` / `remove-skill` CLI commands** — manage which skills belong to a preset from the command line.
- **`presets deactivate` CLI command** (with `close` / `stop` / `off` / `disable` aliases) — close a preset and tear down its sync targets. When the closed preset is the active one a replacement is applied automatically; when it isn't, the active preset is re-synced so any shared skills keep their sync targets.
- **`manage-skills` skill** (`assets/manage-skills/SKILL.md`) — drop into `~/.claude/skills/` so Claude Code (and other agents) prefers `skills-manager-cli` over installing skills directly into one agent's directory.
- **Cmd/Ctrl+R in the app** — refresh skills, presets, and agent status without restarting (ignored while typing in an input).

### Changed
- **User-facing scenario terminology is now preset terminology** — Tauri commands (`apply_preset_to_default`, etc.), CLI subcommands (`skills-manager-cli presets ...`), CLI JSON fields (`preset_id` / `preset_name`), frontend types, and i18n keys now consistently use `preset`. The CLI keeps `scenarios`, `--scenario`, and `--sync-scenario` as hidden backward-compatible aliases for one release. Internal Rust types, the SQLite schema, and Git Backup metadata still use `scenario` for compatibility.
- **Enable/disable a skill by preset membership** — `presets add-skill` / `presets remove-skill` are now the supported way to include or exclude a skill from sync. The legacy `enabled` flag is no longer consulted when computing what to sync.
- **Sidebar preset selection sticks across external switches** — when the CLI or tray menu switches the active preset, the sidebar only follows if you were already viewing the previous active preset. A preset you're browsing manually is no longer yanked away.

### Deprecated
- **`skills enable` / `skills disable` CLI** — both are now no-ops that print a deprecation notice. Use `presets add-skill` / `presets remove-skill` instead.

### Fixed
- **`presets close <non-active preset>` no longer breaks the active preset's sync** — previously closing a non-active preset removed sync targets for any skill it shared with the active preset; the active preset is now re-synced afterwards.
- **`skills disable` no longer secretly re-enables the skill** — the deprecated command used to flip the legacy `enabled` flag back to `true`, the opposite of what was asked. It now leaves the flag alone.

### Removed
- **SkillsMP AI search** — the third-party `skillsmp.com` integration (API key in Settings, "AI Search" toggle in Install Skills, the `search_skillsmp` Tauri command) has been removed. The free skills.sh marketplace and its keyword search remain. The SkillsMP service was not used by any major agent ecosystem and added a paid third-party dependency without unique value.

## [1.19.3] - 2026-05-17

### Added
- **Report Issue button (Settings → About)** — one click copies app version, OS, enabled agents, UI language, and a smart excerpt of recent logs to the clipboard, then opens a pre-filled GitHub issue template so you just paste and submit.
- **Export Logs button (Settings → About)** — bundles the most recent log files (with sensitive paths and tokens sanitized) into a zip in your Downloads folder and reveals it in your file manager so you can drag it straight into an issue.
- **Crash banner on next launch** — if the previous session crashed, Settings → About now shows a red banner with a one-click report button so unexpected exits don't go unnoticed.
- **GitHub issue templates** — bug reports and feature requests now have lightweight bilingual templates that guide you to use the buttons above.

### Changed
- **Production builds now write a log file** (Info level, 5 MB × 3 rotation). User home paths, git credentials, tokens, and email addresses are sanitized before anything is exported or copied. Repeated noisy lines are collapsed so important events stay visible.

### Fixed
- **Runaway git-fetch loop that pinned CPU at 100%+ and could freeze the window** — a self-driving fetch loop (refresh → fetch → file-watcher → refresh) has been cut; on some macOS setups this also presented as the skill preview going black and only `⌘Q` being able to close the app (#144, #69, #151, #150).
- **Tray icon visible on Windows / Linux** — the previous all-white tray icon disappeared on light Windows taskbars; non-macOS platforms now use a colored variant while macOS keeps the template-style white icon (#154, #149).



### Fixed
- **Codex skills now use the official `~/.agents/skills` location** — Codex reads user-level skills only from `~/.agents/skills` per its official docs, but skills-manager was deploying to `~/.codex/skills` (which Codex never reads) and not scanning `~/.agents/skills`. Both deployment target and discovery are now corrected; skills already at the old `~/.codex/skills` remain visible for backward compatibility (#143, #147).
- **GitHub Copilot also scans `~/.agents/skills`** — in addition to the existing `~/.copilot/skills` (#147).
- **Real error message on local install failure** — `[object Object]` no longer shows in the toast when an install fails; the actual error is displayed (#101).
- **Description in the central list refreshes when SKILL.md changes** — editing `SKILL.md` externally now updates the displayed description without re-import (#92).
- **No more false "install failed" toast when install actually succeeded** — post-install refresh failures (background scan / state refresh) are now silently logged instead of being surfaced as install errors (#92).
- **Changing the central repository path twice before restart no longer loses data** — the migration source is now tracked even across multiple path changes within one session (#92).
- **Multi-variant skill installs prefer the generic version** — when a repo ships several agent-specific variants (`.cursor/skills/<id>`, `.claude/skills/<id>`, …), the installer now consistently picks `.agents/skills/<id>` instead of an arbitrary one (#103).

## [1.19.1] - 2026-05-15

### Fixed
- **macOS "app is damaged" error on first launch** — Release builds are now ad-hoc signed in CI, so downloading the `.dmg` no longer triggers the Gatekeeper "damaged" warning that forced users to run `xattr -cr` manually (#138).
- **Black screen when opening a skill detail on older macOS** — The skill detail sheet now uses explicit stacking, fixing a regression where the panel rendered as a black overlay on Monterey/older WKWebView versions (#69, #144).
- **Importing skills from nested category folders** — `git` skill import now walks nested category directories instead of only looking at top-level folders, so repos that organize skills under subcategories import correctly (#121).
## [1.19.0] - 2026-05-13

### Added
- **Agent-local skills in Global Workspace** — Each agent's page now lists every skill in its global folder, including ones installed outside Skills Manager. Per agent you can upload a local-only skill into your central library, pull library updates down to a local copy, or remove a managed one — with search and tag filtering on the list.

### Changed
- **Install skills straight from the card** — Every skill card now shows an agent icon badge for each enabled agent (replacing the old two-letter labels). Click a badge to install or remove that skill for that agent right from the card; the badge shows live sync state with a spinner while the change is applied.
- **Customizable agent order** — Settings lets you drag to reorder agents within each group (mainstream / more / custom), and that order is used everywhere agents appear — skill card badges, workspace lists, and toggles.
- **Unified skill-card click** — Clicking anywhere on a skill card opens its detail panel in the Library, Global Workspace, and Project Workspace; action buttons no longer also trigger the card click.
- **Help dialog** — Added a "Global Workspace" entry and refreshed the Library and Settings entries to cover the new agent icon badges and agent reordering.

### Fixed
- **OpenCode project skills path** — Project-level skills for OpenCode are now installed to `<project>/.opencode/skills/`, where OpenCode actually reads them, instead of `<project>/.config/opencode/skills/`.
- **Opening an agent in Global Workspace no longer reloads the page several times** — the agent-local skills list is fetched once per agent, and a slow request left over from a previously selected agent can no longer overwrite the current one.
- **CLI hardening** — `skills-manager-cli` now returns JSON error envelopes when `--json` is set (including argument-parse errors), refuses to clone into a non-empty non-git directory, sets a 5-second SQLite busy timeout so running it alongside the desktop app doesn't fail immediately, and handles `PATH` correctly on Windows.

## [1.18.0] - 2026-05-09

### Changed

- **Scenarios renamed to Presets** — "场景 / Scenario" has been renamed to "Preset" throughout the app (UI labels, sidebar, settings, help, and all translations). If you were using scenarios, they are now called Presets and work exactly the same way — no data migration needed.
- **Preset bar replaces the "Apply Preset" modal** — Presets now appear as inline pill tags directly below the search and tag filters in Global Workspace and Project Workspace. Click a pill to instantly activate or deactivate all its skills for the current agent scope. Active presets show ✓; partially installed ones show an installed/total count. No more modal dialog.
- **Global Workspace redesigned** — Each agent now has its own dedicated page accessible from the sidebar. Use the pinned **All Agents** entry to manage skills across every installed agent at once. Tag filters, multi-select, and batch remove are all available per-agent.
- **Sidebar improvements** — The Presets and Project Workspaces sections can be collapsed. Agents in the Global Workspace section support drag-to-reorder.
- **Agent icons added** — Built-in agents now show their own icons across Settings, Global Workspace, project dialogs, and agent toggles, making multi-agent lists easier to scan.
- **More Preset icons** — Presets now offer a broader icon picker, including options for agents, CLI work, data, analytics, research, security, automation, infrastructure, and experiments.

## [1.17.0] - 2026-05-07

### Added
- Agent-friendly CLI (`skills-manager-cli`) to operate on the skills repo without opening the desktop app — list, inspect, and export skills; preview and apply scenarios; run git backup commands. Supports `--json` for scripting and `--skills-root` to point at any cloned skills checkout. Install with `npm run cli:install`.

### Fixed
- Git Backup: cloning a remote skills repository on Windows no longer fails — the repo lock has been moved outside the skills directory so the clone target can be empty when needed.
- CLI: `--skills-root` no longer writes `skills-manager.db` and other manager state into the parent directory of the cloned skills repo. Per-checkout state now lives under `~/.skills-manager/external/`, namespaced by the canonical path of the skills root.

## [1.16.1] - 2026-05-01

### Changed
- Project pages now feature **Add Skills to Project** as the primary action — a high-contrast button right next to the project title, plus a one-time inline tip showing where to bulk-add by tag.
- The Add Skills dialog calls out tag filtering ("Filter by tag — pick one or more tags to bulk-add related skills") so the batch workflow is discoverable instead of hidden.
- Empty project pages now show a clear **Add Skills from Library** call-to-action so first-time visitors know what to do next.
- Added a new **Recommended Workflows** entry to the Help dialog covering single-agent, multi-project, and multi-machine flows.

## [1.16.0] - 2026-05-01

### Changed
- Clicking a scene in the sidebar now only opens it for browsing/editing — it no longer immediately syncs skills to your agents. Use the new **Apply to Default** button at the top of My Skills to sync the viewed scene whenever you're ready. The first time you open a scene after upgrading, an inline tip explains the new flow.

### Added
- Show **Applied** / **Not applied yet** status next to the scene title so it's clear which scene is currently live on disk vs. which one you're editing.
- Warn when no agent is enabled/installed so you can't accidentally trigger an apply with no target.

## [1.15.2] - 2026-04-29

### Changed
- Replaced the single-skill delete confirmation modal with an inline popover next to the trash button. Deletions now run in the background with a per-card spinner, so you can keep deleting other skills without waiting for each one to finish.

### Fixed
- Sped up scenario switching, especially for libraries with many skills.

## [1.15.1] - 2026-04-28

### Added
- Show real-time clone progress while installing skills from Git repositories.
- Cache cloned Git repositories to speed up repeated installs and reduce network wait time.

### Changed
- Redesigned the Git backup experience with clearer health status and recovery actions.
- Improved the Git toolbar layout to reduce crowding around filter controls.
- Use symlinks as the default sync mode for faster scenario switching and a single source of truth.

### Fixed
- Improved Git sync robustness and recovery behavior.
- Avoided no-op commit failures when initializing Git backup.
- Hardened sync metadata handling across lifecycle events and Windows directory cleanup.
- Improved cached Git checkout isolation and materialization reliability.
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
