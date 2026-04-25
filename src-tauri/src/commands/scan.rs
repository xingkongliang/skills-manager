use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

use crate::core::{
    error::AppError, installer, scanner, skill_store::SkillStore, sync_metadata, tool_adapters,
};

fn canonicalize_lossy(path: &str) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path))
}

fn match_imported_skill_id(
    rec: &crate::core::skill_store::DiscoveredSkillRecord,
    managed_skills: &[crate::core::skill_store::SkillRecord],
) -> Option<String> {
    let found_path = canonicalize_lossy(&rec.found_path);
    if let Some(existing) = managed_skills.iter().find(|skill| {
        skill.source_ref.as_deref().map(canonicalize_lossy).as_ref() == Some(&found_path)
            || skill
                .source_ref_resolved
                .as_deref()
                .map(canonicalize_lossy)
                .as_ref()
                == Some(&found_path)
    }) {
        return Some(existing.id.clone());
    }

    if let Some(fingerprint) = rec.fingerprint.as_deref() {
        if let Some(existing) = managed_skills
            .iter()
            .find(|skill| skill.content_hash.as_deref() == Some(fingerprint))
        {
            return Some(existing.id.clone());
        }
    }

    None
}

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

        for rec in &mut plan.discovered {
            rec.imported_skill_id = match_imported_skill_id(rec, &managed_skills);
        }

        // Clear and repopulate discovered
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

        let result =
            installer::install_from_local(&path, Some(&resolved_name)).map_err(AppError::io)?;

        if let Some(existing) = store
            .get_skill_by_central_path(&result.central_path.to_string_lossy())
            .map_err(AppError::db)?
        {
            if let Ok(Some(scenario_id)) = store.get_active_scenario_id() {
                store
                    .add_skill_to_scenario(&scenario_id, &existing.id)
                    .map_err(AppError::db)?;
            }
            return Ok(());
        }

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

        sync_metadata::with_repo_lock("import existing skill", || {
            store.insert_skill(&record)?;

            // Auto-add to active scenario
            if let Ok(Some(scenario_id)) = store.get_active_scenario_id() {
                store.add_skill_to_scenario(&scenario_id, &id)?;
            }
            sync_metadata::write_all_from_db_unlocked(&store)
        })
        .map_err(AppError::db)?;

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

                if let Ok(result) = installer::install_from_local(&path, Some(&group.name)) {
                    if let Ok(Some(existing)) =
                        store.get_skill_by_central_path(&result.central_path.to_string_lossy())
                    {
                        if let Some(ref scenario_id) = active_scenario {
                            store.add_skill_to_scenario(scenario_id, &existing.id).ok();
                        }
                        continue;
                    }

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

#[cfg(test)]
mod tests {
    use super::match_imported_skill_id;
    use crate::core::skill_store::{DiscoveredSkillRecord, SkillRecord};

    fn managed_skill(
        id: &str,
        name: &str,
        source_ref: Option<&str>,
        source_ref_resolved: Option<&str>,
        content_hash: Option<&str>,
    ) -> SkillRecord {
        SkillRecord {
            id: id.to_string(),
            name: name.to_string(),
            description: None,
            source_type: "import".to_string(),
            source_ref: source_ref.map(str::to_string),
            source_ref_resolved: source_ref_resolved.map(str::to_string),
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path: format!("/central/{name}"),
            content_hash: content_hash.map(str::to_string),
            enabled: true,
            created_at: 0,
            updated_at: 0,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: None,
            last_check_error: None,
        }
    }

    fn discovered(path: &str, name: &str, fingerprint: Option<&str>) -> DiscoveredSkillRecord {
        DiscoveredSkillRecord {
            id: "discovered-1".to_string(),
            tool: "cursor".to_string(),
            found_path: path.to_string(),
            name_guess: Some(name.to_string()),
            fingerprint: fingerprint.map(str::to_string),
            found_at: 0,
            imported_skill_id: None,
        }
    }

    #[test]
    fn does_not_mark_same_name_as_imported_without_path_or_hash_match() {
        let rec = discovered("/tmp/local/foo", "same-name", None);
        let managed = vec![managed_skill(
            "skill-1",
            "same-name",
            Some("/tmp/other/foo"),
            None,
            None,
        )];

        assert_eq!(match_imported_skill_id(&rec, &managed), None);
    }

    #[test]
    fn marks_same_fingerprint_as_imported() {
        let rec = discovered("/tmp/local/foo", "same-name", Some("abc123"));
        let managed = vec![managed_skill(
            "skill-1",
            "different-name",
            Some("/tmp/other/foo"),
            None,
            Some("abc123"),
        )];

        assert_eq!(
            match_imported_skill_id(&rec, &managed),
            Some("skill-1".to_string())
        );
    }

    #[test]
    fn marks_same_source_path_as_imported() {
        let rec = discovered("/tmp/local/foo", "same-name", None);
        let managed = vec![managed_skill(
            "skill-1",
            "different-name",
            Some("/tmp/local/foo"),
            None,
            None,
        )];

        assert_eq!(
            match_imported_skill_id(&rec, &managed),
            Some("skill-1".to_string())
        );
    }
}
