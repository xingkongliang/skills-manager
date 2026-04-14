# Skill Deduplication + Single Source of Truth

## Problem

When importing skills from local agent directories, the same skill can appear in multiple agents (e.g., `web-access` in Claude Code, Cursor, Windsurf). After import:
- There's no dedup — multiple copies persist in agent directories
- No way to tell which is the canonical version
- No cleanup of stale real directories after SM takes over via symlinks
- Native skills (unique to an agent) may get overwritten

## Architecture: Content-Addressed Central Store + Symlink Farm

Inspired by pnpm's content-addressable store and GNU Stow's symlink farm with conflict detection.

### How It Works

```
Central Store (source of truth):
  ~/.skills-manager/skills/<name>/          ← ONE copy per skill

Agent Directories (symlink farm):
  ~/.claude/skills/<name> → central store   ← symlink (SM-managed)
  ~/.cursor/skills/<name> → central store   ← symlink (SM-managed)
  ~/.hermes/skills/apple/                   ← real dir (native, different content)
```

### Import Flow

When importing a discovered skill:

1. **Hash the skill content** — SHA-256 of the skill directory (already have `content_hash` module)
2. **Check central store**:
   - Same name exists in central with **same hash** → already imported, just add symlinks
   - Same name exists in central with **different hash** → conflict (version mismatch)
   - Name doesn't exist → copy to central store
3. **Update agent directories**:
   - For each agent that has this skill as a real directory with same hash → replace with symlink
   - For each agent that has this skill with different hash → mark as native, leave untouched
4. **Record in DB**:
   - `skills` table: one record per unique skill
   - `discovered_skills.imported_skill_id`: link back to the imported skill

### Content Hash for Identity

Use the existing `content_hash::hash_directory()` function. Two skills are "the same" if:
- Same name AND same content hash → identical, dedup
- Same name but different hash → different versions, flag as conflict
- Different name but same hash → coincidence, treat as separate skills

### Native Skill Detection

A skill is "native" when:
- It exists in an agent directory as a real directory (not symlink)
- Its content hash differs from the SM central version (if one exists)
- It was NOT imported by SM

Store this in the `discovered_skills` table: add a `native` boolean flag.

### Dedup on Import

When user clicks "Import" or "Import All":

```
for each discovered skill:
  central_path = ~/.skills-manager/skills/<name>
  
  if central_path exists:
    central_hash = hash(central_path)
    source_hash = hash(discovered.found_path)
    
    if central_hash == source_hash:
      # Already in central, just need symlinks
      mark discovered as imported (link to existing skill record)
    else:
      # Different content — ask user or auto-resolve
      # Default: keep central version (it's the source of truth)
      # Mark discovered as "native" for this agent
  else:
    # New skill — copy to central
    copy discovered.found_path → central_path
    create skill record in DB
    mark discovered as imported
  
  # Cleanup: replace real dirs with symlinks in all agent dirs
  for each managed agent:
    agent_skill_path = agent.skills_dir/<name>
    if is_real_dir(agent_skill_path) AND hash matches central:
      remove agent_skill_path
      symlink agent_skill_path → central_path
```

### Cleanup Command

New CLI command: `sm dedup` — scans all agent directories, replaces identical copies with symlinks to central store.

```bash
sm dedup                    # dry run — show what would be cleaned
sm dedup --apply            # actually replace copies with symlinks
sm dedup --agent claude_code  # only one agent
```

### UI: Import with Dedup Awareness

The "安裝 Skills" → "本機安裝" page enhanced:

For each discovered skill, show status:
- **New** — not in central store, safe to import
- **Already imported** — same name + same hash in central
- **Conflict** — same name but different hash (show diff option)
- **Native** — marked as agent-native, won't be managed

"Import All" button behavior:
- Imports all "New" skills
- Skips "Already imported" and "Native"
- For "Conflict": uses central version (existing), marks discovered as native

## DB Changes

No new tables. Add `is_native` column to `discovered_skills`:

```sql
ALTER TABLE discovered_skills ADD COLUMN is_native INTEGER NOT NULL DEFAULT 0;
```

This flags skills that should not be imported because they're agent-specific versions.

## Core Functions

### New in skill_store.rs:
- `mark_discovered_as_native(id)` — set is_native=1
- `get_native_skills_for_tool(tool)` → Vec<DiscoveredSkillRecord> where is_native=1

### New in content_hash.rs (or new file):
- Already exists: `hash_directory(path)` → String

### New in installer.rs or new dedup.rs:
- `dedup_agent_skills(store, tool_key, agent_skills_dir)` — scan agent dir, replace identical copies with symlinks
- `import_with_dedup(store, discovered_id)` — import a discovered skill with dedup logic

### CLI:
- `sm dedup [--apply] [--agent <key>]`

### Tauri commands:
- `dedup_agent_skills(tool_key)` → DedupReport
- `import_discovered_with_dedup(discovered_id)` → ImportResult

## Verification

1. Import a skill found in 3 agents → only 1 copy in central, 3 symlinks
2. Import "xlsx" from claude_code → windsurf copy detected as identical → replaced with symlink
3. Import "apple" from hermes → SM already has a different version → hermes version marked native
4. `sm dedup --apply` cleans up all stale copies
5. Agent Detail "Skills Breakdown" shows correct native count after dedup

## Out of Scope

- Content-addressed store paths (like /nix/store/<hash>-<name>) — overkill, name-based is sufficient
- Version management for skills — just latest version
- Merge tool for conflicts — just keep one, mark other as native
