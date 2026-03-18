use crate::core::{central_repo, git_backup};
use std::sync::Arc;
use tauri::State;

use crate::core::skill_store::SkillStore;

#[tauri::command]
pub async fn git_backup_status(
    store: State<'_, Arc<SkillStore>>,
) -> Result<git_backup::GitBackupStatus, String> {
    let _ = store; // ensure DB is available
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::get_status(&skills_dir).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_backup_init(
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), String> {
    let _ = store;
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::init_repo(&skills_dir).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_backup_set_remote(
    store: State<'_, Arc<SkillStore>>,
    url: String,
) -> Result<(), String> {
    let _ = store;
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::set_remote(&skills_dir, &url).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_backup_commit(
    store: State<'_, Arc<SkillStore>>,
    message: String,
) -> Result<(), String> {
    let _ = store;
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::commit_all(&skills_dir, &message).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_backup_push(
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), String> {
    let _ = store;
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::push(&skills_dir).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_backup_pull(
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), String> {
    let _ = store;
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::pull(&skills_dir).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_backup_clone(
    store: State<'_, Arc<SkillStore>>,
    url: String,
) -> Result<(), String> {
    let _ = store;
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::clone_into(&skills_dir, &url).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_backup_create_snapshot(
    store: State<'_, Arc<SkillStore>>,
) -> Result<String, String> {
    let _ = store;
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::create_snapshot_tag(&skills_dir).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_backup_list_versions(
    store: State<'_, Arc<SkillStore>>,
    limit: Option<u32>,
) -> Result<Vec<git_backup::GitBackupVersion>, String> {
    let _ = store;
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::list_snapshot_versions(&skills_dir, limit.map(|v| v as usize))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn git_backup_restore_version(
    store: State<'_, Arc<SkillStore>>,
    tag: String,
) -> Result<(), String> {
    let store = store.inner().clone();
    let skills_dir = central_repo::skills_dir();
    tokio::task::spawn_blocking(move || {
        git_backup::restore_snapshot_version(&skills_dir, &tag).map_err(|e| e.to_string())?;
        reconcile_skills_index(&store).map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn reconcile_skills_index(store: &SkillStore) -> anyhow::Result<()> {
    let skills_dir = central_repo::skills_dir();
    std::fs::create_dir_all(&skills_dir)?;

    // Remove stale DB records whose central directories no longer exist.
    let existing = store.get_all_skills()?;
    for skill in existing {
        if !std::path::Path::new(&skill.central_path).exists() {
            store.delete_skill(&skill.id)?;
        }
    }

    // Add missing DB records for directories present in central repo.
    let entries = std::fs::read_dir(&skills_dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if !is_valid_skill_dir(&path) {
            continue;
        }

        let central_path = path.to_string_lossy().to_string();
        if store.get_skill_by_central_path(&central_path)?.is_some() {
            continue;
        }

        let meta = crate::core::skill_metadata::parse_skill_md(&path);
        let inferred_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown-skill".to_string());
        let name = meta
            .name
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(inferred_name);
        let now = chrono::Utc::now().timestamp_millis();

        let record = crate::core::skill_store::SkillRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            description: meta.description,
            source_type: "import".to_string(),
            source_ref: Some(central_path.clone()),
            source_ref_resolved: None,
            source_subpath: None,
            source_branch: None,
            source_revision: None,
            remote_revision: None,
            central_path,
            content_hash: crate::core::content_hash::hash_directory(&path).ok(),
            enabled: true,
            created_at: now,
            updated_at: now,
            status: "ok".to_string(),
            update_status: "local_only".to_string(),
            last_checked_at: Some(now),
            last_check_error: None,
        };

        store.insert_skill(&record)?;
    }

    Ok(())
}

fn is_valid_skill_dir(dir: &std::path::Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    ["SKILL.md", "skill.md", "CLAUDE.md", "README.md", "readme.md"]
        .iter()
        .any(|name| dir.join(name).exists())
}
