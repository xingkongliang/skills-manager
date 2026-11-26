//! ID-based batch operations used by library groups. The existing deployment
//! services remain the authority for conflicts, shared paths, and preset intent.
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::core::{
    audit_log::AuditDraft, error::AppError, scenario_service, skill_store::SkillStore,
    tool_adapters, tool_service,
};

use super::projects;

fn unique_selection(values: Vec<String>, label: &str) -> Result<Vec<String>, AppError> {
    if values.is_empty() || values.iter().any(|value| value.trim().is_empty()) {
        return Err(AppError::invalid_input(format!("No {label} selected")));
    }
    let mut seen = HashSet::new();
    Ok(values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect())
}

fn validate_skills(store: &SkillStore, skill_ids: &[String], deploy: bool) -> Result<(), AppError> {
    for id in skill_ids {
        let skill = store
            .get_skill_by_id(id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found(format!("Skill not found: {id}")))?;
        if deploy && !Path::new(&skill.central_path).is_dir() {
            return Err(AppError::not_found(format!(
                "Library copy of \"{}\" is missing",
                skill.name
            )));
        }
    }
    Ok(())
}

pub(crate) fn batch_global_internal(
    store: &SkillStore,
    skill_ids: Vec<String>,
    tools: Vec<String>,
    deploy: bool,
) -> Result<(), AppError> {
    let skill_ids = unique_selection(skill_ids, "skills")?;
    let tools = unique_selection(tools, "agents")?;
    validate_skills(store, &skill_ids, deploy)?;
    let disabled = tool_service::get_disabled_tools(store);
    for key in &tools {
        let adapter = tool_adapters::find_adapter_with_store(store, key)
            .ok_or_else(|| AppError::not_found(format!("Unknown agent: {key}")))?;
        if deploy && (!adapter.is_installed() || disabled.contains(key)) {
            return Err(AppError::invalid_input(format!(
                "Agent is not installed or is disabled: {}",
                adapter.display_name
            )));
        }
    }
    // Preflight precedes preset intent changes, so a predictable conflict does
    // not leave a refused group enabled for a later startup retry.
    if deploy {
        scenario_service::preflight_add_skills_to_tools(store, &skill_ids, &tools)?;
    }
    let result = scenario_service::apply_skills_to_tools(
        store,
        &skill_ids,
        &tools,
        if deploy {
            scenario_service::BatchApplyMode::Add
        } else {
            scenario_service::BatchApplyMode::Remove
        },
    );
    for id in &skill_ids {
        let name = store
            .get_skill_by_id(id)
            .ok()
            .flatten()
            .map(|s| s.name)
            .unwrap_or_default();
        for tool in &tools {
            let draft = AuditDraft::new(if deploy { "enable" } else { "disable" })
                .skill(id.clone(), name.clone())
                .tool(tool.clone());
            store.log_audit(match &result {
                Ok(()) => draft.ok(),
                Err(error) => draft.fail(error.to_string()),
            });
        }
    }
    result
}

#[tauri::command]
pub async fn batch_sync_skills_to_tools(
    app: AppHandle,
    skill_ids: Vec<String>,
    tools: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        batch_global_internal(&store, skill_ids, tools, true)
    })
    .await?;
    crate::schedule_tray_refresh(&app);
    result
}

#[tauri::command]
pub async fn batch_unsync_skills_from_tools(
    app: AppHandle,
    skill_ids: Vec<String>,
    tools: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        batch_global_internal(&store, skill_ids, tools, false)
    })
    .await?;
    crate::schedule_tray_refresh(&app);
    result
}

pub(crate) fn batch_project_internal(
    store: &SkillStore,
    skill_ids: Vec<String>,
    project_id: &str,
    agents: Vec<String>,
    deploy: bool,
) -> Result<(), AppError> {
    let skill_ids = unique_selection(skill_ids, "skills")?;
    let agents = unique_selection(agents, "agents")?;
    validate_skills(store, &skill_ids, deploy)?;
    if deploy {
        let plan = projects::plan_project_deploy(store, project_id, &skill_ids, &agents)?;
        if let Some(error) = projects::project_deploy_conflicts(&plan) {
            return Err(error);
        }
        projects::apply_project_batch_deploy(store, project_id, &plan)
    } else {
        let plan = projects::plan_project_undeploy(store, project_id, &skill_ids, &agents)?;
        // Check the entire selection before removing any member. A matching
        // name or even identical content is not evidence that we deployed it.
        projects::preflight_owned_project_removals(store, &plan)?;
        if let Some(error) = projects::project_removal_conflicts(&plan) {
            return Err(error);
        }
        for target in &plan {
            projects::remove_owned_project_copy(store, project_id, target)?;
        }
        Ok(())
    }
}

#[tauri::command]
pub async fn batch_deploy_skills_to_project(
    skill_ids: Vec<String>,
    project_id: String,
    agents: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        batch_project_internal(&store, skill_ids, &project_id, agents, true)
    })
    .await?
}

#[tauri::command]
pub async fn batch_undeploy_skills_from_project(
    skill_ids: Vec<String>,
    project_id: String,
    agents: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        batch_project_internal(&store, skill_ids, &project_id, agents, false)
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        central_repo,
        skill_store::{ScenarioRecord, SkillRecord},
        tool_adapters::CustomToolDef,
    };
    use std::fs;
    use std::path::PathBuf;
    use tempfile::{tempdir, TempDir};

    struct Fixture {
        _guard: std::sync::MutexGuard<'static, ()>,
        _tmp: TempDir,
        store: SkillStore,
        target: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            central_repo::set_test_base_dir_override(None);
        }
    }

    fn fixture() -> Fixture {
        let guard = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        central_repo::set_test_base_dir_override(Some(tmp.path().join("center")));
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        let target = tmp.path().join("shared-agent-skills");
        fs::create_dir_all(&target).unwrap();
        let tools: Vec<CustomToolDef> = ["agent_a", "agent_b"]
            .into_iter()
            .map(|key| CustomToolDef {
                key: key.to_string(),
                display_name: key.to_string(),
                skills_dir: target.to_string_lossy().to_string(),
                project_relative_skills_dir: None,
                category: Default::default(),
                icon: None,
            })
            .collect();
        store
            .set_setting("custom_tools", &serde_json::to_string(&tools).unwrap())
            .unwrap();
        store.set_setting("sync_mode", "copy").unwrap();
        Fixture {
            _guard: guard,
            _tmp: tmp,
            store,
            target,
        }
    }

    fn skill(fx: &Fixture, id: &str) {
        let source = central_repo::skills_dir().join(id);
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            format!("---\nname: {id}\n---\n# {id}\n"),
        )
        .unwrap();
        fx.store
            .insert_skill(&SkillRecord {
                id: id.to_string(),
                name: id.to_string(),
                description: None,
                source_type: "local".to_string(),
                source_ref: None,
                source_ref_resolved: None,
                source_subpath: None,
                source_branch: None,
                source_revision: None,
                remote_revision: None,
                central_path: source.to_string_lossy().to_string(),
                content_hash: None,
                enabled: true,
                created_at: 0,
                updated_at: 0,
                status: "ok".to_string(),
                update_status: "local_only".to_string(),
                last_checked_at: None,
                last_check_error: None,
            })
            .unwrap();
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn group_batch_global_preserves_a_shared_target_until_the_last_agent_is_removed() {
        let fx = fixture();
        skill(&fx, "demo");
        batch_global_internal(
            &fx.store,
            strings(&["demo", "demo"]),
            strings(&["agent_a", "agent_b"]),
            true,
        )
        .unwrap();
        assert_eq!(fx.store.get_targets_for_skill("demo").unwrap().len(), 2);
        batch_global_internal(&fx.store, strings(&["demo"]), strings(&["agent_a"]), false).unwrap();
        assert!(fx.target.join("demo/SKILL.md").is_file());
        assert_eq!(
            fx.store.get_targets_for_skill("demo").unwrap()[0].tool,
            "agent_b"
        );
        batch_global_internal(&fx.store, strings(&["demo"]), strings(&["agent_b"]), false).unwrap();
        assert!(!fx.target.join("demo").exists());
    }

    #[test]
    fn group_batch_global_preflight_leaves_files_and_preset_intent_unchanged_on_conflict() {
        let fx = fixture();
        skill(&fx, "one");
        skill(&fx, "two");
        fx.store
            .insert_scenario(&ScenarioRecord {
                id: "preset".to_string(),
                name: "Preset".to_string(),
                description: None,
                icon: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();
        fx.store.add_skill_to_scenario("preset", "one").unwrap();
        fx.store
            .set_scenario_skill_tool_enabled("preset", "one", "agent_a", false)
            .unwrap();
        fx.store.set_active_scenario("preset").unwrap();
        fs::create_dir_all(fx.target.join("two")).unwrap();
        fs::write(fx.target.join("two/external.txt"), "keep").unwrap();
        let error = batch_global_internal(
            &fx.store,
            strings(&["one", "two"]),
            strings(&["agent_a"]),
            true,
        )
        .unwrap_err();
        assert_eq!(error.kind, crate::core::error::ErrorKind::TargetConflict);
        assert!(!fx.target.join("one").exists());
        assert!(fx.target.join("two/external.txt").is_file());
        assert!(fx.store.get_all_targets().unwrap().is_empty());
        assert!(
            !fx.store
                .get_scenario_skill_tool_toggles("preset", "one")
                .unwrap()[0]
                .enabled
        );
    }

    #[test]
    fn group_batch_global_never_deletes_an_unrecorded_external_lookalike() {
        let fx = fixture();
        skill(&fx, "demo");
        fs::create_dir_all(fx.target.join("demo")).unwrap();
        fs::copy(
            central_repo::skills_dir().join("demo/SKILL.md"),
            fx.target.join("demo/SKILL.md"),
        )
        .unwrap();
        batch_global_internal(&fx.store, strings(&["demo"]), strings(&["agent_a"]), false).unwrap();
        assert!(fx.target.join("demo/SKILL.md").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn group_batch_global_reports_execution_failure() {
        let fx = fixture();
        skill(&fx, "demo");
        // The destination passes ownership preflight; copying fails later
        // because this source entry points at a missing file.
        std::os::unix::fs::symlink(
            "missing",
            central_repo::skills_dir().join("demo/broken-link"),
        )
        .unwrap();
        let error =
            batch_global_internal(&fx.store, strings(&["demo"]), strings(&["agent_a"]), true)
                .unwrap_err();
        assert_eq!(error.kind, crate::core::error::ErrorKind::Io);
        assert!(fx.store.get_all_targets().unwrap().is_empty());
        assert!(error.message.contains("Some skills may have been deployed"));
    }
}
