# Skills Manager Fork — Development Progress

## Overall Status

```
Phase 1 ✅ → Phase 2 🔄 → Phase 3 ⬜ → Phase 4 ⬜ → Phase 5 ⬜
```

---

## Phase 1: Core Crate Extraction + Skill Packs ✅

**Completed:** 2026-04-13
**PR:** knjf/skills-manager#1 (merged)
**Branch:** phase1/core-extraction-packs → main

### What was done
- Extracted 18 core modules into `crates/skills-manager-core/` library crate
- Created Cargo workspace (core crate + Tauri app)
- Feature-gated `error.rs` tauri/tokio deps (`tauri-compat`, `tokio-compat`)
- Added Skill Packs: DB migration v4→v5, pack CRUD, effective skill resolution
- Pack-aware scenario sync (`remove_skill_from_scenario` checks pack membership)
- 12 new Tauri IPC commands for pack management
- 146 tests pass, core builds standalone

### Deferred items
- Default pack seeding (needs UI or CLI to be useful)
- 3 reorder methods (reorder_packs, reorder_pack_skills, reorder_scenario_packs) — needed when Phase 4 UI is built

### Key files
- `crates/skills-manager-core/src/skill_store.rs` — PackRecord, pack CRUD, effective skill resolution
- `crates/skills-manager-core/src/migrations.rs` — v4→v5, PACKS_SCHEMA_DDL constant
- `src-tauri/src/commands/packs.rs` — 12 Tauri IPC wrappers
- `docs/superpowers/specs/2026-04-13-phase1-core-extraction-packs-design.md` — design spec
- `docs/superpowers/plans/2026-04-13-phase1-core-extraction-packs.md` — implementation plan

---

## Phase 2: CLI Binary 🔄

**Status:** Starting
**Goal:** Rust CLI binary (`sm`) using `clap`, replacing `~/.local/bin/sm` shell script
**Depends on:** Phase 1 core crate ✅

### Planned commands
```
sm switch <scenario>     # switch active scenario
sm list                  # list scenarios
sm current               # show current scenario
sm packs [scenario]      # list packs in a scenario
sm diff <a> <b>          # diff two scenarios
sm pack add <pack> <scenario>
sm pack remove <pack> <scenario>
```

### Key decisions (TBD during brainstorming)
- Binary name: `sm` vs `skills-manager-cli`
- Installation method: cargo install vs direct binary
- Output format: human-readable vs JSON flag

---

## Phase 3: Plugin Management ⬜

**Status:** Not started
**Goal:** Per-scenario enable/disable of Claude Code plugins
**Depends on:** Phase 1 core crate ✅

---

## Phase 4: Packs UI ⬜

**Status:** Not started
**Goal:** Frontend PacksView, scenario editing with pack toggles, dashboard pack cards
**Depends on:** Phase 1 packs backend ✅

---

## Phase 5: Matrix View + Plugin UI ⬜

**Status:** Not started
**Goal:** Agent × pack/skill toggle matrix, plugin management UI, token budget display
**Depends on:** Phase 3 + Phase 4

---

## Development Workflow

每個 Phase 嘅流程：
```
brainstorming → writing-plans → [plan-eng-review]
→ subagent-driven-development (TDD)
→ simplify → review → git-commit-push-pr
```

Skills toolkit 詳見 `CLAUDE.md` → "Development Workflow — Skills Toolkit"

## References

- Design specs: `docs/superpowers/specs/`
- Implementation plans: `docs/superpowers/plans/`
- Development plan: `DEVELOPMENT_PLAN.md`
- Project instructions: `CLAUDE.md`
