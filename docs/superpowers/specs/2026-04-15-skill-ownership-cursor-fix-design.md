# Skill Ownership + Cursor Fix + Discovered Import

## Problems

1. **18 unmanaged skills in Claude Code** — SM discovered them but never imported. They should be importable into SM management.
2. **Cursor hardcoded to copy mode** — `sync_engine.rs` forces copy for Cursor, but macOS supports symlinks fine. 97 copies waste space and create false "native" detection.
3. **No skill ownership visibility** — Agent Detail page doesn't show which skills are SM-managed vs discovered vs native vs plugin.

## Changes

### 1. sync_engine.rs — Remove Cursor copy mode hardcode

Change `sync_mode_for_tool`:
```rust
// BEFORE: "cursor" => SyncMode::Copy,
// AFTER: remove this line — Cursor defaults to Symlink like everyone else
```

Users who genuinely need copy mode can set `sync_mode = "copy"` in Settings (already supported via `configured_mode` parameter).

Cursor migration happens automatically: next `sm switch` or scenario change unsyncs old copies and syncs new symlinks.

### 2. Core: scan_agent_skill_ownership

New function in core crate: `scan_agent_skill_ownership(store, tool_key)` → `AgentSkillOwnership`

```rust
pub struct AgentSkillOwnership {
    pub managed: Vec<SkillRecord>,
    pub discovered: Vec<DiscoveredSkillRecord>,
    pub native: Vec<String>,
}
```

Logic:
- **managed**: skills synced to this agent (symlinks pointing to `~/.skills-manager/skills/` OR matching SM scenario skills)
- **discovered**: entries in `discovered_skills` table for this agent's tool key where `imported_skill_id IS NULL`
- **native**: directories in agent's skills dir that are NOT symlinks to SM AND NOT in discovered list

### 3. Tauri commands

- `get_agent_skill_ownership(tool_key)` → serialized AgentSkillOwnership
- `import_discovered_skills(tool, skill_ids)` — bulk import: for each discovered skill, copy to SM central, insert into `skills` table, add to active scenario, create symlink

### 4. Agent Detail page update

Add "Skills Breakdown" section showing:

```
SKILLS BREAKDOWN
  SM-Managed (98)     ← synced from scenario + packs
  Discovered (15)     ← found, not yet imported  [Import All]
  Native (25)         ← agent's own skills (read-only)
```

Each category expandable to show skill names. "Import All" button for discovered skills.

### 5. CLI

`sm agent <key>` shows ownership breakdown:
```
claude_code:
  Scenario: full-dev (98 skills)
  Discovered: 15 skills (not imported)
  Native: 0 skills
```

## Not Changed

- DB schema — no new tables needed
- Pack management — unchanged
- Plugin display — plugins shown separately (already have their own section)
- Settings sync_mode override — still works for users who need copy mode

## Verification

1. Cursor uses symlinks after switch (no more copies)
2. Agent Detail shows managed/discovered/native breakdown
3. "Import All" imports discovered skills into SM
4. Native skills (Hermes) correctly identified
5. SM switch does not overwrite native skills
6. All existing tests pass
