use super::{
    browse,
    client::{source_url, MarketClient},
    download, install,
    models::{validate_identity, Board, Page},
};
use crate::core::{
    audit_log::AuditDraft, error::AppError, install_cancel::InstallCancelRegistry,
    skill_store::SkillStore,
};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use tauri::{Emitter, State};

static ACTIVE_INSTALLS: Mutex<Option<HashSet<String>>> = Mutex::new(None);
struct InstallGuard {
    key: String,
    registry: Arc<InstallCancelRegistry>,
}
impl Drop for InstallGuard {
    fn drop(&mut self) {
        self.registry.remove(&self.key);
        if let Ok(mut active) = ACTIVE_INSTALLS.lock() {
            if let Some(active) = active.as_mut() {
                active.remove(&self.key);
            }
        }
    }
}

#[tauri::command]
pub async fn fetch_bytedance_skills(
    board: Board,
    query: String,
    page: u32,
    page_size: u32,
    refresh: bool,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Page, AppError> {
    browse::fetch(&store, board, &query, page, page_size, refresh).await
}

#[tauri::command]
pub async fn cancel_bytedance_install(
    key: String,
    registry: State<'_, Arc<InstallCancelRegistry>>,
) -> Result<bool, AppError> {
    if !key.starts_with("bytedance:") {
        return Err(AppError::invalid_input(
            "Invalid ByteDance cancellation key",
        ));
    }
    Ok(registry.cancel(&key))
}

#[tauri::command]
pub async fn install_from_bytedance(
    identifier: String,
    repository: String,
    app: tauri::AppHandle,
    store: State<'_, Arc<SkillStore>>,
    registry: State<'_, Arc<InstallCancelRegistry>>,
) -> Result<String, AppError> {
    validate_identity(&identifier, &repository)?;
    if let Some(existing) = store
        .get_skill_by_source_ref("bytedance", &source_url(&identifier, &repository))
        .map_err(AppError::db)?
    {
        return Ok(existing.id);
    }
    let key = format!("bytedance:{repository}:{identifier}");
    {
        let mut active = ACTIVE_INSTALLS.lock().map_err(AppError::internal)?;
        if !active.get_or_insert_with(HashSet::new).insert(key.clone()) {
            return Err(AppError::invalid_input(
                "This ByteDance skill is already being installed",
            ));
        }
    }
    let guard = InstallGuard {
        key: key.clone(),
        registry: registry.inner().clone(),
    };
    let cancel = registry.register(&key);
    let emit = |phase: &str| {
        let _ = app.emit(
            "install-progress",
            serde_json::json!({"skill_id": key, "phase": phase}),
        );
    };
    emit("downloading");
    let result = async {
        let proxy = store.proxy_url();
        let client = MarketClient::shared(proxy.as_deref())?;
        let downloaded = download::prepare(&client, &identifier, &repository, &cancel).await?;
        emit("installing");
        let store = store.inner().clone();
        let identifier = identifier.clone();
        let repository = repository.clone();
        tauri::async_runtime::spawn_blocking(move || {
            install::commit(&store, downloaded, &identifier, &repository, &cancel)
        })
        .await
        .map_err(AppError::internal)?
    }
    .await;
    match &result {
        Ok(id) => {
            emit("done");
            store.log_audit(
                AuditDraft::new("install")
                    .ok()
                    .skill(id, &identifier)
                    .detail(format!("bytedance:{repository}")),
            );
        }
        Err(error) => store.log_audit(
            AuditDraft::new("install")
                .fail(error.to_string())
                .detail(format!("{identifier}: {error}")),
        ),
    }
    drop(guard);
    result
}
