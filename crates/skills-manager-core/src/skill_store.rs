use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::crypto;

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
    pub is_native: bool,
}

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
    pub router_description: Option<String>,
    pub router_body: Option<String>,
    pub is_essential: bool,
    pub router_updated_at: Option<i64>,
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
    pub workspace_type: String,
    pub linked_agent_key: Option<String>,
    pub linked_agent_name: Option<String>,
    pub disabled_path: Option<String>,
    pub sort_order: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManagedPluginRecord {
    pub id: String,
    pub plugin_key: String,
    pub display_name: Option<String>,
    pub plugin_data: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioPluginRecord {
    pub plugin: ManagedPluginRecord,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentConfigRecord {
    pub tool_key: String,
    pub scenario_id: Option<String>,
    pub managed: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentSkillOwnership {
    pub managed: Vec<SkillRecord>,
    pub discovered: Vec<DiscoveredSkillRecord>,
    pub native: Vec<String>,
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

    pub fn update_skill_source_ref(&self, id: &str, source_ref: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE skills SET source_ref = ?1 WHERE id = ?2",
            params![source_ref, id],
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
            "INSERT INTO discovered_skills (id, tool, found_path, name_guess, fingerprint, found_at, imported_skill_id, is_native)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                rec.id,
                rec.tool,
                rec.found_path,
                rec.name_guess,
                rec.fingerprint,
                rec.found_at,
                rec.imported_skill_id,
                rec.is_native as i32,
            ],
        )?;
        Ok(())
    }

    pub fn get_all_discovered(&self) -> Result<Vec<DiscoveredSkillRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tool, found_path, name_guess, fingerprint, found_at, imported_skill_id, is_native FROM discovered_skills",
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
                is_native: row.get::<_, i32>(7)? != 0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn mark_discovered_as_native(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE discovered_skills SET is_native = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn unmark_discovered_as_native(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE discovered_skills SET is_native = 0 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn get_discovered_by_id(&self, id: &str) -> Result<Option<DiscoveredSkillRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tool, found_path, name_guess, fingerprint, found_at, imported_skill_id, is_native
             FROM discovered_skills WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
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

    pub fn link_discovered_to_skill(&self, discovered_id: &str, skill_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE discovered_skills SET imported_skill_id = ?1 WHERE id = ?2",
            params![skill_id, discovered_id],
        )?;
        Ok(())
    }

    pub fn delete_discovered(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM discovered_skills WHERE id = ?1", params![id])?;
        Ok(())
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

    pub fn remap_tool_key_references(&self, old_key: &str, new_key: &str) -> Result<()> {
        if old_key == new_key {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();

        // scenario_skill_tools has a composite PK (scenario_id, skill_id, tool). If both old/new
        // rows exist for the same skill in the same scenario, keep the new-key row.
        conn.execute(
            "DELETE FROM scenario_skill_tools AS old_rows
             WHERE old_rows.tool = ?1
               AND EXISTS (
                 SELECT 1
                 FROM scenario_skill_tools AS new_rows
                 WHERE new_rows.tool = ?2
                   AND new_rows.scenario_id = old_rows.scenario_id
                   AND new_rows.skill_id = old_rows.skill_id
               )",
            params![old_key, new_key],
        )?;
        conn.execute(
            "UPDATE scenario_skill_tools SET tool = ?2 WHERE tool = ?1",
            params![old_key, new_key],
        )?;

        // skill_targets has UNIQUE(skill_id, tool). Same strategy: keep existing new-key rows.
        conn.execute(
            "DELETE FROM skill_targets AS old_rows
             WHERE old_rows.tool = ?1
               AND EXISTS (
                 SELECT 1
                 FROM skill_targets AS new_rows
                 WHERE new_rows.tool = ?2
                   AND new_rows.skill_id = old_rows.skill_id
               )",
            params![old_key, new_key],
        )?;
        conn.execute(
            "UPDATE skill_targets SET tool = ?2 WHERE tool = ?1",
            params![old_key, new_key],
        )?;

        conn.execute(
            "UPDATE discovered_skills SET tool = ?2 WHERE tool = ?1",
            params![old_key, new_key],
        )?;
        Ok(())
    }

    pub fn has_tool_key_references(&self, key: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT EXISTS(SELECT 1 FROM skill_targets WHERE tool = ?1)
             OR EXISTS(SELECT 1 FROM discovered_skills WHERE tool = ?1)
             OR EXISTS(SELECT 1 FROM scenario_skill_tools WHERE tool = ?1)",
        )?;
        let exists: i64 = stmt.query_row(params![key], |row| row.get(0))?;
        Ok(exists != 0)
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

    pub fn clear_active_scenario(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM active_scenario WHERE key = 'current'", [])?;
        Ok(())
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
            "INSERT INTO projects (
                id, name, path, workspace_type, linked_agent_key, linked_agent_name, disabled_path,
                sort_order, created_at, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                project.id,
                project.name,
                project.path,
                project.workspace_type,
                project.linked_agent_key,
                project.linked_agent_name,
                project.disabled_path,
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
            "SELECT id, name, path, workspace_type, linked_agent_key, linked_agent_name, disabled_path,
                    sort_order, created_at, updated_at
             FROM projects
             ORDER BY sort_order, created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ProjectRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                workspace_type: row.get(3)?,
                linked_agent_key: row.get(4)?,
                linked_agent_name: row.get(5)?,
                disabled_path: row.get(6)?,
                sort_order: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_project_by_id(&self, id: &str) -> Result<Option<ProjectRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, path, workspace_type, linked_agent_key, linked_agent_name, disabled_path,
                    sort_order, created_at, updated_at
             FROM projects
             WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(ProjectRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                workspace_type: row.get(3)?,
                linked_agent_key: row.get(4)?,
                linked_agent_name: row.get(5)?,
                disabled_path: row.get(6)?,
                sort_order: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
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

    // ── Managed Plugins ──

    pub fn insert_managed_plugin(&self, record: &ManagedPluginRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO managed_plugins (id, plugin_key, display_name, plugin_data, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.id,
                record.plugin_key,
                record.display_name,
                record.plugin_data,
                record.created_at,
                record.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_all_managed_plugins(&self) -> Result<Vec<ManagedPluginRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, plugin_key, display_name, plugin_data, created_at, updated_at
             FROM managed_plugins ORDER BY display_name, plugin_key",
        )?;
        let rows = stmt.query_map([], map_managed_plugin_row)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_managed_plugin_by_id(&self, id: &str) -> Result<Option<ManagedPluginRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, plugin_key, display_name, plugin_data, created_at, updated_at
             FROM managed_plugins WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], map_managed_plugin_row)?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    pub fn get_managed_plugin_by_key(
        &self,
        plugin_key: &str,
    ) -> Result<Option<ManagedPluginRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, plugin_key, display_name, plugin_data, created_at, updated_at
             FROM managed_plugins WHERE plugin_key = ?1",
        )?;
        let mut rows = stmt.query_map(params![plugin_key], map_managed_plugin_row)?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    pub fn update_managed_plugin_data(&self, id: &str, plugin_data: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "UPDATE managed_plugins SET plugin_data = ?1, updated_at = ?2 WHERE id = ?3",
            params![plugin_data, now, id],
        )?;
        Ok(())
    }

    pub fn delete_managed_plugin(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM managed_plugins WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ── Scenario-Plugin mapping ──

    /// Set whether a plugin is enabled for a given scenario.
    pub fn set_scenario_plugin_enabled(
        &self,
        scenario_id: &str,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO scenario_plugins (scenario_id, plugin_id, enabled)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(scenario_id, plugin_id)
             DO UPDATE SET enabled = excluded.enabled",
            params![scenario_id, plugin_id, enabled],
        )?;
        Ok(())
    }

    /// Get all managed plugins with their enabled state for a scenario.
    /// Plugins with no scenario_plugins row are considered enabled (default).
    pub fn get_scenario_plugins(&self, scenario_id: &str) -> Result<Vec<ScenarioPluginRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT mp.id, mp.plugin_key, mp.display_name, mp.plugin_data, mp.created_at, mp.updated_at,
                    COALESCE(sp.enabled, 1) AS enabled
             FROM managed_plugins mp
             LEFT JOIN scenario_plugins sp ON mp.id = sp.plugin_id AND sp.scenario_id = ?1
             ORDER BY mp.display_name, mp.plugin_key",
        )?;
        let rows = stmt.query_map(params![scenario_id], |row| {
            Ok(ScenarioPluginRecord {
                plugin: ManagedPluginRecord {
                    id: row.get(0)?,
                    plugin_key: row.get(1)?,
                    display_name: row.get(2)?,
                    plugin_data: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                },
                enabled: row.get::<_, i32>(6)? != 0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get the plugin keys that are enabled for a scenario.
    /// Plugins with no scenario_plugins row are considered enabled (default).
    pub fn get_enabled_plugin_keys_for_scenario(&self, scenario_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT mp.plugin_key
             FROM managed_plugins mp
             LEFT JOIN scenario_plugins sp ON mp.id = sp.plugin_id AND sp.scenario_id = ?1
             WHERE COALESCE(sp.enabled, 1) = 1
             ORDER BY mp.plugin_key",
        )?;
        let rows = stmt.query_map(params![scenario_id], |row| row.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get the plugin keys that are disabled for a scenario.
    pub fn get_disabled_plugin_keys_for_scenario(&self, scenario_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT mp.plugin_key
             FROM managed_plugins mp
             INNER JOIN scenario_plugins sp ON mp.id = sp.plugin_id AND sp.scenario_id = ?1
             WHERE sp.enabled = 0
             ORDER BY mp.plugin_key",
        )?;
        let rows = stmt.query_map(params![scenario_id], |row| row.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Packs CRUD ──

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
        let max_order: i32 = conn.query_row(
            "SELECT COALESCE(MAX(sort_order), -1) FROM packs",
            [],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO packs (id, name, description, icon, color, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, name, description, icon, color, max_order + 1, now, now],
        )?;
        Ok(())
    }

    pub fn get_all_packs(&self) -> Result<Vec<PackRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, icon, color, sort_order, created_at, updated_at,
                    router_description, router_body, is_essential, router_updated_at
             FROM packs ORDER BY sort_order, created_at",
        )?;
        let rows = stmt.query_map([], map_pack_row)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_pack_by_id(&self, id: &str) -> Result<Option<PackRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, icon, color, sort_order, created_at, updated_at,
                    router_description, router_body, is_essential, router_updated_at
             FROM packs WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], map_pack_row)?;
        Ok(rows.next().and_then(|r| r.ok()))
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

    pub fn set_pack_router(
        &self,
        pack_id: &str,
        description: Option<&str>,
        body: Option<&str>,
        updated_at: i64,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE packs SET router_description = ?2, router_body = ?3, router_updated_at = ?4 WHERE id = ?1",
            params![pack_id, description, body, updated_at],
        )?;
        if n == 0 {
            anyhow::bail!("pack {pack_id} not found");
        }
        Ok(())
    }

    pub fn set_pack_essential(&self, pack_id: &str, is_essential: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE packs SET is_essential = ?2 WHERE id = ?1",
            params![pack_id, is_essential as i32],
        )?;
        if n == 0 {
            anyhow::bail!("pack {pack_id} not found");
        }
        Ok(())
    }

    pub fn add_skill_to_pack(&self, pack_id: &str, skill_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let max_order: i32 = conn.query_row(
            "SELECT COALESCE(MAX(sort_order), -1) FROM pack_skills WHERE pack_id = ?1",
            params![pack_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO pack_skills (pack_id, skill_id, sort_order) VALUES (?1, ?2, ?3)",
            params![pack_id, skill_id, max_order + 1],
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

    pub fn count_skills_for_pack(&self, pack_id: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pack_skills WHERE pack_id = ?1",
            params![pack_id],
            |row| row.get(0),
        )?;
        Ok(count)
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
        let max_order: i32 = conn.query_row(
            "SELECT COALESCE(MAX(sort_order), -1) FROM scenario_packs WHERE scenario_id = ?1",
            params![scenario_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO scenario_packs (scenario_id, pack_id, sort_order) VALUES (?1, ?2, ?3)",
            params![scenario_id, pack_id, max_order + 1],
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
            "SELECT p.id, p.name, p.description, p.icon, p.color, p.sort_order, p.created_at, p.updated_at,
                    p.router_description, p.router_body, p.is_essential, p.router_updated_at
             FROM packs p
             INNER JOIN scenario_packs sp ON p.id = sp.pack_id
             WHERE sp.scenario_id = ?1
             ORDER BY sp.sort_order, p.name",
        )?;
        let rows = stmt.query_map(params![scenario_id], map_pack_row)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Agent Config ──

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
            "INSERT OR REPLACE INTO agent_configs (tool_key, scenario_id, managed, updated_at)
             VALUES (?1, ?2, COALESCE((SELECT managed FROM agent_configs WHERE tool_key = ?1), 1), ?3)",
            params![tool_key, scenario_id, now],
        )?;
        Ok(())
    }

    pub fn set_agent_managed(&self, tool_key: &str, managed: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "UPDATE agent_configs SET managed = ?1, updated_at = ?2 WHERE tool_key = ?3",
            params![managed, now, tool_key],
        )?;
        Ok(())
    }

    /// Seed agent_configs for each tool_key using the current active scenario.
    /// Existing rows are left unchanged (INSERT OR IGNORE).
    pub fn init_agent_configs(&self, tool_keys: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();

        // Get the current active scenario
        let active_scenario_id: Option<String> = {
            let mut stmt =
                conn.prepare("SELECT scenario_id FROM active_scenario WHERE key = 'current'")?;
            let mut rows = stmt.query_map([], |row| row.get::<_, Option<String>>(0))?;
            rows.next().and_then(|r| r.ok()).flatten()
        };

        let tx = conn.unchecked_transaction()?;
        for tool_key in tool_keys {
            tx.execute(
                "INSERT OR IGNORE INTO agent_configs (tool_key, scenario_id, managed, updated_at)
                 VALUES (?1, ?2, 1, ?3)",
                params![tool_key, active_scenario_id, now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

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
            "SELECT p.id, p.name, p.description, p.icon, p.color, p.sort_order, p.created_at, p.updated_at,
                    p.router_description, p.router_body, p.is_essential, p.router_updated_at
             FROM packs p
             INNER JOIN agent_extra_packs aep ON p.id = aep.pack_id
             WHERE aep.tool_key = ?1
             ORDER BY p.sort_order, p.name",
        )?;
        let rows = stmt.query_map(params![tool_key], map_pack_row)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Returns the deduplicated, ordered effective skill list for an agent.
    /// Combines the agent's scenario skills (packs + direct) with any extra packs
    /// assigned to the agent. Duplicates are removed (first occurrence wins).
    pub fn get_effective_skills_for_agent(&self, tool_key: &str) -> Result<Vec<SkillRecord>> {
        let conn = self.conn.lock().unwrap();

        // Get the agent's scenario_id
        let scenario_id: Option<String> = {
            let mut stmt =
                conn.prepare("SELECT scenario_id FROM agent_configs WHERE tool_key = ?1")?;
            let mut rows =
                stmt.query_map(params![tool_key], |row| row.get::<_, Option<String>>(0))?;
            rows.next().and_then(|r| r.ok()).flatten()
        };

        let scenario_id = match scenario_id {
            Some(id) => id,
            None => return Ok(Vec::new()),
        };

        let mut stmt = conn.prepare(
            "SELECT s.id, s.name, s.description, s.source_type, s.source_ref, s.source_ref_resolved, s.source_subpath,
                    s.source_branch, s.source_revision, s.remote_revision, s.central_path, s.content_hash, s.enabled,
                    s.created_at, s.updated_at, s.status, s.update_status, s.last_checked_at, s.last_check_error
             FROM (
                 SELECT ps.skill_id AS id, sp.sort_order * 10000 + ps.sort_order AS effective_order
                 FROM pack_skills ps
                 INNER JOIN scenario_packs sp ON ps.pack_id = sp.pack_id
                 WHERE sp.scenario_id = ?1
                 UNION ALL
                 SELECT ss.skill_id AS id, 99999000 + ss.sort_order AS effective_order
                 FROM scenario_skills ss
                 WHERE ss.scenario_id = ?1
                 UNION ALL
                 SELECT ps.skill_id AS id, 199999000 + ps.sort_order AS effective_order
                 FROM pack_skills ps
                 INNER JOIN agent_extra_packs aep ON ps.pack_id = aep.pack_id
                 WHERE aep.tool_key = ?2
             ) AS ordering
             INNER JOIN skills s ON s.id = ordering.id
             GROUP BY s.id
             ORDER BY MIN(ordering.effective_order)",
        )?;
        let rows = stmt.query_map(params![scenario_id, tool_key], map_skill_row)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Effective Skill Resolution ──

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
                 SELECT ps.skill_id AS id, sp.sort_order * 10000 + ps.sort_order AS effective_order
                 FROM pack_skills ps
                 INNER JOIN scenario_packs sp ON ps.pack_id = sp.pack_id
                 WHERE sp.scenario_id = ?1
                 UNION ALL
                 SELECT ss.skill_id AS id, 99999000 + ss.sort_order AS effective_order
                 FROM scenario_skills ss
                 WHERE ss.scenario_id = ?2
             ) AS ordering
             INNER JOIN skills s ON s.id = ordering.id
             GROUP BY s.id
             ORDER BY MIN(ordering.effective_order)",
        )?;
        let rows = stmt.query_map(params![scenario_id, scenario_id], map_skill_row)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Check if a skill is in the effective skill list for a scenario.
    pub fn is_skill_in_effective_scenario(
        &self,
        scenario_id: &str,
        skill_id: &str,
    ) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM pack_skills ps
                 INNER JOIN scenario_packs sp ON ps.pack_id = sp.pack_id
                 WHERE sp.scenario_id = ?1 AND ps.skill_id = ?2
             ) OR EXISTS(
                 SELECT 1 FROM scenario_skills ss
                 WHERE ss.scenario_id = ?1 AND ss.skill_id = ?2
             )",
            params![scenario_id, skill_id],
            |row| row.get(0),
        )?;
        Ok(exists)
    }

    /// Returns only the IDs of the effective skill list (lighter than full records).
    pub fn get_effective_skill_ids_for_scenario(&self, scenario_id: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT candidates.id FROM (
                 SELECT ps.skill_id AS id
                 FROM pack_skills ps
                 INNER JOIN scenario_packs sp ON ps.pack_id = sp.pack_id
                 WHERE sp.scenario_id = ?1
                 UNION ALL
                 SELECT ss.skill_id AS id
                 FROM scenario_skills ss
                 WHERE ss.scenario_id = ?2
             ) AS candidates
             INNER JOIN skills s ON s.id = candidates.id",
        )?;
        let rows = stmt.query_map(params![scenario_id, scenario_id], |row| {
            row.get::<_, String>(0)
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Discovered Skills (per-tool) ──

    /// Returns discovered skills for a specific tool that have not been imported.
    pub fn get_discovered_for_tool(&self, tool: &str) -> Result<Vec<DiscoveredSkillRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tool, found_path, name_guess, fingerprint, found_at, imported_skill_id, is_native
             FROM discovered_skills WHERE tool = ?1 AND imported_skill_id IS NULL
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

    // ── Skill Ownership ──

    /// Classify skills in an agent's directory into three categories:
    /// - managed: SM-managed skills (from scenario + extra packs)
    /// - discovered: found by scanner but not yet imported
    /// - native: present in agent dir but not SM-managed, not discovered
    pub fn scan_agent_skill_ownership(
        &self,
        tool_key: &str,
        agent_skills_dir: &std::path::Path,
    ) -> Result<AgentSkillOwnership> {
        let sm_skills_dir = crate::central_repo::skills_dir();
        let sm_prefix = sm_skills_dir.to_string_lossy().to_string();

        // 1. Managed: effective skills for this agent
        let managed = self.get_effective_skills_for_agent(tool_key)?;
        let managed_names: std::collections::HashSet<String> =
            managed.iter().map(|s| s.name.clone()).collect();

        // 2. Discovered: from discovered_skills table, not yet imported
        let discovered = self.get_discovered_for_tool(tool_key)?;
        let discovered_names: std::collections::HashSet<String> = discovered
            .iter()
            .filter_map(|d| d.name_guess.clone())
            .collect();

        // 3. Native: in agent dir, not a SM symlink, not discovered, not managed
        let mut native = Vec::new();
        if agent_skills_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(agent_skills_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let path = entry.path();

                    // Skip SM symlinks
                    if path.is_symlink() {
                        if let Ok(target) = std::fs::read_link(&path) {
                            if target.to_string_lossy().contains(&sm_prefix) {
                                continue;
                            }
                        }
                    }

                    // Skip if it's a managed skill name (copy mode remnant)
                    if managed_names.contains(&name) {
                        continue;
                    }
                    // Skip if it's discovered
                    if discovered_names.contains(&name) {
                        continue;
                    }
                    // Skip non-directories
                    if !path.is_dir() && !path.is_symlink() {
                        continue;
                    }

                    native.push(name);
                }
            }
        }
        native.sort();

        Ok(AgentSkillOwnership {
            managed,
            discovered,
            native,
        })
    }
}

fn map_managed_plugin_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ManagedPluginRecord> {
    Ok(ManagedPluginRecord {
        id: row.get(0)?,
        plugin_key: row.get(1)?,
        display_name: row.get(2)?,
        plugin_data: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn map_pack_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PackRecord> {
    Ok(PackRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        icon: row.get(3)?,
        color: row.get(4)?,
        sort_order: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        router_description: row.get(8)?,
        router_body: row.get(9)?,
        is_essential: row.get::<_, i64>(10)? != 0,
        router_updated_at: row.get(11)?,
    })
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

#[cfg(test)]
mod pack_tests {
    use super::*;
    use tempfile::NamedTempFile;

    /// Create a SkillStore backed by a temporary database file.
    fn test_store() -> (SkillStore, NamedTempFile) {
        let tmp = NamedTempFile::new().unwrap();
        let store = SkillStore::new(&tmp.path().to_path_buf()).unwrap();
        (store, tmp)
    }

    /// Insert a minimal test skill and return its SkillRecord.
    fn insert_test_skill(store: &SkillStore, id: &str, name: &str) -> SkillRecord {
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
        rec
    }

    /// Insert a minimal test scenario.
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

    // ── Task 9: Pack CRUD Tests ──

    #[test]
    fn insert_and_get_pack() {
        let (store, _tmp) = test_store();
        store
            .insert_pack(
                "p1",
                "Core Tools",
                Some("Essential tools"),
                Some("🔧"),
                Some("#ff0000"),
            )
            .unwrap();

        let packs = store.get_all_packs().unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].id, "p1");
        assert_eq!(packs[0].name, "Core Tools");
        assert_eq!(packs[0].description.as_deref(), Some("Essential tools"));
        assert_eq!(packs[0].icon.as_deref(), Some("🔧"));
        assert_eq!(packs[0].color.as_deref(), Some("#ff0000"));
        assert_eq!(packs[0].sort_order, 0);
    }

    #[test]
    fn get_pack_by_id_found_and_not_found() {
        let (store, _tmp) = test_store();
        store
            .insert_pack("p1", "Pack One", None, None, None)
            .unwrap();

        let found = store.get_pack_by_id("p1").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Pack One");

        let not_found = store.get_pack_by_id("nonexistent").unwrap();
        assert!(not_found.is_none());
    }

    #[test]
    fn pack_record_round_trips_router_and_essential() {
        let (store, _tmp) = test_store();
        store
            .insert_pack("p-seo", "mkt-seo", Some("SEO pack"), None, None)
            .unwrap();

        // New fields default to None / false on fresh insert
        let fresh = store.get_pack_by_id("p-seo").unwrap().unwrap();
        assert_eq!(fresh.router_description, None);
        assert_eq!(fresh.router_body, None);
        assert_eq!(fresh.is_essential, false);
        assert_eq!(fresh.router_updated_at, None);

        // Direct column write to confirm they round-trip through SELECT
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "UPDATE packs SET router_description = ?1, router_body = ?2, is_essential = ?3, router_updated_at = ?4 WHERE id = ?5",
                params![
                    "Trigger SEO audit...",
                    Option::<&str>::None,
                    1i32,
                    1_700_000_000i64,
                    "p-seo",
                ],
            )
            .unwrap();
        }
        let fetched = store.get_pack_by_id("p-seo").unwrap().unwrap();
        assert_eq!(
            fetched.router_description.as_deref(),
            Some("Trigger SEO audit...")
        );
        assert_eq!(fetched.router_body, None);
        assert_eq!(fetched.is_essential, true);
        assert_eq!(fetched.router_updated_at, Some(1_700_000_000));
    }

    #[test]
    fn set_pack_router_updates_description_and_timestamp() {
        let (store, _tmp) = test_store();
        store
            .insert_pack("p-seo", "mkt-seo", None, None, None)
            .unwrap();

        store
            .set_pack_router("p-seo", Some("new desc"), None, 1_700_000_500)
            .unwrap();

        let got = store.get_pack_by_id("p-seo").unwrap().unwrap();
        assert_eq!(got.router_description.as_deref(), Some("new desc"));
        assert_eq!(got.router_body, None);
        assert_eq!(got.router_updated_at, Some(1_700_000_500));
    }

    #[test]
    fn set_pack_router_errors_when_pack_missing() {
        let (store, _tmp) = test_store();
        let err = store
            .set_pack_router("nope", Some("x"), None, 1)
            .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn set_pack_essential_toggles_flag() {
        let (store, _tmp) = test_store();
        store
            .insert_pack("p-ess", "essential", None, None, None)
            .unwrap();

        store.set_pack_essential("p-ess", true).unwrap();
        assert_eq!(
            store.get_pack_by_id("p-ess").unwrap().unwrap().is_essential,
            true
        );

        store.set_pack_essential("p-ess", false).unwrap();
        assert_eq!(
            store.get_pack_by_id("p-ess").unwrap().unwrap().is_essential,
            false
        );
    }

    #[test]
    fn set_pack_essential_errors_when_pack_missing() {
        let (store, _tmp) = test_store();
        let err = store.set_pack_essential("nope", true).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn update_pack() {
        let (store, _tmp) = test_store();
        store
            .insert_pack("p1", "Old Name", None, None, None)
            .unwrap();

        store
            .update_pack(
                "p1",
                "New Name",
                Some("New desc"),
                Some("🎯"),
                Some("#00ff00"),
            )
            .unwrap();

        let pack = store.get_pack_by_id("p1").unwrap().unwrap();
        assert_eq!(pack.name, "New Name");
        assert_eq!(pack.description.as_deref(), Some("New desc"));
        assert_eq!(pack.icon.as_deref(), Some("🎯"));
        assert_eq!(pack.color.as_deref(), Some("#00ff00"));
    }

    #[test]
    fn delete_pack() {
        let (store, _tmp) = test_store();
        store
            .insert_pack("p1", "To Delete", None, None, None)
            .unwrap();

        store.delete_pack("p1").unwrap();

        let packs = store.get_all_packs().unwrap();
        assert!(packs.is_empty());
    }

    #[test]
    fn add_and_remove_skill_from_pack() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Skill One");
        insert_test_skill(&store, "s2", "Skill Two");
        store
            .insert_pack("p1", "My Pack", None, None, None)
            .unwrap();

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
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Test Scenario");
        store.insert_pack("p1", "Pack A", None, None, None).unwrap();
        store.insert_pack("p2", "Pack B", None, None, None).unwrap();

        store.add_pack_to_scenario("sc1", "p1").unwrap();
        store.add_pack_to_scenario("sc1", "p2").unwrap();

        let packs = store.get_packs_for_scenario("sc1").unwrap();
        assert_eq!(packs.len(), 2);

        store.remove_pack_from_scenario("sc1", "p1").unwrap();
        let packs = store.get_packs_for_scenario("sc1").unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].id, "p2");
    }

    #[test]
    fn delete_pack_cascades_to_pack_skills() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Skill One");
        store
            .insert_pack("p1", "Cascade Pack", None, None, None)
            .unwrap();
        store.add_skill_to_pack("p1", "s1").unwrap();

        store.delete_pack("p1").unwrap();

        // pack_skills row should be gone via cascade
        let skills = store.get_skills_for_pack("p1").unwrap();
        assert!(skills.is_empty());
    }

    #[test]
    fn insert_pack_auto_sort_order() {
        let (store, _tmp) = test_store();
        store.insert_pack("p1", "First", None, None, None).unwrap();
        store.insert_pack("p2", "Second", None, None, None).unwrap();
        store.insert_pack("p3", "Third", None, None, None).unwrap();

        let packs = store.get_all_packs().unwrap();
        assert_eq!(packs[0].sort_order, 0);
        assert_eq!(packs[1].sort_order, 1);
        assert_eq!(packs[2].sort_order, 2);
    }

    // ── Task 10: Effective Skill Resolution Tests ──

    #[test]
    fn effective_skills_packs_only() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Skill A");
        insert_test_skill(&store, "s2", "Skill B");
        insert_test_scenario(&store, "sc1", "Scenario One");
        store
            .insert_pack("p1", "Pack One", None, None, None)
            .unwrap();
        store.add_skill_to_pack("p1", "s1").unwrap();
        store.add_skill_to_pack("p1", "s2").unwrap();
        store.add_pack_to_scenario("sc1", "p1").unwrap();

        let effective = store.get_effective_skills_for_scenario("sc1").unwrap();
        assert_eq!(effective.len(), 2);
        assert_eq!(effective[0].id, "s1");
        assert_eq!(effective[1].id, "s2");
    }

    #[test]
    fn effective_skills_direct_only_backward_compat() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Skill A");
        insert_test_skill(&store, "s2", "Skill B");
        insert_test_scenario(&store, "sc1", "Scenario One");
        store.add_skill_to_scenario("sc1", "s1").unwrap();
        store.add_skill_to_scenario("sc1", "s2").unwrap();

        let effective = store.get_effective_skills_for_scenario("sc1").unwrap();
        assert_eq!(effective.len(), 2);
        // Direct skills should still work without any packs
        let ids: Vec<&str> = effective.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"s1"));
        assert!(ids.contains(&"s2"));
    }

    #[test]
    fn effective_skills_packs_plus_direct_deduped() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Skill A");
        insert_test_skill(&store, "s2", "Skill B");
        insert_test_scenario(&store, "sc1", "Scenario One");

        // s1 in a pack
        store
            .insert_pack("p1", "Pack One", None, None, None)
            .unwrap();
        store.add_skill_to_pack("p1", "s1").unwrap();
        store.add_pack_to_scenario("sc1", "p1").unwrap();

        // s1 also as a direct skill, plus s2 only direct
        store.add_skill_to_scenario("sc1", "s1").unwrap();
        store.add_skill_to_scenario("sc1", "s2").unwrap();

        let effective = store.get_effective_skills_for_scenario("sc1").unwrap();
        assert_eq!(effective.len(), 2, "s1 should be deduped");
        // Pack skills come first, so s1 should be first (from pack), s2 second (direct)
        assert_eq!(effective[0].id, "s1");
        assert_eq!(effective[1].id, "s2");
    }

    #[test]
    fn effective_skills_duplicate_across_packs() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Shared Skill");
        insert_test_scenario(&store, "sc1", "Scenario One");

        store.insert_pack("p1", "Pack A", None, None, None).unwrap();
        store.insert_pack("p2", "Pack B", None, None, None).unwrap();
        store.add_skill_to_pack("p1", "s1").unwrap();
        store.add_skill_to_pack("p2", "s1").unwrap();
        store.add_pack_to_scenario("sc1", "p1").unwrap();
        store.add_pack_to_scenario("sc1", "p2").unwrap();

        let effective = store.get_effective_skills_for_scenario("sc1").unwrap();
        assert_eq!(
            effective.len(),
            1,
            "same skill in 2 packs should appear once"
        );
        assert_eq!(effective[0].id, "s1");
    }

    #[test]
    fn effective_skills_empty_scenario() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Empty Scenario");

        let effective = store.get_effective_skills_for_scenario("sc1").unwrap();
        assert!(effective.is_empty());
    }

    #[test]
    fn effective_skills_handles_orphaned_skill() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Will Be Deleted");
        insert_test_skill(&store, "s2", "Will Survive");
        insert_test_scenario(&store, "sc1", "Scenario One");

        store
            .insert_pack("p1", "Pack One", None, None, None)
            .unwrap();
        store.add_skill_to_pack("p1", "s1").unwrap();
        store.add_skill_to_pack("p1", "s2").unwrap();
        store.add_pack_to_scenario("sc1", "p1").unwrap();

        // Delete s1 — FK cascade removes pack_skills row, so INNER JOIN excludes it
        store.delete_skill("s1").unwrap();

        let effective = store.get_effective_skills_for_scenario("sc1").unwrap();
        assert_eq!(effective.len(), 1);
        assert_eq!(effective[0].id, "s2");
    }

    #[test]
    fn is_skill_in_effective_scenario_via_pack() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "In Pack");
        insert_test_skill(&store, "s2", "Not In Scenario");
        insert_test_scenario(&store, "sc1", "Scenario");

        store.insert_pack("p1", "Pack", None, None, None).unwrap();
        store.add_skill_to_pack("p1", "s1").unwrap();
        store.add_pack_to_scenario("sc1", "p1").unwrap();

        assert!(store.is_skill_in_effective_scenario("sc1", "s1").unwrap());
        assert!(!store.is_skill_in_effective_scenario("sc1", "s2").unwrap());
    }

    #[test]
    fn is_skill_in_effective_scenario_via_direct() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Direct Skill");
        insert_test_scenario(&store, "sc1", "Scenario");

        store.add_skill_to_scenario("sc1", "s1").unwrap();

        assert!(store.is_skill_in_effective_scenario("sc1", "s1").unwrap());
    }

    #[test]
    fn effective_skill_ids_returns_correct_ids() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Skill A");
        insert_test_skill(&store, "s2", "Skill B");
        insert_test_skill(&store, "s3", "Skill C");
        insert_test_scenario(&store, "sc1", "Scenario");

        // s1, s2 via pack
        store.insert_pack("p1", "Pack", None, None, None).unwrap();
        store.add_skill_to_pack("p1", "s1").unwrap();
        store.add_skill_to_pack("p1", "s2").unwrap();
        store.add_pack_to_scenario("sc1", "p1").unwrap();

        // s2, s3 direct (s2 is duplicate)
        store.add_skill_to_scenario("sc1", "s2").unwrap();
        store.add_skill_to_scenario("sc1", "s3").unwrap();

        let ids = store.get_effective_skill_ids_for_scenario("sc1").unwrap();
        assert_eq!(ids.len(), 3); // s1, s2, s3 — s2 deduped
        assert!(ids.contains(&"s1".to_string()));
        assert!(ids.contains(&"s2".to_string()));
        assert!(ids.contains(&"s3".to_string()));
    }
}

#[cfg(test)]
mod plugin_tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_store() -> (SkillStore, NamedTempFile) {
        let tmp = NamedTempFile::new().unwrap();
        let store = SkillStore::new(&tmp.path().to_path_buf()).unwrap();
        (store, tmp)
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

    fn sample_plugin_record(id: &str, key: &str) -> ManagedPluginRecord {
        ManagedPluginRecord {
            id: id.to_string(),
            plugin_key: key.to_string(),
            display_name: Some(key.split('@').next().unwrap_or(key).to_string()),
            plugin_data: format!(
                r#"[{{"scope":"user","installPath":"/tmp/{key}","version":"1.0.0","installedAt":"2026-01-01T00:00:00Z","lastUpdated":"2026-01-01T00:00:00Z","gitCommitSha":"abc123"}}]"#
            ),
            created_at: 1000,
            updated_at: 1000,
        }
    }

    #[test]
    fn insert_and_get_managed_plugin() {
        let (store, _tmp) = test_store();
        let rec = sample_plugin_record("p1", "superpowers@claude-plugins-official");
        store.insert_managed_plugin(&rec).unwrap();

        let all = store.get_all_managed_plugins().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].plugin_key, "superpowers@claude-plugins-official");
        assert_eq!(all[0].display_name.as_deref(), Some("superpowers"));
    }

    #[test]
    fn get_managed_plugin_by_key() {
        let (store, _tmp) = test_store();
        let rec = sample_plugin_record("p1", "superpowers@claude-plugins-official");
        store.insert_managed_plugin(&rec).unwrap();

        let found = store
            .get_managed_plugin_by_key("superpowers@claude-plugins-official")
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "p1");

        let not_found = store.get_managed_plugin_by_key("nonexistent").unwrap();
        assert!(not_found.is_none());
    }

    #[test]
    fn get_managed_plugin_by_id() {
        let (store, _tmp) = test_store();
        let rec = sample_plugin_record("p1", "superpowers@claude-plugins-official");
        store.insert_managed_plugin(&rec).unwrap();

        let found = store.get_managed_plugin_by_id("p1").unwrap();
        assert!(found.is_some());

        let not_found = store.get_managed_plugin_by_id("nonexistent").unwrap();
        assert!(not_found.is_none());
    }

    #[test]
    fn update_managed_plugin_data() {
        let (store, _tmp) = test_store();
        let rec = sample_plugin_record("p1", "superpowers@claude-plugins-official");
        store.insert_managed_plugin(&rec).unwrap();

        let new_data = r#"[{"scope":"user","installPath":"/tmp/new","version":"2.0.0","installedAt":"2026-01-01T00:00:00Z","lastUpdated":"2026-02-01T00:00:00Z","gitCommitSha":"def456"}]"#;
        store.update_managed_plugin_data("p1", new_data).unwrap();

        let updated = store.get_managed_plugin_by_id("p1").unwrap().unwrap();
        assert!(updated.plugin_data.contains("2.0.0"));
        assert!(updated.updated_at > 1000);
    }

    #[test]
    fn delete_managed_plugin() {
        let (store, _tmp) = test_store();
        let rec = sample_plugin_record("p1", "superpowers@claude-plugins-official");
        store.insert_managed_plugin(&rec).unwrap();

        store.delete_managed_plugin("p1").unwrap();
        let all = store.get_all_managed_plugins().unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn delete_managed_plugin_cascades_to_scenario_plugins() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Test Scenario");
        let rec = sample_plugin_record("p1", "superpowers@claude-plugins-official");
        store.insert_managed_plugin(&rec).unwrap();
        store
            .set_scenario_plugin_enabled("sc1", "p1", false)
            .unwrap();

        store.delete_managed_plugin("p1").unwrap();

        // scenario_plugins row should be gone via cascade
        let scenario_plugins = store.get_scenario_plugins("sc1").unwrap();
        assert!(scenario_plugins.is_empty());
    }

    #[test]
    fn scenario_plugins_default_enabled() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Scenario One");
        let p1 = sample_plugin_record("p1", "superpowers@claude-plugins-official");
        let p2 = sample_plugin_record("p2", "compound-engineering@compound-engineering-plugin");
        store.insert_managed_plugin(&p1).unwrap();
        store.insert_managed_plugin(&p2).unwrap();

        // No scenario_plugins rows — all should be enabled by default
        let plugins = store.get_scenario_plugins("sc1").unwrap();
        assert_eq!(plugins.len(), 2);
        assert!(plugins.iter().all(|p| p.enabled));
    }

    #[test]
    fn set_scenario_plugin_disabled() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Scenario One");
        let p1 = sample_plugin_record("p1", "superpowers@claude-plugins-official");
        let p2 = sample_plugin_record("p2", "compound-engineering@compound-engineering-plugin");
        store.insert_managed_plugin(&p1).unwrap();
        store.insert_managed_plugin(&p2).unwrap();

        store
            .set_scenario_plugin_enabled("sc1", "p1", false)
            .unwrap();

        let plugins = store.get_scenario_plugins("sc1").unwrap();
        let sp = plugins.iter().find(|p| p.plugin.id == "p1").unwrap();
        assert!(!sp.enabled);
        let cp = plugins.iter().find(|p| p.plugin.id == "p2").unwrap();
        assert!(cp.enabled); // still default enabled
    }

    #[test]
    fn get_enabled_plugin_keys() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Scenario One");
        let p1 = sample_plugin_record("p1", "superpowers@claude-plugins-official");
        let p2 = sample_plugin_record("p2", "compound-engineering@compound-engineering-plugin");
        let p3 = sample_plugin_record("p3", "github@claude-plugins-official");
        store.insert_managed_plugin(&p1).unwrap();
        store.insert_managed_plugin(&p2).unwrap();
        store.insert_managed_plugin(&p3).unwrap();

        // Disable p2
        store
            .set_scenario_plugin_enabled("sc1", "p2", false)
            .unwrap();

        let enabled = store.get_enabled_plugin_keys_for_scenario("sc1").unwrap();
        assert_eq!(enabled.len(), 2);
        assert!(enabled.contains(&"superpowers@claude-plugins-official".to_string()));
        assert!(enabled.contains(&"github@claude-plugins-official".to_string()));
        assert!(!enabled.contains(&"compound-engineering@compound-engineering-plugin".to_string()));
    }

    #[test]
    fn get_disabled_plugin_keys() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Scenario One");
        let p1 = sample_plugin_record("p1", "superpowers@claude-plugins-official");
        let p2 = sample_plugin_record("p2", "compound-engineering@compound-engineering-plugin");
        store.insert_managed_plugin(&p1).unwrap();
        store.insert_managed_plugin(&p2).unwrap();

        store
            .set_scenario_plugin_enabled("sc1", "p2", false)
            .unwrap();

        let disabled = store.get_disabled_plugin_keys_for_scenario("sc1").unwrap();
        assert_eq!(disabled.len(), 1);
        assert_eq!(
            disabled[0],
            "compound-engineering@compound-engineering-plugin"
        );
    }

    #[test]
    fn toggle_scenario_plugin_idempotent() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Scenario One");
        let p1 = sample_plugin_record("p1", "superpowers@claude-plugins-official");
        store.insert_managed_plugin(&p1).unwrap();

        // Disable
        store
            .set_scenario_plugin_enabled("sc1", "p1", false)
            .unwrap();
        // Disable again (should be idempotent)
        store
            .set_scenario_plugin_enabled("sc1", "p1", false)
            .unwrap();
        let plugins = store.get_scenario_plugins("sc1").unwrap();
        assert!(!plugins[0].enabled);

        // Re-enable
        store
            .set_scenario_plugin_enabled("sc1", "p1", true)
            .unwrap();
        let plugins = store.get_scenario_plugins("sc1").unwrap();
        assert!(plugins[0].enabled);
    }

    #[test]
    fn different_scenarios_independent_plugin_state() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Minimal");
        insert_test_scenario(&store, "sc2", "Everything");
        let p1 = sample_plugin_record("p1", "superpowers@claude-plugins-official");
        store.insert_managed_plugin(&p1).unwrap();

        // Disable in sc1, leave default (enabled) in sc2
        store
            .set_scenario_plugin_enabled("sc1", "p1", false)
            .unwrap();

        let sc1_plugins = store.get_scenario_plugins("sc1").unwrap();
        assert!(!sc1_plugins[0].enabled);

        let sc2_plugins = store.get_scenario_plugins("sc2").unwrap();
        assert!(sc2_plugins[0].enabled);
    }
}

#[cfg(test)]
mod agent_tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn test_store() -> (SkillStore, NamedTempFile) {
        let tmp = NamedTempFile::new().unwrap();
        let store = SkillStore::new(&tmp.path().to_path_buf()).unwrap();
        (store, tmp)
    }

    fn insert_test_skill(store: &SkillStore, id: &str, name: &str) -> SkillRecord {
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
        rec
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
    fn set_and_get_agent_config() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Scenario One");

        store.set_agent_scenario("claude", "sc1").unwrap();

        let config = store.get_agent_config("claude").unwrap().unwrap();
        assert_eq!(config.tool_key, "claude");
        assert_eq!(config.scenario_id.as_deref(), Some("sc1"));
        assert!(config.managed);
        assert!(config.updated_at > 0);

        // Non-existent agent returns None
        let none = store.get_agent_config("nonexistent").unwrap();
        assert!(none.is_none());
    }

    #[test]
    fn get_all_agent_configs() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Scenario One");
        insert_test_scenario(&store, "sc2", "Scenario Two");

        store.set_agent_scenario("claude", "sc1").unwrap();
        store.set_agent_scenario("windsurf", "sc2").unwrap();

        let configs = store.get_all_agent_configs().unwrap();
        assert_eq!(configs.len(), 2);
        // Ordered by tool_key
        assert_eq!(configs[0].tool_key, "claude");
        assert_eq!(configs[1].tool_key, "windsurf");
    }

    #[test]
    fn set_agent_managed() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Scenario One");

        store.set_agent_scenario("claude", "sc1").unwrap();

        // Default is managed=true
        let config = store.get_agent_config("claude").unwrap().unwrap();
        assert!(config.managed);

        // Set to unmanaged
        store.set_agent_managed("claude", false).unwrap();
        let config = store.get_agent_config("claude").unwrap().unwrap();
        assert!(!config.managed);

        // Set back to managed
        store.set_agent_managed("claude", true).unwrap();
        let config = store.get_agent_config("claude").unwrap().unwrap();
        assert!(config.managed);
    }

    #[test]
    fn init_agent_configs_seeds_from_active_scenario() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Active Scenario");
        store.set_active_scenario("sc1").unwrap();

        let tool_keys = vec![
            "claude".to_string(),
            "windsurf".to_string(),
            "cursor".to_string(),
        ];
        store.init_agent_configs(&tool_keys).unwrap();

        let configs = store.get_all_agent_configs().unwrap();
        assert_eq!(configs.len(), 3);
        for config in &configs {
            assert_eq!(config.scenario_id.as_deref(), Some("sc1"));
            assert!(config.managed);
        }

        // Calling again should not overwrite existing rows (INSERT OR IGNORE)
        insert_test_scenario(&store, "sc2", "New Active");
        store.set_active_scenario("sc2").unwrap();
        store.init_agent_configs(&tool_keys).unwrap();

        let configs = store.get_all_agent_configs().unwrap();
        assert_eq!(configs.len(), 3);
        // All should still point to sc1 (not overwritten)
        for config in &configs {
            assert_eq!(config.scenario_id.as_deref(), Some("sc1"));
        }
    }

    #[test]
    fn add_and_remove_agent_extra_pack() {
        let (store, _tmp) = test_store();
        store
            .insert_pack("p1", "Pack One", None, None, None)
            .unwrap();
        store
            .insert_pack("p2", "Pack Two", None, None, None)
            .unwrap();

        store.add_agent_extra_pack("claude", "p1").unwrap();
        store.add_agent_extra_pack("claude", "p2").unwrap();

        let packs = store.get_agent_extra_packs("claude").unwrap();
        assert_eq!(packs.len(), 2);

        store.remove_agent_extra_pack("claude", "p1").unwrap();
        let packs = store.get_agent_extra_packs("claude").unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].id, "p2");
    }

    #[test]
    fn effective_skills_for_agent_scenario_only() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Skill A");
        insert_test_skill(&store, "s2", "Skill B");
        insert_test_scenario(&store, "sc1", "Scenario One");

        // Set up scenario with skills via pack
        store
            .insert_pack("p1", "Pack One", None, None, None)
            .unwrap();
        store.add_skill_to_pack("p1", "s1").unwrap();
        store.add_skill_to_pack("p1", "s2").unwrap();
        store.add_pack_to_scenario("sc1", "p1").unwrap();

        // Assign agent to scenario
        store.set_agent_scenario("claude", "sc1").unwrap();

        let effective = store.get_effective_skills_for_agent("claude").unwrap();
        assert_eq!(effective.len(), 2);
        assert_eq!(effective[0].id, "s1");
        assert_eq!(effective[1].id, "s2");
    }

    #[test]
    fn effective_skills_for_agent_with_extra_pack() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Skill A");
        insert_test_skill(&store, "s2", "Skill B");
        insert_test_skill(&store, "s3", "Skill C");
        insert_test_scenario(&store, "sc1", "Scenario One");

        // Scenario has s1 via pack
        store
            .insert_pack("p1", "Base Pack", None, None, None)
            .unwrap();
        store.add_skill_to_pack("p1", "s1").unwrap();
        store.add_pack_to_scenario("sc1", "p1").unwrap();

        // Extra pack has s2 and s3
        store
            .insert_pack("p2", "Extra Pack", None, None, None)
            .unwrap();
        store.add_skill_to_pack("p2", "s2").unwrap();
        store.add_skill_to_pack("p2", "s3").unwrap();

        // Assign agent to scenario + extra pack
        store.set_agent_scenario("claude", "sc1").unwrap();
        store.add_agent_extra_pack("claude", "p2").unwrap();

        let effective = store.get_effective_skills_for_agent("claude").unwrap();
        assert_eq!(effective.len(), 3);
        // Scenario skills first, then extra pack skills
        assert_eq!(effective[0].id, "s1");
        assert_eq!(effective[1].id, "s2");
        assert_eq!(effective[2].id, "s3");
    }

    #[test]
    fn effective_skills_for_agent_deduped() {
        let (store, _tmp) = test_store();
        insert_test_skill(&store, "s1", "Shared Skill");
        insert_test_skill(&store, "s2", "Extra Only");
        insert_test_scenario(&store, "sc1", "Scenario One");

        // s1 in scenario's pack
        store
            .insert_pack("p1", "Base Pack", None, None, None)
            .unwrap();
        store.add_skill_to_pack("p1", "s1").unwrap();
        store.add_pack_to_scenario("sc1", "p1").unwrap();

        // s1 also in extra pack (duplicate), plus s2
        store
            .insert_pack("p2", "Extra Pack", None, None, None)
            .unwrap();
        store.add_skill_to_pack("p2", "s1").unwrap();
        store.add_skill_to_pack("p2", "s2").unwrap();

        store.set_agent_scenario("claude", "sc1").unwrap();
        store.add_agent_extra_pack("claude", "p2").unwrap();

        let effective = store.get_effective_skills_for_agent("claude").unwrap();
        assert_eq!(effective.len(), 2, "s1 should be deduped");
        assert_eq!(effective[0].id, "s1");
        assert_eq!(effective[1].id, "s2");
    }

    #[test]
    fn unmanaged_agent_config() {
        let (store, _tmp) = test_store();
        insert_test_scenario(&store, "sc1", "Scenario One");

        store.set_agent_scenario("claude", "sc1").unwrap();
        store.set_agent_managed("claude", false).unwrap();

        let config = store.get_agent_config("claude").unwrap().unwrap();
        assert!(!config.managed);
        assert_eq!(config.scenario_id.as_deref(), Some("sc1"));

        // Changing scenario preserves managed=false
        insert_test_scenario(&store, "sc2", "Scenario Two");
        store.set_agent_scenario("claude", "sc2").unwrap();

        let config = store.get_agent_config("claude").unwrap().unwrap();
        assert!(!config.managed);
        assert_eq!(config.scenario_id.as_deref(), Some("sc2"));
    }
}
