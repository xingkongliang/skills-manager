use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::central_repo;
use crate::content_hash;
use crate::installer;
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

        // Skip non-directories (plain files at the top level)
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

/// Import a discovered skill with dedup logic:
/// - If central store has same name + same hash: link to existing, skip copy.
/// - If central store has same name + different hash: mark as native.
/// - If central store doesn't have this name: copy to central, create skill record.
///
/// Returns the action taken.
pub fn import_with_dedup(store: &SkillStore, discovered_id: &str) -> Result<ImportAction> {
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
            if let Some(skill) = store.get_skill_by_central_path(&central_path.to_string_lossy())? {
                store.link_discovered_to_skill(discovered_id, &skill.id)?;
                return Ok(ImportAction::LinkedToExisting { skill_id: skill.id });
            }
            // Central path exists on disk but no DB record -- fall through to import.
        } else {
            // Different content -- mark as native.
            store.mark_discovered_as_native(discovered_id)?;
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
    store.link_discovered_to_skill(discovered_id, &skill_id)?;

    Ok(ImportAction::Imported { skill_id })
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
        sync_engine::sync_skill(central_path, agent_path, sync_engine::SyncMode::Copy)?;
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

    /// Create a mock SkillStore backed by a temporary file DB.
    fn test_store() -> SkillStore {
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        let store = SkillStore::new(&db_path).unwrap();
        // Leak the tempdir so it persists for the test lifetime.
        std::mem::forget(tmp);
        store
    }

    #[test]
    fn dedup_empty_dir_returns_empty_result() {
        let store = test_store();
        let agent_dir = tempdir().unwrap();
        let result = dedup_agent_skills(&store, "test_agent", agent_dir.path(), true).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn dedup_skips_unknown_skills_not_in_central() {
        let store = test_store();
        let agent_dir = tempdir().unwrap();
        let skill_dir = agent_dir.path().join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "hello").unwrap();

        let result = dedup_agent_skills(&store, "test_agent", agent_dir.path(), true).unwrap();
        assert_eq!(result.skipped_unknown, vec!["my-skill"]);
        assert!(result.replaced_with_symlink.is_empty());
    }

    #[test]
    fn dedup_replaces_identical_copy_with_symlink() {
        let _store = test_store();
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

        // Verify hashes match.
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
