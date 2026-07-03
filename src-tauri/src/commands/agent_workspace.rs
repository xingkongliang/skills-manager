use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::State;

use crate::commands::projects::{
    classify_sync_status, ensure_dir_within_root, ensure_safe_skill_relative_path,
    source_ref_matches_skill_path, ProjectSkillDocumentDto,
};
use crate::core::skill_store::{SkillRecord, SkillStore, SkillTargetRecord};
use crate::core::{
    error::AppError, installer, project_scanner, scenario_service, sync_engine, tool_adapters,
    tool_service,
};

fn target_path_equals_skill(target_path: &str, skill_path: &str) -> bool {
    if target_path == skill_path {
        return true;
    }
    match (
        std::fs::canonicalize(target_path),
        std::fs::canonicalize(skill_path),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn adapter_for_agent(
    store: &SkillStore,
    agent: &str,
) -> Result<tool_adapters::ToolAdapter, AppError> {
    tool_adapters::all_tool_adapters(store)
        .into_iter()
        .find(|adapter| adapter.key == agent)
        .ok_or_else(|| AppError::not_found(format!("Unknown agent: {}", agent)))
}

fn read_agent_local_skills(
    adapter: &tool_adapters::ToolAdapter,
) -> Vec<project_scanner::ProjectSkillInfo> {
    project_scanner::read_linked_workspace_skills(
        &adapter.skills_dir(),
        None,
        &adapter.key,
        &adapter.display_name,
        adapter.recursive_scan,
    )
}

fn enrich_center_status(
    mut skills: Vec<project_scanner::ProjectSkillInfo>,
    all_managed: &[SkillRecord],
    all_targets: &[SkillTargetRecord],
    tags_map: &std::collections::HashMap<String, Vec<String>>,
) -> Vec<project_scanner::ProjectSkillInfo> {
    for skill in &mut skills {
        let matched = find_verified_center_match(skill, all_managed, all_targets);
        skill.in_center = matched.is_some();
        skill.center_skill_id = matched.map(|record| record.id.clone());
        skill.tags = skill
            .center_skill_id
            .as_ref()
            .and_then(|skill_id| tags_map.get(skill_id).cloned())
            .unwrap_or_default();
        skill.sync_status = classify_sync_status(skill, matched);
    }
    skills
}

fn find_agent_skill(
    adapter: &tool_adapters::ToolAdapter,
    skill_relative_path: &str,
) -> Result<project_scanner::ProjectSkillInfo, AppError> {
    ensure_safe_skill_relative_path(skill_relative_path)?;
    read_agent_local_skills(adapter)
        .into_iter()
        .find(|skill| skill.relative_path == skill_relative_path)
        .ok_or_else(|| AppError::not_found("Skill not found in agent local directory"))
}

fn ensure_agent_skill_path(path: &Path, skills_root: &Path) -> Result<(), AppError> {
    ensure_dir_within_root(path, skills_root)?;
    Ok(())
}

fn path_matches_skill_path(
    skill_path: &str,
    skill_canonical: Option<&PathBuf>,
    other: &str,
) -> bool {
    if other == skill_path {
        return true;
    }
    let Some(skill_canonical) = skill_canonical else {
        return false;
    };
    let Ok(other_canonical) = std::fs::canonicalize(other) else {
        return false;
    };
    other_canonical == *skill_canonical
}

fn target_matches_skill_path(
    target: &SkillTargetRecord,
    skill_path: &str,
    skill_canonical: Option<&PathBuf>,
) -> bool {
    path_matches_skill_path(skill_path, skill_canonical, &target.target_path)
}

fn find_verified_center_match<'a>(
    skill: &project_scanner::ProjectSkillInfo,
    all_managed: &'a [SkillRecord],
    all_targets: &[SkillTargetRecord],
) -> Option<&'a SkillRecord> {
    let skill_hash = skill.content_hash.as_deref();
    let canonical_skill_path = std::fs::canonicalize(&skill.path).ok();

    all_managed
        .iter()
        .filter_map(|managed| {
            if source_ref_matches_skill_path(&skill.path, canonical_skill_path.as_ref(), managed) {
                return Some((managed, 3));
            }
            if all_targets.iter().any(|target| {
                target.skill_id == managed.id
                    && target_matches_skill_path(target, &skill.path, canonical_skill_path.as_ref())
            }) {
                return Some((managed, 3));
            }
            if skill_hash.is_some() && managed.content_hash.as_deref() == skill_hash {
                return Some((managed, 2));
            }
            None
        })
        .max_by_key(|(_, score)| *score)
        .map(|(managed, _)| managed)
}

#[tauri::command]
pub async fn get_global_local_skills(
    store: State<'_, Arc<SkillStore>>,
    agent: String,
) -> Result<Vec<project_scanner::ProjectSkillInfo>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let adapter = adapter_for_agent(&store, &agent)?;
        let skills = read_agent_local_skills(&adapter);
        let all_managed = store.get_all_skills().map_err(AppError::db)?;
        let all_targets = store.get_all_targets().map_err(AppError::db)?;
        let tags_map = store.get_tags_map().unwrap_or_default();
        Ok(enrich_center_status(
            skills,
            &all_managed,
            &all_targets,
            &tags_map,
        ))
    })
    .await?
}

#[tauri::command]
pub async fn get_global_local_skill_document(
    store: State<'_, Arc<SkillStore>>,
    agent: String,
    skill_relative_path: String,
) -> Result<ProjectSkillDocumentDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let adapter = adapter_for_agent(&store, &agent)?;
        ensure_safe_skill_relative_path(&skill_relative_path)?;

        let skills_root = adapter.skills_dir();
        let skill_dir = skills_root.join(&skill_relative_path);
        ensure_agent_skill_path(&skill_dir, &skills_root)?;

        let allowed_roots = vec![skills_root];
        let candidates = ["SKILL.md", "skill.md", "CLAUDE.md", "README.md"];
        for candidate in &candidates {
            let file_path = skill_dir.join(candidate);
            if !file_path.exists() {
                continue;
            }
            if let Ok(meta) = std::fs::symlink_metadata(&file_path) {
                if meta.file_type().is_symlink() {
                    let resolved = match std::fs::canonicalize(&file_path) {
                        Ok(path) => path,
                        Err(_) => continue,
                    };
                    let in_allowed_root = allowed_roots.iter().any(|root| {
                        std::fs::canonicalize(root)
                            .map(|canon| resolved.starts_with(&canon))
                            .unwrap_or(false)
                    });
                    if !in_allowed_root {
                        continue;
                    }
                }
            }
            if file_path.is_file() {
                let content = std::fs::read_to_string(&file_path)?;
                return Ok(ProjectSkillDocumentDto {
                    skill_name: skill_relative_path,
                    filename: candidate.to_string(),
                    content,
                });
            }
        }

        Err(AppError::not_found(
            "No document file found in skill directory",
        ))
    })
    .await?
}

#[tauri::command]
pub async fn import_global_local_skill_to_center(
    store: State<'_, Arc<SkillStore>>,
    agent: String,
    skill_relative_path: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        import_agent_local_skill_to_center(&store, &agent, &skill_relative_path)
    })
    .await?
}

fn import_agent_local_skill_to_center(
    store: &SkillStore,
    agent: &str,
    skill_relative_path: &str,
) -> Result<(), AppError> {
    let adapter = adapter_for_agent(store, agent)?;
    let skill = find_agent_skill(&adapter, skill_relative_path)?;

    let skills_root = adapter.skills_dir();
    let source_path = PathBuf::from(&skill.path);
    ensure_agent_skill_path(&source_path, &skills_root)?;

    let all_managed = store.get_all_skills().unwrap_or_default();
    let all_targets = store.get_all_targets().unwrap_or_default();
    if let Some(existing) = find_verified_center_match(&skill, &all_managed, &all_targets) {
        let result = installer::install_from_local_to_destination(
            &source_path,
            Some(&existing.name),
            Path::new(&existing.central_path),
        )
        .map_err(AppError::io)?;
        store
            .update_skill_after_install(
                &existing.id,
                &existing.name,
                result.description.as_deref(),
                existing.source_revision.as_deref(),
                existing.remote_revision.as_deref(),
                Some(&result.content_hash),
                "local_only",
            )
            .map_err(AppError::db)?;

        let already_matched_by_ref = source_ref_matches_skill_path(
            &skill.path,
            std::fs::canonicalize(&skill.path).ok().as_ref(),
            existing,
        );
        if existing.source_type == "local" && already_matched_by_ref {
            store
                .update_skill_source_ref(&existing.id, &skill.path)
                .map_err(AppError::db)?;
        }

        // Register this agent as a managed sync target so the adopted skill is
        // recognized as managed (gives it a delete button). Reusing the regular
        // sync path keeps the target consistent with every other managed skill:
        // sync_engine owns the on-disk artifact, so later unsync/scenario-sync
        // touch only that managed artifact, never the user's source.
        scenario_service::sync_single_skill_to_tool(store, &existing.id, agent)?;
        return Ok(());
    }

    let result =
        installer::install_from_local(&source_path, Some(&skill.name)).map_err(AppError::io)?;
    let now = chrono::Utc::now().timestamp_millis();
    let id = uuid::Uuid::new_v4().to_string();
    let skill_record = SkillRecord {
        id,
        name: result.name.clone(),
        description: result.description.clone(),
        source_type: "local".to_string(),
        source_ref: Some(skill.path.clone()),
        source_ref_resolved: None,
        source_subpath: None,
        source_branch: None,
        source_revision: None,
        remote_revision: None,
        central_path: result.central_path.to_string_lossy().to_string(),
        content_hash: Some(result.content_hash.clone()),
        enabled: true,
        created_at: now,
        updated_at: now,
        status: "ok".to_string(),
        update_status: "local_only".to_string(),
        last_checked_at: Some(now),
        last_check_error: None,
    };

    store.insert_skill(&skill_record).map_err(AppError::db)?;
    // Register the managed sync target (see note above). On failure, drop the
    // just-inserted skill row (which cascades to any target) so we never leave
    // an orphaned, button-less skill behind. We deliberately do NOT delete the
    // central directory: `install_from_local` may have de-duplicated onto a
    // directory shared with another skill, and removing it could corrupt that
    // skill — an orphaned dir is harmless by comparison.
    if let Err(err) = scenario_service::sync_single_skill_to_tool(store, &skill_record.id, agent) {
        let _ = store.delete_skill(&skill_record.id);
        return Err(err);
    }
    Ok(())
}

/// Repair "stranded" center skills left behind by uploads that predate the
/// sync-target registration fix. Such a skill has a center record whose
/// `source_ref` still points at a skill living in an agent's skills directory,
/// but no `skill_targets` row for that agent — so the global workspace treats
/// it as in-sync-but-unmanaged and renders no actions (the missing delete
/// button). Runs once at startup; idempotent (after repair the target exists,
/// so later runs find nothing and exit on the cheap pre-check).
///
/// We match strictly by `source_ref` — the strong "this skill was uploaded
/// FROM here" signal — never by content hash, which could silently adopt a
/// look-alike skill the user never uploaded. We also only repair skills whose
/// on-disk content still equals the center copy (hash match): completing the
/// registration runs `sync_single_skill_to_tool`, which rewrites the agent
/// artifact from the central copy, so acting on a diverged skill could clobber
/// newer local edits. Diverged stranded skills are left for an explicit user
/// action. Best-effort: per-skill failures are logged and skipped.
pub fn backfill_stranded_agent_targets(store: &SkillStore) -> usize {
    let all_managed = store.get_all_skills().unwrap_or_default();
    let all_targets = store.get_all_targets().unwrap_or_default();

    // Cheap pre-check: a stranded skill carries a `source_ref` but has no target
    // row at all. When every source_ref-bearing skill is already targeted there
    // is nothing to repair, so we skip the filesystem scan entirely.
    let has_candidate = all_managed.iter().any(|managed| {
        managed.source_ref.as_deref().is_some_and(|s| !s.is_empty())
            && !all_targets.iter().any(|t| t.skill_id == managed.id)
    });
    if !has_candidate {
        return 0;
    }

    let disabled = tool_service::get_disabled_tools(store);
    let mut repaired = 0usize;

    for adapter in tool_adapters::all_tool_adapters(store) {
        if !adapter.is_installed() || disabled.contains(&adapter.key) {
            continue;
        }
        let targets = store.get_all_targets().unwrap_or_default();

        for skill in read_agent_local_skills(&adapter) {
            let canonical = std::fs::canonicalize(&skill.path).ok();
            let Some(matched) = all_managed
                .iter()
                .find(|managed| source_ref_matches_skill_path(&skill.path, canonical.as_ref(), managed))
            else {
                continue;
            };

            if targets
                .iter()
                .any(|t| t.skill_id == matched.id && t.tool == adapter.key)
            {
                continue;
            }

            // Only safe when the local copy still equals center: the sync below
            // rewrites the agent artifact from central, so a diverged local would
            // lose its newer edits. Reuse the workspace's own classifier (which
            // also recomputes the live center hash when the DB hash is stale) so
            // we repair exactly the skills the UI shows as in-sync, no more.
            if classify_sync_status(&skill, Some(matched)) != "in_sync" {
                log::info!(
                    "backfill: skipping diverged stranded skill '{}' on agent '{}' (needs manual action)",
                    matched.name,
                    adapter.key
                );
                continue;
            }

            match scenario_service::sync_single_skill_to_tool(store, &matched.id, &adapter.key) {
                Ok(()) => {
                    repaired += 1;
                    log::info!(
                        "backfill: registered missing sync target for stranded skill '{}' on agent '{}'",
                        matched.name,
                        adapter.key
                    );
                }
                Err(err) => log::warn!(
                    "backfill: failed to repair stranded skill '{}' on agent '{}': {}",
                    matched.name,
                    adapter.key,
                    err
                ),
            }
        }
    }

    if repaired > 0 {
        log::info!("backfill: repaired {repaired} stranded agent skill target(s)");
    }
    repaired
}

#[tauri::command]
pub async fn update_global_local_skill_from_center(
    store: State<'_, Arc<SkillStore>>,
    agent: String,
    skill_relative_path: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        update_agent_local_skill_from_center(&store, &agent, &skill_relative_path)
    })
    .await?
}

fn update_agent_local_skill_from_center(
    store: &SkillStore,
    agent: &str,
    skill_relative_path: &str,
) -> Result<(), AppError> {
    let adapter = adapter_for_agent(store, agent)?;
    let skill = find_agent_skill(&adapter, skill_relative_path)?;
    let all_managed = store.get_all_skills().unwrap_or_default();
    let all_targets = store.get_all_targets().unwrap_or_default();
    let managed = find_verified_center_match(&skill, &all_managed, &all_targets)
        .ok_or_else(|| AppError::not_found("No matching managed skill in center"))?;

    if classify_sync_status(&skill, Some(managed)) == "project_newer" {
        return Err(AppError::invalid_input(
            "Local skill is newer than the Skills Center version",
        ));
    }

    let skills_root = adapter.skills_dir();
    let target_path = PathBuf::from(&skill.path);
    ensure_agent_skill_path(&target_path, &skills_root)?;

    let source = PathBuf::from(&managed.central_path);
    let configured_mode = store.get_setting("sync_mode").map_err(AppError::db)?;
    let mode = sync_engine::sync_mode_for_tool(agent, configured_mode.as_deref());
    sync_engine::sync_skill(&source, &target_path, mode).map_err(AppError::io)?;
    Ok(())
}

#[tauri::command]
pub async fn delete_global_local_skill(
    store: State<'_, Arc<SkillStore>>,
    agent: String,
    skill_relative_path: String,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        delete_agent_local_skill(&store, &agent, &skill_relative_path)
    })
    .await?
}

fn delete_agent_local_skill(
    store: &SkillStore,
    agent: &str,
    skill_relative_path: &str,
) -> Result<(), AppError> {
    let adapter = adapter_for_agent(store, agent)?;
    let skill = find_agent_skill(&adapter, skill_relative_path)?;

    let all_managed = store.get_all_skills().unwrap_or_default();
    let all_targets = store.get_all_targets().unwrap_or_default();
    if let Some(managed) = find_verified_center_match(&skill, &all_managed, &all_targets) {
        let still_linked = all_targets
            .iter()
            .any(|t| t.skill_id == managed.id && target_path_equals_skill(&t.target_path, &skill.path));
        if still_linked {
            return Err(AppError::invalid_input(
                "Skill is managed by Skills Center — remove from the agent first.",
            ));
        }
    }

    let skills_root = adapter.skills_dir();
    let target_path = PathBuf::from(&skill.path);
    ensure_agent_skill_path(&target_path, &skills_root)?;
    sync_engine::remove_target(&target_path).map_err(AppError::io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        backfill_stranded_agent_targets, enrich_center_status,
        import_agent_local_skill_to_center, update_agent_local_skill_from_center,
    };
    use crate::core::content_hash;
    use crate::core::project_scanner::ProjectSkillInfo;
    use crate::core::skill_store::{ScenarioRecord, SkillRecord, SkillStore};
    use crate::core::{central_repo, installer};
    use std::collections::HashMap;

    #[test]
    fn importing_agent_local_skill_attaches_target_but_not_scenario() {
        let _guard = central_repo::test_base_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        central_repo::set_test_base_dir_override(Some(temp.path().join("center")));

        let db_path = temp.path().join("store.db");
        let store = SkillStore::new(&db_path).unwrap();

        let skills_root = temp.path().join("agent-skills");
        let skill_dir = skills_root.join("local-tool");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: local-tool\ndescription: Local test skill\n---\n",
        )
        .unwrap();

        store
            .set_setting(
                "custom_tools",
                &serde_json::json!([
                    {
                        "key": "test_agent",
                        "display_name": "Test Agent",
                        "skills_dir": skills_root.to_string_lossy(),
                        "project_relative_skills_dir": ".test-agent/skills"
                    }
                ])
                .to_string(),
            )
            .unwrap();

        let now = chrono::Utc::now().timestamp_millis();
        store
            .insert_scenario(&ScenarioRecord {
                id: "active".to_string(),
                name: "Active".to_string(),
                description: None,
                icon: None,
                sort_order: 0,
                created_at: now,
                updated_at: now,
            })
            .unwrap();
        store.set_active_scenario("active").unwrap();

        import_agent_local_skill_to_center(&store, "test_agent", "local-tool").unwrap();

        let skills = store.get_all_skills().unwrap();
        assert_eq!(skills.len(), 1);
        // Importing must NOT silently enroll the skill into the active scenario.
        assert!(store
            .get_scenarios_for_skill(&skills[0].id)
            .unwrap()
            .is_empty());
        // But it MUST register a managed sync target for the importing agent,
        // pointed at the skill's actual on-disk location, so the workspace
        // recognizes it as managed and shows its delete button.
        let targets = store.get_all_targets().unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].skill_id, skills[0].id);
        assert_eq!(targets[0].tool, "test_agent");
        assert_eq!(targets[0].target_path, skill_dir.to_string_lossy());

        // The on-disk artifact must be a sync_engine-owned symlink resolving to
        // the central copy — NOT the user's original real directory. This is
        // the property that makes a later unsync safe: removing the target only
        // drops the managed link, leaving the central skill intact.
        let meta = std::fs::symlink_metadata(&skill_dir).unwrap();
        assert!(meta.file_type().is_symlink());
        assert_eq!(
            std::fs::canonicalize(&skill_dir).unwrap(),
            std::fs::canonicalize(&skills[0].central_path).unwrap()
        );

        central_repo::set_test_base_dir_override(None);
    }

    #[test]
    fn enriching_agent_local_skills_copies_center_tags() {
        let skill = ProjectSkillInfo {
            name: "local-tool".to_string(),
            dir_name: "local-tool".to_string(),
            relative_path: "local-tool".to_string(),
            description: Some("Agent copy".to_string()),
            path: "/tmp/agent-skills/local-tool".to_string(),
            files: vec![],
            enabled: true,
            agent: "test_agent".to_string(),
            agent_display_name: "Test Agent".to_string(),
            tags: Vec::new(),
            in_center: false,
            sync_status: "project_only".to_string(),
            center_skill_id: None,
            last_modified_at: None,
            content_hash: Some("same-hash".to_string()),
        };

        let managed = SkillRecord {
            id: "center-id".to_string(),
            name: "local-tool".to_string(),
            description: Some("Center copy".to_string()),
            source_type: "local".to_string(),
            source_ref: None,
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: "/tmp/center/local-tool".to_string(),
            content_hash: Some("same-hash".to_string()),
            enabled: true,
            created_at: 0,
            updated_at: 0,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: Some(0),
            last_check_error: None,
        };

        let mut tags_map = HashMap::new();
        tags_map.insert(
            "center-id".to_string(),
            vec!["create".to_string(), "manage".to_string()],
        );

        let enriched = enrich_center_status(vec![skill], &[managed], &[], &tags_map);
        assert_eq!(enriched[0].center_skill_id.as_deref(), Some("center-id"));
        assert_eq!(
            enriched[0].tags,
            vec!["create".to_string(), "manage".to_string()]
        );
    }

    #[test]
    fn importing_agent_local_skill_does_not_overwrite_name_only_center_match() {
        let _guard = central_repo::test_base_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        central_repo::set_test_base_dir_override(Some(temp.path().join("center")));

        let db_path = temp.path().join("store.db");
        let store = SkillStore::new(&db_path).unwrap();

        let center_source = temp.path().join("center-source");
        std::fs::create_dir_all(&center_source).unwrap();
        std::fs::write(
            center_source.join("SKILL.md"),
            "---\nname: local-tool\ndescription: Center copy\n---\ncenter\n",
        )
        .unwrap();
        let existing = installer::install_from_local(&center_source, Some("local-tool")).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        store
            .insert_skill(&SkillRecord {
                id: "existing-center".to_string(),
                name: "local-tool".to_string(),
                description: existing.description.clone(),
                source_type: "local".to_string(),
                source_ref: Some(center_source.to_string_lossy().to_string()),
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                central_path: existing.central_path.to_string_lossy().to_string(),
                content_hash: Some(existing.content_hash.clone()),
                enabled: true,
                created_at: now,
                updated_at: now,
                status: "ok".to_string(),
                update_status: "local_only".to_string(),
                last_checked_at: Some(now),
                last_check_error: None,
            })
            .unwrap();

        let skills_root = temp.path().join("agent-skills");
        let skill_dir = skills_root.join("local-tool");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: local-tool\ndescription: Agent copy\n---\nagent\n",
        )
        .unwrap();

        store
            .set_setting(
                "custom_tools",
                &serde_json::json!([
                    {
                        "key": "test_agent",
                        "display_name": "Test Agent",
                        "skills_dir": skills_root.to_string_lossy(),
                        "project_relative_skills_dir": ".test-agent/skills"
                    }
                ])
                .to_string(),
            )
            .unwrap();

        import_agent_local_skill_to_center(&store, "test_agent", "local-tool").unwrap();

        let skills = store.get_all_skills().unwrap();
        assert_eq!(skills.len(), 2);
        let original_content =
            std::fs::read_to_string(std::path::Path::new(&existing.central_path).join("SKILL.md"))
                .unwrap();
        assert!(original_content.contains("Center copy"));
        assert!(skills.iter().any(|skill| skill.name == "local-tool-2"));

        central_repo::set_test_base_dir_override(None);
    }

    #[test]
    fn importing_verified_center_match_reuses_skill_and_attaches_target() {
        let _guard = central_repo::test_base_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        central_repo::set_test_base_dir_override(Some(temp.path().join("center")));

        let db_path = temp.path().join("store.db");
        let store = SkillStore::new(&db_path).unwrap();

        let skills_root = temp.path().join("agent-skills");
        let skill_dir = skills_root.join("local-tool");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: local-tool\ndescription: Agent copy\n---\nlocal\n",
        )
        .unwrap();

        // Pre-existing center skill whose source_ref points at the local skill,
        // so the import resolves to a *verified* match (the existing-match
        // branch) rather than creating a duplicate.
        let existing = installer::install_from_local(&skill_dir, Some("local-tool")).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        store
            .insert_skill(&SkillRecord {
                id: "existing-center".to_string(),
                name: "local-tool".to_string(),
                description: existing.description.clone(),
                source_type: "local".to_string(),
                source_ref: Some(skill_dir.to_string_lossy().to_string()),
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                central_path: existing.central_path.to_string_lossy().to_string(),
                content_hash: Some(existing.content_hash.clone()),
                enabled: true,
                created_at: now,
                updated_at: now,
                status: "ok".to_string(),
                update_status: "local_only".to_string(),
                last_checked_at: Some(now),
                last_check_error: None,
            })
            .unwrap();

        store
            .set_setting(
                "custom_tools",
                &serde_json::json!([
                    {
                        "key": "test_agent",
                        "display_name": "Test Agent",
                        "skills_dir": skills_root.to_string_lossy(),
                        "project_relative_skills_dir": ".test-agent/skills"
                    }
                ])
                .to_string(),
            )
            .unwrap();

        import_agent_local_skill_to_center(&store, "test_agent", "local-tool").unwrap();

        // The existing center skill is reused, not duplicated.
        let skills = store.get_all_skills().unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "existing-center");

        // And a managed target is attached for the importing agent at the
        // skill's actual on-disk path.
        let targets = store.get_all_targets().unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].skill_id, "existing-center");
        assert_eq!(targets[0].tool, "test_agent");
        assert_eq!(targets[0].target_path, skill_dir.to_string_lossy());

        central_repo::set_test_base_dir_override(None);
    }

    #[test]
    fn backfill_registers_target_for_stranded_in_sync_skill() {
        let _guard = central_repo::test_base_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        central_repo::set_test_base_dir_override(Some(temp.path().join("center")));

        let db_path = temp.path().join("store.db");
        let store = SkillStore::new(&db_path).unwrap();

        let skills_root = temp.path().join("agent-skills");
        let skill_dir = skills_root.join("local-tool");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: local-tool\ndescription: Agent copy\n---\nlocal\n",
        )
        .unwrap();

        // A center skill that was uploaded before targets were registered:
        // source_ref points at the agent dir, content matches, but NO target.
        let existing = installer::install_from_local(&skill_dir, Some("local-tool")).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        store
            .insert_skill(&SkillRecord {
                id: "stranded".to_string(),
                name: "local-tool".to_string(),
                description: existing.description.clone(),
                source_type: "local".to_string(),
                source_ref: Some(skill_dir.to_string_lossy().to_string()),
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                central_path: existing.central_path.to_string_lossy().to_string(),
                content_hash: Some(existing.content_hash.clone()),
                enabled: true,
                created_at: now,
                updated_at: now,
                status: "ok".to_string(),
                update_status: "local_only".to_string(),
                last_checked_at: Some(now),
                last_check_error: None,
            })
            .unwrap();

        store
            .set_setting(
                "custom_tools",
                &serde_json::json!([
                    {
                        "key": "test_agent",
                        "display_name": "Test Agent",
                        "skills_dir": skills_root.to_string_lossy(),
                        "project_relative_skills_dir": ".test-agent/skills"
                    }
                ])
                .to_string(),
            )
            .unwrap();

        // Stranded precondition: no targets at all.
        assert!(store.get_all_targets().unwrap().is_empty());

        let repaired = backfill_stranded_agent_targets(&store);
        assert_eq!(repaired, 1);

        let targets = store.get_all_targets().unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].skill_id, "stranded");
        assert_eq!(targets[0].tool, "test_agent");
        assert_eq!(targets[0].target_path, skill_dir.to_string_lossy());

        // Idempotent: a second run sees the target and repairs nothing.
        assert_eq!(backfill_stranded_agent_targets(&store), 0);
        assert_eq!(store.get_all_targets().unwrap().len(), 1);

        central_repo::set_test_base_dir_override(None);
    }

    #[test]
    fn pulling_from_center_rejects_newer_local_skill() {
        let _guard = central_repo::test_base_dir_lock();
        let temp = tempfile::tempdir().unwrap();
        central_repo::set_test_base_dir_override(Some(temp.path().join("center")));

        let db_path = temp.path().join("store.db");
        let store = SkillStore::new(&db_path).unwrap();

        let center_source = temp.path().join("center-source");
        std::fs::create_dir_all(&center_source).unwrap();
        std::fs::write(
            center_source.join("SKILL.md"),
            "---\nname: local-tool\ndescription: Center copy\n---\ncenter\n",
        )
        .unwrap();
        let existing = installer::install_from_local(&center_source, Some("local-tool")).unwrap();

        let skills_root = temp.path().join("agent-skills");
        let skill_dir = skills_root.join("local-tool");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: local-tool\ndescription: Agent copy\n---\nagent newer\n",
        )
        .unwrap();

        store
            .set_setting(
                "custom_tools",
                &serde_json::json!([
                    {
                        "key": "test_agent",
                        "display_name": "Test Agent",
                        "skills_dir": skills_root.to_string_lossy(),
                        "project_relative_skills_dir": ".test-agent/skills"
                    }
                ])
                .to_string(),
            )
            .unwrap();

        store
            .insert_skill(&SkillRecord {
                id: "existing-center".to_string(),
                name: "local-tool".to_string(),
                description: existing.description.clone(),
                source_type: "local".to_string(),
                source_ref: Some(skill_dir.to_string_lossy().to_string()),
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                central_path: existing.central_path.to_string_lossy().to_string(),
                content_hash: Some(content_hash::hash_directory(&existing.central_path).unwrap()),
                enabled: true,
                created_at: 0,
                updated_at: 0,
                status: "ok".to_string(),
                update_status: "local_only".to_string(),
                last_checked_at: Some(0),
                last_check_error: None,
            })
            .unwrap();

        let result = update_agent_local_skill_from_center(&store, "test_agent", "local-tool");
        assert!(result.is_err());
        let local_content = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        assert!(local_content.contains("agent newer"));

        central_repo::set_test_base_dir_override(None);
    }
}
