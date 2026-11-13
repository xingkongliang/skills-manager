use std::sync::Arc;
use tauri::State;
use crate::core::translation_provider::{TranslationConfig, TranslationSettings, TranslationSettingsView};
use crate::core::{error::AppError, skill_browser::BrowserSessionStore, skill_store::SkillStore, translation_cache::{CacheEntry, CacheResult, TranslationCache}, translation_transport::{is_supported_target, TranslationResponse, TranslationTransport}};

#[tauri::command]
pub async fn translate_skill_blocks(session_id: String, request_id: String, texts: Vec<String>, target_language: String, expected_namespace: String, sessions: State<'_, Arc<BrowserSessionStore>>, transport: State<'_, Arc<TranslationTransport>>, store: State<'_, Arc<SkillStore>>) -> Result<TranslationResponse, AppError> {
    let _session = sessions.get(&session_id)?;
    let config = TranslationConfig::load(&store)?;
    if config.namespace() != expected_namespace { return Err(AppError::invalid_input("reader.translationConfigChanged")); }
    transport.translate_configured(request_id, texts, target_language, store.proxy_url(), config).await
}

#[tauri::command]
pub async fn get_translation_settings(store: State<'_, Arc<SkillStore>>) -> Result<TranslationSettingsView, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || TranslationConfig::load(&store).map(|config| config.view())).await?
}

#[tauri::command]
pub async fn save_translation_settings(settings: TranslationSettings, api_key: Option<String>, store: State<'_, Arc<SkillStore>>, transport: State<'_, Arc<TranslationTransport>>) -> Result<TranslationSettingsView, AppError> {
    let store = store.inner().clone();
    let view = tauri::async_runtime::spawn_blocking(move || TranslationConfig::save(&store, settings, api_key)).await??;
    transport.cancel_all();
    Ok(view)
}

#[tauri::command]
pub async fn test_translation_settings(request_id: String, store: State<'_, Arc<SkillStore>>, transport: State<'_, Arc<TranslationTransport>>) -> Result<TranslationResponse, AppError> {
    let config = TranslationConfig::load(&store)?;
    transport.translate_configured(request_id, vec!["Hello, world.".into()], "zh-CN".into(), store.proxy_url(), config).await
}

#[tauri::command]
pub fn cancel_skill_translation(request_id: String, transport: State<'_, Arc<TranslationTransport>>) { transport.cancel(&request_id); }

fn validate_keys(keys: &[String]) -> Result<(), AppError> {
    if keys.len() > 500 || keys.iter().any(|s| s.len() != 64 || !s.bytes().all(|c| c.is_ascii_hexdigit())) { return Err(AppError::invalid_input("reader.translationInvalidRequest")); }
    Ok(())
}

#[tauri::command]
pub async fn get_translation_cache(keys: Vec<String>, cache: State<'_, Arc<TranslationCache>>) -> Result<CacheResult, AppError> {
    validate_keys(&keys)?;
    let cache = cache.inner().clone();
    tauri::async_runtime::spawn_blocking(move || Ok(cache.get(&keys))).await?
}

#[tauri::command]
pub async fn put_translation_cache(entries: Vec<CacheEntry>, epoch: u64, cache: State<'_, Arc<TranslationCache>>) -> Result<bool, AppError> {
    validate_keys(&entries.iter().map(|e| e.cache_key.clone()).collect::<Vec<_>>())?;
    if entries.iter().map(|e| e.text.len()).sum::<usize>() > 2 * 1024 * 1024 || entries.iter().any(|e| !is_supported_target(&e.target_language)) { return Err(AppError::invalid_input("reader.translationInvalidRequest")); }
    let cache = cache.inner().clone();
    tauri::async_runtime::spawn_blocking(move || Ok(cache.put(entries, epoch))).await?
}

#[tauri::command]
pub async fn clear_translation_cache(cache: State<'_, Arc<TranslationCache>>, transport: State<'_, Arc<TranslationTransport>>) -> Result<u64, AppError> {
    transport.cancel_all();
    let cache = cache.inner().clone();
    tauri::async_runtime::spawn_blocking(move || Ok(cache.clear())).await?
}
