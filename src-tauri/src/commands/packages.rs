use std::sync::Arc;

use tauri::State;

use crate::core::error::AppError;
use crate::core::package_manager::{self, ApplyResult, BindingPlan, PackageDetails};
use crate::core::skill_store::SkillStore;

#[tauri::command]
pub async fn get_packages(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<PackageDetails>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        package_manager::list_packages(&store).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn import_git_package(
    source_url: String,
    requested_revision: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<PackageDetails, AppError> {
    if source_url.trim().is_empty() {
        return Err(AppError::invalid_input("Git URL is required"));
    }
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        package_manager::import_git_package(&store, &source_url, requested_revision.as_deref())
            .map_err(AppError::classify_git_error)
    })
    .await?
}

#[tauri::command]
pub async fn update_package(
    package_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<PackageDetails, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        package_manager::update_package(&store, &package_id).map_err(AppError::classify_git_error)
    })
    .await?
}

#[tauri::command]
pub async fn delete_package(
    package_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        package_manager::delete_package(&store, &package_id).map_err(AppError::io)
    })
    .await?
}

#[tauri::command]
pub async fn create_package_binding(
    package_id: String,
    artifact_key: String,
    tool: String,
    scope: String,
    project_id: Option<String>,
    surface_policy: String,
    requested_components: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<BindingPlan, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        package_manager::create_binding(
            &store,
            &package_id,
            &artifact_key,
            &tool,
            &scope,
            project_id.as_deref(),
            &surface_policy,
            &requested_components,
        )
        .map_err(|error| AppError::invalid_input(error.to_string()))
    })
    .await?
}

#[tauri::command]
pub async fn preview_package_binding(
    binding_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<BindingPlan, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        package_manager::preview_binding(&store, &binding_id).map_err(AppError::internal)
    })
    .await?
}

#[tauri::command]
pub async fn apply_package_binding(
    binding_id: String,
    approved_plan_hash: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<ApplyResult, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        package_manager::apply_binding(&store, &binding_id, &approved_plan_hash)
            .map_err(AppError::internal)
    })
    .await?
}

#[tauri::command]
pub async fn remove_package_binding(
    binding_id: String,
    forget_setup: bool,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        package_manager::remove_binding(&store, &binding_id, forget_setup)
            .map_err(AppError::internal)
    })
    .await?
}

#[tauri::command]
pub async fn sync_project_package_manifest(
    project_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<BindingPlan>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        package_manager::sync_project_manifest(&store, &project_id)
            .map_err(AppError::classify_git_error)
    })
    .await?
}
