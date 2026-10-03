use std::sync::Arc;
use tauri::State;

use crate::core::custom_repos::{self, CustomRepoRecord};
use crate::core::error::AppError;
use crate::core::skill_store::SkillStore;

#[derive(Debug, serde::Serialize)]
pub struct CustomRepoDto {
    pub id: String,
    pub url: String,
    pub label: String,
    pub added_at: i64,
}

fn custom_repo_dto(record: &CustomRepoRecord) -> CustomRepoDto {
    CustomRepoDto {
        id: record.id.clone(),
        url: record.url.clone(),
        label: record.label.clone(),
        added_at: record.added_at,
    }
}

#[tauri::command]
pub async fn list_custom_repos(
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<CustomRepoDto>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let records = custom_repos::list(&store)?;
        Ok(records.iter().map(custom_repo_dto).collect())
    })
    .await?
}

#[tauri::command]
pub async fn add_custom_repo(
    url: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<CustomRepoDto, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let record = custom_repos::add(&store, &url)?;
        Ok(custom_repo_dto(&record))
    })
    .await?
}

#[tauri::command]
pub async fn remove_custom_repo(
    id: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || custom_repos::remove(&store, &id))
        .await?
}
