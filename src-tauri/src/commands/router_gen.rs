use std::sync::Arc;
use tauri::State;

use crate::core::{
    central_repo,
    error::AppError,
    pending_router_gen::{self, PendingMarker},
    skill_store::SkillStore,
};

#[tauri::command]
pub async fn write_pending_router_marker(
    pack_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let pack = store
            .get_pack_by_id(&pack_id)
            .map_err(AppError::db)?
            .ok_or_else(|| AppError::not_found(format!("pack {pack_id} not found")))?;
        let skills = store
            .get_skills_for_pack(&pack.id)
            .map_err(AppError::db)?
            .into_iter()
            .map(|s| (s.name, s.description))
            .collect();
        let sm_root = central_repo::base_dir();
        let marker = PendingMarker {
            pack_id: pack.id,
            pack_name: pack.name,
            created_at: chrono::Utc::now().timestamp(),
            skills,
        };
        pending_router_gen::write_marker(&sm_root, &marker).map_err(AppError::io)
    })
    .await?
}

#[tauri::command]
pub async fn list_pending_router_markers() -> Result<Vec<PendingMarker>, AppError> {
    tauri::async_runtime::spawn_blocking(|| {
        let sm_root = central_repo::base_dir();
        pending_router_gen::list_markers(&sm_root).map_err(AppError::io)
    })
    .await?
}

#[tauri::command]
pub async fn clear_pending_router_marker(pack_id: String) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let sm_root = central_repo::base_dir();
        pending_router_gen::delete_marker(&sm_root, &pack_id).map_err(AppError::io)
    })
    .await?
}
