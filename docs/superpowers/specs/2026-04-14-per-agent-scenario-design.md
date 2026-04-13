# Per-Agent Scenario Assignment — Design Spec

## Problem

All agents share one global active scenario. When you switch scenarios, every agent gets the same skill set. There's no way to give Claude Code `full-dev + marketing` while Codex uses `minimal`.

## Architecture: Base Scenario + Pack Overrides per Agent

Each agent independently has:
- A **base scenario** (e.g., full-dev, minimal, standard)
- Optional **extra packs** added on top (e.g., +marketing)
- An **effective skill list** = scenario skills ∪ extra pack skills

```
Agent Config:
  claude_code: scenario=full-dev, +packs=[marketing]  → 133 skills
  cursor:      scenario=full-dev                       → 98 skills
  codex:       scenario=minimal                        → 17 skills
  hermes:      unmanaged                               → (skip)
```

Scenarios remain globally defined (shared pool). Assignment is per-agent.

## DB Schema Changes (Migration v6 → v7)

```sql
-- Per-agent scenario assignment + pack overrides
CREATE TABLE agent_configs (
    tool_key TEXT PRIMARY KEY,              -- e.g., "claude_code"
    scenario_id TEXT REFERENCES scenarios(id) ON DELETE SET NULL,
    managed INTEGER NOT NULL DEFAULT 1,     -- 0 = unmanaged (skip sync)
    updated_at INTEGER
);

-- Per-agent extra packs (on top of scenario)
CREATE TABLE agent_extra_packs (
    tool_key TEXT NOT NULL,
    pack_id TEXT NOT NULL REFERENCES packs(id) ON DELETE CASCADE,
    PRIMARY KEY(tool_key, pack_id)
);
```

The existing `active_scenario` table is preserved for backward compatibility (used by the global `sm switch` command and the Tauri tray menu). When a global switch happens, all managed agents update to the new scenario.

## Core Functions

### `skill_store.rs` additions

- `get_agent_config(tool_key)` → Option<AgentConfig>
- `set_agent_scenario(tool_key, scenario_id)` — set base scenario for an agent
- `set_agent_managed(tool_key, managed)` — mark agent as managed/unmanaged
- `add_agent_extra_pack(tool_key, pack_id)` — add extra pack
- `remove_agent_extra_pack(tool_key, pack_id)` — remove extra pack
- `get_agent_extra_packs(tool_key)` → Vec<PackRecord>
- `get_effective_skills_for_agent(tool_key)` → Vec<SkillRecord> — returns scenario skills ∪ extra pack skills
- `init_agent_configs(adapters)` — on first run, seed agent_configs from current active_scenario for all installed agents

### Sync changes

`sync_scenario_skills()` and `unsync_scenario_skills()` updated to use per-agent configs:
- For each managed agent, get its own effective skill list
- Sync only that agent's skills to its directory

Global scenario switch (`sm switch`, tray menu) updates all managed agents to the same scenario, preserving extra packs.

### CLI changes

- `sm switch <scenario>` — updates all managed agents (backward compat)
- `sm switch <agent> <scenario>` — updates one agent only
- `sm agents` — list agents with their assigned scenarios
- `sm agent <name> add-pack <pack>` — add extra pack to agent
- `sm agent <name> remove-pack <pack>` — remove extra pack

## UI Changes

### Sidebar

SCENARIOS section replaced by AGENTS section:
- Each agent shows: status dot (green=active, grey=unmanaged) + name + scenario name + extra pack count
- Click agent → navigate to Agent Detail page
- Scenarios remain in sidebar as a collapsed "Presets" section (for quick "apply to all" action)

### New Page: Agent Detail (`/agent/:toolKey`)

Layout (top to bottom):
1. **Header** — agent name, skills dir path, sync mode, status badge
2. **Base Scenario** — dropdown to select scenario, Apply button
3. **Additional Packs** — checkboxes for packs not in base scenario, with skill counts
4. **Effective Skills** — summary bar (N from scenario + N from packs) + tag cloud of skill names

### New Page: Skills Matrix (merges Packs + old Matrix)

The existing Matrix view enhanced:
- Top section: pack management (create/edit/delete packs, assign skills)
- Bottom section: agent × pack/skill toggle grid (existing matrix)
- Tab-based or collapsible sections

### Preserved Pages

- Dashboard — updated to show per-agent status instead of single scenario
- My Skills — preserved as-is (fine-grained skill management)
- Install Skills — unchanged
- Plugins — unchanged
- Settings — unchanged

## Migration Strategy

On first launch after migration:
1. Create `agent_configs` rows for all installed agents, all pointing to the current `active_scenario`
2. No `agent_extra_packs` rows (no overrides yet)
3. Behavior is identical to before — all agents share the same scenario

This ensures zero disruption on upgrade.

## Verification Criteria

1. After migration, app behaves identically (all agents same scenario)
2. Can assign different scenarios to different agents
3. Extra packs add skills on top of base scenario
4. `sm switch` updates all managed agents (backward compat)
5. `sm switch claude_code minimal` updates one agent only
6. Agent Detail page shows correct effective skills
7. Skills Matrix shows per-agent toggles correctly
8. Tray menu scenario switch updates all agents
9. All existing tests pass + new tests for per-agent logic

## Out of Scope

- Removing My Skills page (deferred until new pages are validated)
- Per-agent plugin management (plugins already have per-scenario toggles)
- Scenario editing UI (scenarios are managed via existing sidebar)
