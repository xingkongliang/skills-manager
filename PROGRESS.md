# Skills Manager Fork — Development Progress

## Overall Status

```
Phase 1 ✅ → Phase 2 ✅ → Phase 3 ✅ → Phase 4 ✅ → Phase 5 ✅
Per-Agent ✅ → Matrix Fix ✅ → Native Skills 🔄 → Pack Seeding ⬜ → Dashboard ⬜ → Tray Menu ⬜
```

✅ = merged | 🔄 = in progress | ⬜ = planned

---

## Completed Phases

### Phase 1: Core Crate Extraction + Skill Packs ✅
- **PR:** #1 (merged 2026-04-13)
- Core crate at `crates/skills-manager-core/`, DB v5, pack CRUD, effective skill resolution

### Phase 2: CLI Binary ✅
- **PR:** #3 (merged 2026-04-13)
- `sm` CLI replacing shell script, pack-aware, installed at `~/.local/bin/sm`

### Phase 3: Plugin Management ✅
- **PR:** #4 (merged 2026-04-13)
- DB v6, plugin discovery, per-scenario enable/disable via `installed_plugins.json`

### Phase 4: Packs UI ✅
- **PR:** #5 (merged 2026-04-13)
- PacksView with CRUD, icon/color picker, skill assignment

### Phase 5: Matrix View + Plugin UI ✅
- **PR:** #6 (merged 2026-04-13)
- MatrixView (agent × pack grid), PluginsView with per-scenario toggles

### Matrix Architecture Fix ✅
- **PR:** #8 (merged 2026-04-14)
- Fixed toggle flash/revert for pack-inherited skills
- MatrixView shows all effective skills + ungrouped section
- Fixed `copy_dir_recursive` symlink handling for Cursor

### Per-Agent Scenario Assignment ✅
- **PR:** #9 (merged 2026-04-15)
- DB v7: `agent_configs` + `agent_extra_packs` tables
- Each agent independently has base scenario + extra packs
- Agent Detail page, Sidebar AGENTS section
- CLI: `sm agents`, `sm switch <agent> <scenario>`, `sm agent add-pack/remove-pack`

---

## Current Iteration: Polish + Features

### Agent Native Skills Management 🔄
**Status:** Starting
**Goal:** Identify and manage agent-native skills (pre-installed by agent, not SM). Show in Agent Detail page. Prevent SM from overwriting native skills.

### Default Pack Seeding ⬜
**Status:** Planned
**Goal:** Seed 132 skills into 9 packs (base, gstack, marketing, etc.) on first run

### Dashboard Update ⬜
**Status:** Planned
**Goal:** Show per-agent status instead of single global scenario

### Tray Menu Update ⬜
**Status:** Planned
**Goal:** Per-agent quick switch in tray menu

### My Skills Retirement ⬜
**Status:** Planned (low priority)
**Goal:** Evaluate after new pages are validated

### Cursor Copy Fix ⬜
**Status:** Planned (low priority)
**Goal:** Further improve copy_dir_recursive edge cases

---

## References

- Design specs: `docs/superpowers/specs/`
- Implementation plans: `docs/superpowers/plans/`
- Development plan: `DEVELOPMENT_PLAN.md`
- Project instructions: `CLAUDE.md`
