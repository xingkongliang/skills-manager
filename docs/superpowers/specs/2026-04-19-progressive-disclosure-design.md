# Progressive Disclosure for Skills Manager

## Problem

Claude Code scans `~/.claude/skills/**/SKILL.md` at session start and injects every `description` into the system prompt. With 189 shared skills plus plugin skills, this consumes ~15–20K tokens of system-prompt budget before any user work begins.

At the same time, the current pack taxonomy in Skills Manager is coarse:
- `base` (13) has scraping tools mixed with essentials
- `gstack` (45) is a single monolithic pack
- `marketing` has 4 skills while ~30 marketing-adjacent skills are uncategorized
- ~80 skills sit outside any pack

The user wants to keep access to all 189 skills, add many more packs with clear scoping, and *still* shrink the default system-prompt cost — without losing the ability to bring any skill into context when needed.

## Goals

1. Reduce default system-prompt token overhead for skill metadata by ≥80%.
2. Preserve discoverability — any relevant skill must still be reachable within one or two additional tool calls.
3. Keep the central skill vault (`~/.skills-manager/skills/`) as single source of truth; no content duplication.
4. Backward-compatible with existing `full`-mode scenarios.
5. Agent-native: every UI action (generate router, switch mode) must also be invocable via CLI and from inside a Claude Code session.

## Non-Goals

- Controlling plugin-sourced skill descriptions (`superpowers:*`, `compound-engineering:*`). Those live in `~/.claude/plugins/` and are Claude Code's concern.
- Mid-session skill hot-swap. Claude Code scans the skills directory once at session start; mode changes take effect next session.
- Per-skill `description_short` metadata. Out of scope; may be added later if UI needs it.

## Architecture: File-Based Routers + Read-on-Demand

Two storage locations for skills, and the mode controls which ones get linked into the agent directory.

```
~/.skills-manager/skills/           ← vault (source of truth, all 189 skills)
  ├── seo-audit/SKILL.md
  ├── ai-seo/SKILL.md
  ├── copywriting/SKILL.md
  └── ... (unchanged)

~/.claude/skills/                   ← hybrid mode contents:
  ├── find-skills/SKILL.md          ← Essential pack skills (full sync)
  ├── skill-creator/SKILL.md
  ├── web-access/SKILL.md
  ├── pack-mkt-seo/SKILL.md         ← Pack routers (short desc only)
  ├── pack-dev-frontend/SKILL.md
  ├── pack-research/SKILL.md
  └── ... (15 routers)
```

Non-essential pack skills stay in the vault and are **never** materialized into the agent directory in `hybrid` mode. Claude Code's directory scan never sees them, so their descriptions never enter the system prompt.

### Three Disclosure Modes

| Mode | Essential skills | Domain pack skills | Pack routers |
|---|---|---|---|
| `full` | materialized | materialized | not generated |
| `hybrid` (default) | materialized | vault-only | materialized |
| `router_only` | vault-only | vault-only | materialized |

### The Read-on-Demand Flow

1. User says "help me audit SEO".
2. Claude sees `pack-mkt-seo`'s router description in the system prompt (keyword match: "SEO").
3. Claude invokes `Skill: pack-mkt-seo`. Router body returns, listing the pack's skills with vault paths.
4. Claude selects `seo-audit` and runs `Read ~/.skills-manager/skills/seo-audit/SKILL.md`.
5. The real skill body enters context. Claude follows its instructions.

This mirrors the deferred-tool pattern Claude Code already uses (tools listed by name in system prompt, schemas fetched via `ToolSearch`).

### Token Math

| Mode | Layer 1 system-prompt cost |
|---|---|
| Today (`full` only) | ~15–20K (189 descriptions) |
| `hybrid` (recommended default) | ~2K (~10 essentials + ~15 routers) |
| `router_only` | ~1.5K (routers only) |

Savings in `hybrid`: ~85–90%.

## Data Model

### v9 → v10 Migration

```sql
ALTER TABLE packs ADD COLUMN router_description TEXT;
ALTER TABLE packs ADD COLUMN router_body TEXT;
ALTER TABLE packs ADD COLUMN is_essential INTEGER NOT NULL DEFAULT 0;
ALTER TABLE packs ADD COLUMN router_updated_at INTEGER;

ALTER TABLE scenarios ADD COLUMN disclosure_mode TEXT NOT NULL DEFAULT 'full';
CREATE INDEX idx_scenarios_mode ON scenarios(disclosure_mode);
```

Field semantics:
- `router_description` — concise frontmatter description (target 150–400 chars: trigger keywords + when-to-use phrasing). `NULL` means router not yet generated; UI shows "Generate" button.
- `router_body` — optional markdown body. `NULL` means auto-render from `pack_skills`.
- `is_essential` — `1` means this pack's skills always materialize in `hybrid`/`router_only` modes; never routed.
- `router_updated_at` — unix timestamp; sync engine uses this to decide when to rewrite the router SKILL.md file on disk.

### Pack Taxonomy (seeded by v10)

Existing scenarios keep their IDs and names; `scenario_packs` rows are restructured in-place.

**Tier 0 — Essential** (`is_essential=1`)
- `essential` — `find-skills`, `skill-creator`, `scenario`, `discover`, `web-access`, `smart-search`, plus new `pack-router-gen`

**Tier 1 — Route packs**
- `route-gstack` — `office-hours`, `autoplan`, `plan-*-review`, `review`, `qa`, `ship`, `investigate`, `document-release`, `retro`, `health`, `learn`, `learned`, `benchmark`, `checkpoint`, `careful`, `freeze`, `unfreeze`, `guard`, `cso`, `canary`, `setup-deploy`, `setup-browser-cookies`, `open-gstack-browser`, `gstack`, `gstack-upgrade`, `devex-review`, `pair-agent`, `qa-only`
- `route-superpowers` — **deferred**. Superpowers ships as a Claude Code plugin, so its skill descriptions are loaded by Claude Code outside SM's control. A future SM-managed "supporting" pack can be added if/when we identify SM-shipped companions. Not included in v10 seed.
- `route-ecc` — `learn`, `learned`, `checkpoint`, `eval`, `build-fix`, `refactor-clean`, `simplify`, `quality-gate`

**Tier 2 — Domain packs**
- `dev-frontend` — `frontend-design`, `stitch-*`, `shadcn-ui`, `taste-design`, `canvas-design`, `brand-guidelines`, `web-design-guidelines`, `vercel-*`, `react-components`, `web-artifacts-builder`, `remotion`, `design-*`, `enhance-prompt`
- `dev-backend` — `supabase-postgres-best-practices`, `data-science`, `mlops`, `devops`, `red-teaming`, `software-development`
- `ai-engineering` — `claude-api`, `mcp-builder`, `skill-creator`, `cli-creator`, `claude-code-router`, `template-skill`
- `browser-automation` — `bb-browser`, `agent-browser`, `opencli`, `opencli-*`, `connect-chrome`, `verify-deploy`, `webapp-testing`, `dogfood`
- `web-research` — `smart-search`, `agent-reach`, `codex-deep-search`, `perp-search`, `last30days`, `x-tweet-fetcher`, `follow-builders`, `autoresearch`, `defuddle`, `obsidian-defuddle`
- `knowledge-library` — `library`, `obsidian-cli`, `obsidian-markdown`, `notebooklm`, `readwise-*`, `reader-recap`, `feed-catchup`, `build-persona`, `triage`, `quiz`, `book-review`, `highlight-graph`, `now-reading-page`
- `docs-office` — `pdf`, `docx`, `pptx`, `xlsx`, `documentation-writer`, `prd`, `internal-comms`
- `agent-orchestration` — `paseo`, `paseo-*`, `paperclip`, `loop`

**Tier 2 — Marketing sub-packs** (replacing monolithic `marketing`)
- `mkt-strategy` — `marketing`, `marketing-ideas`, `marketing-psychology`, `product-marketing-context`, `launch-strategy`, `content-strategy`, `site-architecture`
- `mkt-seo` — `seo-audit`, `ai-seo`, `schema-markup`, `programmatic-seo`, `competitor-alternatives`
- `mkt-copy` — `copywriting`, `copy-editing`, `cold-email`, `email-sequence`, `ad-creative`, `sales-enablement`, `social-content`
- `mkt-cro` — `page-cro`, `signup-flow-cro`, `onboarding-cro`, `paywall-upgrade-cro`, `form-cro`, `popup-cro`, `churn-prevention`, `ab-test-setup`, `analytics-tracking`
- `mkt-revenue` — `pricing-strategy`, `paid-ads`, `referral-program`, `revops`, `lead-magnets`, `free-tool-strategy`

### Scenario Remapping (In-Place)

| Scenario | Disclosure mode | Pack composition |
|---|---|---|
| `minimal` | `full` | essential |
| `core` | `hybrid` | essential + route-gstack |
| `standard` | `hybrid` | essential + route-gstack + route-superpowers + dev-frontend + browser-automation + web-research + knowledge-library |
| `standard-marketing` | `hybrid` | standard + mkt-strategy + mkt-copy + mkt-cro |
| `full-dev` | `hybrid` | standard + dev-backend + ai-engineering + docs-office + agent-orchestration |
| `full-dev-marketing` | `hybrid` | full-dev + all mkt-* |
| `everything` | `full` | all packs (debug/discovery use) |

`minimal` and `everything` stay in `full` mode intentionally: minimal is so small it doesn't matter, and everything is opt-in maximal-context debug mode.

### Migration Safety

- Pre-migration: auto-backup `skills-manager.db` → `skills-manager.db.bak-pre-v10-<timestamp>` (reuse existing backup routine).
- `sm db migrate --dry-run` lists changes without applying.
- New columns only — no drops. Rollback = restore backup.
- Pack/scenario restructuring is destructive to `scenario_packs` / `pack_skills` rows. Backup is the recovery path.

## Sync Engine

### Desired-State Resolution

```rust
fn resolve_desired_state(agent: &Agent, scenario: &Scenario) -> HashSet<PathBuf> {
    let mode = scenario.disclosure_mode;
    let effective_packs = resolve_effective_packs(scenario, agent);
    let mut desired = HashSet::new();

    for pack in &effective_packs {
        let materialize = match mode {
            Full => true,
            Hybrid => pack.is_essential,
            RouterOnly => false,
        };

        if materialize {
            for skill in pack.skills() {
                desired.insert(agent.skills_dir.join(&skill.name));
            }
        }

        if mode != Full && !pack.is_essential {
            desired.insert(agent.skills_dir.join(format!("pack-{}", pack.name)));
        }
    }

    desired
}
```

### Reconciliation

Sync engine scans the agent's skills directory, computes desired state, and takes the minimal diff:

- Additions: symlink/copy skill from vault, or render and write router SKILL.md.
- Removals: unlink only SM-managed entries. Native skills (detected via Phase "Agent Native Skills Management" markers) stay untouched.
- Staleness: if `packs.router_updated_at > agent_router_file.mtime`, rewrite the router.

### Router Rendering

Deterministic. Sync engine does no LLM calls.

```rust
fn render_router_skill_md(pack: &Pack, skills: &[Skill]) -> String {
    let desc = pack.router_description.as_deref()
        .unwrap_or("Router for pack — description pending generation.");
    let body = pack.router_body.clone().unwrap_or_else(|| auto_render_skill_table(pack, skills));
    format!("---\nname: pack-{}\ndescription: {}\n---\n\n{}", pack.name, desc, body)
}
```

Auto-rendered body lists skills as a markdown table with name, first-sentence summary, and absolute vault path for `Read` tool use.

### Edge Cases

- **Concurrent sessions**: if Claude Code is running when mode switches, materialized files disappearing mid-session will confuse the agent. Sync CLI checks for running sessions (reuse Phase 2 check) and warns/prompts.
- **Router name collision**: pack named `research` produces `pack-research`. If a user skill happens to be named `pack-research`, sync refuses and surfaces a rename suggestion.
- **Pack rename**: sync treats it as delete + create. Stale `pack-oldname/SKILL.md` files get cleaned up in reconcile.
- **Cursor / Codex compatibility**: routers are regular SKILL.md files. Agents that don't implement skill scanning will ignore them — router architecture is a Claude-Code–first optimization. Document as a known scope limit.

## Router Generation Flow (Agent-Native)

No external API calls. Generation runs in the user's existing Claude Code session.

### Built-in Skill: `pack-router-gen`

Shipped in `~/.skills-manager/skills/pack-router-gen/SKILL.md`. SM bootstraps this on first run; it is always in the Essential pack.

Responsibilities:
1. Scan `~/.skills-manager/pending-router-gen/*.json` for pending markers.
2. For each marker, call `sm pack context <pack>` to get pack metadata + skill descriptions.
3. Generate router description (target 150–400 chars: trigger keywords + when-to-use phrasing) and optional body, following the quality checklist in the skill body.
4. Write back via `sm pack set-router <pack> --description <text> [--body <file>]`.
5. Delete the marker, print summary.

### New CLI commands

```
sm pack context <pack>              # Print pack context for LLM prompt
sm pack set-router <pack> --description <text> [--body <file>]
sm pack list-routers                # All pack routers + staleness
sm pack gen-router <pack>           # Write pending marker; prompt user to open Claude Code
sm pack regen-all-routers           # Write markers for all packs
```

### UI Trigger Mechanism

File-marker pattern (no real-time IPC required):

1. PacksView "Generate Router" button → Tauri command writes `~/.skills-manager/pending-router-gen/<pack-id>.json` with pack id, name, timestamp, and skill snapshot.
2. Modal tells user: "Open Claude Code. The `pack-router-gen` skill will trigger automatically; refresh this page when done."
3. Skill consumes the marker, writes to DB, deletes the marker.
4. UI polls the markers directory (or reacts to DB change) and refreshes pack status.

Batch path: "Regenerate All" writes a marker per pending pack.

## UI Changes

### PacksView

- Pack list adds columns: `Mode` (Essential / Router) and `Router status` (✓ Generated / ⚠ Pending / — N/A).
- Pack detail page adds a `Progressive Disclosure` section (hidden for essential packs):
  - `is_essential` checkbox with warning copy
  - `router_description` textarea with char counter (green ≤400, yellow ≤600, red >600)
  - `router_body` textarea (placeholder: "Leave empty for auto-rendered table")
  - `Last generated` timestamp
  - Buttons: `Generate with Claude Code`, `Preview Sync Output`, `Reset`

### ScenariosView / Scenario Editor

- `Disclosure Mode` dropdown (`full` / `hybrid` / `router_only`) with inline help tooltips.
- Token estimate badge: approximates system-prompt size based on mode + pack composition.
- Preview panel lists which skills materialize and which stay in the vault.

### MatrixView

- Cell states extended:
  - `●` materialized in agent dir
  - `◐` accessible via router
  - `○` not in scope
- Legend at top; click cell for drill-down (pack + mode).

### RoutersView (new, optional)

- Standalone page summarizing all pack routers with staleness indicators.
- Batch `Regenerate all pending` action.
- If deferred: add a `Show routers only` filter on PacksView instead.

### Dashboard / Sidebar

- Sidebar shows current agent's `disclosure_mode` badge.
- Dashboard widget estimates tokens saved vs `full` baseline for the active scenario.

## Testing Strategy

### Unit (Rust core)

1. **Migration v9→v10**: fresh DB, existing v9 DB (preserves all rows), default values correct, rollback-by-backup works.
2. **`resolve_desired_state`**: covers all three modes × essential/domain packs × scenario + agent_extra_packs combinations.
3. **Reconciliation**: full↔hybrid switches produce minimal correct diffs; native skills never removed; router staleness triggers rewrite.
4. **`render_router_skill_md`**: deterministic, handles null body (auto-render), escapes special chars in YAML.

### Integration

5. **End-to-end sync** against a temp home dir with two scenarios and four packs; asserts file tree on mode switches; vault remains complete.
6. **Router generation flow**: marker → `pack-router-gen` simulator → DB update → marker deletion; concurrent markers are race-safe.

### Frontend (Vitest)

7. **PacksView Progressive Disclosure section**: essential toggle hides/shows router fields; preview renders expected markdown; char counter warns over 600.
8. **ScenariosView mode dropdown**: mode change updates token estimate and preview; save triggers sync and sidebar update.

### Manual / Smoke

9. **Real Claude Code session** with `hybrid` scenario: measure actual system-prompt token reduction; test router trigger accuracy for common queries; confirm `Read` of vault SKILL.md returns correct content.
10. **Multi-agent**: Claude Code + Cursor + Codex all enabled; assert each agent's skills dir matches expectations; document Cursor router support as known limitation.

### Router-Accuracy Eval (stretch)

11. `sm pack eval-routers`: for each pack, run 20 canned user queries and measure whether Claude selects the intended pack router. Target ≥85% for shipping default routers.

### Performance

12. Sync latency: reconcile 200+ skills under 500ms; router render throughput; mode switch for a full scenario under 1s.

## Dependencies and Sequencing

This feature depends on:
- **Agent Native Skills Management (🔄 in progress)** — reconciliation relies on its SM-managed vs native markers. This feature should land after, or coordinate with, that work.

This feature subsumes:
- **Default Pack Seeding (⬜ planned)** — seeded taxonomy lives in this migration rather than a separate seed phase.

## Open Questions

None at brainstorming close. Any surfaced during implementation planning move to the plan doc.

## Success Criteria

- Default `hybrid` scenario system-prompt cost drops by ≥80% in measurement (success target: ~2K tokens for Layer 1).
- Router trigger accuracy ≥85% across canned eval queries.
- No broken sync on existing `full` scenarios (backward compat).
- All three UI surfaces (PacksView, ScenariosView, MatrixView) reflect the new mode and are usable end-to-end.
- `pack-router-gen` successfully generates routers for all seeded packs in ≤2 Claude Code sessions of user time.
