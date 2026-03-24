use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;

use super::crypto;

/// Settings keys whose values are encrypted at rest with AES-256-GCM.
const SENSITIVE_KEYS: &[&str] = &["proxy_url", "git_backup_remote_url", "skillsmp_api_key"];

pub struct SkillStore {
    conn: Mutex<Connection>,
    secret_key: [u8; 32],
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillRecord {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub source_type: String,
    pub source_ref: Option<String>,
    pub source_ref_resolved: Option<String>,
    pub source_subpath: Option<String>,
    pub source_branch: Option<String>,
    pub source_revision: Option<String>,
    pub remote_revision: Option<String>,
    pub central_path: String,
    pub content_hash: Option<String>,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub status: String,
    pub update_status: String,
    pub last_checked_at: Option<i64>,
    pub last_check_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillTargetRecord {
    pub id: String,
    pub skill_id: String,
    pub tool: String,
    pub target_path: String,
    pub mode: String,
    pub status: String,
    pub synced_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredSkillRecord {
    pub id: String,
    pub tool: String,
    pub found_path: String,
    pub name_guess: Option<String>,
    pub fingerprint: Option<String>,
    pub found_at: i64,
    pub imported_skill_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioRecord {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub sort_order: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub path: String,
    pub sort_order: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioSkillToolToggleRecord {
    pub scenario_id: String,
    pub skill_id: String,
    pub tool: String,
    pub enabled: bool,
    pub updated_at: i64,
}

impl SkillStore {
    pub fn new(db_path: &PathBuf) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        super::migrations::run_migrations(&conn)?;

        // Derive key file path from the database directory.
        let key_path = db_path
            .parent()
            .map(|p| p.join(".secret.key"))
            .unwrap_or_else(|| PathBuf::from(".secret.key"));
        let secret_key = crypto::load_or_create_key(&key_path)?;

        Ok(Self {
            conn: Mutex::new(conn),
            secret_key,
        })
    }

    // ── Skills CRUD ──

    pub fn insert_skill(&self, skill: &SkillRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO skills (
                id, name, description, source_type, source_ref, source_ref_resolved, source_subpath,
                source_branch, source_revision, remote_revision, central_path, content_hash, enabled,
                created_at, updated_at, status, update_status, last_checked_at, last_check_error
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                skill.id,
                skill.name,
                skill.description,
                skill.source_type,
                skill.source_ref,
                skill.source_ref_resolved,
                skill.source_subpath,
                skill.source_branch,
                skill.source_revision,
                skill.remote_revision,
                skill.central_path,
                skill.content_hash,
                skill.enabled,
                skill.created_at,
                skill.updated_at,
                skill.status,
                skill.update_status,
                skill.last_checked_at,
                skill.last_check_error,
            ],
        )?;
        Ok(())
    }

    pub fn get_all_skills(&self) -> Result<Vec<SkillRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, source_type, source_ref, source_ref_resolved, source_subpath,
                    source_branch, source_revision, remote_revision, central_path, content_hash, enabled,
                    created_at, updated_at, status, update_status, last_checked_at, last_check_error
             FROM skills ORDER BY name",
        )?;
        let rows = stmt.query_map([], map_skill_row)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_skill_by_id(&self, id: &str) -> Result<Option<SkillRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, source_type, source_ref, source_ref_resolved, source_subpath,
                    source_branch, source_revision, remote_revision, central_path, content_hash, enabled,
                    created_at, updated_at, status, update_status, last_checked_at, last_check_error
             FROM skills WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], map_skill_row)?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    pub fn get_skill_by_central_path(&self, central_path: &str) -> Result<Option<SkillRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, source_type, source_ref, source_ref_resolved, source_subpath,
                    source_branch, source_revision, remote_revision, central_path, content_hash, enabled,
                    created_at, updated_at, status, update_status, last_checked_at, last_check_error
             FROM skills WHERE central_path = ?1",
        )?;
        let mut rows = stmt.query_map(params![central_path], map_skill_row)?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    pub fn get_skill_by_source_ref(
        &self,
        source_type: &str,
        source_ref: &str,
    ) -> Result<Option<SkillRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, source_type, source_ref, source_ref_resolved, source_subpath,
                    source_branch, source_revision, remote_revision, central_path, content_hash, enabled,
                    created_at, updated_at, status, update_status, last_checked_at, last_check_error
             FROM skills
             WHERE source_type = ?1 AND source_ref = ?2",
        )?;
        let mut rows = stmt.query_map(params![source_type, source_ref], map_skill_row)?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    pub fn update_skill_source_metadata(
        &self,
        id: &str,
        source_ref_resolved: Option<&str>,
        source_subpath: Option<&str>,
        source_branch: Option<&str>,
        source_revision: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "UPDATE skills
             SET source_ref_resolved = ?1, source_subpath = ?2, source_branch = ?3, source_revision = ?4, updated_at = ?5
             WHERE id = ?6",
            params![
                source_ref_resolved,
                source_subpath,
                source_branch,
                source_revision,
                now,
                id
            ],
        )?;
        Ok(())
    }

    pub fn update_skill_check_state(
        &self,
        id: &str,
        remote_revision: Option<&str>,
        update_status: &str,
        last_check_error: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "UPDATE skills
             SET remote_revision = ?1, update_status = ?2, last_checked_at = ?3, last_check_error = ?4
             WHERE id = ?5",
            params![remote_revision, update_status, now, last_check_error, id],
        )?;
        Ok(())
    }

    pub fn update_skill_update_status(&self, id: &str, update_status: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE skills SET update_status = ?1 WHERE id = ?2",
            params![update_status, id],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_skill_after_install(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        source_revision: Option<&str>,
        remote_revision: Option<&str>,
        content_hash: Option<&str>,
        update_status: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "UPDATE skills
             SET name = ?1, description = ?2, source_revision = ?3, remote_revision = ?4, content_hash = ?5,
                 updated_at = ?6, update_status = ?7, last_checked_at = ?6, last_check_error = NULL
             WHERE id = ?8",
            params![
                name,
                description,
                source_revision,
                remote_revision,
                content_hash,
                now,
                update_status,
                id
            ],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_skill_after_reinstall(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        source_type: &str,
        source_ref: Option<&str>,
        source_ref_resolved: Option<&str>,
        source_subpath: Option<&str>,
        source_branch: Option<&str>,
        source_revision: Option<&str>,
        remote_revision: Option<&str>,
        content_hash: Option<&str>,
        update_status: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "UPDATE skills
             SET name = ?1, description = ?2, source_type = ?3, source_ref = ?4, source_ref_resolved = ?5,
                 source_subpath = ?6, source_branch = ?7, source_revision = ?8, remote_revision = ?9,
                 content_hash = ?10, updated_at = ?11, status = 'ok', update_status = ?12, last_checked_at = ?11,
                 last_check_error = NULL
             WHERE id = ?13",
            params![
                name,
                description,
                source_type,
                source_ref,
                source_ref_resolved,
                source_subpath,
                source_branch,
                source_revision,
                remote_revision,
                content_hash,
                now,
                update_status,
                id
            ],
        )?;
        Ok(())
    }

    pub fn delete_skill(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM skills WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ── Targets ──

    pub fn insert_target(&self, target: &SkillTargetRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO skill_targets (id, skill_id, tool, target_path, mode, status, synced_at, last_error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                target.id,
                target.skill_id,
                target.tool,
                target.target_path,
                target.mode,
                target.status,
                target.synced_at,
                target.last_error,
            ],
        )?;
        Ok(())
    }

    pub fn get_targets_for_skill(&self, skill_id: &str) -> Result<Vec<SkillTargetRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, skill_id, tool, target_path, mode, status, synced_at, last_error FROM skill_targets WHERE skill_id = ?1",
        )?;
        let rows = stmt.query_map(params![skill_id], |row| {
            Ok(SkillTargetRecord {
                id: row.get(0)?,
                skill_id: row.get(1)?,
                tool: row.get(2)?,
                target_path: row.get(3)?,
                mode: row.get(4)?,
                status: row.get(5)?,
                synced_at: row.get(6)?,
                last_error: row.get(7)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_all_targets(&self) -> Result<Vec<SkillTargetRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, skill_id, tool, target_path, mode, status, synced_at, last_error FROM skill_targets",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SkillTargetRecord {
                id: row.get(0)?,
                skill_id: row.get(1)?,
                tool: row.get(2)?,
                target_path: row.get(3)?,
                mode: row.get(4)?,
                status: row.get(5)?,
                synced_at: row.get(6)?,
                last_error: row.get(7)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn delete_target(&self, skill_id: &str, tool: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM skill_targets WHERE skill_id = ?1 AND tool = ?2",
            params![skill_id, tool],
        )?;
        Ok(())
    }

    // ── Discovered Skills ──

    pub fn clear_discovered(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM discovered_skills", [])?;
        Ok(())
    }

    pub fn insert_discovered(&self, rec: &DiscoveredSkillRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO discovered_skills (id, tool, found_path, name_guess, fingerprint, found_at, imported_skill_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                rec.id,
                rec.tool,
                rec.found_path,
                rec.name_guess,
                rec.fingerprint,
                rec.found_at,
                rec.imported_skill_id,
            ],
        )?;
        Ok(())
    }

    pub fn get_all_discovered(&self) -> Result<Vec<DiscoveredSkillRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tool, found_path, name_guess, fingerprint, found_at, imported_skill_id FROM discovered_skills",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DiscoveredSkillRecord {
                id: row.get(0)?,
                tool: row.get(1)?,
                found_path: row.get(2)?,
                name_guess: row.get(3)?,
                fingerprint: row.get(4)?,
                found_at: row.get(5)?,
                imported_skill_id: row.get(6)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Cache ──

    pub fn get_cache(&self, key: &str, ttl_secs: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        let mut stmt = conn
            .prepare("SELECT data FROM skillssh_cache WHERE cache_key = ?1 AND fetched_at > ?2")?;
        let cutoff = now - ttl_secs;
        let mut rows = stmt.query_map(params![key, cutoff], |row| row.get::<_, String>(0))?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    pub fn set_cache(&self, key: &str, data: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT OR REPLACE INTO skillssh_cache (cache_key, data, fetched_at) VALUES (?1, ?2, ?3)",
            params![key, data, now],
        )?;
        Ok(())
    }

    // ── Settings ──

    pub fn proxy_url(&self) -> Option<String> {
        self.get_setting("proxy_url")
            .ok()
            .flatten()
            .filter(|s| !s.is_empty())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        // Read the raw stored value while holding the lock, then release it
        // before any write-back so we don't re-enter the mutex.
        let raw = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
            let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;
            rows.next().and_then(|r| r.ok())
        };

        let value = match raw {
            None => return Ok(None),
            Some(v) => v,
        };

        if SENSITIVE_KEYS.contains(&key) {
            if crypto::is_encrypted(&value) {
                // Happy path: already encrypted, just decrypt.
                Ok(Some(crypto::decrypt(&self.secret_key, &value)?))
            } else {
                // Backward compat: old plaintext value — upgrade it silently.
                let encrypted = crypto::encrypt(&self.secret_key, &value)?;
                let conn = self.conn.lock().unwrap();
                conn.execute(
                    "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                    params![key, encrypted],
                )?;
                Ok(Some(value))
            }
        } else {
            Ok(Some(value))
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let stored = if SENSITIVE_KEYS.contains(&key) {
            crypto::encrypt(&self.secret_key, value)?
        } else {
            value.to_string()
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, stored],
        )?;
        Ok(())
    }

    // ── Scenarios ──

    pub fn insert_scenario(&self, scenario: &ScenarioRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO scenarios (id, name, description, icon, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                scenario.id,
                scenario.name,
                scenario.description,
                scenario.icon,
                scenario.sort_order,
                scenario.created_at,
                scenario.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_all_scenarios(&self) -> Result<Vec<ScenarioRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, icon, sort_order, created_at, updated_at FROM scenarios ORDER BY sort_order, created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ScenarioRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                icon: row.get(3)?,
                sort_order: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn update_scenario(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        icon: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "UPDATE scenarios SET name = ?1, description = ?2, icon = ?3, updated_at = ?4 WHERE id = ?5",
            params![name, description, icon, now, id],
        )?;
        Ok(())
    }

    pub fn delete_scenario(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM scenarios WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn reorder_scenarios(&self, ids: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        for (i, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE scenarios SET sort_order = ?1 WHERE id = ?2",
                params![i as i32, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn reorder_projects(&self, ids: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        for (i, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE projects SET sort_order = ?1 WHERE id = ?2",
                params![i as i32, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // ── Scenario-Skill mapping ──

    pub fn add_skill_to_scenario(&self, scenario_id: &str, skill_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT OR IGNORE INTO scenario_skills (scenario_id, skill_id, added_at) VALUES (?1, ?2, ?3)",
            params![scenario_id, skill_id, now],
        )?;
        Ok(())
    }

    pub fn remove_skill_from_scenario(&self, scenario_id: &str, skill_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM scenario_skills WHERE scenario_id = ?1 AND skill_id = ?2",
            params![scenario_id, skill_id],
        )?;
        Ok(())
    }

    pub fn reorder_scenario_skills(&self, scenario_id: &str, skill_ids: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        for (i, skill_id) in skill_ids.iter().enumerate() {
            tx.execute(
                "UPDATE scenario_skills SET sort_order = ?1 WHERE scenario_id = ?2 AND skill_id = ?3",
                params![i as i32, scenario_id, skill_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_skill_ids_for_scenario(&self, scenario_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT skill_id FROM scenario_skills WHERE scenario_id = ?1 ORDER BY sort_order, added_at",
        )?;
        let rows = stmt.query_map(params![scenario_id], |row| row.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_skills_for_scenario(&self, scenario_id: &str) -> Result<Vec<SkillRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.name, s.description, s.source_type, s.source_ref, s.source_ref_resolved, s.source_subpath,
                    s.source_branch, s.source_revision, s.remote_revision, s.central_path, s.content_hash, s.enabled,
                    s.created_at, s.updated_at, s.status, s.update_status, s.last_checked_at, s.last_check_error
             FROM skills s
             INNER JOIN scenario_skills ss ON s.id = ss.skill_id
             WHERE ss.scenario_id = ?1
             ORDER BY ss.sort_order, s.name",
        )?;
        let rows = stmt.query_map(params![scenario_id], map_skill_row)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn count_skills_for_scenario(&self, scenario_id: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM scenario_skills WHERE scenario_id = ?1",
            params![scenario_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn get_scenarios_for_skill(&self, skill_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT scenario_id FROM scenario_skills WHERE skill_id = ?1")?;
        let rows = stmt.query_map(params![skill_id], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn ensure_scenario_skill_tool_defaults(
        &self,
        scenario_id: &str,
        skill_id: &str,
        tools: &[String],
    ) -> Result<()> {
        if tools.is_empty() {
            return Ok(());
        }

        let conn = self.conn.lock().unwrap();
        let mut existing_stmt = conn.prepare(
            "SELECT tool
             FROM scenario_skill_tools
             WHERE scenario_id = ?1 AND skill_id = ?2",
        )?;
        let existing_rows = existing_stmt.query_map(params![scenario_id, skill_id], |row| {
            row.get::<_, String>(0)
        })?;
        let existing_tools: std::collections::HashSet<String> = existing_rows
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .collect();

        let missing_tools: Vec<&String> = tools
            .iter()
            .filter(|tool| !existing_tools.contains(*tool))
            .collect();
        if missing_tools.is_empty() {
            return Ok(());
        }

        let tx = conn.unchecked_transaction()?;
        let now = chrono::Utc::now().timestamp_millis();

        for tool in missing_tools {
            tx.execute(
                "INSERT OR IGNORE INTO scenario_skill_tools (scenario_id, skill_id, tool, enabled, updated_at)
                 VALUES (?1, ?2, ?3, 1, ?4)",
                params![scenario_id, skill_id, tool, now],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn set_scenario_skill_tool_enabled(
        &self,
        scenario_id: &str,
        skill_id: &str,
        tool: &str,
        enabled: bool,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO scenario_skill_tools (scenario_id, skill_id, tool, enabled, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(scenario_id, skill_id, tool)
             DO UPDATE SET enabled = excluded.enabled, updated_at = excluded.updated_at",
            params![scenario_id, skill_id, tool, enabled, now],
        )?;
        Ok(())
    }

    pub fn get_scenario_skill_tool_toggles(
        &self,
        scenario_id: &str,
        skill_id: &str,
    ) -> Result<Vec<ScenarioSkillToolToggleRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT scenario_id, skill_id, tool, enabled, updated_at
             FROM scenario_skill_tools
             WHERE scenario_id = ?1 AND skill_id = ?2
             ORDER BY tool",
        )?;
        let rows = stmt.query_map(params![scenario_id, skill_id], |row| {
            Ok(ScenarioSkillToolToggleRecord {
                scenario_id: row.get(0)?,
                skill_id: row.get(1)?,
                tool: row.get(2)?,
                enabled: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_enabled_tools_for_scenario_skill(
        &self,
        scenario_id: &str,
        skill_id: &str,
    ) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT tool
             FROM scenario_skill_tools
             WHERE scenario_id = ?1 AND skill_id = ?2 AND enabled = 1",
        )?;
        let rows = stmt.query_map(params![scenario_id, skill_id], |row| {
            row.get::<_, String>(0)
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ── Active Scenario ──

    pub fn get_active_scenario_id(&self) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT scenario_id FROM active_scenario WHERE key = 'current'")?;
        let mut rows = stmt.query_map([], |row| row.get::<_, Option<String>>(0))?;
        Ok(rows.next().and_then(|r| r.ok()).flatten())
    }

    pub fn set_active_scenario(&self, scenario_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO active_scenario (key, scenario_id) VALUES ('current', ?1)",
            params![scenario_id],
        )?;
        Ok(())
    }

    // ── Projects ──

    pub fn insert_project(&self, project: &ProjectRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                project.id,
                project.name,
                project.path,
                project.sort_order,
                project.created_at,
                project.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_all_projects(&self) -> Result<Vec<ProjectRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, path, sort_order, created_at, updated_at FROM projects ORDER BY sort_order, created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ProjectRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                sort_order: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_project_by_id(&self, id: &str) -> Result<Option<ProjectRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, path, sort_order, created_at, updated_at FROM projects WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(ProjectRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                sort_order: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    pub fn delete_project(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ── Skill Tags ──

    pub fn get_all_tags(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT DISTINCT tag FROM skill_tags ORDER BY tag")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn set_tags_for_skill(&self, skill_id: &str, tags: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM skill_tags WHERE skill_id = ?1",
            params![skill_id],
        )?;
        for tag in tags {
            let trimmed = tag.trim();
            if !trimmed.is_empty() {
                conn.execute(
                    "INSERT OR IGNORE INTO skill_tags (skill_id, tag) VALUES (?1, ?2)",
                    params![skill_id, trimmed],
                )?;
            }
        }
        Ok(())
    }

    pub fn get_tags_map(&self) -> Result<std::collections::HashMap<String, Vec<String>>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT skill_id, tag FROM skill_tags ORDER BY tag")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for row in rows.filter_map(|r| r.ok()) {
            map.entry(row.0).or_default().push(row.1);
        }
        Ok(map)
    }
}

fn map_skill_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SkillRecord> {
    Ok(SkillRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        source_type: row.get(3)?,
        source_ref: row.get(4)?,
        source_ref_resolved: row.get(5)?,
        source_subpath: row.get(6)?,
        source_branch: row.get(7)?,
        source_revision: row.get(8)?,
        remote_revision: row.get(9)?,
        central_path: row.get(10)?,
        content_hash: row.get(11)?,
        enabled: row.get::<_, i32>(12)? != 0,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
        status: row.get(15)?,
        update_status: row.get(16)?,
        last_checked_at: row.get(17)?,
        last_check_error: row.get(18)?,
    })
}
