# Per-Agent Scenario Assignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Allow each agent (Claude Code, Cursor, Codex, etc.) to independently have its own base scenario + optional extra packs, instead of all agents sharing one global active scenario.

**Architecture:** Each agent gets an `agent_configs` row mapping it to a scenario, plus optional `agent_extra_packs` rows. The effective skill list for an agent = scenario skills UNION extra pack skills. The global `active_scenario` table is preserved for backward compatibility.

**Tech Stack:** Rust (Tauri + rusqlite), React + TypeScript + Tailwind CSS, clap CLI.

---

## Task 1: DB Migration v6 to v7

**Files to modify:**
- `crates/skills-manager-core/src/migrations.rs`

### 1a. Add DDL constant for agent config tables

Add near the top of the file, after `PACKS_SCHEMA_DDL`:

```rust
const AGENT_CONFIG_SCHEMA_DDL: &str = "
    CREATE TABLE IF NOT EXISTS agent_configs (
        tool_key TEXT PRIMARY KEY,
        scenario_id TEXT REFERENCES scenarios(id) ON DELETE SET NULL,
        managed INTEGER NOT NULL DEFAULT 1,
        updated_at INTEGER
    );

    CREATE TABLE IF NOT EXISTS agent_extra_packs (
        tool_key TEXT NOT NULL,
        pack_id TEXT NOT NULL REFERENCES packs(id) ON DELETE CASCADE,
        PRIMARY KEY(tool_key, pack_id)
    );
";
```

### 1b. Add `migrate_v6_to_v7`

```rust
/// v6 -> v7: Add per-agent scenario assignment and extra packs.
fn migrate_v6_to_v7(conn: &Connection) -> Result<()> {
    conn.execute_batch(AGENT_CONFIG_SCHEMA_DDL)?;
    Ok(())
}
```

### 1c. Update `migrate_step` match arm

Add case `6 => migrate_v6_to_v7(conn),` to the match in `migrate_step`.

### 1d. Bump LATEST_VERSION

Change `const LATEST_VERSION: u32 = 6;` to `const LATEST_VERSION: u32 = 7;`.

### 1e. Add `AGENT_CONFIG_SCHEMA_DDL` to `migrate_v0_to_v1`

After `conn.execute_batch(PLUGINS_SCHEMA_DDL)?;` on line 224, add:

```rust
conn.execute_batch(AGENT_CONFIG_SCHEMA_DDL)?;
```

### 1f. Add tests

```rust
#[test]
fn fresh_db_creates_agent_config_tables() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM agent_configs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);

    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM agent_extra_packs", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn v6_to_v7_migration_adds_agent_config_tables() {
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "user_version", 6).unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS skills (id TEXT PRIMARY KEY, name TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS scenarios (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE);
         CREATE TABLE IF NOT EXISTS packs (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE);",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO scenarios (id, name) VALUES ('sc1', 'test-scenario')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO packs (id, name) VALUES ('p1', 'test-pack')",
        [],
    )
    .unwrap();

    run_migrations(&conn).unwrap();

    // Insert an agent config
    conn.execute(
        "INSERT INTO agent_configs (tool_key, scenario_id, managed, updated_at) VALUES ('claude_code', 'sc1', 1, 0)",
        [],
    )
    .unwrap();

    // Insert an agent extra pack
    conn.execute(
        "INSERT INTO agent_extra_packs (tool_key, pack_id) VALUES ('claude_code', 'p1')",
        [],
    )
    .unwrap();

    let version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, LATEST_VERSION);
}

#[test]
fn agent_extra_packs_cascade_on_pack_delete() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO packs (id, name) VALUES ('p1', 'test-pack')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_extra_packs (tool_key, pack_id) VALUES ('claude_code', 'p1')",
        [],
    )
    .unwrap();

    conn.execute("DELETE FROM packs WHERE id = 'p1'", []).unwrap();

    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_extra_packs WHERE pack_id = 'p1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn agent_config_scenario_set_null_on_delete() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO scenarios (id, name) VALUES ('sc1', 'test')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_configs (tool_key, scenario_id, managed, updated_at) VALUES ('claude_code', 'sc1', 1, 0)",
        [],
    )
    .unwrap();

    conn.execute("DELETE FROM scenarios WHERE id = 'sc1'", []).unwrap();

    let scenario_id: Option<String> = conn
        .query_row(
            "SELECT scenario_id FROM agent_configs WHERE tool_key = 'claude_code'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(scenario_id.is_none());
}
```

### Verification

```bash
cargo test -p skills-manager-core -- migrations
```

---

## Task 2: AgentConfig type + CRUD in skill_store.rs

**Files to modify:**
- `crates/skills-manager-core/src/skill_store.rs`
- `crates/skills-manager-core/src/lib.rs` (re-export)

### 2a. Add `AgentConfigRecord` struct

Add after `ScenarioSkillToolToggleRecord`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct AgentConfigRecord {
    pub tool_key: String,
    pub scenario_id: Option<String>,
    pub managed: bool,
    pub updated_at: Option<i64>,
}
```

### 2b. Add CRUD methods to `impl SkillStore`

Add a new section `// -- Agent Config --` after the Packs section:

```rust
// -- Agent Config --

pub fn get_agent_config(&self, tool_key: &str) -> Result<Option<AgentConfigRecord>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT tool_key, scenario_id, managed, updated_at FROM agent_configs WHERE tool_key = ?1",
    )?;
    let mut rows = stmt.query_map(params![tool_key], |row| {
        Ok(AgentConfigRecord {
            tool_key: row.get(0)?,
            scenario_id: row.get(1)?,
            managed: row.get::<_, i32>(2)? != 0,
            updated_at: row.get(3)?,
        })
    })?;
    Ok(rows.next().and_then(|r| r.ok()))
}

pub fn get_all_agent_configs(&self) -> Result<Vec<AgentConfigRecord>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT tool_key, scenario_id, managed, updated_at FROM agent_configs ORDER BY tool_key",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(AgentConfigRecord {
            tool_key: row.get(0)?,
            scenario_id: row.get(1)?,
            managed: row.get::<_, i32>(2)? != 0,
            updated_at: row.get(3)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn set_agent_scenario(&self, tool_key: &str, scenario_id: &str) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "INSERT INTO agent_configs (tool_key, scenario_id, managed, updated_at)
         VALUES (?1, ?2, 1, ?3)
         ON CONFLICT(tool_key)
         DO UPDATE SET scenario_id = excluded.scenario_id, updated_at = excluded.updated_at",
        params![tool_key, scenario_id, now],
    )?;
    Ok(())
}

pub fn set_agent_managed(&self, tool_key: &str, managed: bool) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "INSERT INTO agent_configs (tool_key, managed, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(tool_key)
         DO UPDATE SET managed = excluded.managed, updated_at = excluded.updated_at",
        params![tool_key, managed, now],
    )?;
    Ok(())
}

/// Seed agent_configs from the current active_scenario for all given tool_keys.
/// Only inserts rows for tool_keys that don't already have a config.
pub fn init_agent_configs(&self, tool_keys: &[String]) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    let active_scenario_id: Option<String> = {
        let mut stmt =
            conn.prepare("SELECT scenario_id FROM active_scenario WHERE key = 'current'")?;
        let mut rows = stmt.query_map([], |row| row.get::<_, Option<String>>(0))?;
        rows.next().and_then(|r| r.ok()).flatten()
    };
    let now = chrono::Utc::now().timestamp_millis();
    for key in tool_keys {
        conn.execute(
            "INSERT OR IGNORE INTO agent_configs (tool_key, scenario_id, managed, updated_at)
             VALUES (?1, ?2, 1, ?3)",
            params![key, active_scenario_id, now],
        )?;
    }
    Ok(())
}

// -- Agent Extra Packs --

pub fn add_agent_extra_pack(&self, tool_key: &str, pack_id: &str) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO agent_extra_packs (tool_key, pack_id) VALUES (?1, ?2)",
        params![tool_key, pack_id],
    )?;
    Ok(())
}

pub fn remove_agent_extra_pack(&self, tool_key: &str, pack_id: &str) -> Result<()> {
    let conn = self.conn.lock().unwrap();
    conn.execute(
        "DELETE FROM agent_extra_packs WHERE tool_key = ?1 AND pack_id = ?2",
        params![tool_key, pack_id],
    )?;
    Ok(())
}

pub fn get_agent_extra_packs(&self, tool_key: &str) -> Result<Vec<PackRecord>> {
    let conn = self.conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, p.description, p.icon, p.color, p.sort_order, p.created_at, p.updated_at
         FROM packs p
         INNER JOIN agent_extra_packs aep ON p.id = aep.pack_id
         WHERE aep.tool_key = ?1
         ORDER BY p.name",
    )?;
    let rows = stmt.query_map(params![tool_key], map_pack_row)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Returns the deduplicated effective skill list for an agent.
/// = scenario effective skills UNION extra pack skills.
/// If the agent has no config or no scenario, returns empty.
pub fn get_effective_skills_for_agent(&self, tool_key: &str) -> Result<Vec<SkillRecord>> {
    let conn = self.conn.lock().unwrap();

    // Get agent's scenario_id
    let scenario_id: Option<String> = {
        let mut stmt = conn.prepare(
            "SELECT scenario_id FROM agent_configs WHERE tool_key = ?1 AND managed = 1",
        )?;
        let mut rows = stmt.query_map(params![tool_key], |row| row.get::<_, Option<String>>(0))?;
        rows.next().and_then(|r| r.ok()).flatten()
    };

    let scenario_id = match scenario_id {
        Some(id) => id,
        None => return Ok(Vec::new()),
    };

    // Build effective skills: scenario packs + scenario direct skills + agent extra packs
    // Using the same ordering strategy as get_effective_skills_for_scenario, but adding
    // agent extra packs at the end (effective_order 199990000+).
    let mut stmt = conn.prepare(
        "SELECT s.id, s.name, s.description, s.source_type, s.source_ref, s.source_ref_resolved, s.source_subpath,
                s.source_branch, s.source_revision, s.remote_revision, s.central_path, s.content_hash, s.enabled,
                s.created_at, s.updated_at, s.status, s.update_status, s.last_checked_at, s.last_check_error
         FROM (
             -- Scenario pack skills
             SELECT ps.skill_id AS id, sp.sort_order * 10000 + ps.sort_order AS effective_order
             FROM pack_skills ps
             INNER JOIN scenario_packs sp ON ps.pack_id = sp.pack_id
             WHERE sp.scenario_id = ?1
             UNION ALL
             -- Scenario direct skills
             SELECT ss.skill_id AS id, 99999000 + ss.sort_order AS effective_order
             FROM scenario_skills ss
             WHERE ss.scenario_id = ?1
             UNION ALL
             -- Agent extra pack skills
             SELECT ps2.skill_id AS id, 199990000 + ps2.sort_order AS effective_order
             FROM pack_skills ps2
             INNER JOIN agent_extra_packs aep ON ps2.pack_id = aep.pack_id
             WHERE aep.tool_key = ?2
         ) AS ordering
         INNER JOIN skills s ON s.id = ordering.id
         GROUP BY s.id
         ORDER BY MIN(ordering.effective_order)",
    )?;
    let rows = stmt.query_map(params![scenario_id, tool_key], map_skill_row)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}
```

### 2c. Re-export in lib.rs

In `crates/skills-manager-core/src/lib.rs`, add `AgentConfigRecord` to the re-export:

```rust
pub use skill_store::{
    AgentConfigRecord, DiscoveredSkillRecord, ManagedPluginRecord, PackRecord, ProjectRecord,
    ScenarioPluginRecord, ScenarioRecord, ScenarioSkillToolToggleRecord, SkillRecord, SkillStore,
    SkillTargetRecord,
};
```

### 2d. Tests (TDD -- write first)

Add a new test module `agent_config_tests` at the bottom of `skill_store.rs`:

```rust
#[cfg(test)]
mod agent_config_tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_store() -> (SkillStore, NamedTempFile) {
        let tmp = NamedTempFile::new().unwrap();
        let store = SkillStore::new(&tmp.path().to_path_buf()).unwrap();
        (store, tmp)
    }

    fn insert_test_skill(store: &SkillStore, id: &str, name: &str) {
        let rec = SkillRecord {
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
            central_path: format!("/tmp/skills/{id}"),
            content_hash: None,
            enabled: true,
            created_at: 1000,
            updated_at: 1000,
            status: "ok".to_string(),
            update_status: "unknown".to_string(),
            last_checked_at: None,
            last_check_error: None,
        };
        store.insert_skill(&rec).unwrap();
    }

    fn insert_test_scenario(store: &SkillStore, id: &str, name: &str) {
        let rec = ScenarioRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            icon: None,
            sort_order: 0,
            created_at: 1000,
            updated_at: 1000,
        };
        store.insert_scenario(&rec).unwrap();
    }

    #[test]
    fn get_agent_config_none_by_default() {
        let (store, _tmp) = test_store();
        let config = store.get_agent_config("claude_code").unwrap();
        assert!(config.is_none());
    }

    #[test]
    fn set_and_get_agent_scenario() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Full Dev");
        store.set_agent_scenario("claude_code", "sc1").unwrap();

        let config = store.get_agent_config("claude_code").unwrap().unwrap();
        assert_eq!(config.tool_key, "claude_code");
        assert_eq!(config.scenario_id.as_deref(), Some("sc1"));
        assert!(config.managed);
    }

    #[test]
    fn set_agent_scenario_upserts() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Full Dev");
        insert_test_scenario(&store, "sc2", "Minimal");

        store.set_agent_scenario("claude_code", "sc1").unwrap();
        store.set_agent_scenario("claude_code", "sc2").unwrap();

        let config = store.get_agent_config("claude_code").unwrap().unwrap();
        assert_eq!(config.scenario_id.as_deref(), Some("sc2"));
    }

    #[test]
    fn set_agent_managed_flag() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Full Dev");
        store.set_agent_scenario("claude_code", "sc1").unwrap();

        store.set_agent_managed("claude_code", false).unwrap();
        let config = store.get_agent_config("claude_code").unwrap().unwrap();
        assert!(!config.managed);

        store.set_agent_managed("claude_code", true).unwrap();
        let config = store.get_agent_config("claude_code").unwrap().unwrap();
        assert!(config.managed);
    }

    #[test]
    fn get_all_agent_configs() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Full Dev");
        insert_test_scenario(&store, "sc2", "Minimal");

        store.set_agent_scenario("claude_code", "sc1").unwrap();
        store.set_agent_scenario("cursor", "sc2").unwrap();

        let configs = store.get_all_agent_configs().unwrap();
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].tool_key, "claude_code");
        assert_eq!(configs[1].tool_key, "cursor");
    }

    #[test]
    fn init_agent_configs_seeds_from_active_scenario() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Full Dev");
        store.set_active_scenario("sc1").unwrap();

        let keys = vec!["claude_code".to_string(), "cursor".to_string()];
        store.init_agent_configs(&keys).unwrap();

        let configs = store.get_all_agent_configs().unwrap();
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].scenario_id.as_deref(), Some("sc1"));
        assert_eq!(configs[1].scenario_id.as_deref(), Some("sc1"));
        assert!(configs[0].managed);
    }

    #[test]
    fn init_agent_configs_does_not_overwrite_existing() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Full Dev");
        insert_test_scenario(&store, "sc2", "Minimal");
        store.set_active_scenario("sc1").unwrap();

        // Manually set claude_code to sc2
        store.set_agent_scenario("claude_code", "sc2").unwrap();

        // Init should not overwrite claude_code
        let keys = vec!["claude_code".to_string(), "cursor".to_string()];
        store.init_agent_configs(&keys).unwrap();

        let cc = store.get_agent_config("claude_code").unwrap().unwrap();
        assert_eq!(cc.scenario_id.as_deref(), Some("sc2")); // preserved
        let cursor = store.get_agent_config("cursor").unwrap().unwrap();
        assert_eq!(cursor.scenario_id.as_deref(), Some("sc1")); // seeded
    }

    #[test]
    fn add_and_remove_agent_extra_pack() {
        let (store, _tmp) = test_store();
        store
            .insert_pack("p1", "Marketing", None, None, None)
            .unwrap();
        store
            .insert_pack("p2", "Design", None, None, None)
            .unwrap();

        store.add_agent_extra_pack("claude_code", "p1").unwrap();
        store.add_agent_extra_pack("claude_code", "p2").unwrap();

        let packs = store.get_agent_extra_packs("claude_code").unwrap();
        assert_eq!(packs.len(), 2);

        store.remove_agent_extra_pack("claude_code", "p1").unwrap();
        let packs = store.get_agent_extra_packs("claude_code").unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].id, "p2");
    }

    #[test]
    fn add_agent_extra_pack_idempotent() {
        let (store, _tmp) = test_store();
        store
            .insert_pack("p1", "Marketing", None, None, None)
            .unwrap();

        store.add_agent_extra_pack("claude_code", "p1").unwrap();
        store.add_agent_extra_pack("claude_code", "p1").unwrap(); // duplicate is ignored

        let packs = store.get_agent_extra_packs("claude_code").unwrap();
        assert_eq!(packs.len(), 1);
    }

    #[test]
    fn effective_skills_for_agent_scenario_only() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Skill A");
        insert_test_skill(&store, "s2", "Skill B");
        insert_test_scenario(&store, "sc1", "Full Dev");

        store.add_skill_to_scenario("sc1", "s1").unwrap();
        store.add_skill_to_scenario("sc1", "s2").unwrap();
        store.set_agent_scenario("claude_code", "sc1").unwrap();

        let effective = store
            .get_effective_skills_for_agent("claude_code")
            .unwrap();
        assert_eq!(effective.len(), 2);
    }

    #[test]
    fn effective_skills_for_agent_with_extra_packs() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Skill A");
        insert_test_skill(&store, "s2", "Skill B");
        insert_test_skill(&store, "s3", "Skill C");
        insert_test_scenario(&store, "sc1", "Full Dev");

        // s1 in scenario via direct
        store.add_skill_to_scenario("sc1", "s1").unwrap();

        // s2, s3 in extra pack
        store
            .insert_pack("p1", "Marketing", None, None, None)
            .unwrap();
        store.add_skill_to_pack("p1", "s2").unwrap();
        store.add_skill_to_pack("p1", "s3").unwrap();

        store.set_agent_scenario("claude_code", "sc1").unwrap();
        store.add_agent_extra_pack("claude_code", "p1").unwrap();

        let effective = store
            .get_effective_skills_for_agent("claude_code")
            .unwrap();
        assert_eq!(effective.len(), 3);
        let ids: Vec<&str> = effective.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"s1"));
        assert!(ids.contains(&"s2"));
        assert!(ids.contains(&"s3"));
    }

    #[test]
    fn effective_skills_for_agent_dedupes_across_scenario_and_extra() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Shared Skill");
        insert_test_scenario(&store, "sc1", "Full Dev");

        // s1 in scenario directly
        store.add_skill_to_scenario("sc1", "s1").unwrap();

        // s1 also in extra pack
        store
            .insert_pack("p1", "Extra", None, None, None)
            .unwrap();
        store.add_skill_to_pack("p1", "s1").unwrap();

        store.set_agent_scenario("claude_code", "sc1").unwrap();
        store.add_agent_extra_pack("claude_code", "p1").unwrap();

        let effective = store
            .get_effective_skills_for_agent("claude_code")
            .unwrap();
        assert_eq!(effective.len(), 1, "s1 should appear only once");
    }

    #[test]
    fn effective_skills_for_unmanaged_agent_is_empty() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Skill A");
        insert_test_scenario(&store, "sc1", "Full Dev");
        store.add_skill_to_scenario("sc1", "s1").unwrap();

        store.set_agent_scenario("claude_code", "sc1").unwrap();
        store.set_agent_managed("claude_code", false).unwrap();

        let effective = store
            .get_effective_skills_for_agent("claude_code")
            .unwrap();
        assert!(effective.is_empty(), "unmanaged agents return no skills");
    }

    #[test]
    fn effective_skills_for_nonexistent_agent_is_empty() {
        let (store, _tmp) = test_store();
        let effective = store
            .get_effective_skills_for_agent("nonexistent")
            .unwrap();
        assert!(effective.is_empty());
    }

    #[test]
    fn different_agents_different_scenarios() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Big Skill");
        insert_test_skill(&store, "s2", "Small Skill");
        insert_test_scenario(&store, "sc1", "Full Dev");
        insert_test_scenario(&store, "sc2", "Minimal");

        store.add_skill_to_scenario("sc1", "s1").unwrap();
        store.add_skill_to_scenario("sc1", "s2").unwrap();
        store.add_skill_to_scenario("sc2", "s2").unwrap();

        store.set_agent_scenario("claude_code", "sc1").unwrap();
        store.set_agent_scenario("codex", "sc2").unwrap();

        let cc_skills = store
            .get_effective_skills_for_agent("claude_code")
            .unwrap();
        assert_eq!(cc_skills.len(), 2);

        let codex_skills = store
            .get_effective_skills_for_agent("codex")
            .unwrap();
        assert_eq!(codex_skills.len(), 1);
        assert_eq!(codex_skills[0].id, "s2");
    }
}
```

### Verification

```bash
cargo test -p skills-manager-core -- agent_config_tests
```

---

## Task 3: Update sync logic

**Files to modify:**
- `src-tauri/src/commands/scenarios.rs`

### 3a. Update `sync_scenario_skills` to be per-agent aware

Replace the existing `sync_scenario_skills` function body. The key change: instead of getting one global effective skill list, iterate over managed agents and get each agent's effective skill list via `get_effective_skills_for_agent`. If no agent_configs exist in the DB (backward compat), fall back to the existing behavior using the global scenario.

```rust
pub(crate) fn sync_scenario_skills(store: &SkillStore, scenario_id: &str) -> Result<(), AppError> {
    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
    let adapters = tool_adapters::enabled_installed_adapters(store);

    // Ensure agent_configs are seeded
    let adapter_keys: Vec<String> = adapters.iter().map(|a| a.key.clone()).collect();
    store.init_agent_configs(&adapter_keys).map_err(AppError::db)?;

    for adapter in &adapters {
        let skills = store
            .get_effective_skills_for_agent(&adapter.key)
            .map_err(AppError::db)?;

        for skill in &skills {
            let source = PathBuf::from(&skill.central_path);
            let target = adapter.skills_dir().join(&skill.name);

            // Ensure scenario_skill_tools defaults for the agent's scenario
            if let Some(config) = store.get_agent_config(&adapter.key).map_err(AppError::db)? {
                if let Some(ref sid) = config.scenario_id {
                    store
                        .ensure_scenario_skill_tool_defaults(sid, &skill.id, &[adapter.key.clone()])
                        .map_err(AppError::db)?;
                    let enabled = store
                        .get_enabled_tools_for_scenario_skill(sid, &skill.id)
                        .map_err(AppError::db)?;
                    if !enabled.contains(&adapter.key) {
                        continue; // tool disabled for this skill in this scenario
                    }
                }
            }

            let mode =
                sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
            match sync_engine::sync_skill(&source, &target, mode) {
                Ok(actual_mode) => {
                    let now = chrono::Utc::now().timestamp_millis();
                    let target_record = crate::core::skill_store::SkillTargetRecord {
                        id: uuid::Uuid::new_v4().to_string(),
                        skill_id: skill.id.clone(),
                        tool: adapter.key.clone(),
                        target_path: target.to_string_lossy().to_string(),
                        mode: actual_mode.as_str().to_string(),
                        status: "ok".to_string(),
                        synced_at: Some(now),
                        last_error: None,
                    };
                    if let Err(e) = store.insert_target(&target_record) {
                        log::warn!("Failed to insert sync target for skill {}: {e}", skill.id);
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Failed to sync skill {} to {}: {e}",
                        skill.id,
                        target.display()
                    );
                }
            }
        }
    }

    // Apply per-scenario plugin state to installed_plugins.json
    if let Err(e) = plugins::apply_scenario_plugins(store, scenario_id) {
        log::warn!("Failed to apply scenario plugin state: {e}");
    }

    Ok(())
}
```

### 3b. Update `unsync_scenario_skills` similarly

Replace the existing function. For each managed agent, get its effective skill list and remove only those synced targets.

```rust
pub(crate) fn unsync_scenario_skills(
    store: &SkillStore,
    scenario_id: &str,
) -> Result<(), AppError> {
    let adapters = tool_adapters::enabled_installed_adapters(store);

    for adapter in &adapters {
        // Get all targets for this tool and remove them
        // We use the existing targets table rather than recomputing effective skills,
        // which handles edge cases better (e.g. skill deleted during session)
        let all_targets = store.get_all_targets().unwrap_or_default();
        let tool_targets: Vec<_> = all_targets
            .iter()
            .filter(|t| t.tool == adapter.key)
            .collect();

        for target in &tool_targets {
            let path = PathBuf::from(&target.target_path);
            if let Err(e) = sync_engine::remove_target(&path) {
                log::warn!("Failed to remove sync target {}: {e}", path.display());
            }
            if let Err(e) = store.delete_target(&target.skill_id, &target.tool) {
                log::warn!(
                    "Failed to delete target record for skill {}, tool {}: {e}",
                    target.skill_id, target.tool
                );
            }
        }
    }

    // Restore all plugins when unsyncing
    if let Err(e) = plugins::restore_all_plugins(store) {
        log::warn!("Failed to restore plugins during unsync: {e}");
    }

    Ok(())
}
```

### 3c. Update `switch_scenario` to update all managed agents

In the `switch_scenario` command, after setting the active scenario, also update all managed agent configs:

```rust
// After store.set_active_scenario(&id):
// Update all managed agent configs to the new scenario
let configs = store.get_all_agent_configs().map_err(AppError::db)?;
for config in &configs {
    if config.managed {
        store
            .set_agent_scenario(&config.tool_key, &id)
            .map_err(AppError::db)?;
    }
}
```

### Verification

```bash
cargo build -p skills-manager-app
cargo test -p skills-manager-core
```

---

## Task 4: Tauri IPC commands for agent config

**Files to create/modify:**
- `src-tauri/src/commands/agents.rs` (new)
- `src-tauri/src/commands/mod.rs` (add module)
- `src-tauri/src/lib.rs` (register commands)

### 4a. Create `agents.rs`

```rust
use std::sync::Arc;
use tauri::State;

use crate::core::{
    error::AppError,
    skill_store::{AgentConfigRecord, PackRecord, SkillRecord, SkillStore},
    tool_adapters,
};

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AgentConfigDto {
    pub tool_key: String,
    pub display_name: String,
    pub installed: bool,
    pub managed: bool,
    pub scenario_id: Option<String>,
    pub scenario_name: Option<String>,
    pub extra_pack_count: usize,
    pub effective_skill_count: usize,
    pub updated_at: Option<i64>,
}

#[tauri::command]
pub async fn get_all_agent_configs(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<AgentConfigDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let adapters = tool_adapters::enabled_installed_adapters(&store);
        let adapter_keys: Vec<String> = adapters.iter().map(|a| a.key.clone()).collect();
        store.init_agent_configs(&adapter_keys).map_err(AppError::db)?;

        let configs = store.get_all_agent_configs().map_err(AppError::db)?;
        let scenarios = store.get_all_scenarios().map_err(AppError::db)?;

        let mut result = Vec::new();
        for config in &configs {
            let adapter = adapters.iter().find(|a| a.key == config.tool_key);
            let display_name = adapter
                .map(|a| a.display_name.clone())
                .unwrap_or_else(|| config.tool_key.clone());
            let installed = adapter.map(|a| a.is_installed()).unwrap_or(false);
            let scenario_name = config
                .scenario_id
                .as_ref()
                .and_then(|sid| scenarios.iter().find(|s| s.id == *sid))
                .map(|s| s.name.clone());
            let extra_packs = store
                .get_agent_extra_packs(&config.tool_key)
                .map_err(AppError::db)?;
            let effective = store
                .get_effective_skills_for_agent(&config.tool_key)
                .map_err(AppError::db)?;

            result.push(AgentConfigDto {
                tool_key: config.tool_key.clone(),
                display_name,
                installed,
                managed: config.managed,
                scenario_id: config.scenario_id.clone(),
                scenario_name,
                extra_pack_count: extra_packs.len(),
                effective_skill_count: effective.len(),
                updated_at: config.updated_at,
            });
        }
        Ok(result)
    })
    .await?
}

#[tauri::command]
pub async fn get_agent_config(
    tool_key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Option<AgentConfigRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.get_agent_config(&tool_key).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn set_agent_scenario(
    app: tauri::AppHandle,
    tool_key: String,
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        store
            .set_agent_scenario(&tool_key, &scenario_id)
            .map_err(AppError::db)?;

        // Re-sync this specific agent
        let adapters = tool_adapters::enabled_installed_adapters(&store);
        if let Some(adapter) = adapters.iter().find(|a| a.key == tool_key) {
            let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
            let skills = store
                .get_effective_skills_for_agent(&tool_key)
                .map_err(AppError::db)?;

            // Remove existing targets for this tool first
            let existing_targets = store.get_all_targets().unwrap_or_default();
            for target in existing_targets.iter().filter(|t| t.tool == tool_key) {
                let path = std::path::PathBuf::from(&target.target_path);
                let _ = crate::core::sync_engine::remove_target(&path);
                let _ = store.delete_target(&target.skill_id, &target.tool);
            }

            // Sync new skills
            let mode = crate::core::sync_engine::sync_mode_for_tool(
                &adapter.key,
                configured_mode.as_deref(),
            );
            for skill in &skills {
                let source = std::path::PathBuf::from(&skill.central_path);
                let target_path = adapter.skills_dir().join(&skill.name);
                match crate::core::sync_engine::sync_skill(&source, &target_path, mode) {
                    Ok(actual_mode) => {
                        let now = chrono::Utc::now().timestamp_millis();
                        let target_record = crate::core::skill_store::SkillTargetRecord {
                            id: uuid::Uuid::new_v4().to_string(),
                            skill_id: skill.id.clone(),
                            tool: adapter.key.clone(),
                            target_path: target_path.to_string_lossy().to_string(),
                            mode: actual_mode.as_str().to_string(),
                            status: "ok".to_string(),
                            synced_at: Some(now),
                            last_error: None,
                        };
                        let _ = store.insert_target(&target_record);
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to sync skill {} to {}: {e}",
                            skill.id,
                            target_path.display()
                        );
                    }
                }
            }
        }
        Ok(())
    })
    .await?;
    if result.is_ok() {
        crate::refresh_tray_menu_best_effort(&app);
    }
    result
}

fn refresh_tray_menu_best_effort(app: &tauri::AppHandle) {
    if let Err(err) = crate::refresh_tray_menu(app) {
        log::warn!("Failed to refresh tray menu after agent config change: {err}");
    }
}

#[tauri::command]
pub async fn set_agent_managed(
    tool_key: String,
    managed: bool,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .set_agent_managed(&tool_key, managed)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn add_agent_extra_pack(
    tool_key: String,
    pack_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .add_agent_extra_pack(&tool_key, &pack_id)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn remove_agent_extra_pack(
    tool_key: String,
    pack_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .remove_agent_extra_pack(&tool_key, &pack_id)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_agent_extra_packs(
    tool_key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<PackRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .get_agent_extra_packs(&tool_key)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_effective_skills_for_agent(
    tool_key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .get_effective_skills_for_agent(&tool_key)
            .map_err(AppError::db)
    })
    .await?
}
```

### 4b. Register in `mod.rs`

Add to `src-tauri/src/commands/mod.rs`:

```rust
pub mod agents;
```

### 4c. Register in `lib.rs`

Add to the `invoke_handler` macro in `src-tauri/src/lib.rs`, after the Scenarios block:

```rust
// Agents
commands::agents::get_all_agent_configs,
commands::agents::get_agent_config,
commands::agents::set_agent_scenario,
commands::agents::set_agent_managed,
commands::agents::add_agent_extra_pack,
commands::agents::remove_agent_extra_pack,
commands::agents::get_agent_extra_packs,
commands::agents::get_effective_skills_for_agent,
```

### Verification

```bash
cargo build -p skills-manager-app
```

---

## Task 5: CLI commands for agent management

**Files to modify:**
- `crates/skills-manager-cli/src/main.rs`
- `crates/skills-manager-cli/src/commands.rs`

### 5a. Add CLI subcommands in `main.rs`

Add to `Commands` enum:

```rust
/// List agents with their scenario assignments
Agents,

/// Manage a specific agent
Agent {
    #[command(subcommand)]
    action: AgentAction,
},
```

Add a new enum:

```rust
#[derive(Subcommand)]
enum AgentAction {
    /// Add an extra pack to an agent
    AddPack {
        /// Agent name (tool key, e.g. "claude_code")
        agent: String,
        /// Pack name
        pack: String,
    },
    /// Remove an extra pack from an agent
    RemovePack {
        /// Agent name (tool key, e.g. "claude_code")
        agent: String,
        /// Pack name
        pack: String,
    },
}
```

Update the `Switch` variant to support optional agent-specific switch:

```rust
/// Switch to a scenario (all agents, or one agent)
#[command(alias = "sw")]
Switch {
    /// Scenario name, or agent name when used with a second arg
    name: String,
    /// If provided, first arg is agent name and this is the scenario
    scenario: Option<String>,
},
```

Add match arms in `main()`:

```rust
Commands::Agents => commands::cmd_agents(),
Commands::Agent { action } => match action {
    AgentAction::AddPack { agent, pack } => commands::cmd_agent_add_pack(&agent, &pack),
    AgentAction::RemovePack { agent, pack } => commands::cmd_agent_remove_pack(&agent, &pack),
},
```

Update the Switch match arm:

```rust
Commands::Switch { name, scenario } => match scenario {
    Some(scenario_name) => commands::cmd_switch_agent(&name, &scenario_name),
    None => commands::cmd_switch(&name),
},
```

### 5b. Add command implementations in `commands.rs`

```rust
pub fn cmd_agents() -> Result<()> {
    let store = open_store()?;
    let adapters = tool_adapters::enabled_installed_adapters(&store);
    let adapter_keys: Vec<String> = adapters.iter().map(|a| a.key.clone()).collect();
    store.init_agent_configs(&adapter_keys)?;

    let configs = store.get_all_agent_configs()?;
    let scenarios = store.get_all_scenarios()?;

    println!("Agents:");
    for config in &configs {
        let adapter = adapters.iter().find(|a| a.key == config.tool_key);
        let display_name = adapter
            .map(|a| a.display_name.as_str())
            .unwrap_or(&config.tool_key);
        let installed = adapter.map(|a| a.is_installed()).unwrap_or(false);

        if !installed {
            continue;
        }

        let scenario_name = config
            .scenario_id
            .as_ref()
            .and_then(|sid| scenarios.iter().find(|s| s.id == *sid))
            .map(|s| s.name.as_str())
            .unwrap_or("(none)");

        let extra_packs = store.get_agent_extra_packs(&config.tool_key)?;
        let effective = store.get_effective_skills_for_agent(&config.tool_key)?;

        let status = if config.managed { "*" } else { "-" };
        let extra = if extra_packs.is_empty() {
            String::new()
        } else {
            format!(
                " +{}",
                extra_packs
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", +")
            )
        };

        println!(
            "  {} {:<20} {:<20}{} ({} skills)",
            status,
            display_name,
            scenario_name,
            extra,
            effective.len()
        );
    }
    println!();
    println!("  * = managed, - = unmanaged");

    Ok(())
}

pub fn cmd_switch_agent(agent_name: &str, scenario_name: &str) -> Result<()> {
    let store = open_store()?;
    let target = find_scenario_by_name(&store, scenario_name)?;

    // Resolve agent key -- accept tool key directly or display name
    let adapters = tool_adapters::enabled_installed_adapters(&store);
    let adapter_keys: Vec<String> = adapters.iter().map(|a| a.key.clone()).collect();
    store.init_agent_configs(&adapter_keys)?;

    let agent_lower = agent_name.to_lowercase();
    let adapter = adapters
        .iter()
        .find(|a| {
            a.key.to_lowercase() == agent_lower
                || a.display_name.to_lowercase() == agent_lower
        })
        .ok_or_else(|| {
            let available: Vec<&str> = adapters.iter().map(|a| a.key.as_str()).collect();
            anyhow::anyhow!(
                "Agent '{}' not found. Available: {}",
                agent_name,
                available.join(", ")
            )
        })?;

    let configured_mode = store.get_setting("sync_mode").ok().flatten();

    // Unsync old skills for this agent
    let old_targets = store.get_all_targets().unwrap_or_default();
    for t in old_targets.iter().filter(|t| t.tool == adapter.key) {
        let path = PathBuf::from(&t.target_path);
        let _ = sync_engine::remove_target(&path);
        let _ = store.delete_target(&t.skill_id, &t.tool);
    }

    // Set new scenario for this agent
    store.set_agent_scenario(&adapter.key, &target.id)?;

    // Sync new skills
    let skills = store.get_effective_skills_for_agent(&adapter.key)?;
    let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
    let mut synced = 0;
    for skill in &skills {
        let source = PathBuf::from(&skill.central_path);
        if !source.exists() {
            continue;
        }
        let target_path = adapter.skills_dir().join(&skill.name);
        match sync_engine::sync_skill(&source, &target_path, mode) {
            Ok(_) => synced += 1,
            Err(e) => eprintln!("  Warning: failed to sync '{}': {}", skill.name, e),
        }
    }

    println!(
        "Switched {} to {} ({} skills synced)",
        adapter.display_name, target.name, synced
    );
    Ok(())
}

pub fn cmd_agent_add_pack(agent_name: &str, pack_name: &str) -> Result<()> {
    let store = open_store()?;
    let pack = find_pack_by_name(&store, pack_name)?;

    let adapters = tool_adapters::enabled_installed_adapters(&store);
    let agent_lower = agent_name.to_lowercase();
    let adapter = adapters
        .iter()
        .find(|a| {
            a.key.to_lowercase() == agent_lower
                || a.display_name.to_lowercase() == agent_lower
        })
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found", agent_name))?;

    store.add_agent_extra_pack(&adapter.key, &pack.id)?;
    println!(
        "Added extra pack '{}' to {}",
        pack.name, adapter.display_name
    );

    // Re-sync if managed
    if let Some(config) = store.get_agent_config(&adapter.key)? {
        if config.managed {
            let configured_mode = store.get_setting("sync_mode").ok().flatten();
            let skills = store.get_effective_skills_for_agent(&adapter.key)?;
            let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
            for skill in &skills {
                let source = PathBuf::from(&skill.central_path);
                if source.exists() {
                    let target_path = adapter.skills_dir().join(&skill.name);
                    let _ = sync_engine::sync_skill(&source, &target_path, mode);
                }
            }
            println!("  Re-synced ({} effective skills)", skills.len());
        }
    }

    Ok(())
}

pub fn cmd_agent_remove_pack(agent_name: &str, pack_name: &str) -> Result<()> {
    let store = open_store()?;
    let pack = find_pack_by_name(&store, pack_name)?;

    let adapters = tool_adapters::enabled_installed_adapters(&store);
    let agent_lower = agent_name.to_lowercase();
    let adapter = adapters
        .iter()
        .find(|a| {
            a.key.to_lowercase() == agent_lower
                || a.display_name.to_lowercase() == agent_lower
        })
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found", agent_name))?;

    store.remove_agent_extra_pack(&adapter.key, &pack.id)?;
    println!(
        "Removed extra pack '{}' from {}",
        pack.name, adapter.display_name
    );

    // Re-sync if managed -- unsync removed pack skills, re-sync effective set
    if let Some(config) = store.get_agent_config(&adapter.key)? {
        if config.managed {
            let configured_mode = store.get_setting("sync_mode").ok().flatten();

            // Remove old targets
            let old_targets = store.get_all_targets().unwrap_or_default();
            for t in old_targets.iter().filter(|t| t.tool == adapter.key) {
                let path = PathBuf::from(&t.target_path);
                let _ = sync_engine::remove_target(&path);
                let _ = store.delete_target(&t.skill_id, &t.tool);
            }

            // Re-sync effective set
            let skills = store.get_effective_skills_for_agent(&adapter.key)?;
            let mode = sync_engine::sync_mode_for_tool(&adapter.key, configured_mode.as_deref());
            for skill in &skills {
                let source = PathBuf::from(&skill.central_path);
                if source.exists() {
                    let target_path = adapter.skills_dir().join(&skill.name);
                    let _ = sync_engine::sync_skill(&source, &target_path, mode);
                }
            }
            println!("  Re-synced ({} effective skills)", skills.len());
        }
    }

    Ok(())
}
```

Add the import for `AgentConfigRecord` at the top of `commands.rs` if not already available via the core re-export.

### 5c. Update existing `cmd_switch` for backward compat

Modify `cmd_switch` to also update all managed agents' scenario_id:

After `store.set_active_scenario(&target.id)?;`, add:

```rust
// Update all managed agent configs to the new scenario
let adapters_for_update = tool_adapters::enabled_installed_adapters(&store);
let adapter_keys: Vec<String> = adapters_for_update.iter().map(|a| a.key.clone()).collect();
store.init_agent_configs(&adapter_keys)?;
let configs = store.get_all_agent_configs()?;
for config in &configs {
    if config.managed {
        store.set_agent_scenario(&config.tool_key, &target.id)?;
    }
}
```

### Verification

```bash
cargo build -p skills-manager-cli
# Manual test:
# sm agents
# sm switch claude_code minimal
# sm agent claude_code add-pack marketing
```

---

## Task 6: Sidebar -- Agents section

**Files to modify:**
- `src/lib/tauri.ts` (add TypeScript types + API wrappers)
- `src/components/Sidebar.tsx` (replace Scenarios with Agents)
- `src/context/AppContext.tsx` (add agent config state)

### 6a. Add TypeScript types and API wrappers to `tauri.ts`

```typescript
// -- Agents --

export interface AgentConfig {
  tool_key: string;
  display_name: string;
  installed: boolean;
  managed: boolean;
  scenario_id: string | null;
  scenario_name: string | null;
  extra_pack_count: number;
  effective_skill_count: number;
  updated_at: number | null;
}

export interface AgentConfigRecord {
  tool_key: string;
  scenario_id: string | null;
  managed: boolean;
  updated_at: number | null;
}

export const getAllAgentConfigs = () =>
  invoke<AgentConfig[]>("get_all_agent_configs");

export const getAgentConfig = (toolKey: string) =>
  invoke<AgentConfigRecord | null>("get_agent_config", { toolKey });

export const setAgentScenario = (toolKey: string, scenarioId: string) =>
  invoke<void>("set_agent_scenario", { toolKey, scenarioId });

export const setAgentManaged = (toolKey: string, managed: boolean) =>
  invoke<void>("set_agent_managed", { toolKey, managed });

export const addAgentExtraPack = (toolKey: string, packId: string) =>
  invoke<void>("add_agent_extra_pack", { toolKey, packId });

export const removeAgentExtraPack = (toolKey: string, packId: string) =>
  invoke<void>("remove_agent_extra_pack", { toolKey, packId });

export const getAgentExtraPacks = (toolKey: string) =>
  invoke<PackRecord[]>("get_agent_extra_packs", { toolKey });

export const getEffectiveSkillsForAgent = (toolKey: string) =>
  invoke<PackSkillRecord[]>("get_effective_skills_for_agent", { toolKey });
```

### 6b. Add agent config state to AppContext

In `src/context/AppContext.tsx`, add:
- `agentConfigs` state array
- `refreshAgentConfigs` function that calls `api.getAllAgentConfigs()`
- Include in the context value
- Call `refreshAgentConfigs` on init and after scenario switches

### 6c. Update Sidebar.tsx

Replace the SCENARIOS section with an AGENTS section. Keep scenarios accessible as a collapsed "Presets" subsection.

Key changes:
1. Import `Bot` (or `Monitor`) icon from lucide-react for agent items
2. Replace the scenario drag-drop list with an agent list:
   - Each agent shows: colored dot (green if managed, grey if unmanaged), display name, scenario name, extra pack count badge
   - Click navigates to `/agent/${agent.tool_key}`
3. Add a collapsible "Presets" subsection below agents that shows scenarios with a quick "Apply to all" click action
4. Remove the drag handle from agents (no reorder needed for agents)
5. Keep the "New Scenario" button under the Presets subsection

Agent item structure:

```tsx
<button
  onClick={() => navigate(`/agent/${agent.tool_key}`)}
  className={cn(
    "flex min-w-0 flex-1 items-center gap-2 px-2.5 py-[7px] text-left text-[15px] leading-5 outline-none",
    isActive ? "font-medium text-primary" : "text-tertiary group-hover:text-secondary"
  )}
>
  <span className={cn(
    "h-2 w-2 shrink-0 rounded-full",
    agent.managed ? "bg-emerald-400" : "bg-zinc-400"
  )} />
  <span className="flex-1 truncate">{agent.display_name}</span>
  <span className="ml-auto flex items-center gap-1.5">
    <span className="text-[11px] text-muted truncate max-w-[60px]">
      {agent.scenario_name || "none"}
    </span>
    {agent.extra_pack_count > 0 && (
      <span className="text-[11px] text-accent-light">
        +{agent.extra_pack_count}
      </span>
    )}
  </span>
</button>
```

### Verification

```bash
pnpm run lint
cargo tauri dev
# Visual: agents appear in sidebar, clicking navigates to /agent/:toolKey
```

---

## Task 7: Agent Detail page

**Files to create/modify:**
- `src/views/AgentDetail.tsx` (new)
- `src/App.tsx` (add route)

### 7a. Create `AgentDetail.tsx`

```tsx
import { useState, useEffect, useCallback } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { toast } from "sonner";
import * as api from "../lib/tauri";
import type { AgentConfig, PackRecord, Scenario, PackSkillRecord } from "../lib/tauri";
import { useApp } from "../context/AppContext";

export function AgentDetail() {
  const { toolKey } = useParams<{ toolKey: string }>();
  const navigate = useNavigate();
  const { scenarios, refreshAgentConfigs } = useApp();

  const [config, setConfig] = useState<AgentConfig | null>(null);
  const [extraPacks, setExtraPacks] = useState<PackRecord[]>([]);
  const [allPacks, setAllPacks] = useState<PackRecord[]>([]);
  const [effectiveSkills, setEffectiveSkills] = useState<PackSkillRecord[]>([]);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    if (!toolKey) return;
    try {
      const [configs, packs, extra, effective] = await Promise.all([
        api.getAllAgentConfigs(),
        api.getAllPacks(),
        api.getAgentExtraPacks(toolKey),
        api.getEffectiveSkillsForAgent(toolKey),
      ]);
      const agentConfig = configs.find((c) => c.tool_key === toolKey) ?? null;
      setConfig(agentConfig);
      setAllPacks(packs);
      setExtraPacks(extra);
      setEffectiveSkills(effective);
    } finally {
      setLoading(false);
    }
  }, [toolKey]);

  useEffect(() => { load(); }, [load]);

  if (loading) return <div className="p-6 text-muted">Loading...</div>;
  if (!config) return <div className="p-6 text-muted">Agent not found</div>;

  const handleScenarioChange = async (scenarioId: string) => {
    await api.setAgentScenario(config.tool_key, scenarioId);
    await refreshAgentConfigs();
    await load();
    toast.success(`Switched ${config.display_name} to new scenario`);
  };

  const handleTogglePack = async (packId: string, isExtra: boolean) => {
    if (isExtra) {
      await api.removeAgentExtraPack(config.tool_key, packId);
    } else {
      await api.addAgentExtraPack(config.tool_key, packId);
    }
    await refreshAgentConfigs();
    await load();
  };

  const handleToggleManaged = async () => {
    await api.setAgentManaged(config.tool_key, !config.managed);
    await refreshAgentConfigs();
    await load();
  };

  // Packs that are in the scenario (not available as extras)
  const scenarioPacks = config.scenario_id
    ? [] // Will be loaded from scenario packs API if needed
    : [];
  const extraPackIds = new Set(extraPacks.map((p) => p.id));

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold text-primary">
            {config.display_name}
          </h1>
          <p className="text-sm text-muted mt-0.5">
            {config.managed ? "Managed" : "Unmanaged"} &middot;{" "}
            {config.effective_skill_count} effective skills
          </p>
        </div>
        <button
          onClick={handleToggleManaged}
          className={`px-3 py-1.5 rounded-md text-sm font-medium transition-colors ${
            config.managed
              ? "bg-emerald-500/10 text-emerald-500 hover:bg-emerald-500/20"
              : "bg-zinc-500/10 text-zinc-400 hover:bg-zinc-500/20"
          }`}
        >
          {config.managed ? "Managed" : "Unmanaged"}
        </button>
      </div>

      {/* Base Scenario */}
      <section className="space-y-2">
        <h2 className="text-sm font-semibold text-secondary">Base Scenario</h2>
        <select
          value={config.scenario_id ?? ""}
          onChange={(e) => handleScenarioChange(e.target.value)}
          className="w-full max-w-xs rounded-md border border-border bg-surface px-3 py-2 text-sm text-primary"
          disabled={!config.managed}
        >
          <option value="">None</option>
          {scenarios.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name} ({s.skill_count} skills)
            </option>
          ))}
        </select>
      </section>

      {/* Additional Packs */}
      <section className="space-y-2">
        <h2 className="text-sm font-semibold text-secondary">
          Additional Packs
        </h2>
        <p className="text-xs text-muted">
          Extra packs added on top of the base scenario. These skills are synced
          in addition to the scenario skills.
        </p>
        <div className="space-y-1">
          {allPacks.map((pack) => {
            const isExtra = extraPackIds.has(pack.id);
            return (
              <label
                key={pack.id}
                className="flex items-center gap-2.5 px-3 py-2 rounded-md hover:bg-surface-hover transition-colors cursor-pointer"
              >
                <input
                  type="checkbox"
                  checked={isExtra}
                  onChange={() => handleTogglePack(pack.id, isExtra)}
                  disabled={!config.managed}
                  className="rounded border-border"
                />
                <span className="text-sm text-primary">{pack.name}</span>
                {pack.description && (
                  <span className="text-xs text-muted ml-1">
                    {pack.description}
                  </span>
                )}
              </label>
            );
          })}
          {allPacks.length === 0 && (
            <p className="text-sm text-muted italic">No packs created yet.</p>
          )}
        </div>
      </section>

      {/* Effective Skills */}
      <section className="space-y-2">
        <h2 className="text-sm font-semibold text-secondary">
          Effective Skills ({effectiveSkills.length})
        </h2>
        <div className="flex flex-wrap gap-1.5">
          {effectiveSkills.map((skill) => (
            <span
              key={skill.id}
              className="inline-flex items-center rounded-full bg-surface-hover px-2.5 py-1 text-xs text-secondary"
            >
              {skill.name}
            </span>
          ))}
          {effectiveSkills.length === 0 && (
            <p className="text-sm text-muted italic">
              No skills. Select a scenario and/or add packs.
            </p>
          )}
        </div>
      </section>
    </div>
  );
}
```

### 7b. Add route in `App.tsx`

Add import:
```tsx
import { AgentDetail } from "./views/AgentDetail";
```

Add route:
```tsx
<Route path="/agent/:toolKey" element={<AgentDetail />} />
```

### Verification

```bash
pnpm run lint
cargo tauri dev
# Visual: navigate to /agent/claude_code, verify scenario dropdown, pack checkboxes, effective skills
```

---

## Task 8: Skills Matrix update

**Files to modify:**
- `src/views/MatrixView.tsx`

### 8a. Merge Packs + Matrix

Update the existing `MatrixView.tsx` to show:
1. **Top section**: Pack management (existing pack CRUD from PacksView, or a simplified version)
2. **Bottom section**: Agent x pack/skill toggle grid

The grid should show:
- Columns: agents (from agent configs)
- Rows: packs (and optionally individual skills)
- Cells: toggle checkboxes

For each agent x pack cell:
- If the pack is in the agent's scenario: show as checked + greyed out (inherited)
- If the pack is in the agent's extra packs: show as checked (removable)
- If neither: show as unchecked (addable)

Clicking a cell calls `addAgentExtraPack` or `removeAgentExtraPack`.

### 8b. Keep the route

The route `/matrix` stays the same. Update the sidebar NAV_ITEMS if "Matrix" label needs changing (consider "Skills Matrix").

### Verification

```bash
pnpm run lint
cargo tauri dev
# Visual: Matrix page shows agent columns with pack rows, toggles work
```

---

## Task 9: Verification

### 9a. Run all tests

```bash
cargo test                    # All Rust tests pass
pnpm run lint                 # No lint errors
```

### 9b. Integration tests

1. **Fresh DB**: Delete `~/.skills-manager/skills-manager.db` and start app. Verify migration creates all tables, agent_configs are seeded from active_scenario.
2. **Existing DB (v6)**: Start app with existing DB. Verify v6->v7 migration runs, agent_configs table created, behavior identical to before.
3. **Per-agent assignment**: Navigate to Agent Detail for claude_code, change scenario to "minimal", verify only minimal skills synced for Claude Code while other agents retain their scenario.
4. **Extra packs**: Add a pack as extra for one agent, verify those skills appear in the effective list and are synced.
5. **Global switch**: Use `sm switch everything`, verify all managed agents update.
6. **Agent-specific switch**: Use `sm switch claude_code minimal`, verify only Claude Code updates.
7. **Backward compat**: Verify `sm switch` (no agent) still works for all agents.
8. **Unmanaged agent**: Set hermes as unmanaged, verify its skills dir is not touched during switches.

### 9c. Edge cases to verify

- Delete a scenario that an agent is using: `scenario_id` should become NULL (ON DELETE SET NULL)
- Delete a pack that is an agent extra: `agent_extra_packs` row should cascade delete
- Agent with no scenario (scenario_id = NULL): effective skills should be empty
- Multiple agents with same extra pack: each agent independently managed

---

## Commit Strategy

One commit per task:

1. `feat(db): add agent_configs and agent_extra_packs tables (migration v7)`
2. `feat(core): add AgentConfigRecord type and CRUD methods`
3. `refactor(sync): update sync logic for per-agent scenario assignment`
4. `feat(tauri): add agent config IPC commands`
5. `feat(cli): add sm agents and per-agent switch commands`
6. `feat(ui): replace sidebar scenarios with agents section`
7. `feat(ui): add Agent Detail page`
8. `feat(ui): merge Skills Matrix with per-agent pack toggles`
9. `chore: verification pass and cleanup`
