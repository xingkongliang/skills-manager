use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};

use super::{
    central_repo, scenario_service, skill_metadata, skill_store::SkillStore, sync_engine,
    sync_metadata, tool_adapters, tool_service,
};

/// Per-stage timings collected during `initialize_store`. The struct is
/// returned to the caller so the log lines can be emitted once
/// `tauri_plugin_log` is registered — anything logged from inside this
/// function would otherwise be dropped because the logger isn't installed
/// until later in `tauri::Builder::setup`. See issue #153.
#[derive(Debug, Clone)]
pub struct StartupTimings {
    pub ensure_central_repo_ms: u128,
    pub open_store_ms: u128,
    pub migrate_legacy_tool_keys_ms: u128,
    pub skill_count: usize,
    pub reindex_from_metadata_ms: Option<u128>,
    pub restore_sync_included_ms: u128,
    pub restore_sync_included_changed: bool,
    pub write_all_from_db_ms: Option<u128>,
    pub apply_scenario_ms: u128,
    /// "default_startup" (Tauri app) or "cli" (CLI bin). Defaults to
    /// `"unknown"` so a struct that escapes `initialize_store_inner`
    /// without being fully populated still produces an obvious value in
    /// the log instead of an empty string.
    pub apply_scenario_kind: &'static str,
    pub total_ms: u128,
}

impl Default for StartupTimings {
    fn default() -> Self {
        Self {
            ensure_central_repo_ms: 0,
            open_store_ms: 0,
            migrate_legacy_tool_keys_ms: 0,
            skill_count: 0,
            reindex_from_metadata_ms: None,
            restore_sync_included_ms: 0,
            restore_sync_included_changed: false,
            write_all_from_db_ms: None,
            apply_scenario_ms: 0,
            apply_scenario_kind: "unknown",
            total_ms: 0,
        }
    }
}

pub fn initialize_store() -> Result<(Arc<SkillStore>, StartupTimings)> {
    initialize_store_inner(true, true)
}

pub fn initialize_cli_store() -> Result<Arc<SkillStore>> {
    initialize_store_inner(false, false).map(|(store, _)| store)
}

fn reindex_or_rebuild_metadata(store: &SkillStore) -> Result<()> {
    let Err(err) = sync_metadata::reindex_from_metadata(store) else {
        return Ok(());
    };
    // #421: empty skill metadata used to abort every launch. When the
    // database still holds a library whose directories all exist, it is the
    // better record: rebuild the metadata from it. Otherwise keep refusing —
    // starting with an empty library would let the auto backup commit that
    // emptiness.
    if err.is::<sync_metadata::EmptySkillMetadata>() && db_library_is_intact(store) {
        central_repo::record_startup_error(format!(
            "sync metadata: {err}; rebuilt it from the database"
        ));
        return sync_metadata::write_all_from_db(store)
            .context("Failed to rebuild sync metadata from the database");
    }
    Err(err.context("Failed to reindex from sync metadata"))
}

/// The database can stand in for empty metadata only if it covers the whole
/// library: non-empty, every record's directory exists, and every skill
/// directory in the central repo has a record.
fn db_library_is_intact(store: &SkillStore) -> bool {
    let Ok(skills) = store.get_all_skills() else {
        return false;
    };
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let mut recorded = std::collections::HashSet::new();
    for skill in &skills {
        let path = Path::new(&skill.central_path);
        if !path.is_dir() {
            return false;
        }
        recorded.insert(canon(path));
    }
    if recorded.is_empty() {
        return false;
    }
    let mut walk = walkdir::WalkDir::new(central_repo::skills_dir())
        .min_depth(1)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            name != ".git" && name != ".skills-manager"
        });
    while let Some(entry) = walk.next() {
        let Ok(entry) = entry else {
            return false;
        };
        if entry.file_type().is_dir() && skill_metadata::is_valid_skill_dir(entry.path()) {
            if !recorded.contains(&canon(entry.path())) {
                return false;
            }
            // A skill's own subfolders are its content, not more skills.
            walk.skip_current_dir();
        }
    }
    true
}

/// For CLI `repo set-path` / `reset-path`: also carry out the move now, unless
/// the app is running (it then moves at its next launch).
pub fn initialize_cli_store_moving_repo() -> Result<Arc<SkillStore>> {
    initialize_store_inner(false, true).map(|(store, _)| store)
}

fn initialize_store_inner(
    apply_startup_default: bool,
    allow_migration: bool,
) -> Result<(Arc<SkillStore>, StartupTimings)> {
    let total_start = Instant::now();
    let mut timings = StartupTimings::default();

    let step = Instant::now();
    central_repo::ensure_central_repo(allow_migration).context("Failed to create central repo")?;
    timings.ensure_central_repo_ms = step.elapsed().as_millis();

    let db_path = central_repo::db_path();
    let step = Instant::now();
    let store = Arc::new(SkillStore::new(&db_path).context("Failed to initialize database")?);
    timings.open_store_ms = step.elapsed().as_millis();

    let step = Instant::now();
    tool_service::migrate_legacy_tool_keys(&store)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("Failed to migrate legacy tool keys")?;
    timings.migrate_legacy_tool_keys_ms = step.elapsed().as_millis();

    if allow_migration {
        if let Some((from, to)) = central_repo::take_repoint_from() {
            // Never fatal: the library itself is intact at `to`. Keep the
            // marker while anything failed, so the next launch retries.
            let done = match repoint_after_move(&store, &from, &to) {
                Ok(failures) => failures == 0,
                Err(err) => {
                    central_repo::record_startup_error(format!(
                        "central repo: repointing paths after the move failed ({err:#})"
                    ));
                    false
                }
            };
            if done {
                if let Err(err) = central_repo::clear_repoint_from() {
                    central_repo::record_startup_error(format!(
                        "central repo: cannot clear the repoint marker ({err:#}); it will rerun"
                    ));
                }
            }
        }
    }

    timings.skill_count = store.get_all_skills().map(|s| s.len()).unwrap_or(0);

    if sync_metadata::metadata_exists() {
        let step = Instant::now();
        reindex_or_rebuild_metadata(&store)?;
        timings.reindex_from_metadata_ms = Some(step.elapsed().as_millis());
    }

    let step = Instant::now();
    let changed = scenario_service::restore_all_skills_sync_included(&store)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("Failed to restore skill sync inclusion")?;
    timings.restore_sync_included_ms = step.elapsed().as_millis();
    timings.restore_sync_included_changed = changed;
    if changed {
        let step = Instant::now();
        sync_metadata::write_all_from_db(&store)
            .context("Failed to persist restored skill sync inclusion")?;
        timings.write_all_from_db_ms = Some(step.elapsed().as_millis());
    }

    let step = Instant::now();
    if apply_startup_default {
        scenario_service::ensure_default_startup_scenario(&store)
            .map_err(|e| anyhow::anyhow!(e.to_string()))
            .context("Failed to initialize startup scenario")?;
        timings.apply_scenario_kind = "default_startup";
        // Deployments recorded in a mode an agent no longer gets, such as
        // Antigravity's Windows links from before #653, are converted here;
        // a no-op when every record matches. Never fatal, and whatever failed
        // is still off-mode at the next launch, so it is retried.
        match scenario_service::resync_targets_to_sync_mode(&store) {
            Ok(0) => {}
            Ok(converted) => log::info!("startup: converted {converted} deployment(s) to their sync mode"),
            Err(err) => central_repo::record_startup_error(format!(
                "sync mode: some deployments could not be converted ({err})"
            )),
        }
    } else {
        scenario_service::ensure_cli_scenario_state(&store)
            .map_err(|e| anyhow::anyhow!(e.to_string()))
            .context("Failed to initialize CLI scenario state")?;
        timings.apply_scenario_kind = "cli";
    }
    timings.apply_scenario_ms = step.elapsed().as_millis();

    timings.total_ms = total_start.elapsed().as_millis();
    Ok((store, timings))
}

impl StartupTimings {
    /// Emit a single human-readable log block from the captured timings.
    /// Called from `tauri::Builder::setup` once `tauri_plugin_log` is
    /// installed; calling it before that point would lose the output to
    /// the no-op default logger.
    pub fn log(&self) {
        log::info!(
            "startup: initialize_store total {} ms (skills={})",
            self.total_ms,
            self.skill_count
        );
        log::info!(
            "startup: ensure_central_repo {} ms, open_store {} ms, migrate_legacy_tool_keys {} ms",
            self.ensure_central_repo_ms,
            self.open_store_ms,
            self.migrate_legacy_tool_keys_ms
        );
        if let Some(ms) = self.reindex_from_metadata_ms {
            log::info!(
                "startup: reindex_from_metadata {} ms (skills={})",
                ms,
                self.skill_count
            );
        }
        if self.restore_sync_included_changed {
            log::info!(
                "startup: restore_sync_included changed in {} ms, write_all_from_db {} ms",
                self.restore_sync_included_ms,
                self.write_all_from_db_ms.unwrap_or(0)
            );
        } else {
            log::info!(
                "startup: restore_sync_included no-op in {} ms",
                self.restore_sync_included_ms
            );
        }
        log::info!(
            "startup: apply_scenario ({}) {} ms (skills={})",
            self.apply_scenario_kind,
            self.apply_scenario_ms,
            self.skill_count
        );
    }
}

/// After the library moved, point what still names the old location at the new
/// one: DB paths, and the symlinks deployed into agent and project skills
/// directories. Project deployments have no target records and startup sync
/// only covers the active preset, so neither heals on its own. Only links that
/// resolve into the old library are touched — never anything else in there.
/// Returns how many links could not be inspected or repointed.
fn repoint_after_move(store: &SkillStore, from: &Path, to: &Path) -> Result<usize> {
    let rebase = |path: &str| -> Option<String> {
        let rel = Path::new(path).strip_prefix(from).ok()?;
        Some(to.join(rel).to_string_lossy().to_string())
    };
    for mut skill in store.get_all_skills()? {
        let mut changed = false;
        if let Some(path) = rebase(&skill.central_path) {
            skill.central_path = path;
            changed = true;
        }
        for field in [&mut skill.source_ref, &mut skill.source_ref_resolved] {
            if let Some(path) = field.as_deref().and_then(|p| rebase(p)) {
                *field = Some(path);
                changed = true;
            }
        }
        if changed {
            store.upsert_skill(&skill)?;
        }
    }

    // Recorded deployments by exact path (nested ones included), then every
    // entry of the agent and project skills roots, which also covers project
    // deployments — those have no records.
    let mut links: Vec<PathBuf> = store
        .get_all_targets()?
        .into_iter()
        .filter(|target| target.mode == "symlink")
        .map(|target| PathBuf::from(target.target_path))
        .collect();
    let adapters = tool_adapters::all_tool_adapters(store);
    let mut roots: Vec<PathBuf> = adapters.iter().map(|a| a.skills_dir()).collect();
    for project in store.get_all_projects()? {
        if project.workspace_type == "linked" {
            roots.push(PathBuf::from(&project.path));
            roots.extend(project.disabled_path.map(PathBuf::from));
            continue;
        }
        for adapter in &adapters {
            let dir = adapter.project_relative_skills_dir();
            if !dir.is_empty() {
                roots.push(Path::new(&project.path).join(dir));
                roots.push(Path::new(&project.path).join(format!("{dir}-disabled")));
            }
        }
    }
    // Skills can sit in category subdirectories (Hermes, and the project
    // scanner recurses too), so walk a few levels without following links.
    let mut failures = 0;
    for root in roots {
        match root.try_exists() {
            Ok(true) => {}
            Ok(false) => continue,
            Err(err) => {
                failures += 1;
                central_repo::record_startup_error(format!(
                    "central repo: cannot check {} to repoint links ({err})",
                    root.display()
                ));
                continue;
            }
        }
        for entry in walkdir::WalkDir::new(&root).min_depth(1).max_depth(4) {
            match entry {
                Ok(entry) if entry.path_is_symlink() => links.push(entry.into_path()),
                Ok(_) => {}
                Err(err) => {
                    failures += 1;
                    central_repo::record_startup_error(format!(
                        "central repo: cannot scan {} to repoint links ({err})",
                        root.display()
                    ));
                }
            }
        }
    }
    links.sort();
    links.dedup();

    for link in links {
        let pointee = match std::fs::symlink_metadata(&link) {
            Ok(meta) if meta.file_type().is_symlink() => std::fs::read_link(&link),
            Ok(_) => continue,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => Err(err),
        };
        let moved = pointee.and_then(|pointee| {
            let Ok(rel) = pointee.strip_prefix(from) else {
                return Ok(None);
            };
            let moved = to.join(rel);
            Ok(moved.try_exists()?.then_some(moved))
        });
        let moved = match moved {
            Ok(Some(moved)) => moved,
            Ok(None) => continue,
            Err(err) => {
                failures += 1;
                central_repo::record_startup_error(format!(
                    "central repo: cannot inspect {} to repoint it ({err})",
                    link.display()
                ));
                continue;
            }
        };
        if let Err(err) = sync_engine::sync_skill(
            &moved,
            &link,
            sync_engine::SyncMode::Symlink,
            sync_engine::ReplacePolicy::Recorded { mode: "symlink" },
        ) {
            failures += 1;
            central_repo::record_startup_error(format!(
                "central repo: cannot repoint {} to {} ({err:#})",
                link.display(),
                moved.display()
            ));
        }
    }
    Ok(failures)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::skill_store::{ProjectRecord, SkillRecord, SkillTargetRecord};


    fn skill_at(id: &str, central_path: &Path) -> SkillRecord {
        SkillRecord {
            id: id.into(),
            name: id.into(),
            description: None,
            source_type: "local".into(),
            source_ref: None,
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: central_path.to_string_lossy().into(),
            content_hash: None,
            enabled: true,
            created_at: 0,
            updated_at: 0,
            status: "ok".into(),
            update_status: "unknown".into(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    /// Central repo with one skill dir and metadata whose skills/ is empty (#421).
    fn library_with_empty_metadata() -> (
        std::sync::MutexGuard<'static, ()>,
        tempfile::TempDir,
        SkillStore,
        PathBuf,
    ) {
        let lock = central_repo::test_base_dir_lock();
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("repo");
        central_repo::set_test_base_dir_override(Some(base.clone()));
        let skill_dir = central_repo::skills_dir().join("s");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "---\nname: s\n---\n").unwrap();
        std::fs::create_dir_all(sync_metadata::metadata_dir().join("skills")).unwrap();
        std::fs::write(sync_metadata::metadata_dir().join("schema.json"), "{}").unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();
        (lock, tmp, store, skill_dir)
    }

    #[test]
    fn empty_metadata_is_rebuilt_from_an_intact_database() {
        let (_lock, _tmp, store, skill_dir) = library_with_empty_metadata();
        store.insert_skill(&skill_at("s", &skill_dir)).unwrap();

        reindex_or_rebuild_metadata(&store).unwrap();

        assert!(store.get_skill_by_id("s").unwrap().is_some());
        let written = std::fs::read_dir(sync_metadata::metadata_dir().join("skills"))
            .unwrap()
            .count();
        assert_eq!(written, 1, "metadata rebuilt from the database");
        central_repo::set_test_base_dir_override(None);
    }

    #[test]
    fn empty_metadata_still_refuses_when_the_database_is_empty_or_stale() {
        let (_lock, tmp, store, _skill_dir) = library_with_empty_metadata();
        let err = reindex_or_rebuild_metadata(&store).unwrap_err();
        assert!(err.is::<sync_metadata::EmptySkillMetadata>());

        store
            .insert_skill(&skill_at("gone", &tmp.path().join("missing")))
            .unwrap();
        assert!(reindex_or_rebuild_metadata(&store).is_err());
        central_repo::set_test_base_dir_override(None);
    }

    #[test]
    fn empty_metadata_still_refuses_when_the_database_misses_a_skill_dir() {
        let (_lock, _tmp, store, skill_dir) = library_with_empty_metadata();
        store.insert_skill(&skill_at("s", &skill_dir)).unwrap();
        let unrecorded = central_repo::skills_dir().join("cat/b");
        std::fs::create_dir_all(&unrecorded).unwrap();
        std::fs::write(unrecorded.join("SKILL.md"), "---\nname: b\n---\n").unwrap();

        let err = reindex_or_rebuild_metadata(&store).unwrap_err();
        assert!(err.is::<sync_metadata::EmptySkillMetadata>());
        assert!(store.get_skill_by_id("s").unwrap().is_some());
        central_repo::set_test_base_dir_override(None);
    }

    #[test]
    #[cfg(unix)]
    fn repoint_after_move_rewrites_db_paths_and_only_links_into_the_old_library() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("old");
        let to = tmp.path().join("new");
        std::fs::create_dir_all(to.join("skills/s")).unwrap();
        std::fs::create_dir_all(to.join("skills/cat/nested")).unwrap();
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();

        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        store
            .insert_skill(&SkillRecord {
                id: "s".into(),
                name: "s".into(),
                description: None,
                source_type: "local".into(),
                source_ref: Some(from.join("skills/s").to_string_lossy().into()),
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                central_path: from.join("skills/s").to_string_lossy().into(),
                content_hash: None,
                enabled: true,
                created_at: 0,
                updated_at: 0,
                status: "ok".into(),
                update_status: "unknown".into(),
                last_checked_at: None,
                last_check_error: None,
            })
            .unwrap();

        // A project deployment: no target record, so only this pass can fix it.
        let project_root = tmp.path().join("project-skills");
        std::fs::create_dir_all(&project_root).unwrap();
        std::os::unix::fs::symlink(from.join("skills/s"), project_root.join("s")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, project_root.join("foreign")).unwrap();
        // Nested and unrecorded: a category dir inside the project root.
        std::fs::create_dir_all(project_root.join("cat")).unwrap();
        std::os::unix::fs::symlink(from.join("skills/cat/nested"), project_root.join("cat/nested"))
            .unwrap();
        store
            .insert_project(&ProjectRecord {
                id: "p".into(),
                name: "p".into(),
                path: project_root.to_string_lossy().into(),
                workspace_type: "linked".into(),
                linked_agent_key: Some("claude_code".into()),
                linked_agent_name: None,
                disabled_path: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();

        // A nested recorded deployment (Hermes-style category dir), outside
        // any scanned root and not in the active preset.
        let nested = tmp.path().join("agent/cat/nested");
        std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(from.join("skills/cat/nested"), &nested).unwrap();
        store
            .insert_target(&SkillTargetRecord {
                id: "t".into(),
                skill_id: "s".into(),
                tool: "hermes".into(),
                target_path: nested.to_string_lossy().into(),
                mode: "symlink".into(),
                status: "ok".into(),
                synced_at: None,
                last_error: None,
                source_hash: None,
            })
            .unwrap();

        assert_eq!(repoint_after_move(&store, &from, &to).unwrap(), 0);
        assert_eq!(
            std::fs::read_link(&nested).unwrap(),
            to.join("skills/cat/nested")
        );

        assert_eq!(
            std::fs::read_link(project_root.join("s")).unwrap(),
            to.join("skills/s")
        );
        assert_eq!(
            std::fs::read_link(project_root.join("cat/nested")).unwrap(),
            to.join("skills/cat/nested")
        );
        assert_eq!(
            std::fs::read_link(project_root.join("foreign")).unwrap(),
            elsewhere,
            "a link that does not point into the old library is not ours to touch"
        );
        let skill = store.get_all_skills().unwrap().remove(0);
        assert_eq!(skill.central_path, to.join("skills/s").to_string_lossy());
        assert_eq!(
            skill.source_ref.as_deref(),
            Some(to.join("skills/s").to_string_lossy().as_ref())
        );
    }
}
