# Phase 1: Core Crate Extraction + Skill Packs — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract 18 core modules into a standalone Rust library crate, then add Skill Packs (grouping + scenario composition) on top.

**Architecture:** Cargo workspace with two members: `crates/skills-manager-core` (library) and `src-tauri` (Tauri app). Core crate is framework-agnostic by default, with optional `tauri`/`tokio` feature flags for error conversions. Packs add three DB tables and a composition query that preserves full backward compatibility.

**Tech Stack:** Rust, rusqlite (bundled), Cargo workspace, feature flags

**Spec:** `docs/superpowers/specs/2026-04-13-phase1-core-extraction-packs-design.md`

---

## Phase 1A: Core Crate Extraction

### Task 1: Create Cargo workspace root

**Files:**
- Create: `Cargo.toml` (workspace root)
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Create workspace root Cargo.toml**

Create `/Users/jfkn/projects/skills-manager/Cargo.toml`:

```toml
[workspace]
members = ["crates/skills-manager-core", "src-tauri"]
resolver = "2"
```

- [ ] **Step 2: Update src-tauri/Cargo.toml to be a workspace member**

The existing `src-tauri/Cargo.toml` is already a valid package. No changes needed yet — it will automatically be a workspace member. Verify by running:

Run: `cd /Users/jfkn/projects/skills-manager && cargo metadata --format-version 1 2>&1 | head -5`
Expected: workspace metadata listing `src-tauri` (may warn about missing `crates/skills-manager-core`)

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore: create Cargo workspace root"
```

---

### Task 2: Create skills-manager-core crate skeleton

**Files:**
- Create: `crates/skills-manager-core/Cargo.toml`
- Create: `crates/skills-manager-core/src/lib.rs`

- [ ] **Step 1: Create directory structure**

Run: `mkdir -p /Users/jfkn/projects/skills-manager/crates/skills-manager-core/src`

- [ ] **Step 2: Create core crate Cargo.toml**

Create `/Users/jfkn/projects/skills-manager/crates/skills-manager-core/Cargo.toml`:

```toml
[package]
name = "skills-manager-core"
version = "0.1.0"
edition = "2021"
rust-version = "1.77.2"
description = "Core library for Skills Manager — DB, sync, adapters, and pack management"

[features]
default = []
tauri-compat = ["dep:tauri"]
tokio-compat = ["dep:tokio"]

[dependencies]
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml = "0.9"
rusqlite = { version = "0.31", features = ["bundled"] }
uuid = { version = "1", features = ["v4"] }
chrono = "0.4"
sha2 = "0.10"
aes-gcm = "0.10"
rand = "0.8"
hex = "0.4"
dirs = "5.0"
walkdir = "2.5"
git2 = { version = "0.19", features = ["vendored-openssl"] }
reqwest = { version = "0.12", features = ["blocking", "json"] }
urlencoding = "2"
zip = "2"
tempfile = "3.26.0"
semver = "1"
regex = "1"
image = { version = "0.25", default-features = false, features = ["png"] }
log = "0.4"

# Optional framework-specific deps
tauri = { version = "2.10.0", features = ["tray-icon"], optional = true }
tokio = { version = "1", features = ["full"], optional = true }
```

- [ ] **Step 3: Create lib.rs with empty module declarations**

Create `/Users/jfkn/projects/skills-manager/crates/skills-manager-core/src/lib.rs`:

```rust
// Core library for Skills Manager.
// Framework-agnostic by default. Enable `tauri-compat` or `tokio-compat`
// features for framework-specific error conversions.
```

- [ ] **Step 4: Verify workspace compiles**

Run: `cd /Users/jfkn/projects/skills-manager && cargo build -p skills-manager-core`
Expected: compiles successfully (empty lib)

- [ ] **Step 5: Commit**

```bash
git add crates/
git commit -m "chore: create skills-manager-core crate skeleton"
```

---

### Task 3: Move core modules to core crate

**Files:**
- Move: 18 files from `src-tauri/src/core/*.rs` → `crates/skills-manager-core/src/*.rs`
- Modify: `crates/skills-manager-core/src/lib.rs`
- Modify: `src-tauri/src/core/mod.rs`

- [ ] **Step 1: Move all 18 module files**

```bash
cd /Users/jfkn/projects/skills-manager
for f in central_repo content_hash crypto error git_backup git_fetcher install_cancel installer migrations project_scanner scanner skill_metadata skill_store skillsmp_api skillssh_api sync_engine tool_adapters; do
  git mv src-tauri/src/core/${f}.rs crates/skills-manager-core/src/${f}.rs
done
```

- [ ] **Step 2: Write lib.rs with pub mod declarations and re-exports**

Replace `/Users/jfkn/projects/skills-manager/crates/skills-manager-core/src/lib.rs`:

```rust
pub mod central_repo;
pub mod content_hash;
pub mod crypto;
pub mod error;
pub mod git_backup;
pub mod git_fetcher;
pub mod install_cancel;
pub mod installer;
pub mod migrations;
pub mod project_scanner;
pub mod scanner;
pub mod skill_metadata;
pub mod skill_store;
pub mod skillsmp_api;
pub mod skillssh_api;
pub mod sync_engine;
pub mod tool_adapters;

// Re-export commonly used types
pub use error::{AppError, ErrorKind};
pub use skill_store::{
    DiscoveredSkillRecord, ProjectRecord, ScenarioRecord, ScenarioSkillToolToggleRecord,
    SkillRecord, SkillStore, SkillTargetRecord,
};
pub use sync_engine::SyncMode;
```

- [ ] **Step 3: Rewrite src-tauri/src/core/mod.rs as re-export bridge**

Replace `/Users/jfkn/projects/skills-manager/src-tauri/src/core/mod.rs`:

```rust
// Re-export everything from the core crate so that
// `use crate::core::*` continues to work in commands/.
pub use skills_manager_core::*;

// Tauri-dependent module stays local
pub mod file_watcher;
```

- [ ] **Step 4: Verify file structure**

Run: `ls crates/skills-manager-core/src/ | wc -l`
Expected: 19 (18 modules + lib.rs)

Run: `ls src-tauri/src/core/`
Expected: `mod.rs  file_watcher.rs`

- [ ] **Step 5: Commit (won't compile yet — imports need fixing)**

```bash
git add -A
git commit -m "refactor: move 18 core modules to skills-manager-core crate"
```

---

### Task 4: Fix internal imports in core crate

**Files:**
- Modify: `crates/skills-manager-core/src/skill_store.rs`
- Modify: `crates/skills-manager-core/src/installer.rs`
- Modify: `crates/skills-manager-core/src/scanner.rs`
- Modify: `crates/skills-manager-core/src/project_scanner.rs`
- Modify: `crates/skills-manager-core/src/skillsmp_api.rs`
- Modify: `crates/skills-manager-core/src/git_fetcher.rs`

- [ ] **Step 1: Fix skill_store.rs**

Change line 7:
```rust
// OLD: use super::crypto;
// NEW:
use crate::crypto;
```

- [ ] **Step 2: Fix installer.rs**

Change lines 5-7:
```rust
// OLD:
// use super::central_repo;
// use super::content_hash;
// use super::skill_metadata::{self, sanitize_skill_name};
// NEW:
use crate::central_repo;
use crate::content_hash;
use crate::skill_metadata::{self, sanitize_skill_name};
```

- [ ] **Step 3: Fix scanner.rs**

Change lines 5-8:
```rust
// OLD:
// use super::content_hash;
// use super::skill_metadata;
// use super::skill_store::DiscoveredSkillRecord;
// use super::tool_adapters;
// NEW:
use crate::content_hash;
use crate::skill_metadata;
use crate::skill_store::DiscoveredSkillRecord;
use crate::tool_adapters;
```

- [ ] **Step 4: Fix project_scanner.rs**

Change line 4:
```rust
// OLD: use super::{content_hash, skill_metadata};
// NEW:
use crate::{content_hash, skill_metadata};
```

- [ ] **Step 5: Fix skillsmp_api.rs**

Change line 4:
```rust
// OLD: use super::skillssh_api::{build_http_client, SkillsShSkill};
// NEW:
use crate::skillssh_api::{build_http_client, SkillsShSkill};
```

- [ ] **Step 6: Fix git_fetcher.rs**

Change line 1:
```rust
// OLD: use crate::core::skill_metadata;
// NEW:
use crate::skill_metadata;
```

- [ ] **Step 7: Verify core crate compiles**

Run: `cd /Users/jfkn/projects/skills-manager && cargo build -p skills-manager-core`
Expected: compiles successfully

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor: fix internal imports in skills-manager-core"
```

---

### Task 5: Feature-gate error.rs tauri/tokio From impls

**Files:**
- Modify: `crates/skills-manager-core/src/error.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Add feature gates to error.rs**

In `/Users/jfkn/projects/skills-manager/crates/skills-manager-core/src/error.rs`, wrap the two From impls:

```rust
// Replace lines 132-148:

#[cfg(feature = "tokio-compat")]
impl From<tokio::task::JoinError> for AppError {
    fn from(e: tokio::task::JoinError) -> Self {
        Self {
            kind: ErrorKind::Internal,
            message: e.to_string(),
        }
    }
}

#[cfg(feature = "tauri-compat")]
impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        Self {
            kind: ErrorKind::Internal,
            message: e.to_string(),
        }
    }
}
```

- [ ] **Step 2: Add core crate dependency to src-tauri/Cargo.toml**

Add to `[dependencies]` section in `src-tauri/Cargo.toml`:

```toml
skills-manager-core = { path = "../crates/skills-manager-core", features = ["tauri-compat", "tokio-compat"] }
```

- [ ] **Step 3: Remove duplicated deps from src-tauri/Cargo.toml**

Remove these lines from `src-tauri/Cargo.toml` `[dependencies]` (now provided by core crate):
- `rusqlite`, `uuid`, `sha2`, `dirs`, `walkdir`, `reqwest`, `anyhow`, `chrono`
- `serde_yaml`, `git2`, `regex`, `hex`, `urlencoding`, `zip`, `tempfile`
- `semver`, `aes-gcm`, `rand`, `image`

Keep only:
- `serde`, `serde_json`, `log` (used directly by commands/)
- `tauri`, `tauri-plugin-*` (Tauri framework)
- `tokio` (used by commands/ for spawn_blocking)
- `notify` (used by file_watcher.rs)
- `skills-manager-core` (new)

- [ ] **Step 4: Fix file_watcher.rs imports**

In `src-tauri/src/core/file_watcher.rs`, change line 9:
```rust
// OLD: use super::{central_repo, skill_store::SkillStore, tool_adapters};
// NEW:
use skills_manager_core::{central_repo, skill_store::SkillStore, tool_adapters};
```

And change lines 158-159 (in test module):
```rust
// OLD:
// use super::collect_watch_paths;
// use crate::core::skill_store::{ProjectRecord, SkillStore};
// NEW:
use super::collect_watch_paths;
use skills_manager_core::skill_store::{ProjectRecord, SkillStore};
```

- [ ] **Step 5: Verify full workspace compiles**

Run: `cd /Users/jfkn/projects/skills-manager && cargo build`
Expected: both crates compile successfully

- [ ] **Step 6: Run all tests**

Run: `cd /Users/jfkn/projects/skills-manager && cargo test`
Expected: all existing tests pass

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor: feature-gate error.rs tauri/tokio deps, wire up workspace"
```

---

### Task 6: Update CI and tooling paths

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `.github/workflows/prepare-release.yml` (if needed)

- [ ] **Step 1: Check release.yml for src-tauri references**

In `.github/workflows/release.yml`, line 69:
```yaml
# OLD: run: cargo check --manifest-path src-tauri/Cargo.toml
# NEW:
run: cargo check --manifest-path src-tauri/Cargo.toml
```

This path is still valid — `src-tauri/Cargo.toml` still exists as a workspace member. The `workspaces: src-tauri` on line 52 is for Tauri's action, which is also still correct.

Verify Cargo.lock placement — workspace puts it at the root. Check if `.gitignore` ignores it:

Run: `grep -n "Cargo.lock" /Users/jfkn/projects/skills-manager/.gitignore`

If found, remove the line (workspace lock file should be committed).

- [ ] **Step 2: Verify tauri dev still works**

Run: `cd /Users/jfkn/projects/skills-manager && cargo tauri dev`
Expected: app launches, scenario switching works

- [ ] **Step 3: Commit (if any changes)**

```bash
git add -A
git commit -m "chore: update CI paths for workspace structure"
```

---

### Task 7: Phase 1A verification

- [ ] **Step 1: Run full test suite**

Run: `cd /Users/jfkn/projects/skills-manager && cargo test`
Expected: all tests pass

- [ ] **Step 2: Verify core crate builds independently**

Run: `cargo build -p skills-manager-core`
Expected: success

- [ ] **Step 3: Verify core crate without optional features**

Run: `cargo build -p skills-manager-core --no-default-features`
Expected: success (no tauri/tokio deps)

- [ ] **Step 4: Verify app runs**

Run: `cargo tauri dev`
Expected: app starts, can switch scenarios

- [ ] **Step 5: Check diff is only moves + import changes**

Run: `git diff HEAD~5..HEAD --stat`
Review: no logic changes, only file moves + import path updates + Cargo.toml changes

---

## Phase 1B: Skill Packs

### Task 8: Add packs migration (v4 → v5)

**Files:**
- Modify: `crates/skills-manager-core/src/migrations.rs`

- [ ] **Step 1: Write the failing test — fresh DB creates packs tables**

Add to the `#[cfg(test)]` module at the bottom of `crates/skills-manager-core/src/migrations.rs`:

```rust
#[test]
fn fresh_db_creates_packs_tables() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    // Verify packs table exists
    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM packs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);

    // Verify pack_skills table exists
    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM pack_skills", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);

    // Verify scenario_packs table exists
    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM scenario_packs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);

    // Verify schema version is 5
    let version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 5);
}

#[test]
fn v4_to_v5_migration_adds_packs_tables() {
    let conn = Connection::open_in_memory().unwrap();
    // Set version to 4 (pre-packs)
    conn.pragma_update(None, "user_version", 4).unwrap();
    // Create minimal existing tables that v4 expects
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS skills (id TEXT PRIMARY KEY, name TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS scenarios (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE);
         CREATE TABLE IF NOT EXISTS scenario_skills (
             scenario_id TEXT NOT NULL, skill_id TEXT NOT NULL,
             added_at INTEGER, sort_order INTEGER DEFAULT 0,
             PRIMARY KEY(scenario_id, skill_id)
         );"
    ).unwrap();

    run_migrations(&conn).unwrap();

    // packs table should exist
    conn.execute(
        "INSERT INTO packs (id, name, sort_order, created_at, updated_at) VALUES ('p1', 'test', 0, 0, 0)",
        [],
    ).unwrap();

    // pack_skills table should exist with FK
    conn.execute(
        "INSERT INTO pack_skills (pack_id, skill_id, sort_order) VALUES ('p1', 'nonexistent', 0)",
        [],
    ).unwrap();

    // scenario_packs table should exist
    conn.execute(
        "INSERT INTO scenario_packs (scenario_id, pack_id, sort_order) VALUES ('nonexistent', 'p1', 0)",
        [],
    ).unwrap();

    let version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 5);
}

#[test]
fn packs_cascade_delete() {
    let conn = Connection::open_in_memory().unwrap();
    // Enable foreign keys
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();

    // Insert a skill and a pack
    conn.execute("INSERT INTO skills (id, name, description, source_type, source_ref, central_path, content_hash, enabled, created_at, updated_at, status, update_status) VALUES ('s1', 'test-skill', '', 'local', '', '', '', 1, 0, 0, 'installed', 'none')", []).unwrap();
    conn.execute("INSERT INTO packs (id, name, sort_order, created_at, updated_at) VALUES ('p1', 'test-pack', 0, 0, 0)", []).unwrap();
    conn.execute("INSERT INTO pack_skills (pack_id, skill_id, sort_order) VALUES ('p1', 's1', 0)", []).unwrap();

    // Delete the pack — pack_skills should cascade
    conn.execute("DELETE FROM packs WHERE id = 'p1'", []).unwrap();

    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM pack_skills WHERE pack_id = 'p1'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p skills-manager-core -- fresh_db_creates_packs v4_to_v5 packs_cascade`
Expected: FAIL — packs table does not exist

- [ ] **Step 3: Implement migration v4→v5**

In `crates/skills-manager-core/src/migrations.rs`:

Change `LATEST_VERSION` from 4 to 5:
```rust
const LATEST_VERSION: u32 = 5;
```

Add match arm in `migrate_step`:
```rust
4 => migrate_v4_to_v5(conn),
```

Add the migration function:
```rust
/// v4 → v5: Add packs tables for skill grouping
fn migrate_v4_to_v5(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS packs (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            description TEXT,
            icon TEXT,
            color TEXT,
            sort_order INTEGER DEFAULT 0,
            created_at INTEGER,
            updated_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS pack_skills (
            pack_id TEXT NOT NULL REFERENCES packs(id) ON DELETE CASCADE,
            skill_id TEXT NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
            sort_order INTEGER DEFAULT 0,
            PRIMARY KEY(pack_id, skill_id)
        );

        CREATE TABLE IF NOT EXISTS scenario_packs (
            scenario_id TEXT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
            pack_id TEXT NOT NULL REFERENCES packs(id) ON DELETE CASCADE,
            sort_order INTEGER DEFAULT 0,
            PRIMARY KEY(scenario_id, pack_id)
        );",
    )?;
    Ok(())
}
```

Also add `4 =>` to the `migrate_v0_to_v1` fresh DB schema — add the three CREATE TABLE statements to the end of the v0→v1 migration so fresh DBs get them.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p skills-manager-core -- fresh_db_creates_packs v4_to_v5 packs_cascade`
Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: all tests pass (existing + new)

- [ ] **Step 6: Commit**

```bash
git add crates/skills-manager-core/src/migrations.rs
git commit -m "feat: add packs schema migration v4→v5"
```

---

### Task 9: Add PackRecord type and pack CRUD to skill_store

**Files:**
- Modify: `crates/skills-manager-core/src/skill_store.rs`

- [ ] **Step 1: Write failing tests for pack CRUD**

Add to the bottom of `skill_store.rs` (create a `#[cfg(test)]` module if one doesn't exist, or add to existing):

```rust
#[cfg(test)]
mod pack_tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_store() -> SkillStore {
        let tmp = NamedTempFile::new().unwrap();
        SkillStore::new(tmp.path().to_path_buf()).unwrap()
    }

    fn insert_test_skill(store: &SkillStore, id: &str, name: &str) {
        let skill = SkillRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            source_type: "local".to_string(),
            source_ref: None,
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: format!("/tmp/{}", name),
            content_hash: None,
            enabled: true,
            created_at: 0,
            updated_at: 0,
            status: "installed".to_string(),
            update_status: "none".to_string(),
            last_checked_at: None,
            last_check_error: None,
        };
        store.insert_skill(&skill).unwrap();
    }

    #[test]
    fn insert_and_get_pack() {
        let store = test_store();
        store.insert_pack("p1", "base", Some("Core skills"), None, None).unwrap();
        let packs = store.get_all_packs().unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].name, "base");
        assert_eq!(packs[0].description, Some("Core skills".to_string()));
    }

    #[test]
    fn get_pack_by_id() {
        let store = test_store();
        store.insert_pack("p1", "base", None, None, None).unwrap();
        let pack = store.get_pack_by_id("p1").unwrap();
        assert!(pack.is_some());
        assert_eq!(pack.unwrap().name, "base");

        let missing = store.get_pack_by_id("nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn update_pack() {
        let store = test_store();
        store.insert_pack("p1", "base", None, None, None).unwrap();
        store.update_pack("p1", "base-updated", Some("New desc"), None, None).unwrap();
        let pack = store.get_pack_by_id("p1").unwrap().unwrap();
        assert_eq!(pack.name, "base-updated");
        assert_eq!(pack.description, Some("New desc".to_string()));
    }

    #[test]
    fn delete_pack() {
        let store = test_store();
        store.insert_pack("p1", "base", None, None, None).unwrap();
        store.delete_pack("p1").unwrap();
        assert!(store.get_pack_by_id("p1").unwrap().is_none());
    }

    #[test]
    fn add_and_remove_skill_from_pack() {
        let store = test_store();
        insert_test_skill(&store, "s1", "skill-one");
        insert_test_skill(&store, "s2", "skill-two");
        store.insert_pack("p1", "base", None, None, None).unwrap();

        store.add_skill_to_pack("p1", "s1").unwrap();
        store.add_skill_to_pack("p1", "s2").unwrap();

        let skills = store.get_skills_for_pack("p1").unwrap();
        assert_eq!(skills.len(), 2);

        store.remove_skill_from_pack("p1", "s1").unwrap();
        let skills = store.get_skills_for_pack("p1").unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "s2");
    }

    #[test]
    fn add_and_remove_pack_from_scenario() {
        let store = test_store();
        store.insert_pack("p1", "base", None, None, None).unwrap();
        store.insert_scenario("sc1", "test-scenario", None, None).unwrap();

        store.add_pack_to_scenario("sc1", "p1").unwrap();
        let packs = store.get_packs_for_scenario("sc1").unwrap();
        assert_eq!(packs.len(), 1);

        store.remove_pack_from_scenario("sc1", "p1").unwrap();
        let packs = store.get_packs_for_scenario("sc1").unwrap();
        assert_eq!(packs.len(), 0);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p skills-manager-core -- pack_tests`
Expected: FAIL — `insert_pack` not found

- [ ] **Step 3: Add PackRecord type**

Add to `crates/skills-manager-core/src/skill_store.rs` after the existing record types:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct PackRecord {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub sort_order: i32,
    pub created_at: i64,
    pub updated_at: i64,
}
```

- [ ] **Step 4: Implement pack CRUD methods**

Add to the `impl SkillStore` block in `skill_store.rs`:

```rust
// ── Pack management ──

pub fn insert_pack(
    &self,
    id: &str,
    name: &str,
    description: Option<&str>,
    icon: Option<&str>,
    color: Option<&str>,
) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "INSERT INTO packs (id, name, description, icon, color, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM packs), ?6, ?7)",
        params![id, name, description, icon, color, now, now],
    )?;
    Ok(())
}

pub fn get_all_packs(&self) -> Result<Vec<PackRecord>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, name, description, icon, color, sort_order, created_at, updated_at
         FROM packs ORDER BY sort_order, name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PackRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            icon: row.get(3)?,
            color: row.get(4)?,
            sort_order: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn get_pack_by_id(&self, id: &str) -> Result<Option<PackRecord>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, name, description, icon, color, sort_order, created_at, updated_at
         FROM packs WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok(PackRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            icon: row.get(3)?,
            color: row.get(4)?,
            sort_order: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;
    Ok(rows.next().transpose()?)
}

pub fn update_pack(
    &self,
    id: &str,
    name: &str,
    description: Option<&str>,
    icon: Option<&str>,
    color: Option<&str>,
) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "UPDATE packs SET name = ?1, description = ?2, icon = ?3, color = ?4, updated_at = ?5 WHERE id = ?6",
        params![name, description, icon, color, now, id],
    )?;
    Ok(())
}

pub fn delete_pack(&self, id: &str) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    conn.execute("DELETE FROM packs WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn add_skill_to_pack(&self, pack_id: &str, skill_id: &str) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO pack_skills (pack_id, skill_id, sort_order)
         VALUES (?1, ?2, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM pack_skills WHERE pack_id = ?1))",
        params![pack_id, skill_id],
    )?;
    Ok(())
}

pub fn remove_skill_from_pack(&self, pack_id: &str, skill_id: &str) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    conn.execute(
        "DELETE FROM pack_skills WHERE pack_id = ?1 AND skill_id = ?2",
        params![pack_id, skill_id],
    )?;
    Ok(())
}

pub fn get_skills_for_pack(&self, pack_id: &str) -> Result<Vec<SkillRecord>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT s.id, s.name, s.description, s.source_type, s.source_ref, s.source_ref_resolved, s.source_subpath,
                s.source_branch, s.source_revision, s.remote_revision, s.central_path, s.content_hash, s.enabled,
                s.created_at, s.updated_at, s.status, s.update_status, s.last_checked_at, s.last_check_error
         FROM skills s
         INNER JOIN pack_skills ps ON s.id = ps.skill_id
         WHERE ps.pack_id = ?1
         ORDER BY ps.sort_order, s.name",
    )?;
    let rows = stmt.query_map(params![pack_id], map_skill_row)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn add_pack_to_scenario(&self, scenario_id: &str, pack_id: &str) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO scenario_packs (scenario_id, pack_id, sort_order)
         VALUES (?1, ?2, (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM scenario_packs WHERE scenario_id = ?1))",
        params![scenario_id, pack_id],
    )?;
    Ok(())
}

pub fn remove_pack_from_scenario(&self, scenario_id: &str, pack_id: &str) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    conn.execute(
        "DELETE FROM scenario_packs WHERE scenario_id = ?1 AND pack_id = ?2",
        params![scenario_id, pack_id],
    )?;
    Ok(())
}

pub fn get_packs_for_scenario(&self, scenario_id: &str) -> Result<Vec<PackRecord>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, p.description, p.icon, p.color, p.sort_order, p.created_at, p.updated_at
         FROM packs p
         INNER JOIN scenario_packs sp ON p.id = sp.pack_id
         WHERE sp.scenario_id = ?1
         ORDER BY sp.sort_order, p.name",
    )?;
    let rows = stmt.query_map(params![scenario_id], |row| {
        Ok(PackRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            icon: row.get(3)?,
            color: row.get(4)?,
            sort_order: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p skills-manager-core -- pack_tests`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/skills-manager-core/src/skill_store.rs
git commit -m "feat: add PackRecord type and pack CRUD operations"
```

---

### Task 10: Add effective skill resolution

**Files:**
- Modify: `crates/skills-manager-core/src/skill_store.rs`

- [ ] **Step 1: Write failing tests for effective skill resolution**

Add to the `pack_tests` module:

```rust
#[test]
fn effective_skills_packs_only() {
    let store = test_store();
    insert_test_skill(&store, "s1", "skill-one");
    insert_test_skill(&store, "s2", "skill-two");
    store.insert_pack("p1", "base", None, None, None).unwrap();
    store.add_skill_to_pack("p1", "s1").unwrap();
    store.add_skill_to_pack("p1", "s2").unwrap();
    store.insert_scenario("sc1", "test", None, None).unwrap();
    store.add_pack_to_scenario("sc1", "p1").unwrap();

    let skills = store.get_effective_skills_for_scenario("sc1").unwrap();
    assert_eq!(skills.len(), 2);
}

#[test]
fn effective_skills_direct_only_backward_compat() {
    let store = test_store();
    insert_test_skill(&store, "s1", "skill-one");
    store.insert_scenario("sc1", "test", None, None).unwrap();
    store.add_skill_to_scenario("sc1", "s1").unwrap();

    let skills = store.get_effective_skills_for_scenario("sc1").unwrap();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].id, "s1");
}

#[test]
fn effective_skills_packs_plus_direct_deduped() {
    let store = test_store();
    insert_test_skill(&store, "s1", "skill-one");
    insert_test_skill(&store, "s2", "skill-two");
    insert_test_skill(&store, "s3", "skill-three");
    store.insert_pack("p1", "base", None, None, None).unwrap();
    store.add_skill_to_pack("p1", "s1").unwrap();
    store.add_skill_to_pack("p1", "s2").unwrap();
    store.insert_scenario("sc1", "test", None, None).unwrap();
    store.add_pack_to_scenario("sc1", "p1").unwrap();
    // s2 is in pack AND direct — should be deduped
    store.add_skill_to_scenario("sc1", "s2").unwrap();
    store.add_skill_to_scenario("sc1", "s3").unwrap();

    let skills = store.get_effective_skills_for_scenario("sc1").unwrap();
    assert_eq!(skills.len(), 3); // s1, s2, s3 — s2 not doubled
    // Ordering: pack skills first (s1, s2), then direct-only (s3)
    assert_eq!(skills[0].id, "s1");
    assert_eq!(skills[2].id, "s3");
}

#[test]
fn effective_skills_duplicate_across_packs() {
    let store = test_store();
    insert_test_skill(&store, "s1", "skill-one");
    store.insert_pack("p1", "pack-a", None, None, None).unwrap();
    store.insert_pack("p2", "pack-b", None, None, None).unwrap();
    store.add_skill_to_pack("p1", "s1").unwrap();
    store.add_skill_to_pack("p2", "s1").unwrap();
    store.insert_scenario("sc1", "test", None, None).unwrap();
    store.add_pack_to_scenario("sc1", "p1").unwrap();
    store.add_pack_to_scenario("sc1", "p2").unwrap();

    let skills = store.get_effective_skills_for_scenario("sc1").unwrap();
    assert_eq!(skills.len(), 1); // s1 appears once
}

#[test]
fn effective_skills_empty_scenario() {
    let store = test_store();
    store.insert_scenario("sc1", "empty", None, None).unwrap();

    let skills = store.get_effective_skills_for_scenario("sc1").unwrap();
    assert_eq!(skills.len(), 0);
}

#[test]
fn effective_skills_handles_orphaned_skill() {
    let store = test_store();
    insert_test_skill(&store, "s1", "skill-one");
    store.insert_pack("p1", "base", None, None, None).unwrap();
    store.add_skill_to_pack("p1", "s1").unwrap();
    store.insert_scenario("sc1", "test", None, None).unwrap();
    store.add_pack_to_scenario("sc1", "p1").unwrap();

    // Delete the skill — pack_skills row orphaned
    store.delete_skill("s1").unwrap();

    let skills = store.get_effective_skills_for_scenario("sc1").unwrap();
    assert_eq!(skills.len(), 0); // orphaned row filtered out by INNER JOIN
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p skills-manager-core -- effective_skills`
Expected: FAIL — `get_effective_skills_for_scenario` not found

- [ ] **Step 3: Implement get_effective_skills_for_scenario**

Add to `impl SkillStore`:

```rust
/// Returns the deduplicated, ordered effective skill list for a scenario.
/// Order: pack skills first (by scenario_packs.sort_order, then pack_skills.sort_order),
/// then direct scenario_skills appended. Duplicates removed (first occurrence wins).
pub fn get_effective_skills_for_scenario(&self, scenario_id: &str) -> Result<Vec<SkillRecord>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT s.id, s.name, s.description, s.source_type, s.source_ref, s.source_ref_resolved, s.source_subpath,
                s.source_branch, s.source_revision, s.remote_revision, s.central_path, s.content_hash, s.enabled,
                s.created_at, s.updated_at, s.status, s.update_status, s.last_checked_at, s.last_check_error
         FROM (
             SELECT s.*, sp.sort_order * 10000 + ps.sort_order AS effective_order
             FROM skills s
             INNER JOIN pack_skills ps ON ps.skill_id = s.id
             INNER JOIN scenario_packs sp ON sp.pack_id = ps.pack_id
             WHERE sp.scenario_id = ?1
             UNION ALL
             SELECT s.*, 99999000 + ss.sort_order AS effective_order
             FROM skills s
             INNER JOIN scenario_skills ss ON ss.skill_id = s.id
             WHERE ss.scenario_id = ?1
         ) s
         GROUP BY s.id
         ORDER BY MIN(s.effective_order)",
    )?;
    let rows = stmt.query_map(params![scenario_id, scenario_id], map_skill_row)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Check if a skill is in the effective skill list for a scenario
/// (via packs or direct assignment).
pub fn is_skill_in_effective_scenario(&self, scenario_id: &str, skill_id: &str) -> Result<bool> {
    let conn = self.conn.lock().unwrap();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM (
             SELECT 1 FROM pack_skills ps
             INNER JOIN scenario_packs sp ON sp.pack_id = ps.pack_id
             WHERE sp.scenario_id = ?1 AND ps.skill_id = ?2
             UNION ALL
             SELECT 1 FROM scenario_skills
             WHERE scenario_id = ?1 AND skill_id = ?2
         )",
        params![scenario_id, skill_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p skills-manager-core -- effective_skills`
Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/skills-manager-core/src/skill_store.rs
git commit -m "feat: add effective skill resolution with pack composition"
```

---

### Task 11: Add packs Tauri commands

**Files:**
- Create: `src-tauri/src/commands/packs.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create commands/packs.rs**

Create `/Users/jfkn/projects/skills-manager/src-tauri/src/commands/packs.rs`:

```rust
use std::sync::Arc;
use tauri::State;

use crate::core::error::AppError;
use crate::core::skill_store::{PackRecord, SkillRecord, SkillStore};

#[tauri::command]
pub async fn get_all_packs(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<PackRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.get_all_packs().map_err(AppError::db))
        .await?
}

#[tauri::command]
pub async fn get_pack_by_id(
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Option<PackRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.get_pack_by_id(&id).map_err(AppError::db))
        .await?
}

#[tauri::command]
pub async fn create_pack(
    id: String,
    name: String,
    description: Option<String>,
    icon: Option<String>,
    color: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .insert_pack(&id, &name, description.as_deref(), icon.as_deref(), color.as_deref())
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn update_pack(
    id: String,
    name: String,
    description: Option<String>,
    icon: Option<String>,
    color: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .update_pack(&id, &name, description.as_deref(), icon.as_deref(), color.as_deref())
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn delete_pack(
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.delete_pack(&id).map_err(AppError::db))
        .await?
}

#[tauri::command]
pub async fn add_skill_to_pack(
    pack_id: String,
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.add_skill_to_pack(&pack_id, &skill_id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn remove_skill_from_pack(
    pack_id: String,
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.remove_skill_from_pack(&pack_id, &skill_id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_skills_for_pack(
    pack_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.get_skills_for_pack(&pack_id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_packs_for_scenario(
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<PackRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.get_packs_for_scenario(&scenario_id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn add_pack_to_scenario(
    scenario_id: String,
    pack_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.add_pack_to_scenario(&scenario_id, &pack_id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn remove_pack_from_scenario(
    scenario_id: String,
    pack_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.remove_pack_from_scenario(&scenario_id, &pack_id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_effective_skills_for_scenario(
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.get_effective_skills_for_scenario(&scenario_id).map_err(AppError::db)
    })
    .await?
}
```

- [ ] **Step 2: Add packs module to commands/mod.rs**

Add `pub mod packs;` to `src-tauri/src/commands/mod.rs`.

- [ ] **Step 3: Register commands in lib.rs**

Find the `.invoke_handler(tauri::generate_handler![...])` call in `src-tauri/src/lib.rs` and add all pack commands:

```rust
commands::packs::get_all_packs,
commands::packs::get_pack_by_id,
commands::packs::create_pack,
commands::packs::update_pack,
commands::packs::delete_pack,
commands::packs::add_skill_to_pack,
commands::packs::remove_skill_from_pack,
commands::packs::get_skills_for_pack,
commands::packs::get_packs_for_scenario,
commands::packs::add_pack_to_scenario,
commands::packs::remove_pack_from_scenario,
commands::packs::get_effective_skills_for_scenario,
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/packs.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat: add packs Tauri IPC commands"
```

---

### Task 12: Update scenario sync to use effective skills

**Files:**
- Modify: `src-tauri/src/commands/scenarios.rs`

- [ ] **Step 1: Find sync_scenario_skills function**

Read `src-tauri/src/commands/scenarios.rs` and locate `sync_scenario_skills()` and `remove_skill_from_scenario()`.

- [ ] **Step 2: Update sync_scenario_skills to use effective skills**

In `sync_scenario_skills()`, change the line that gets skills:

```rust
// OLD: let skills = store.get_skills_for_scenario(&scenario_id)?;
// NEW:
let skills = store.get_effective_skills_for_scenario(&scenario_id)?;
```

Do the same in `unsync_scenario_skills()` if it has a similar pattern.

- [ ] **Step 3: Update remove_skill_from_scenario to check pack membership**

In the `remove_skill_from_scenario` command, after removing the direct `scenario_skills` row, check if the skill is still in the effective list:

```rust
// After: store.remove_skill_from_scenario(&scenario_id, &skill_id).map_err(AppError::db)?;
// Add:
if let Ok(Some(active_id)) = store.get_active_scenario_id() {
    if active_id == scenario_id {
        // Only unsync if the skill is NOT still inherited via a pack
        let still_effective = store
            .is_skill_in_effective_scenario(&scenario_id, &skill_id)
            .unwrap_or(false);
        if !still_effective {
            // existing unsync logic here
        }
    }
}
```

- [ ] **Step 4: Verify compilation and tests**

Run: `cargo build && cargo test`
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/scenarios.rs
git commit -m "feat: scenario sync uses effective skill resolution (pack-aware)"
```

---

### Task 13: Phase 1B verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: all tests pass (existing + 14 new pack tests)

- [ ] **Step 2: Verify app starts with migration**

Run: `cargo tauri dev`
Expected: app starts (migration v4→v5 runs on existing DB), scenario switching works

- [ ] **Step 3: Verify backward compat**

In the running app, switch between scenarios. Verify all skills still appear correctly — behavior should be identical to pre-packs.

- [ ] **Step 4: Check DB migration ran**

```bash
sqlite3 ~/.skills-manager/skills-manager.db "PRAGMA user_version;"
```
Expected: `5`

```bash
sqlite3 ~/.skills-manager/skills-manager.db ".tables" | tr ' ' '\n' | sort
```
Expected: includes `packs`, `pack_skills`, `scenario_packs`

---

## Summary

| Task | Description | Phase |
|------|-------------|-------|
| 1 | Create Cargo workspace root | 1A |
| 2 | Create core crate skeleton | 1A |
| 3 | Move 18 modules | 1A |
| 4 | Fix internal imports | 1A |
| 5 | Feature-gate error.rs, wire deps | 1A |
| 6 | Update CI/tooling | 1A |
| 7 | Phase 1A verification | 1A |
| 8 | Add packs migration v4→v5 | 1B |
| 9 | Add PackRecord + pack CRUD | 1B |
| 10 | Add effective skill resolution | 1B |
| 11 | Add packs Tauri commands | 1B |
| 12 | Update scenario sync | 1B |
| 13 | Phase 1B verification | 1B |

Note: Default pack seeding (Task from spec) is deferred to a follow-up task after Phase 1B core is verified working. Seeding logic depends on matching existing skill names to pack definitions, which requires reading the actual DB content — better done as a separate, focused task.
