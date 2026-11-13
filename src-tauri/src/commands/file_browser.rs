use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde::Deserialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::core::{error::AppError, file_manager, git_fetcher, skill_browser::{self, BrowserSession, BrowserSessionStore, DirectoryPage, FilePreview, ResolvedLink, SessionInfo}, skill_store::SkillStore, tool_adapters};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserTarget {
    Managed { skill_id: String, origin: String },
    Project { project_id: String, agent: String, skill_relative_path: String },
    Global { agent: String, skill_relative_path: String },
}

fn local_workspace_root(store: &SkillStore, roots: (PathBuf, Option<PathBuf>), relative: &str) -> Result<PathBuf, AppError> {
    skill_browser::validate_relative(relative)?;
    if relative.is_empty() { return Err(AppError::invalid_input("reader.invalidPath")); }
    let managed = store.get_all_skills().map_err(AppError::db)?;
    for root in std::iter::once(roots.0).chain(roots.1) {
        let path = root.join(relative);
        if !path.is_dir() { continue; }
        let canonical_root = root.canonicalize().map_err(AppError::io)?;
        let canonical = path.canonicalize().map_err(AppError::io)?;
        let is_link = std::fs::symlink_metadata(&path).map_err(AppError::io)?.file_type().is_symlink();
        if is_link || !canonical.starts_with(&canonical_root) {
            if !managed.iter().any(|skill| Path::new(&skill.central_path).canonicalize().ok().as_ref() == Some(&canonical)) {
                return Err(AppError::invalid_input("reader.unmanagedLink"));
            }
        }
        return Ok(canonical);
    }
    Err(AppError::not_found("reader.missingDirectory"))
}

#[tauri::command]
pub async fn open_skill_browser(target: BrowserTarget, request_id: String, store: State<'_, Arc<SkillStore>>, sessions: State<'_, Arc<BrowserSessionStore>>) -> Result<SessionInfo, AppError> {
    let store = store.inner().clone();
    let sessions = sessions.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cancel = sessions.begin(&request_id)?;
        if cancel.load(Ordering::SeqCst) { return Err(AppError::cancelled("reader.cancelled")); }
        let session = match target {
            BrowserTarget::Managed { skill_id, origin } => {
                let skill = store.get_skill_by_id(&skill_id).map_err(AppError::db)?.ok_or_else(|| AppError::not_found("reader.missingDirectory"))?;
                match origin.as_str() {
                    "local" => BrowserSession::local(PathBuf::from(&skill.central_path), "Local".into())?,
                    "source" if matches!(skill.source_type.as_str(), "local" | "import") => {
                        BrowserSession::local(PathBuf::from(skill.source_ref.ok_or_else(|| AppError::not_found("reader.missingDirectory"))?), "Source".into())?
                    }
                    "source" if matches!(skill.source_type.as_str(), "git" | "skillssh") => {
                        let source = super::skills::git_source_from_skill(&skill)?;
                        git_fetcher::validate_git_url(&source.clone_url).map_err(AppError::git)?;
                        let proxy = store.proxy_url();
                        let temp = git_fetcher::clone_repo_ref_scoped(&source.clone_url, source.branch.as_deref(), source.subpath.as_deref(), Some(&cancel), proxy.as_deref(), None).map_err(AppError::classify_git_error)?;
                        let result = (|| {
                            // The clone already resolved the requested ref under
                            // its timeout/cancel policy. Use the revision actually
                            // fetched instead of starting a second remote query.
                            let revision = git2::Repository::open(&temp).and_then(|repo| repo.head()?.peel_to_commit().map(|commit| commit.id().to_string())).map_err(AppError::git)?;
                            skill_browser::checkout_snapshot(&temp, &revision, Some(&cancel), proxy.as_deref())?;
                            let root = super::skills::resolve_skill_dir(&temp, source.subpath.as_deref(), source.locator_skill_id.as_deref())?;
                            let root = root.canonicalize().map_err(AppError::io)?;
                            if !root.starts_with(temp.canonicalize().map_err(AppError::io)?) { return Err(AppError::invalid_input("reader.invalidPath")); }
                            BrowserSession::snapshot(root, if skill.source_type == "skillssh" { "skills.sh" } else { "Git" }.into(), revision, temp.clone())
                        })();
                        if result.is_err() { git_fetcher::cleanup_temp(&temp); }
                        result?
                    }
                    _ => return Err(AppError::invalid_input("reader.invalidPath")),
                }
            }
            BrowserTarget::Project { project_id, agent, skill_relative_path } => {
                let project = store.get_project_by_id(&project_id).map_err(AppError::db)?.ok_or_else(|| AppError::not_found("reader.missingDirectory"))?;
                let roots = super::projects::resolve_agent_skills_roots(&store, &project, &agent).ok_or_else(|| AppError::not_found("reader.missingDirectory"))?;
                BrowserSession::local(local_workspace_root(&store, roots, &skill_relative_path)?, "Local".into())?
            }
            BrowserTarget::Global { agent, skill_relative_path } => {
                let adapter = tool_adapters::all_tool_adapters(&store).into_iter().find(|a| a.key == agent).ok_or_else(|| AppError::not_found("reader.missingDirectory"))?;
                BrowserSession::local(local_workspace_root(&store, (adapter.skills_dir(), None), &skill_relative_path)?, "Local".into())?
            }
        };
        sessions.publish(&request_id, session)
    }).await?
}

#[tauri::command]
pub async fn cancel_open_skill_browser(request_id: String, sessions: State<'_, Arc<BrowserSessionStore>>) -> Result<(), AppError> {
    let sessions = sessions.inner().clone();
    tauri::async_runtime::spawn_blocking(move || { sessions.cancel(&request_id); Ok(()) }).await?
}

#[tauri::command]
pub async fn close_skill_browser(session_id: String, sessions: State<'_, Arc<BrowserSessionStore>>) -> Result<(), AppError> {
    let sessions = sessions.inner().clone();
    tauri::async_runtime::spawn_blocking(move || { sessions.close(&session_id); Ok(()) }).await?
}

#[tauri::command]
pub async fn list_skill_directory(session_id: String, relative_path: String, cursor: Option<usize>, sessions: State<'_, Arc<BrowserSessionStore>>) -> Result<DirectoryPage, AppError> {
    let session = sessions.get(&session_id)?;
    tauri::async_runtime::spawn_blocking(move || session.list(&relative_path, cursor.unwrap_or(0))).await?
}

#[tauri::command]
pub async fn read_skill_file(session_id: String, relative_path: String, sessions: State<'_, Arc<BrowserSessionStore>>) -> Result<FilePreview, AppError> {
    let session = sessions.get(&session_id)?;
    tauri::async_runtime::spawn_blocking(move || session.read(&relative_path)).await?
}

#[tauri::command]
pub async fn resolve_skill_link(session_id: String, from_path: String, href: String, sessions: State<'_, Arc<BrowserSessionStore>>) -> Result<ResolvedLink, AppError> {
    let session = sessions.get(&session_id)?;
    tauri::async_runtime::spawn_blocking(move || session.resolve_link(&from_path, &href)).await?
}

#[tauri::command]
pub async fn reveal_skill_file(session_id: String, relative_path: String, sessions: State<'_, Arc<BrowserSessionStore>>) -> Result<(), AppError> {
    let session = sessions.get(&session_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let status = file_manager::reveal_item(&session.resolve(&relative_path)?).map_err(AppError::io)?;
        if file_manager::exit_is_failure(status.success()) { return Err(AppError::io("reader.openFailed")); }
        Ok(())
    }).await?
}

#[tauri::command]
pub async fn open_skill_file_external(app: AppHandle, session_id: String, relative_path: String, sessions: State<'_, Arc<BrowserSessionStore>>) -> Result<(), AppError> {
    let session = sessions.get(&session_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let path = session.resolve(&relative_path)?;
        app.opener().open_path(path.to_string_lossy().to_string(), None::<&str>).map_err(|_| AppError::io("reader.openFailed"))
    }).await?
}
