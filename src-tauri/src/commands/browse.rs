use std::sync::Arc;
use tauri::State;

use crate::core::{
    error::AppError,
    skill_store::SkillStore,
    skillsmp_api,
    skillssh_api::{self, LeaderboardType, SkillsShSkill},
};

const LEADERBOARD_CACHE_TTL: i64 = 300; // 5 minutes

#[tauri::command]
pub async fn fetch_leaderboard(
    board: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillsShSkill>, AppError> {
    let cache_key = format!("leaderboard_{}", board);

    // Check cache
    if let Ok(Some(cached)) = store.get_cache(&cache_key, LEADERBOARD_CACHE_TTL) {
        if let Ok(skills) = serde_json::from_str::<Vec<SkillsShSkill>>(&cached) {
            return Ok(skills);
        }
    }

    let proxy_url = store.proxy_url();
    let board_type = LeaderboardType::from_str(&board);
    let skills = tauri::async_runtime::spawn_blocking(move || {
        skillssh_api::fetch_leaderboard(board_type, proxy_url.as_deref()).map_err(AppError::network)
    })
    .await??;

    // Update cache
    if let Ok(json) = serde_json::to_string(&skills) {
        store.set_cache(&cache_key, &json).ok();
    }

    Ok(skills)
}

#[tauri::command]
pub async fn search_skillssh(
    query: String,
    limit: Option<usize>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillsShSkill>, AppError> {
    let proxy_url = store.proxy_url();
    let requested = limit.unwrap_or(60);
    let bounded = requested.clamp(1, 300);
    tauri::async_runtime::spawn_blocking(move || {
        skillssh_api::search_skills(&query, bounded, proxy_url.as_deref())
            .map_err(AppError::network)
    })
    .await?
}

#[tauri::command]
pub async fn search_skillsmp(
    query: String,
    ai: Option<bool>,
    page: Option<u32>,
    limit: Option<u32>,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Vec<SkillsShSkill>, AppError> {
    let api_key = store
        .get_setting("skillsmp_api_key")
        .map_err(AppError::db)?
        .filter(|k| !k.is_empty())
        .ok_or_else(|| AppError::network(anyhow::anyhow!("SkillsMP API key not configured")))?;
    let proxy_url = store.proxy_url();
    let mode = if ai.unwrap_or(false) {
        skillsmp_api::SearchMode::Ai
    } else {
        skillsmp_api::SearchMode::Keyword
    };
    tauri::async_runtime::spawn_blocking(move || {
        skillsmp_api::search(&api_key, &query, mode, page, limit, proxy_url.as_deref())
            .map_err(AppError::network)
    })
    .await?
}
