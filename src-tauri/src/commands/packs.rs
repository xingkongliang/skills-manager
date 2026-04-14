use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

use crate::core::{
    error::AppError,
    pack_seeder::{self, SeedResult},
    skill_store::{PackRecord, SkillRecord, SkillStore},
};

#[tauri::command]
pub async fn get_all_packs(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<PackRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.get_all_packs().map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_pack_by_id(
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Option<PackRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.get_pack_by_id(&id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn create_pack(
    name: String,
    description: Option<String>,
    icon: Option<String>,
    color: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<PackRecord, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let id = Uuid::new_v4().to_string();
        store
            .insert_pack(&id, &name, description.as_deref(), icon.as_deref(), color.as_deref())
            .map_err(AppError::db)?;
        store.get_pack_by_id(&id).map_err(AppError::db)?.ok_or_else(|| {
            AppError::db("Pack not found after insert")
        })
    })
    .await?
}

#[tauri::command]
pub async fn update_pack(
    id: String,
    name: String,
    description: Option<String>,
    icon: Option<String>,
    color: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .update_pack(&id, &name, description.as_deref(), icon.as_deref(), color.as_deref())
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn delete_pack(
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.delete_pack(&id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn add_skill_to_pack(
    pack_id: String,
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.add_skill_to_pack(&pack_id, &skill_id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn remove_skill_from_pack(
    pack_id: String,
    skill_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.remove_skill_from_pack(&pack_id, &skill_id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_skills_for_pack(
    pack_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.get_skills_for_pack(&pack_id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_packs_for_scenario(
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<PackRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.get_packs_for_scenario(&scenario_id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn add_pack_to_scenario(
    scenario_id: String,
    pack_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.add_pack_to_scenario(&scenario_id, &pack_id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn remove_pack_from_scenario(
    scenario_id: String,
    pack_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.remove_pack_from_scenario(&scenario_id, &pack_id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_effective_skills_for_scenario(
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .get_effective_skills_for_scenario(&scenario_id)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn seed_default_packs(
    force: bool,
    store: State<'_, Arc<SkillStore>>,
) -> Result<SeedResult, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        pack_seeder::seed_default_packs(&store, force).map_err(AppError::db)
    })
    .await?
}
