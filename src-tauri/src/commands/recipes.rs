use std::sync::Arc;
use tauri::State;

use crate::core::{
    error::AppError,
    skill_store::{RecipeRecord, SkillRecord, SkillStore},
};

#[tauri::command]
pub async fn create_recipe(
    scenario_id: String,
    name: String,
    description: Option<String>,
    icon: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<RecipeRecord, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .create_recipe(&scenario_id, &name, description.as_deref(), icon.as_deref())
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn update_recipe(
    id: String,
    name: String,
    description: Option<String>,
    icon: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .update_recipe(&id, &name, description.as_deref(), icon.as_deref())
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn delete_recipe(
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.delete_recipe(&id).map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_recipes_for_scenario(
    scenario_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<RecipeRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .get_recipes_for_scenario(&scenario_id)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn save_recipe_prompt_template(
    recipe_id: String,
    template: Option<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .save_recipe_prompt_template(&recipe_id, template.as_deref())
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_recipe_prompt_template(
    recipe_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Option<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .get_recipe_prompt_template(&recipe_id)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn set_recipe_skills(
    recipe_id: String,
    skill_ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .set_recipe_skills(&recipe_id, &skill_ids)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn get_recipe_skills(
    recipe_id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillRecord>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .get_recipe_skills(&recipe_id)
            .map_err(AppError::db)
    })
    .await?
}

#[tauri::command]
pub async fn reorder_recipes(
    scenario_id: String,
    recipe_ids: Vec<String>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .reorder_recipes(&scenario_id, &recipe_ids)
            .map_err(AppError::db)
    })
    .await?
}
