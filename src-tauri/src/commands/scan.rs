use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

use crate::core::{
    central_repo, error::AppError, installer, scanner, skill_store::SkillStore, tool_adapters,
};

#[derive(Debug, Serialize)]
pub struct ScanResultDto {
    pub tools_scanned: usize,
    pub skills_found: usize,
    pub groups: Vec<scanner::DiscoveredGroup>,
}

#[tauri::command]
pub async fn scan_local_skills(
    store: State<'_, Arc<SkillStore>>,
) -> Result<ScanResultDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let all_targets = store.get_all_targets().map_err(AppError::db)?;
        let managed_paths: Vec<String> =
            all_targets.iter().map(|t| t.target_path.clone()).collect();
        let managed_skills = store.get_all_skills().map_err(AppError::db)?;

        let adapters = tool_adapters::all_tool_adapters(&store);
        let mut plan = scanner::scan_local_skills_with_adapters(&managed_paths, &adapters)
            .map_err(AppError::io)?;

        // Preserve imported_skill_id and is_native from previous scans
        let old_discovered = store.get_all_discovered().map_err(AppError::db)?;
        for rec in &mut plan.discovered {
            // Check if this skill was previously discovered with import/native state
            if let Some(old) = old_discovered.iter().find(|d| {
                d.tool == rec.tool && d.found_path == rec.found_path
            }) {
                if old.imported_skill_id.is_some() {
                    rec.imported_skill_id = old.imported_skill_id.clone();
                }
                if old.is_native {
                    rec.is_native = true;
                }
            }
            // Also check by name against managed skills
            if rec.imported_skill_id.is_none() {
                if let Some(name) = rec.name_guess.as_deref() {
                    if let Some(existing) = managed_skills.iter().find(|skill| skill.name == name) {
                        rec.imported_skill_id = Some(existing.id.clone());
                    }
                }
            }
        }

        // Clear and repopulate discovered (now with preserved state)
        store.clear_discovered().map_err(AppError::db)?;
        for rec in &plan.discovered {
            store.insert_discovered(rec).map_err(AppError::db)?;
        }

        let all_discovered = store.get_all_discovered().map_err(AppError::db)?;
        let groups = scanner::group_discovered(&all_discovered);

        Ok(ScanResultDto {
            tools_scanned: plan.tools_scanned,
            skills_found: plan.skills_found,
            groups,
        })
    })
    .await?
}

#[tauri::command]
pub async fn import_existing_skill(
    source_path: String,
    name: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = PathBuf::from(&source_path);
        let resolved_name =
            installer::resolve_local_skill_name(&path, name.as_deref()).map_err(AppError::io)?;
        let central_path = central_repo::skills_dir().join(&resolved_name);

        if let Some(existing) = store
            .get_skill_by_central_path(&central_path.to_string_lossy())
            .map_err(AppError::db)?
        {
            if let Ok(Some(scenario_id)) = store.get_active_scenario_id() {
                store
                    .add_skill_to_scenario(&scenario_id, &existing.id)
                    .map_err(AppError::db)?;
            }
            return Ok(());
        }

        let result = installer::install_from_local_to_destination(
            &path,
            Some(&resolved_name),
            &central_path,
        )
        .map_err(AppError::io)?;

        let now = chrono::Utc::now().timestamp_millis();
        let id = uuid::Uuid::new_v4().to_string();

        let record = crate::core::skill_store::SkillRecord {
            id: id.clone(),
            name: result.name,
            description: result.description,
            source_type: "import".to_string(),
            source_ref: Some(source_path),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: result.central_path.to_string_lossy().to_string(),
            content_hash: Some(result.content_hash),
            enabled: true,
            created_at: now,
            updated_at: now,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: Some(now),
            last_check_error: None,
        };

        store.insert_skill(&record).map_err(AppError::db)?;

        // Auto-add to active scenario
        if let Ok(Some(scenario_id)) = store.get_active_scenario_id() {
            store
                .add_skill_to_scenario(&scenario_id, &id)
                .map_err(AppError::db)?;
        }

        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn import_all_discovered(store: State<'_, Arc<SkillStore>>) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let discovered = store.get_all_discovered().map_err(AppError::db)?;
        let groups = scanner::group_discovered(&discovered);

        let active_scenario = store.get_active_scenario_id().ok().flatten();

        for group in groups {
            if group.imported {
                continue;
            }
            if let Some(first) = group.locations.first() {
                let path = PathBuf::from(&first.found_path);
                let central_path = central_repo::skills_dir().join(&group.name);

                if let Ok(Some(existing)) =
                    store.get_skill_by_central_path(&central_path.to_string_lossy())
                {
                    if let Some(ref scenario_id) = active_scenario {
                        store.add_skill_to_scenario(scenario_id, &existing.id).ok();
                    }
                    continue;
                }

                if let Ok(result) = installer::install_from_local_to_destination(
                    &path,
                    Some(&group.name),
                    &central_path,
                ) {
                    let now = chrono::Utc::now().timestamp_millis();
                    let id = uuid::Uuid::new_v4().to_string();
                    let record = crate::core::skill_store::SkillRecord {
                        id: id.clone(),
                        name: result.name,
                        description: result.description,
                        source_type: "import".to_string(),
                        source_ref: Some(first.found_path.clone()),
                        source_ref_resolved: None,
                        source_subpath: None,
                        source_branch: None,
                        source_revision: None,
                        remote_revision: None,
                        central_path: result.central_path.to_string_lossy().to_string(),
                        content_hash: Some(result.content_hash),
                        enabled: true,
                        created_at: now,
                        updated_at: now,
                        status: "ok".to_string(),
                        update_status: "local_only".to_string(),
                        last_checked_at: Some(now),
                        last_check_error: None,
                    };
                    store.insert_skill(&record).ok();

                    if let Some(ref scenario_id) = active_scenario {
                        store.add_skill_to_scenario(scenario_id, &id).ok();
                    }
                }
            }
        }

        Ok(())
    })
    .await?
}
