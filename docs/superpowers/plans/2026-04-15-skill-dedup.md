# Skill Deduplication Implementation Plan (Backend + CLI)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Deduplicate skills across agent directories by detecting copies identical to the central store and replacing them with symlinks. Mark agent-specific variants as "native" so they are preserved. Expose via CLI (`sm dedup`) and Tauri command. No UI changes.

**Architecture:** Scan each agent's skills directory. For every real directory (not already a symlink), hash it and compare against the central store (`~/.skills-manager/skills/<name>/`). If the hashes match, replace the copy with a symlink. If the name matches but content differs, flag it as native in the DB. The `discovered_skills` table gains an `is_native` column (migration v7 to v8).

**Tech Stack:** Rust (rusqlite, SHA-256 content hashing, symlinks), clap CLI.

---

## Task 1: DB Migration v7 to v8 -- Add `is_native` column

**Files to modify:**
- `crates/skills-manager-core/src/migrations.rs`

**Time estimate:** 2 min

### 1a. Bump LATEST_VERSION

Change:
```rust
const LATEST_VERSION: u32 = 7;
```
To:
```rust
const LATEST_VERSION: u32 = 8;
```

### 1b. Add `migrate_v7_to_v8`

Add after `migrate_v6_to_v7`:

```rust
/// v7 -> v8: Add is_native flag to discovered_skills for dedup awareness.
fn migrate_v7_to_v8(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "discovered_skills", "is_native", "INTEGER NOT NULL DEFAULT 0")?;
    Ok(())
}
```

### 1c. Update `migrate_step` match arm

Add case to the match in `migrate_step`:

```rust
7 => migrate_v7_to_v8(conn),
```

### 1d. Add `is_native` to `migrate_v0_to_v1` for fresh databases

After the existing `add_column_if_missing` calls at the bottom of `migrate_v0_to_v1`, add:

```rust
add_column_if_missing(conn, "discovered_skills", "is_native", "INTEGER NOT NULL DEFAULT 0")?;
```

### 1e. Update `DiscoveredSkillRecord` struct

**File:** `crates/skills-manager-core/src/skill_store.rs`

Add `is_native` field to `DiscoveredSkillRecord`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredSkillRecord {
    pub id: String,
    pub tool: String,
    pub found_path: String,
    pub name_guess: Option<String>,
    pub fingerprint: Option<String>,
    pub found_at: i64,
    pub imported_skill_id: Option<String>,
    pub is_native: bool,
}
```

### 1f. Update all SQL queries that read `discovered_skills`

**File:** `crates/skills-manager-core/src/skill_store.rs`

Every `SELECT` from `discovered_skills` must now include `is_native` as the 8th column. Update these methods:

1. `get_all_discovered` -- add `is_native` to the SELECT list and the row mapper:
   ```rust
   // In the SELECT:
   "SELECT id, tool, found_path, name_guess, fingerprint, found_at, imported_skill_id, is_native FROM discovered_skills"
   // In the row mapper, add:
   is_native: row.get::<_, i32>(7)? != 0,
   ```

2. `get_discovered_for_tool` -- same pattern, add `is_native` to SELECT and mapper.

3. `insert_discovered` -- add `is_native` to the INSERT:
   ```rust
   "INSERT INTO discovered_skills (id, tool, found_path, name_guess, fingerprint, found_at, imported_skill_id, is_native)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
   ```
   And add `rec.is_native as i32` as param `?8`.

### 1g. Add tests

```rust
#[test]
fn v7_to_v8_migration_adds_is_native_column() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();

    // Start at v7
    conn.pragma_update(None, "user_version", 7).unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS skills (id TEXT PRIMARY KEY, name TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS scenarios (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE);
         CREATE TABLE IF NOT EXISTS discovered_skills (
             id TEXT PRIMARY KEY,
             tool TEXT NOT NULL,
             found_path TEXT NOT NULL,
             name_guess TEXT,
             fingerprint TEXT,
             found_at INTEGER NOT NULL,
             imported_skill_id TEXT
         );
         CREATE TABLE IF NOT EXISTS packs (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE,
             description TEXT, icon TEXT, color TEXT, sort_order INTEGER DEFAULT 0,
             created_at INTEGER, updated_at INTEGER);
         CREATE TABLE IF NOT EXISTS managed_plugins (id TEXT PRIMARY KEY,
             plugin_key TEXT NOT NULL UNIQUE, display_name TEXT,
             plugin_data TEXT NOT NULL, created_at INTEGER, updated_at INTEGER);
         CREATE TABLE IF NOT EXISTS scenario_plugins (
             scenario_id TEXT NOT NULL, plugin_id TEXT NOT NULL,
             enabled INTEGER NOT NULL DEFAULT 1,
             PRIMARY KEY(scenario_id, plugin_id));
         CREATE TABLE IF NOT EXISTS agent_configs (
             tool_key TEXT PRIMARY KEY,
             scenario_id TEXT,
             managed INTEGER NOT NULL DEFAULT 1,
             updated_at INTEGER);
         CREATE TABLE IF NOT EXISTS agent_extra_packs (
             tool_key TEXT NOT NULL,
             pack_id TEXT NOT NULL,
             PRIMARY KEY(tool_key, pack_id));",
    )
    .unwrap();

    run_migrations(&conn).unwrap();

    assert!(has_column(&conn, "discovered_skills", "is_native").unwrap());

    // Verify default value works
    conn.execute(
        "INSERT INTO discovered_skills (id, tool, found_path, found_at) VALUES ('d1', 'claude_code', '/tmp/test', 0)",
        [],
    )
    .unwrap();
    let is_native: i32 = conn
        .query_row("SELECT is_native FROM discovered_skills WHERE id = 'd1'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(is_native, 0);

    let version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, LATEST_VERSION);
}
```

### 1h. Verify

```bash
cargo test -p skills-manager-core migrations
```

**Commit:** `feat: add is_native column to discovered_skills (migration v7->v8)`

---

## Task 2: Add `mark_discovered_as_native` and `get_native_skills_for_tool` to SkillStore

**Files to modify:**
- `crates/skills-manager-core/src/skill_store.rs`

**Time estimate:** 3 min

### 2a. Add `mark_discovered_as_native`

Add to the `// -- Discovered Skills --` section of `impl SkillStore`:

```rust
pub fn mark_discovered_as_native(&self, id: &str) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    conn.execute(
        "UPDATE discovered_skills SET is_native = 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}
```

### 2b. Add `get_native_skills_for_tool`

```rust
pub fn get_native_skills_for_tool(&self, tool: &str) -> Result<Vec<DiscoveredSkillRecord>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, tool, found_path, name_guess, fingerprint, found_at, imported_skill_id, is_native
         FROM discovered_skills WHERE tool = ?1 AND is_native = 1
         ORDER BY name_guess",
    )?;
    let rows = stmt.query_map(params![tool], |row| {
        Ok(DiscoveredSkillRecord {
            id: row.get(0)?,
            tool: row.get(1)?,
            found_path: row.get(2)?,
            name_guess: row.get(3)?,
            fingerprint: row.get(4)?,
            found_at: row.get(5)?,
            imported_skill_id: row.get(6)?,
            is_native: row.get::<_, i32>(7)? != 0,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}
```

### 2c. Add `get_skill_by_name`

The dedup module needs to look up central skills by name. Add:

```rust
pub fn get_skill_by_name(&self, name: &str) -> Result<Option<SkillRecord>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, name, description, source_type, source_ref, source_ref_resolved, source_subpath,
                source_branch, source_revision, remote_revision, central_path, content_hash, enabled,
                created_at, updated_at, status, update_status, last_checked_at, last_check_error
         FROM skills WHERE name = ?1",
    )?;
    let mut rows = stmt.query_map(params![name], map_skill_row)?;
    Ok(rows.next().and_then(|r| r.ok()))
}
```

### 2d. Add `find_discovered_by_tool_and_path`

For linking existing discovered records during dedup:

```rust
pub fn find_discovered_by_tool_and_path(
    &self,
    tool: &str,
    found_path: &str,
) -> Result<Option<DiscoveredSkillRecord>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, tool, found_path, name_guess, fingerprint, found_at, imported_skill_id, is_native
         FROM discovered_skills WHERE tool = ?1 AND found_path = ?2",
    )?;
    let mut rows = stmt.query_map(params![tool, found_path], |row| {
        Ok(DiscoveredSkillRecord {
            id: row.get(0)?,
            tool: row.get(1)?,
            found_path: row.get(2)?,
            name_guess: row.get(3)?,
            fingerprint: row.get(4)?,
            found_at: row.get(5)?,
            imported_skill_id: row.get(6)?,
            is_native: row.get::<_, i32>(7)? != 0,
        })
    })?;
    Ok(rows.next().and_then(|r| r.ok()))
}
```

### 2e. Verify

```bash
cargo test -p skills-manager-core skill_store
cargo check -p skills-manager-core
```

**Commit:** `feat: add dedup-related queries to SkillStore`

---

## Task 3: Create `dedup.rs` core module

**Files to create:**
- `crates/skills-manager-core/src/dedup.rs`

**Files to modify:**
- `crates/skills-manager-core/src/lib.rs` (add `pub mod dedup;`)

**Time estimate:** 5 min

### 3a. Create `dedup.rs`

```rust
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::central_repo;
use crate::content_hash;
use crate::skill_store::SkillStore;
use crate::sync_engine;

/// Result of deduplicating skills in a single agent directory.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DedupResult {
    /// Skills already symlinked to central store (no action needed).
    pub already_linked: Vec<String>,
    /// Copies replaced with symlinks to central store.
    pub replaced_with_symlink: Vec<String>,
    /// Skills with same name but different content, marked as native.
    pub marked_native: Vec<String>,
    /// Skills not in central store at all (skipped).
    pub skipped_unknown: Vec<String>,
    /// Errors encountered during dedup.
    pub errors: Vec<String>,
}

impl DedupResult {
    pub fn is_empty(&self) -> bool {
        self.already_linked.is_empty()
            && self.replaced_with_symlink.is_empty()
            && self.marked_native.is_empty()
            && self.skipped_unknown.is_empty()
            && self.errors.is_empty()
    }
}

/// Scan a single agent's skills directory and deduplicate against the central store.
///
/// When `dry_run` is true, the function reports what *would* happen without
/// modifying the filesystem or database.
pub fn dedup_agent_skills(
    store: &SkillStore,
    tool_key: &str,
    agent_skills_dir: &Path,
    dry_run: bool,
) -> Result<DedupResult> {
    let central_skills_dir = central_repo::skills_dir();
    let mut result = DedupResult::default();

    if !agent_skills_dir.exists() {
        return Ok(result);
    }

    let entries = std::fs::read_dir(agent_skills_dir)
        .with_context(|| format!("Failed to read agent skills dir: {:?}", agent_skills_dir))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip non-directories
        if !path.is_dir() && !path.is_symlink() {
            continue;
        }

        // Already a symlink to central store? Nothing to do.
        if path.is_symlink() {
            if let Ok(target) = std::fs::read_link(&path) {
                if target.starts_with(&central_skills_dir) {
                    result.already_linked.push(name);
                    continue;
                }
            }
            // Symlink pointing elsewhere -- skip, not our business.
            result.skipped_unknown.push(name);
            continue;
        }

        // Real directory. Check if a skill with this name exists in central store.
        let central_path = central_skills_dir.join(&name);
        if !central_path.exists() {
            result.skipped_unknown.push(name);
            continue;
        }

        // Hash both and compare.
        let agent_hash = match content_hash::hash_directory(&path) {
            Ok(h) => h,
            Err(e) => {
                result
                    .errors
                    .push(format!("{}: failed to hash agent copy: {}", name, e));
                continue;
            }
        };

        let central_hash = match content_hash::hash_directory(&central_path) {
            Ok(h) => h,
            Err(e) => {
                result
                    .errors
                    .push(format!("{}: failed to hash central copy: {}", name, e));
                continue;
            }
        };

        if agent_hash == central_hash {
            // Identical content. Replace real dir with symlink.
            if !dry_run {
                if let Err(e) = replace_with_symlink(&path, &central_path) {
                    result
                        .errors
                        .push(format!("{}: failed to replace with symlink: {}", name, e));
                    continue;
                }
            }
            result.replaced_with_symlink.push(name);
        } else {
            // Different content. Mark as native.
            if !dry_run {
                mark_as_native_in_db(store, tool_key, &path, &name);
            }
            result.marked_native.push(name);
        }
    }

    Ok(result)
}

/// Run dedup across all installed agents. Returns a vec of (tool_key, DedupResult).
pub fn dedup_all_agents(
    store: &SkillStore,
    adapters: &[crate::tool_adapters::ToolAdapter],
    dry_run: bool,
) -> Vec<(String, DedupResult)> {
    let mut results = Vec::new();

    for adapter in adapters {
        if !adapter.is_installed() {
            continue;
        }
        let skills_dir = adapter.skills_dir();
        match dedup_agent_skills(store, &adapter.key, &skills_dir, dry_run) {
            Ok(r) => results.push((adapter.key.clone(), r)),
            Err(e) => {
                let mut r = DedupResult::default();
                r.errors
                    .push(format!("Failed to scan {}: {}", adapter.key, e));
                results.push((adapter.key.clone(), r));
            }
        }
    }

    results
}

/// Replace a real directory with a symlink to the central store copy.
fn replace_with_symlink(agent_path: &Path, central_path: &Path) -> Result<()> {
    // Safety: remove the agent copy first, then create symlink.
    sync_engine::remove_target(agent_path)
        .with_context(|| format!("Failed to remove {:?}", agent_path))?;

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(central_path, agent_path).with_context(|| {
            format!(
                "Failed to create symlink {:?} -> {:?}",
                agent_path, central_path
            )
        })?;
    }

    #[cfg(not(unix))]
    {
        // On non-unix, fall back to copy (symlinks may not be available).
        crate::sync_engine::sync_skill(
            central_path,
            agent_path,
            crate::sync_engine::SyncMode::Copy,
        )?;
    }

    Ok(())
}

/// Best-effort: find or create a discovered_skills record and mark it native.
fn mark_as_native_in_db(store: &SkillStore, tool_key: &str, path: &Path, name: &str) {
    let path_str = path.to_string_lossy().to_string();

    // Try to find an existing discovered record for this path.
    if let Ok(Some(rec)) = store.find_discovered_by_tool_and_path(tool_key, &path_str) {
        let _ = store.mark_discovered_as_native(&rec.id);
        return;
    }

    // No record yet -- create one and mark it native.
    let now = chrono::Utc::now().timestamp_millis();
    let fingerprint = content_hash::hash_directory(path).ok();
    let rec = crate::skill_store::DiscoveredSkillRecord {
        id: uuid::Uuid::new_v4().to_string(),
        tool: tool_key.to_string(),
        found_path: path_str,
        name_guess: Some(name.to_string()),
        fingerprint,
        found_at: now,
        imported_skill_id: None,
        is_native: true,
    };
    let _ = store.insert_discovered(&rec);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Create a mock SkillStore backed by an in-memory DB.
    fn test_store() -> SkillStore {
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        // SkillStore::new creates the DB if missing and runs migrations.
        let store = SkillStore::new(&db_path).unwrap();
        // Leak the tempdir so it persists for the test lifetime.
        std::mem::forget(tmp);
        store
    }

    #[test]
    fn dedup_empty_dir_returns_empty_result() {
        let store = test_store();
        let agent_dir = tempdir().unwrap();
        let result =
            dedup_agent_skills(&store, "test_agent", agent_dir.path(), true).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn dedup_skips_unknown_skills_not_in_central() {
        let store = test_store();
        let agent_dir = tempdir().unwrap();
        let skill_dir = agent_dir.path().join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "hello").unwrap();

        let result =
            dedup_agent_skills(&store, "test_agent", agent_dir.path(), true).unwrap();
        assert_eq!(result.skipped_unknown, vec!["my-skill"]);
        assert!(result.replaced_with_symlink.is_empty());
    }

    #[test]
    fn dedup_replaces_identical_copy_with_symlink() {
        let store = test_store();
        let central_dir = tempdir().unwrap();
        let agent_dir = tempdir().unwrap();

        // Create a skill in "central" store.
        let central_skill = central_dir.path().join("test-skill");
        fs::create_dir_all(&central_skill).unwrap();
        fs::write(central_skill.join("SKILL.md"), "content").unwrap();

        // Create an identical copy in the agent dir.
        let agent_skill = agent_dir.path().join("test-skill");
        fs::create_dir_all(&agent_skill).unwrap();
        fs::write(agent_skill.join("SKILL.md"), "content").unwrap();

        // We can't easily mock central_repo::skills_dir(), so test the lower-level
        // hash comparison and replace_with_symlink directly.
        let central_hash = content_hash::hash_directory(&central_skill).unwrap();
        let agent_hash = content_hash::hash_directory(&agent_skill).unwrap();
        assert_eq!(central_hash, agent_hash);

        // Test replace_with_symlink.
        replace_with_symlink(&agent_skill, &central_skill).unwrap();
        assert!(agent_skill.is_symlink());
        let target = fs::read_link(&agent_skill).unwrap();
        assert_eq!(target, central_skill);
    }

    #[test]
    fn dedup_detects_different_content() {
        let central_dir = tempdir().unwrap();
        let agent_dir = tempdir().unwrap();

        let central_skill = central_dir.path().join("test-skill");
        fs::create_dir_all(&central_skill).unwrap();
        fs::write(central_skill.join("SKILL.md"), "central version").unwrap();

        let agent_skill = agent_dir.path().join("test-skill");
        fs::create_dir_all(&agent_skill).unwrap();
        fs::write(agent_skill.join("SKILL.md"), "agent version").unwrap();

        let central_hash = content_hash::hash_directory(&central_skill).unwrap();
        let agent_hash = content_hash::hash_directory(&agent_skill).unwrap();
        assert_ne!(central_hash, agent_hash);
    }

    #[test]
    fn replace_with_symlink_works() {
        let central = tempdir().unwrap();
        let agent = tempdir().unwrap();

        let src = central.path().join("skill-a");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("file.md"), "data").unwrap();

        let dst = agent.path().join("skill-a");
        fs::create_dir_all(&dst).unwrap();
        fs::write(dst.join("file.md"), "data").unwrap();

        replace_with_symlink(&dst, &src).unwrap();

        assert!(dst.is_symlink());
        let resolved = fs::read_link(&dst).unwrap();
        assert_eq!(resolved, src);
        // Content should still be readable through the symlink.
        let content = fs::read_to_string(dst.join("file.md")).unwrap();
        assert_eq!(content, "data");
    }
}
```

### 3b. Register the module

**File:** `crates/skills-manager-core/src/lib.rs`

Add `pub mod dedup;` after the existing modules:

```rust
pub mod dedup;
```

And add to the re-exports:

```rust
pub use dedup::DedupResult;
```

### 3c. Verify

```bash
cargo test -p skills-manager-core dedup
cargo check -p skills-manager-core
```

**Commit:** `feat: add dedup module for skill deduplication against central store`

---

## Task 4: Enhance import with dedup awareness

**Files to modify:**
- `crates/skills-manager-core/src/dedup.rs` (add `import_with_dedup`)

**Time estimate:** 4 min

### 4a. Add `ImportWithDedupResult`

Add to `dedup.rs`:

```rust
/// Result of importing a discovered skill with dedup awareness.
#[derive(Debug, Clone, Serialize)]
pub enum ImportAction {
    /// Skill was new -- copied to central store.
    Imported { skill_id: String },
    /// Identical skill already in central -- linked discovered record to existing.
    LinkedToExisting { skill_id: String },
    /// Same name but different content -- marked as native, not imported.
    MarkedNative,
}
```

### 4b. Add `import_with_dedup`

```rust
use crate::installer;
use crate::skill_metadata;

/// Import a discovered skill with dedup logic:
/// - If central store has same name + same hash: link to existing, skip copy.
/// - If central store has same name + different hash: mark as native.
/// - If central store doesn't have this name: copy to central, create skill record.
///
/// Returns the action taken.
pub fn import_with_dedup(
    store: &SkillStore,
    discovered_id: &str,
) -> Result<ImportAction> {
    let discovered = store
        .get_all_discovered()?
        .into_iter()
        .find(|d| d.id == discovered_id)
        .ok_or_else(|| anyhow::anyhow!("Discovered skill '{}' not found", discovered_id))?;

    let source_path = PathBuf::from(&discovered.found_path);
    if !source_path.exists() {
        anyhow::bail!("Source path no longer exists: {}", discovered.found_path);
    }

    let name = discovered
        .name_guess
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Discovered skill has no name_guess"))?;

    let central_skills_dir = central_repo::skills_dir();
    let central_path = central_skills_dir.join(name);

    if central_path.exists() {
        let source_hash = content_hash::hash_directory(&source_path)?;
        let central_hash = content_hash::hash_directory(&central_path)?;

        if source_hash == central_hash {
            // Already in central with same content. Link the discovered record.
            if let Some(skill) = store.get_skill_by_central_path(
                &central_path.to_string_lossy(),
            )? {
                store.link_discovered_to_skill(&discovered_id, &skill.id)?;
                return Ok(ImportAction::LinkedToExisting {
                    skill_id: skill.id,
                });
            }
            // Central path exists on disk but no DB record -- fall through to import.
        } else {
            // Different content -- mark as native.
            store.mark_discovered_as_native(&discovered_id)?;
            return Ok(ImportAction::MarkedNative);
        }
    }

    // New skill or no DB record for existing central path -- do a normal import.
    let install_result = installer::install_from_local(&source_path, Some(name))?;

    let now = chrono::Utc::now().timestamp_millis();
    let skill_id = uuid::Uuid::new_v4().to_string();
    let skill_record = crate::skill_store::SkillRecord {
        id: skill_id.clone(),
        name: install_result.name.clone(),
        description: install_result.description.clone(),
        source_type: "local".to_string(),
        source_ref: Some(discovered.found_path.clone()),
        source_ref_resolved: None,
        source_subpath: None,
        source_branch: None,
        source_revision: None,
        remote_revision: None,
        central_path: install_result.central_path.to_string_lossy().to_string(),
        content_hash: Some(install_result.content_hash),
        enabled: true,
        created_at: now,
        updated_at: now,
        status: "ok".to_string(),
        update_status: "unknown".to_string(),
        last_checked_at: None,
        last_check_error: None,
    };
    store.insert_skill(&skill_record)?;
    store.link_discovered_to_skill(&discovered_id, &skill_id)?;

    Ok(ImportAction::Imported { skill_id })
}
```

### 4c. Add `link_discovered_to_skill` to SkillStore

**File:** `crates/skills-manager-core/src/skill_store.rs`

Add to the Discovered Skills section:

```rust
pub fn link_discovered_to_skill(&self, discovered_id: &str, skill_id: &str) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    conn.execute(
        "UPDATE discovered_skills SET imported_skill_id = ?1 WHERE id = ?2",
        params![skill_id, discovered_id],
    )?;
    Ok(())
}
```

### 4d. Verify

```bash
cargo check -p skills-manager-core
cargo test -p skills-manager-core
```

**Commit:** `feat: add import_with_dedup for dedup-aware skill importing`

---

## Task 5: CLI `sm dedup` command

**Files to modify:**
- `crates/skills-manager-cli/src/main.rs`
- `crates/skills-manager-cli/src/commands.rs`

**Time estimate:** 5 min

### 5a. Add `Dedup` to the `Commands` enum

**File:** `crates/skills-manager-cli/src/main.rs`

Add after the `Agent` variant:

```rust
/// Deduplicate agent skill directories against the central store.
/// Replaces identical copies with symlinks.
Dedup {
    /// Actually replace copies with symlinks (default: dry run)
    #[arg(long)]
    apply: bool,
    /// Only dedup a specific agent (e.g., claude_code)
    #[arg(long)]
    agent: Option<String>,
},
```

### 5b. Add match arm in `main()`

```rust
Commands::Dedup { apply, agent } => commands::cmd_dedup(apply, agent.as_deref()),
```

### 5c. Implement `cmd_dedup`

**File:** `crates/skills-manager-cli/src/commands.rs`

Add the import at the top:

```rust
use skills_manager_core::dedup;
```

Add the command function:

```rust
pub fn cmd_dedup(apply: bool, agent: Option<&str>) -> Result<()> {
    let store = open_store()?;
    let all_adapters = tool_adapters::enabled_installed_adapters(&store);

    let dry_run = !apply;

    if dry_run {
        println!("Dedup dry run (use --apply to execute):\n");
    } else {
        println!("Dedup applying changes:\n");
    }

    let results = match agent {
        Some(agent_key) => {
            let adapter = all_adapters
                .iter()
                .find(|a| a.key == agent_key)
                .ok_or_else(|| {
                    let available: Vec<&str> = all_adapters.iter().map(|a| a.key.as_str()).collect();
                    anyhow::anyhow!(
                        "Agent '{}' not found. Available: {}",
                        agent_key,
                        available.join(", ")
                    )
                })?;
            let r = dedup::dedup_agent_skills(
                &store,
                &adapter.key,
                &adapter.skills_dir(),
                dry_run,
            )?;
            vec![(adapter.key.clone(), r)]
        }
        None => dedup::dedup_all_agents(&store, &all_adapters, dry_run),
    };

    let mut total_linked = 0;
    let mut total_replaced = 0;
    let mut total_native = 0;
    let mut total_skipped = 0;
    let mut total_errors = 0;

    for (tool_key, r) in &results {
        if r.is_empty() {
            continue;
        }

        println!("{}:", tool_key);

        if !r.already_linked.is_empty() {
            println!("  Already linked: {}", r.already_linked.len());
            total_linked += r.already_linked.len();
        }

        if !r.replaced_with_symlink.is_empty() {
            let verb = if dry_run { "Would replace" } else { "Replaced" };
            println!(
                "  {} with symlink: {}",
                verb,
                r.replaced_with_symlink.len()
            );
            for name in &r.replaced_with_symlink {
                println!("    {}", name);
            }
            total_replaced += r.replaced_with_symlink.len();
        }

        if !r.marked_native.is_empty() {
            let verb = if dry_run { "Would mark" } else { "Marked" };
            println!("  {} as native: {}", verb, r.marked_native.len());
            for name in &r.marked_native {
                println!("    {}", name);
            }
            total_native += r.marked_native.len();
        }

        if !r.skipped_unknown.is_empty() {
            println!("  Skipped (not in central): {}", r.skipped_unknown.len());
            total_skipped += r.skipped_unknown.len();
        }

        for err in &r.errors {
            eprintln!("  Error: {}", err);
            total_errors += 1;
        }

        println!();
    }

    println!("Summary:");
    println!("  Already linked:  {}", total_linked);
    println!(
        "  {}:  {}",
        if dry_run {
            "Would replace"
        } else {
            "Replaced"
        },
        total_replaced
    );
    println!(
        "  {}:    {}",
        if dry_run {
            "Would mark native"
        } else {
            "Marked native"
        },
        total_native
    );
    println!("  Skipped:         {}", total_skipped);
    if total_errors > 0 {
        println!("  Errors:          {}", total_errors);
    }

    Ok(())
}
```

### 5d. Verify

```bash
cargo build -p skills-manager-cli
# Manual test:
cargo run -p skills-manager-cli -- dedup
cargo run -p skills-manager-cli -- dedup --agent claude_code
```

**Commit:** `feat: add sm dedup CLI command for skill deduplication`

---

## Task 6: Tauri command for dedup

**Files to modify:**
- `src-tauri/src/commands/agents.rs`
- `src-tauri/src/lib.rs` (register the new command)

**Time estimate:** 3 min

### 6a. Add the Tauri command

**File:** `src-tauri/src/commands/agents.rs`

Add the import at the top (alongside the existing ones):

```rust
use crate::core::dedup;
```

Add the command:

```rust
#[tauri::command]
pub async fn dedup_agent_skills(
    store: State<'_, Arc<SkillStore>>,
    tool_key: String,
) -> Result<dedup::DedupResult, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let adapter = tool_adapters::find_adapter_with_store(&store, &tool_key)
            .ok_or_else(|| AppError::validation(format!("Unknown agent: {}", tool_key)))?;
        let skills_dir = adapter.skills_dir();
        dedup::dedup_agent_skills(&store, &tool_key, &skills_dir, false)
            .map_err(AppError::internal)
    })
    .await
    .map_err(|e| AppError::internal(e))?
}

#[tauri::command]
pub async fn dedup_all_agents(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<(String, dedup::DedupResult)>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let adapters = tool_adapters::enabled_installed_adapters(&store);
        Ok(dedup::dedup_all_agents(&store, &adapters, false))
    })
    .await
    .map_err(|e| AppError::internal(e))?
}
```

### 6b. Ensure `dedup` module is accessible from Tauri

The Tauri app uses `src-tauri/src/core/` which re-exports from the core crate. Check if it has a blanket re-export or per-module re-exports. If per-module, add:

**File:** `src-tauri/src/core/mod.rs` (or equivalent)

Add:
```rust
pub use skills_manager_core::dedup;
```

### 6c. Register the commands in `lib.rs`

**File:** `src-tauri/src/lib.rs`

In the `invoke_handler(tauri::generate_handler![...])` block, add under the `// Agents` section:

```rust
commands::agents::dedup_agent_skills,
commands::agents::dedup_all_agents,
```

### 6d. Verify

```bash
cargo build -p skills-manager
```

**Commit:** `feat: add Tauri commands for skill deduplication`

---

## Task 7: Update scanner to set `is_native` on new DiscoveredSkillRecord

**Files to modify:**
- `crates/skills-manager-core/src/scanner.rs`

**Time estimate:** 2 min

### 7a. Update DiscoveredSkillRecord construction

Every place in `scanner.rs` that creates a `DiscoveredSkillRecord` must now include the `is_native` field. It should default to `false` during scan (dedup marks it later).

Find the `DiscoveredSkillRecord { ... }` construction in `scan_local_skills_with_adapters` and add:

```rust
is_native: false,
```

### 7b. Update any other DiscoveredSkillRecord constructors

Search across the codebase for any other place that builds `DiscoveredSkillRecord` and add the field. Key files to check:
- `src-tauri/src/commands/scan.rs`

### 7c. Verify

```bash
cargo check --workspace
cargo test --workspace
```

**Commit:** `fix: set is_native field on DiscoveredSkillRecord in scanner and scan commands`

---

## Task 8: End-to-end verification

**Time estimate:** 3 min

### 8a. Run full test suite

```bash
cargo test --workspace
```

### 8b. Manual CLI tests

```bash
# Build the CLI
cargo build -p skills-manager-cli

# Dry run
cargo run -p skills-manager-cli -- dedup

# Check a specific agent
cargo run -p skills-manager-cli -- dedup --agent claude_code

# Apply (after reviewing dry run output)
cargo run -p skills-manager-cli -- dedup --apply
```

### 8c. Verify dedup correctness

1. Check that after `--apply`, agent directories contain symlinks pointing to `~/.skills-manager/skills/<name>`.
2. Run `sm dedup` again -- all previously replaced skills should now show as "Already linked".
3. Confirm native skills are untouched (real directories preserved).
4. Run `sm agent info claude_code` to verify native count includes newly marked native skills.

### 8d. Verify migration on existing DB

1. Back up `~/.skills-manager/skills-manager.db`.
2. Run the app (`cargo tauri dev`). Migration v7->v8 should run automatically.
3. Verify `PRAGMA user_version` is 8.
4. Verify `is_native` column exists in `discovered_skills`.

**Commit:** (no commit, verification only)
