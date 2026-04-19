# Progressive Disclosure — Parallel Execution Bundles

**Baseline commit:** `16963e5` on `docs/skill-pack-taxonomy` (Phase 1 merged: v9 migration + preservation test, 188 tests passing)

**Plan reference:** `docs/superpowers/plans/2026-04-19-progressive-disclosure.md`
**Spec reference:** `docs/superpowers/specs/2026-04-19-progressive-disclosure-design.md`

This doc splits the remaining 21 tasks into **5 bundles** you can run in parallel Claude Code sessions. Each bundle lists its worktree branch, dependencies, the tasks it owns, and a one-line briefing to paste into the new session.

## Dependency Graph

```
Phase 1 (done, aaec3bb + 16963e5)
  │
  ├── Bundle A: Store API (T3 → T4 → T5)
  │     │
  │     ├── Bundle C: Seed + Builtin skill (T9, T10)  ← needs A
  │     ├── Bundle D: Render + Sync engine (T6 → T7 → T8)  ← needs A
  │     └── Bundle E: CLI (T11 → T12 → T13)  ← needs A
  │
  │     (wait for A+D+E) → Bundle F: Tauri IPC (T14) + Frontend (T15–T21)
  │
  └── Bundle F → Bundle G: Eval + Docs (T22, T23)
```

**Practical parallelism:** A runs alone first (all downstream depends on it). After A merges back, C / D / E can run simultaneously in three sessions. F waits for all three. G is last.

If you want to start **two sessions right now**: start A (blocker) + C-prep (reading plan, drafting taxonomy data file) in parallel; C-implementer actually executes after A lands.

---

## Bundle A — Store API foundation

**Branch:** `feat/pd-store-api`
**Base:** `16963e5`
**Depends on:** Phase 1 (done)
**Blocks:** Bundles C, D, E (all need pack/scenario store changes)
**Tasks:** Task 3, Task 4, Task 5 (plan Tasks 3–5)
**Files touched:** `crates/skills-manager-core/src/skill_store.rs`, possibly `lib.rs` for new enum re-exports

**Scope:**
- Task 3: Extend `PackRecord` with `router_description`, `router_body`, `is_essential`, `router_updated_at`. Update `insert_pack` / `get_pack_by_*` / `get_all_packs` to read/write new cols.
- Task 4: Add `set_pack_router(pack_id, desc?, body?, ts)` + `set_pack_essential(pack_id, bool)`.
- Task 5: Add `DisclosureMode` enum (`Full | Hybrid | RouterOnly` + `as_str()` + `parse()`). Extend `ScenarioRecord` with `disclosure_mode`. Update scenario store reads/writes. Add `set_scenario_disclosure_mode(id, mode)`.

**Session brief** (paste after spawning new CC session in worktree):
```
I'm using the superpowers:executing-plans skill. Work on branch feat/pd-store-api in
/Users/jfkn/.superconductor/worktrees/skills-manager/sc-superfluid-niobium-1f14. Execute
Tasks 3, 4, 5 from docs/superpowers/plans/2026-04-19-progressive-disclosure.md (Phase 2
and Phase 3). Branch off 16963e5. Follow TDD exactly. Run `cargo test -p
skills-manager-core` after each task. Commit each task separately per plan.
```

---

## Bundle C — Builtin skill + Seed taxonomy

**Branch:** `feat/pd-seed`
**Base:** merged tip of A (after A's PR lands) OR rebased as A progresses
**Depends on:** A (needs `set_pack_essential`, `set_scenario_disclosure_mode`, `PackRecord.is_essential`)
**Blocks:** nothing critical; could land anytime after A
**Tasks:** Task 9 (builtin skill), Task 10 (v9 taxonomy seed) — note Task 9 has no store dep so *can* run without A if worktree branches off Phase 1
**Files touched:** `crates/skills-manager-core/src/builtin_skills.rs` (new), `assets/builtin-skills/pack-router-gen/SKILL.md` (new), `src/pack_seeder.rs`, `src/lib.rs`

**Session brief:**
```
I'm using the superpowers:executing-plans skill. Work on branch feat/pd-seed in the
skills-manager worktree. Execute Task 9 (pack-router-gen builtin skill) and Task 10
(v9 pack taxonomy seed) from docs/superpowers/plans/2026-04-19-progressive-disclosure.md.
Task 9 can proceed immediately (branches off Phase 1 tip 16963e5). Task 10 requires
Bundle A (feat/pd-store-api) to be merged first — rebase onto merged main before
starting Task 10. Follow TDD. Run cargo tests after each commit.
```

---

## Bundle D — Render + Sync engine

**Branch:** `feat/pd-sync`
**Base:** tip of A merged
**Depends on:** A (needs `PackRecord.router_description` and `DisclosureMode`)
**Blocks:** nothing — but F (Tauri preview endpoint) will import `router_render`
**Tasks:** Task 6 (router_render module), Task 7 (disclosure resolver), Task 8 (reconciliation)
**Files touched:** `crates/skills-manager-core/src/router_render.rs` (new), `src/sync_engine.rs` → `src/sync_engine/mod.rs` + `src/sync_engine/disclosure.rs` (refactor), `src/lib.rs`

**Note:** Task 8 integrates with the in-progress Agent Native Skills Management marker system. Use the symlink + `pack-*` heuristic in the interim per plan. Call out in PR description so native-skills PR author can reconcile.

**Session brief:**
```
I'm using the superpowers:executing-plans skill. Work on branch feat/pd-sync off
merged main after Bundle A lands. Execute Tasks 6, 7, 8 from
docs/superpowers/plans/2026-04-19-progressive-disclosure.md (Phases 4 and 5).
Includes a file-to-module refactor in Task 7 — keep existing sync_engine tests green
through the restructure. For Task 8, use the symlink + pack-* heuristic for
is_sm_managed per plan note; Agent Native Skills marker integration is a follow-up.
```

---

## Bundle E — CLI

**Branch:** `feat/pd-cli`
**Base:** tip of A merged
**Depends on:** A (store access)
**Blocks:** F (Tauri `write_pending_router_marker` uses `pending_router_gen` module from T13)
**Tasks:** Task 11 (pack context), Task 12 (set-router / list-routers), Task 13 (gen-router + marker I/O)
**Files touched:** `crates/skills-manager-core/src/pending_router_gen.rs` (new), `src/lib.rs`, `crates/skills-manager-cli/src/commands.rs`, `src/main.rs`, `tests/pack.rs` (new)

**Session brief:**
```
I'm using the superpowers:executing-plans skill. Work on branch feat/pd-cli off merged
main after Bundle A. Execute Tasks 11, 12, 13 from the progressive-disclosure plan
(Phase 8). Task 13 creates crates/skills-manager-core/src/pending_router_gen.rs — that
module is also used by Bundle F (Tauri), so surface the public API clearly. Follow TDD.
```

---

## Bundle F — Tauri IPC + Frontend

**Branch:** `feat/pd-frontend`
**Base:** merged tips of A + D + E
**Depends on:** A, D (sync engine & router_render for preview), E (pending_router_gen module)
**Blocks:** G (docs references final UI)
**Tasks:** Task 14 (Tauri IPC), Tasks 15–21 (frontend: RouterEditor, PacksView integration, DisclosureModeSelect, TokenEstimateBadge, MatrixView, Sidebar, Dashboard)
**Files touched:** `src-tauri/src/commands/{packs,scenarios,router_gen}.rs`, `src-tauri/src/lib.rs`, `src/components/*.tsx`, `src/views/{PacksView,MatrixView,Dashboard,Sidebar}.tsx`

**Session brief:**
```
I'm using the superpowers:subagent-driven-development skill (frontend benefits from
fresh subagent per component). Work on branch feat/pd-frontend off merged main after
Bundles A, D, E land. Execute Tasks 14–21 from the plan (Phases 9–13). Task 14
(Tauri) must land first; components can dispatch in parallel via subagents.
Run `pnpm test` + `cargo tauri dev` smoke before committing each task.
```

---

## Bundle G — Eval + Docs

**Branch:** `feat/pd-eval-docs`
**Base:** merged main after F
**Depends on:** all prior bundles
**Tasks:** Task 22 (router accuracy eval — stretch, optional), Task 23 (PROGRESS.md + CLAUDE.md + README)

**Session brief:**
```
I'm using the superpowers:executing-plans skill. Work on branch feat/pd-eval-docs off
merged main after all other bundles. Execute Task 23 (docs updates) mandatory and
Task 22 (router eval harness) if time permits. Plan: docs/superpowers/plans/
2026-04-19-progressive-disclosure.md Phases 14–15.
```

---

## Worktree Setup Commands

From inside Superconductor, spawn new tabs per bundle. Git commands to create worktrees manually if needed:

```bash
cd /Users/jfkn/.superconductor/worktrees/skills-manager/sc-superfluid-niobium-1f14

# After Bundle A lands and is on main:
git worktree add ../pd-seed     -b feat/pd-seed     main
git worktree add ../pd-sync     -b feat/pd-sync     main
git worktree add ../pd-cli      -b feat/pd-cli      main

# After A + D + E land:
git worktree add ../pd-frontend -b feat/pd-frontend main

# Last:
git worktree add ../pd-eval-docs -b feat/pd-eval-docs main
```

Each worktree is an independent directory; opening a Claude Code session in that directory gives it isolated `git status` and branch state.

## Merge Order

1. Bundle A — PR → merge to main
2. Bundles C, D, E — open PRs in parallel, rebase each on A's merge commit, merge in any order
3. Bundle F — PR after all above, merge
4. Bundle G — PR, merge

## Start State Right Now

Current branch `docs/skill-pack-taxonomy` at `16963e5` contains Phase 1 + docs. Recommended: convert `docs/skill-pack-taxonomy` into PR "Phase 1: PD schema migration" and merge to main before spawning Bundle A (gives it a clean base).
