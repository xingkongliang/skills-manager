use anyhow::{bail, Context, Result};
use rusqlite::Connection;

/// Current schema version. Bump this when adding a new migration.
const LATEST_VERSION: u32 = 11;

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
        7 => migrate_v7_to_v8(conn),
        8 => migrate_v8_to_v9(conn),
        9 => migrate_v9_to_v10(conn),
        10 => migrate_v10_to_v11(conn),
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
            source_hash TEXT,
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

/// v4 → v5: Add audit log table — append-only history of user/system actions.
fn migrate_v4_to_v5(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS audit_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts INTEGER NOT NULL,
            action TEXT NOT NULL,
            skill_id TEXT,
            skill_name TEXT,
            tool TEXT,
            success INTEGER NOT NULL,
            detail TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_audit_log_ts ON audit_log(ts);
        ",
    )?;
    Ok(())
}

/// v5 → v6: Add `source_hash` to `skill_targets`. Lets the sync engine
/// skip a Copy-mode resync when the central skill content has not
/// changed since the last successful sync, avoiding the per-startup
/// recursive copy that pinned Windows users on issue #153.
///
/// Existing rows get NULL, which is treated as "no recorded hash" and
/// forces one copy on the first post-upgrade sync. No backfill needed.
fn migrate_v5_to_v6(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "skill_targets", "source_hash", "TEXT")?;
    Ok(())
}

/// v6 → v7: pending-conflict projection for the object merge engine
/// (merge-engine design §4). A local UI cache only — the source of truth is
/// the commit trailers plus `refs/skills-manager/conflict/*`, from which
/// this table is rebuilt at startup and after every merge.
fn migrate_v6_to_v7(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS pending_conflicts (
            skill_id TEXT PRIMARY KEY,
            theirs_commit TEXT NOT NULL,
            theirs_path TEXT,
            detected_at INTEGER NOT NULL
        );
        ",
    )?;
    Ok(())
}

/// v7 → v8: package inventory and scope-aware host bindings.
///
/// Package sources stay in the managed cache. Bindings describe desired and
/// observed host state; project-shared bindings are mirrored to the project's
/// `.skillapse/project.json` manifest by the package service.
fn migrate_v7_to_v8(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS packages (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            source_url TEXT NOT NULL UNIQUE,
            requested_revision TEXT,
            resolved_revision TEXT NOT NULL,
            cache_path TEXT NOT NULL UNIQUE,
            manifest_kind TEXT NOT NULL DEFAULT 'none',
            status TEXT NOT NULL DEFAULT 'ready'
                CHECK(status IN ('ready', 'invalid', 'update_available')),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS package_components (
            id TEXT PRIMARY KEY,
            package_id TEXT NOT NULL REFERENCES packages(id) ON DELETE CASCADE,
            kind TEXT NOT NULL
                CHECK(kind IN ('skill', 'rule', 'agent', 'command', 'hook', 'mcp')),
            name TEXT NOT NULL,
            relative_path TEXT NOT NULL,
            host_hint TEXT,
            required INTEGER NOT NULL DEFAULT 0,
            UNIQUE(package_id, kind, relative_path)
        );

        CREATE TABLE IF NOT EXISTS package_surfaces (
            id TEXT PRIMARY KEY,
            package_id TEXT NOT NULL REFERENCES packages(id) ON DELETE CASCADE,
            tool TEXT NOT NULL,
            kind TEXT NOT NULL
                CHECK(kind IN ('native_plugin', 'host_bundle', 'portable_skills', 'setup_script')),
            root_path TEXT NOT NULL,
            manifest_path TEXT,
            priority INTEGER NOT NULL DEFAULT 0,
            coverage_json TEXT NOT NULL DEFAULT '[]',
            install_command_json TEXT,
            UNIQUE(package_id, tool, kind, root_path)
        );

        CREATE TABLE IF NOT EXISTS package_bindings (
            id TEXT PRIMARY KEY,
            package_id TEXT NOT NULL REFERENCES packages(id) ON DELETE CASCADE,
            tool TEXT NOT NULL,
            scope TEXT NOT NULL
                CHECK(scope IN ('user', 'project_shared', 'project_local', 'managed')),
            project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
            surface_policy TEXT NOT NULL DEFAULT 'auto'
                CHECK(surface_policy IN ('auto', 'native', 'portable', 'setup')),
            requested_components_json TEXT NOT NULL DEFAULT '[]',
            desired_enabled INTEGER NOT NULL DEFAULT 1,
            resolved_surface_id TEXT REFERENCES package_surfaces(id) ON DELETE SET NULL,
            compatibility TEXT NOT NULL DEFAULT 'unsupported'
                CHECK(compatibility IN ('full', 'partial', 'unsupported')),
            state TEXT NOT NULL DEFAULT 'not_applied'
                CHECK(state IN ('not_applied', 'planned', 'installed', 'partial', 'drifted', 'failed')),
            target_ref TEXT,
            applied_revision TEXT,
            approved_plan_hash TEXT,
            last_error TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            CHECK(
                (scope IN ('user', 'managed') AND project_id IS NULL)
                OR (scope IN ('project_shared', 'project_local') AND project_id IS NOT NULL)
            )
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_package_bindings_identity
            ON package_bindings(package_id, tool, scope, IFNULL(project_id, ''));
        CREATE INDEX IF NOT EXISTS idx_package_components_package
            ON package_components(package_id);
        CREATE INDEX IF NOT EXISTS idx_package_surfaces_package_tool
            ON package_surfaces(package_id, tool);
        ",
    )?;
    Ok(())
}

fn migrate_v8_to_v9(conn: &Connection) -> Result<()> {
    add_column_if_missing(
        conn,
        "package_bindings",
        "ownership",
        "TEXT NOT NULL DEFAULT 'managed' CHECK(ownership IN ('managed', 'adopted'))",
    )
}

fn migrate_v9_to_v10(conn: &Connection) -> Result<()> {
    add_column_if_missing(conn, "package_bindings", "applied_surface_kind", "TEXT")?;
    conn.execute_batch(
        "
        UPDATE package_bindings
        SET applied_surface_kind = (
            SELECT kind FROM package_surfaces
            WHERE package_surfaces.id = package_bindings.resolved_surface_id
        )
        WHERE applied_surface_kind IS NULL AND target_ref IS NOT NULL;
        ",
    )?;
    Ok(())
}

fn migrate_v10_to_v11(conn: &Connection) -> Result<()> {
    add_column_if_missing(
        conn,
        "package_components",
        "artifact_key",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column_if_missing(
        conn,
        "package_surfaces",
        "artifact_key",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column_if_missing(
        conn,
        "package_bindings",
        "artifact_key",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    conn.execute_batch(
        "
        DROP INDEX IF EXISTS idx_package_bindings_identity;
        CREATE UNIQUE INDEX idx_package_bindings_identity
            ON package_bindings(package_id, artifact_key, tool, scope, IFNULL(project_id, ''));
        CREATE INDEX IF NOT EXISTS idx_package_components_artifact
            ON package_components(package_id, artifact_key);
        CREATE INDEX IF NOT EXISTS idx_package_surfaces_artifact_tool
            ON package_surfaces(package_id, artifact_key, tool);
        ",
    )?;
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
        assert!(tables.contains(&"audit_log".to_string()));
        assert!(tables.contains(&"packages".to_string()));
        assert!(tables.contains(&"package_components".to_string()));
        assert!(tables.contains(&"package_surfaces".to_string()));
        assert!(tables.contains(&"package_bindings".to_string()));
        assert!(has_column(&conn, "package_bindings", "ownership").unwrap());
        assert!(has_column(&conn, "package_bindings", "applied_surface_kind").unwrap());
        assert!(has_column(&conn, "package_components", "artifact_key").unwrap());
        assert!(has_column(&conn, "package_surfaces", "artifact_key").unwrap());
        assert!(has_column(&conn, "package_bindings", "artifact_key").unwrap());
    }

    #[test]
    fn artifact_migration_preserves_installed_binding_state() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        conn.execute_batch("CREATE TABLE projects (id TEXT PRIMARY KEY);")
            .unwrap();
        migrate_v7_to_v8(&conn).unwrap();
        migrate_v8_to_v9(&conn).unwrap();
        migrate_v9_to_v10(&conn).unwrap();
        conn.execute(
            "INSERT INTO packages (
                id, name, source_url, resolved_revision, cache_path, created_at, updated_at
             ) VALUES ('p1', 'Demo', 'https://example.com/demo.git', 'abc', '/tmp/p1', 1, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO package_surfaces (
                id, package_id, tool, kind, root_path, coverage_json
             ) VALUES ('s1', 'p1', 'codex', 'native_plugin', '.', '[]')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO package_bindings (
                id, package_id, tool, scope, resolved_surface_id, compatibility, state,
                target_ref, applied_revision, approved_plan_hash, created_at, updated_at,
                ownership, applied_surface_kind
             ) VALUES (
                'b1', 'p1', 'codex', 'user', 's1', 'full', 'installed',
                '{\"target\":\"keep\"}', 'abc', 'hash', 1, 1, 'managed', 'native_plugin'
             )",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 10).unwrap();

        run_migrations(&conn).unwrap();

        let row: (String, String, String, String, String) = conn
            .query_row(
                "SELECT artifact_key, target_ref, ownership, applied_surface_kind, state
                 FROM package_bindings WHERE id = 'b1'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, "");
        assert_eq!(row.1, r#"{"target":"keep"}"#);
        assert_eq!(row.2, "managed");
        assert_eq!(row.3, "native_plugin");
        assert_eq!(row.4, "installed");
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
            CREATE TABLE skill_targets (
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
            PRAGMA user_version = 1;
            ",
        )
        .unwrap();

        run_migrations(&conn).unwrap();
        assert!(has_column(&conn, "scenario_skill_tools", "enabled").unwrap());
        assert!(has_column(&conn, "skill_targets", "source_hash").unwrap());

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
}
