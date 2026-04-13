# Matrix Architecture Fix — Sound Mixer Model

## Problem

The Matrix view's per-skill per-agent toggles fail for pack-inherited skills. Two API endpoints (`get_skill_tool_toggles` and `set_skill_tool_toggle` in `sync.rs`) check skill membership using `get_skill_ids_for_scenario()`, which only queries the `scenario_skills` table (direct assignments). Skills inherited via packs are rejected with "Skill is not enabled in this scenario", causing the UI to flash the toggle then revert.

## Root Cause

```
get_skill_ids_for_scenario()  →  queries scenario_skills only
is_skill_in_effective_scenario()  →  queries scenario_skills + pack_skills via scenario_packs
```

The API uses the first; it should use the second.

## Architecture Model

The correct mental model is a Sound Mixer:
- **Matrix** = ground truth (skill × agent = on/off per cell)
- **Scenario** = saved preset (loads a complete Matrix snapshot)
- **Pack** = group fader (batch-toggle a group of skills)

These three concepts serve one goal and must not conflict:
1. **Effective skills** determine which rows exist (packs + direct skills)
2. **Per-agent toggles** determine each cell's value (scenario_skill_tools)
3. **Scenario switch** = swap rows + load corresponding toggle values
4. **Pack toggle** = batch-change toggle values for a group of rows

## Changes

### Backend: `src-tauri/src/commands/sync.rs`

**`get_skill_tool_toggles`** (line ~164):
- Replace `get_skill_ids_for_scenario().contains()` with `is_skill_in_effective_scenario()`
- This allows pack-inherited skills to have their toggles queried

**`set_skill_tool_toggle`** (line ~223):
- Same replacement
- This allows pack-inherited skills to have their toggles set

### Backend: `crates/skills-manager-core/src/skill_store.rs`

**`ensure_scenario_skill_tool_defaults`** — verify it works with effective skills (it should, since it operates on the `scenario_skill_tools` table directly with a scenario_id + skill_id pair, regardless of how the skill entered the scenario).

### Frontend: `src/views/MatrixView.tsx`

**Show all effective skills, not just pack skills:**
- Load effective skills for the active scenario
- Group by pack (skills that belong to a pack)
- Add "Ungrouped" section for effective skills not in any pack
- Load ALL toggles on initial render (not just when expanded)
- Remove the expand-to-load pattern — all data available immediately

### CLI: `crates/skills-manager-cli/src/commands.rs`

**`unsync_scenario` removes symlinks for ALL agents, not just claude_code:**

The CLI unsync logic (line ~242) scans every agent's skills directory and removes symlinks pointing to `~/.skills-manager/skills/`. This is correct. BUT for copy-mode agents (Cursor), it removes directories matching skill names from the *current scenario's effective list* — which is the NEW scenario after `set_active_scenario` has already been called. This means copy-mode agents lose skills that exist in both old and new scenarios.

**Fix:** In `cmd_switch`, call `unsync_scenario` BEFORE `set_active_scenario`. Currently the order is:
1. `unsync_scenario(old_id)` — correct, called before set_active
2. `store.set_active_scenario(target.id)` — sets new active
3. `sync_scenario(target.id)` — syncs new

Wait — looking at the code again (line 89-93), unsync IS called before set_active. The actual bug may be in `sync_scenario` — some skills fail to sync silently (the `source.exists()` check on line 309 skips skills whose central_path doesn't exist, and the error is only a warning).

**Actual fix needed:** `sync_scenario` should report skipped skills more visibly, and ensure ALL effective skills are synced. The "source path is neither a regular file" error from the Tauri app suggests `sync_engine::sync_skill` has issues with certain directory structures.

### Not Changed

- DB schema — no changes needed
- Pack CRUD — unchanged
- Plugin management — unaffected

## Verification Criteria

1. Matrix toggle works for pack-inherited skills (no flash/revert)
2. Matrix toggle works for direct skills (backward compat)
3. Ungrouped skills appear in Matrix
4. "source path" error does not occur for normal toggle operations
5. All existing tests pass
6. New test: `set_skill_tool_toggle` accepts effective (pack) skills
7. `sm switch` syncs ALL effective skills (no missing symlinks)
8. `sm switch` back and forth leaves correct skill count

## Out of Scope

- Scenario preset save/restore (future enhancement)
- Pack reorder drag-and-drop (Phase 4 follow-up)
- Token budget display (future enhancement)
