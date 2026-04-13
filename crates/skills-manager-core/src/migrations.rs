use anyhow::{bail, Context, Result};
use rusqlite::Connection;

/// Current schema version. Bump this when adding a new migration.
const LATEST_VERSION: u32 = 7;

const PLUGINS_SCHEMA_DDL: &str = "
    CREATE TABLE IF NOT EXISTS managed_plugins (
        id TEXT PRIMARY KEY,
        plugin_key TEXT NOT NULL UNIQUE,
        display_name TEXT,
        plugin_data TEXT NOT NULL,
        created_at INTEGER,
        updated_at INTEGER
    );

    CREATE TABLE IF NOT EXISTS scenario_plugins (
        scenario_id TEXT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
        plugin_id TEXT NOT NULL REFERENCES managed_plugins(id) ON DELETE CASCADE,
        enabled INTEGER NOT NULL DEFAULT 1,
        PRIMARY KEY(scenario_id, plugin_id)
    );
";

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

const PACKS_SCHEMA_DDL: &str = "
    CREATE TABLE IF NOT EXISTS packs (
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
    );
";

/// Run all pending migrations on the database.
///
/// - New databases: creates full schema and sets version to LATEST_VERSION.
/// - Existing databases (user_version == 0): runs incremental migrations
///   to bring them up to date.
/// - Databases newer than this app version: returns an error.
pub fn run_migrations(conn: &Connection) -> Result<()> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if current > LATEST_VERSION {
        bail!(
            "Database schema version ({current}) is newer than this app supports ({LATEST_VERSION}). \
             Please upgrade the application."
        );
    }

    if current == LATEST_VERSION {
        return Ok(());
    }

    // Run each migration step in a transaction
    for version in current..LATEST_VERSION {
        conn.execute_batch("BEGIN EXCLUSIVE")?;
        match migrate_step(conn, version) {
            Ok(()) => {
                conn.pragma_update(None, "user_version", version + 1)?;
                conn.execute_batch("COMMIT")?;
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e).with_context(|| {
                    format!("migration from version {version} to {} failed", version + 1)
                });
            }
        }
    }

    Ok(())
}

/// Execute a single migration step: version N → N+1.
fn migrate_step(conn: &Connection, from_version: u32) -> Result<()> {
    match from_version {
        0 => migrate_v0_to_v1(conn),
        1 => migrate_v1_to_v2(conn),
        2 => migrate_v2_to_v3(conn),
        3 => migrate_v3_to_v4(conn),
        4 => migrate_v4_to_v5(conn),
        5 => migrate_v5_to_v6(conn),
        6 => migrate_v6_to_v7(conn),
        _ => bail!("unknown migration version: {from_version}"),
    }
}

/// v0 → v1: Initial schema.
///
/// For new databases this creates all tables from scratch.
/// For existing pre-migration databases, the `CREATE TABLE IF NOT EXISTS`
/// statements are no-ops, and the `add_column_if_missing` calls handle
/// columns that were added incrementally before the migration system existed.
fn migrate_v0_to_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS skills (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            source_type TEXT NOT NULL,
            source_ref TEXT,
            source_ref_resolved TEXT,
            source_subpath TEXT,
            source_branch TEXT,
            source_revision TEXT,
            remote_revision TEXT,
            central_path TEXT NOT NULL UNIQUE,
            content_hash TEXT,
            enabled INTEGER DEFAULT 1,
            created_at INTEGER,
            updated_at INTEGER,
            status TEXT DEFAULT 'ok',
            update_status TEXT DEFAULT 'unknown',
            last_checked_at INTEGER,
            last_check_error TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_skills_name ON skills(name);

        CREATE TABLE IF NOT EXISTS skill_targets (
            id TEXT PRIMARY KEY,
            skill_id TEXT NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
            tool TEXT NOT NULL,
            target_path TEXT NOT NULL,
            mode TEXT NOT NULL,
            status TEXT DEFAULT 'ok',
            synced_at INTEGER,
            last_error TEXT,
            UNIQUE(skill_id, tool)
        );

        CREATE TABLE IF NOT EXISTS discovered_skills (
            id TEXT PRIMARY KEY,
            tool TEXT NOT NULL,
            found_path TEXT NOT NULL,
            name_guess TEXT,
            fingerprint TEXT,
            found_at INTEGER NOT NULL,
            imported_skill_id TEXT REFERENCES skills(id) ON DELETE SET NULL
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS skillssh_cache (
            cache_key TEXT PRIMARY KEY,
            data TEXT NOT NULL,
            fetched_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS scenarios (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            description TEXT,
            icon TEXT,
            sort_order INTEGER DEFAULT 0,
            created_at INTEGER,
            updated_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS scenario_skills (
            scenario_id TEXT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
            skill_id TEXT NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
            added_at INTEGER,
            PRIMARY KEY(scenario_id, skill_id)
        );

        CREATE TABLE IF NOT EXISTS scenario_skill_tools (
            scenario_id TEXT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
            skill_id TEXT NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
            tool TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY(scenario_id, skill_id, tool)
        );

        CREATE TABLE IF NOT EXISTS active_scenario (
            key TEXT PRIMARY KEY DEFAULT 'current',
            scenario_id TEXT REFERENCES scenarios(id) ON DELETE SET NULL
        );

        CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL UNIQUE,
            workspace_type TEXT NOT NULL DEFAULT 'project',
            linked_agent_key TEXT,
            linked_agent_name TEXT,
            disabled_path TEXT,
            sort_order INTEGER DEFAULT 0,
            created_at INTEGER,
            updated_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS skill_tags (
            skill_id TEXT NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
            tag TEXT NOT NULL,
            PRIMARY KEY(skill_id, tag)
        );
        CREATE INDEX IF NOT EXISTS idx_skill_tags_tag ON skill_tags(tag);

        ",
    )?;
    conn.execute_batch(PACKS_SCHEMA_DDL)?;
    conn.execute_batch(PLUGINS_SCHEMA_DDL)?;
    conn.execute_batch(AGENT_CONFIG_SCHEMA_DDL)?;

    // For pre-migration databases: add columns that didn't exist in the original schema.
    // For new databases these are already in the CREATE TABLE, so the calls are no-ops.
    add_column_if_missing(conn, "scenarios", "icon", "TEXT")?;
    add_column_if_missing(conn, "skills", "source_ref_resolved", "TEXT")?;
    add_column_if_missing(conn, "skills", "source_subpath", "TEXT")?;
    add_column_if_missing(conn, "skills", "source_branch", "TEXT")?;
    add_column_if_missing(conn, "skills", "remote_revision", "TEXT")?;
    add_column_if_missing(conn, "skills", "update_status", "TEXT DEFAULT 'unknown'")?;
    add_column_if_missing(conn, "skills", "last_checked_at", "INTEGER")?;
    add_column_if_missing(conn, "skills", "last_check_error", "TEXT")?;

    Ok(())
}

/// v1 → v2: Add per-scenario, per-skill tool toggle table.
fn migrate_v1_to_v2(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS scenario_skill_tools (
            scenario_id TEXT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
            skill_id TEXT NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
            tool TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY(scenario_id, skill_id, tool)
        );
        ",
    )?;
    Ok(())
}

/// v2 → v3: Add sort_order to scenario_skills for drag-and-drop reordering.
fn migrate_v2_to_v3(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "scenario_skills", "sort_order", "INTEGER DEFAULT 0")?;
    Ok(())
}

/// v3 → v4: Expand projects into generic workspace records.
fn migrate_v3_to_v4(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL UNIQUE,
            workspace_type TEXT NOT NULL DEFAULT 'project',
            linked_agent_key TEXT,
            linked_agent_name TEXT,
            disabled_path TEXT,
            sort_order INTEGER DEFAULT 0,
            created_at INTEGER,
            updated_at INTEGER
        );
        ",
    )?;
    add_column_if_missing(
        conn,
        "projects",
        "workspace_type",
        "TEXT NOT NULL DEFAULT 'project'",
    )?;
    add_column_if_missing(conn, "projects", "linked_agent_key", "TEXT")?;
    add_column_if_missing(conn, "projects", "linked_agent_name", "TEXT")?;
    add_column_if_missing(conn, "projects", "disabled_path", "TEXT")?;
    Ok(())
}

/// v4 → v5: Add packs, pack_skills, and scenario_packs tables.
fn migrate_v4_to_v5(conn: &Connection) -> Result<()> {
    conn.execute_batch(PACKS_SCHEMA_DDL)?;
    Ok(())
}

/// v5 → v6: Add managed_plugins and scenario_plugins tables for per-scenario
/// plugin enable/disable.
fn migrate_v5_to_v6(conn: &Connection) -> Result<()> {
    conn.execute_batch(PLUGINS_SCHEMA_DDL)?;
    Ok(())
}

/// v6 → v7: Add agent_configs and agent_extra_packs tables for per-agent
/// scenario assignment and extra pack layering.
fn migrate_v6_to_v7(conn: &Connection) -> Result<()> {
    conn.execute_batch(AGENT_CONFIG_SCHEMA_DDL)?;
    Ok(())
}

// ── Helpers ──

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    // Validate identifiers to prevent SQL injection if call sites ever change.
    validate_identifier(table)?;
    validate_identifier(column)?;

    if !has_column(conn, table, column)? {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

fn validate_identifier(name: &str) -> Result<()> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        anyhow::bail!("Invalid SQL identifier: {}", name);
    }
    Ok(())
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(columns.iter().any(|name| name == column))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fresh_database_migrates_to_latest() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();

        run_migrations(&conn).unwrap();

        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_VERSION);

        // Verify tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"skills".to_string()));
        assert!(tables.contains(&"skill_targets".to_string()));
        assert!(tables.contains(&"scenarios".to_string()));
        assert!(tables.contains(&"projects".to_string()));
        assert!(tables.contains(&"skill_tags".to_string()));
        assert!(tables.contains(&"scenario_skill_tools".to_string()));
    }

    #[test]
    fn test_idempotent_migration() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();

        run_migrations(&conn).unwrap();
        // Running again should be a no-op
        run_migrations(&conn).unwrap();

        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_VERSION);
    }

    #[test]
    fn test_pre_migration_database_upgrades() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();

        // Simulate a pre-migration database: create skills table without newer columns
        conn.execute_batch(
            "
            CREATE TABLE skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                source_type TEXT NOT NULL,
                source_ref TEXT,
                source_revision TEXT,
                central_path TEXT NOT NULL UNIQUE,
                content_hash TEXT,
                enabled INTEGER DEFAULT 1,
                created_at INTEGER,
                updated_at INTEGER,
                status TEXT DEFAULT 'ok'
            );
            CREATE TABLE scenarios (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                description TEXT,
                sort_order INTEGER DEFAULT 0,
                created_at INTEGER,
                updated_at INTEGER
            );
            ",
        )
        .unwrap();

        // user_version is 0 (default), so migration should run
        run_migrations(&conn).unwrap();

        // Verify new columns were added
        assert!(has_column(&conn, "skills", "source_ref_resolved").unwrap());
        assert!(has_column(&conn, "skills", "source_subpath").unwrap());
        assert!(has_column(&conn, "skills", "source_branch").unwrap());
        assert!(has_column(&conn, "skills", "remote_revision").unwrap());
        assert!(has_column(&conn, "skills", "update_status").unwrap());
        assert!(has_column(&conn, "skills", "last_checked_at").unwrap());
        assert!(has_column(&conn, "skills", "last_check_error").unwrap());
        assert!(has_column(&conn, "scenarios", "icon").unwrap());

        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_VERSION);
    }

    #[test]
    fn test_v1_database_upgrades_to_v2() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();

        conn.execute_batch(
            "
            CREATE TABLE skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                source_type TEXT NOT NULL,
                source_ref TEXT,
                source_ref_resolved TEXT,
                source_subpath TEXT,
                source_branch TEXT,
                source_revision TEXT,
                remote_revision TEXT,
                central_path TEXT NOT NULL UNIQUE,
                content_hash TEXT,
                enabled INTEGER DEFAULT 1,
                created_at INTEGER,
                updated_at INTEGER,
                status TEXT DEFAULT 'ok',
                update_status TEXT DEFAULT 'unknown',
                last_checked_at INTEGER,
                last_check_error TEXT
            );
            CREATE TABLE scenarios (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                description TEXT,
                icon TEXT,
                sort_order INTEGER DEFAULT 0,
                created_at INTEGER,
                updated_at INTEGER
            );
            CREATE TABLE scenario_skills (
                scenario_id TEXT NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
                skill_id TEXT NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
                added_at INTEGER,
                PRIMARY KEY(scenario_id, skill_id)
            );
            PRAGMA user_version = 1;
            ",
        )
        .unwrap();

        run_migrations(&conn).unwrap();
        assert!(has_column(&conn, "scenario_skill_tools", "enabled").unwrap());

        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_VERSION);
    }

    #[test]
    fn test_newer_schema_rejected() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", LATEST_VERSION + 1)
            .unwrap();

        let err = run_migrations(&conn).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("newer than this app supports"),
            "unexpected error: {msg}"
        );
    }

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

        // Verify schema version is at latest
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_VERSION);
    }

    #[test]
    fn v4_to_v5_migration_adds_packs_tables() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 4).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS skills (id TEXT PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS scenarios (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE);
             CREATE TABLE IF NOT EXISTS scenario_skills (
                 scenario_id TEXT NOT NULL, skill_id TEXT NOT NULL,
                 added_at INTEGER, sort_order INTEGER DEFAULT 0,
                 PRIMARY KEY(scenario_id, skill_id)
             );",
        )
        .unwrap();

        // Pre-seed valid rows required for FK constraints (FK enforcement is always on).
        conn.execute(
            "INSERT INTO skills (id, name) VALUES ('s1', 'test-skill')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO scenarios (id, name) VALUES ('sc1', 'test-scenario')",
            [],
        )
        .unwrap();

        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO packs (id, name, sort_order, created_at, updated_at) VALUES ('p1', 'test', 0, 0, 0)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO pack_skills (pack_id, skill_id, sort_order) VALUES ('p1', 's1', 0)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO scenario_packs (scenario_id, pack_id, sort_order) VALUES ('sc1', 'p1', 0)",
            [],
        )
        .unwrap();

        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_VERSION);
    }

    #[test]
    fn packs_cascade_delete() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        conn.execute("INSERT INTO skills (id, name, description, source_type, source_ref, central_path, content_hash, enabled, created_at, updated_at, status, update_status) VALUES ('s1', 'test-skill', '', 'local', '', '', '', 1, 0, 0, 'installed', 'none')", []).unwrap();
        conn.execute("INSERT INTO packs (id, name, sort_order, created_at, updated_at) VALUES ('p1', 'test-pack', 0, 0, 0)", []).unwrap();
        conn.execute(
            "INSERT INTO pack_skills (pack_id, skill_id, sort_order) VALUES ('p1', 's1', 0)",
            [],
        )
        .unwrap();

        conn.execute("DELETE FROM packs WHERE id = 'p1'", [])
            .unwrap();

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM pack_skills WHERE pack_id = 'p1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn fresh_db_creates_plugin_tables() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        // Verify managed_plugins table exists
        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM managed_plugins", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        // Verify scenario_plugins table exists
        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM scenario_plugins", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn v5_to_v6_migration_adds_plugin_tables() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 5).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS skills (id TEXT PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS scenarios (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO scenarios (id, name) VALUES ('sc1', 'test-scenario')",
            [],
        )
        .unwrap();

        run_migrations(&conn).unwrap();

        // Insert a managed plugin
        conn.execute(
            "INSERT INTO managed_plugins (id, plugin_key, display_name, plugin_data, created_at, updated_at) VALUES ('mp1', 'test@plugin', 'test', '[]', 0, 0)",
            [],
        )
        .unwrap();

        // Insert a scenario_plugins row
        conn.execute(
            "INSERT INTO scenario_plugins (scenario_id, plugin_id, enabled) VALUES ('sc1', 'mp1', 0)",
            [],
        )
        .unwrap();

        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_VERSION);
    }

    #[test]
    fn plugin_cascade_delete_scenario() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO scenarios (id, name) VALUES ('sc1', 'test')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO managed_plugins (id, plugin_key, display_name, plugin_data, created_at, updated_at) VALUES ('mp1', 'test@plugin', 'test', '[]', 0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO scenario_plugins (scenario_id, plugin_id, enabled) VALUES ('sc1', 'mp1', 0)",
            [],
        )
        .unwrap();

        // Delete scenario — should cascade to scenario_plugins
        conn.execute("DELETE FROM scenarios WHERE id = 'sc1'", [])
            .unwrap();

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM scenario_plugins WHERE scenario_id = 'sc1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn fresh_db_creates_agent_config_tables() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        // Verify agent_configs table exists
        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM agent_configs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        // Verify agent_extra_packs table exists
        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM agent_extra_packs", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn v6_to_v7_migration_adds_agent_config_tables() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.pragma_update(None, "user_version", 6).unwrap();
        // Create prerequisite tables needed by FK references
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS skills (id TEXT PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS scenarios (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE);
             CREATE TABLE IF NOT EXISTS packs (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE,
                 description TEXT, icon TEXT, color TEXT, sort_order INTEGER DEFAULT 0,
                 created_at INTEGER, updated_at INTEGER);
             CREATE TABLE IF NOT EXISTS managed_plugins (id TEXT PRIMARY KEY,
                 plugin_key TEXT NOT NULL UNIQUE, display_name TEXT,
                 plugin_data TEXT NOT NULL, created_at INTEGER, updated_at INTEGER);
             CREATE TABLE IF NOT EXISTS scenario_plugins (
                 scenario_id TEXT NOT NULL, plugin_id TEXT NOT NULL,
                 enabled INTEGER NOT NULL DEFAULT 1,
                 PRIMARY KEY(scenario_id, plugin_id));",
        )
        .unwrap();

        run_migrations(&conn).unwrap();

        // Insert test data to verify tables work
        conn.execute(
            "INSERT INTO scenarios (id, name) VALUES ('sc1', 'test-scenario')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agent_configs (tool_key, scenario_id, managed, updated_at) VALUES ('claude', 'sc1', 1, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO packs (id, name, sort_order, created_at, updated_at) VALUES ('p1', 'test-pack', 0, 0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agent_extra_packs (tool_key, pack_id) VALUES ('claude', 'p1')",
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
            "INSERT INTO packs (id, name, sort_order, created_at, updated_at) VALUES ('p1', 'test-pack', 0, 0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agent_extra_packs (tool_key, pack_id) VALUES ('claude', 'p1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agent_extra_packs (tool_key, pack_id) VALUES ('windsurf', 'p1')",
            [],
        )
        .unwrap();

        // Delete the pack — should cascade to agent_extra_packs
        conn.execute("DELETE FROM packs WHERE id = 'p1'", [])
            .unwrap();

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
    fn plugin_cascade_delete_managed_plugin() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO scenarios (id, name) VALUES ('sc1', 'test')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO managed_plugins (id, plugin_key, display_name, plugin_data, created_at, updated_at) VALUES ('mp1', 'test@plugin', 'test', '[]', 0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO scenario_plugins (scenario_id, plugin_id, enabled) VALUES ('sc1', 'mp1', 0)",
            [],
        )
        .unwrap();

        // Delete managed_plugin — should cascade to scenario_plugins
        conn.execute("DELETE FROM managed_plugins WHERE id = 'mp1'", [])
            .unwrap();

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM scenario_plugins WHERE plugin_id = 'mp1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}
