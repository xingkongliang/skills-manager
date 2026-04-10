use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::Emitter;

use super::{central_repo, skill_store::SkillStore, tool_adapters};

const APP_FS_CHANGED_EVENT: &str = "app-files-changed";
const WATCH_RESCAN_INTERVAL: Duration = Duration::from_secs(3);
const WATCH_EMIT_DEBOUNCE: Duration = Duration::from_millis(500);

fn collect_watch_paths(store: &SkillStore) -> Vec<PathBuf> {
    let mut paths = vec![central_repo::skills_dir(), central_repo::scenarios_dir()];

    for adapter in tool_adapters::all_tool_adapters(store) {
        paths.push(adapter.skills_dir());
        paths.extend(adapter.all_scan_dirs());
    }

    if let Ok(projects) = store.get_all_projects() {
        let adapters = tool_adapters::all_tool_adapters(store);
        let mut seen_dirs = std::collections::HashSet::new();
        for project in projects {
            if project.workspace_type == "linked" {
                let skills_dir = PathBuf::from(&project.path);
                paths.push(skills_dir);
                if let Some(disabled_path) = project.disabled_path {
                    let disabled_dir = PathBuf::from(disabled_path);
                    paths.push(disabled_dir);
                }
                continue;
            }

            let project_path = PathBuf::from(&project.path);
            seen_dirs.clear();
            for adapter in &adapters {
                if adapter.relative_skills_dir.is_empty() {
                    continue;
                }
                if !seen_dirs.insert(adapter.relative_skills_dir.clone()) {
                    continue;
                }
                let skills_dir = project_path.join(&adapter.relative_skills_dir);
                let disabled_dir =
                    project_path.join(format!("{}-disabled", &adapter.relative_skills_dir));
                // Watch the parent directory so we detect creation of new skills dirs.
                if let Some(parent) = skills_dir.parent() {
                    paths.push(parent.to_path_buf());
                }
                paths.push(skills_dir);
                paths.push(disabled_dir);
            }
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

fn watch_target(path: &Path) -> Option<PathBuf> {
    if path.exists() {
        return Some(path.to_path_buf());
    }
    path.parent()
        .filter(|parent| parent.exists())
        .map(|parent| parent.to_path_buf())
}

fn sync_watch_set(
    watcher: &mut RecommendedWatcher,
    watched: &mut HashSet<PathBuf>,
    store: &SkillStore,
) {
    let desired: HashSet<PathBuf> = collect_watch_paths(store)
        .into_iter()
        .filter_map(|path| watch_target(&path))
        .collect();

    for stale in watched.difference(&desired).cloned().collect::<Vec<_>>() {
        if let Err(err) = watcher.unwatch(&stale) {
            log::debug!("Failed to unwatch {}: {err}", stale.display());
        }
        watched.remove(&stale);
    }

    for path in desired {
        if watched.contains(&path) {
            continue;
        }
        match watcher.watch(&path, RecursiveMode::Recursive) {
            Ok(()) => {
                watched.insert(path);
            }
            Err(err) => {
                log::debug!("Failed to watch {}: {err}", path.display());
            }
        }
    }
}

fn should_emit(event: &Event) -> bool {
    !event.paths.is_empty()
}

pub fn start_file_watcher<R: tauri::Runtime>(app: tauri::AppHandle<R>, store: Arc<SkillStore>) {
    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match RecommendedWatcher::new(
            move |result| {
                let _ = tx.send(result);
            },
            Config::default().with_poll_interval(Duration::from_secs(2)),
        ) {
            Ok(watcher) => watcher,
            Err(err) => {
                log::error!("Failed to create filesystem watcher: {err}");
                return;
            }
        };

        let mut watched = HashSet::new();
        let mut last_sync = Instant::now() - WATCH_RESCAN_INTERVAL;
        let mut last_emit = Instant::now() - WATCH_EMIT_DEBOUNCE;

        loop {
            if last_sync.elapsed() >= WATCH_RESCAN_INTERVAL {
                sync_watch_set(&mut watcher, &mut watched, &store);
                last_sync = Instant::now();
            }

            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Ok(event)) => {
                    if !should_emit(&event) || last_emit.elapsed() < WATCH_EMIT_DEBOUNCE {
                        continue;
                    }
                    if let Err(err) = app.emit(APP_FS_CHANGED_EVENT, ()) {
                        log::debug!("Failed to emit app-files-changed: {err}");
                    } else {
                        last_emit = Instant::now();
                    }
                }
                Ok(Err(err)) => {
                    log::debug!("Filesystem watcher error: {err}");
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::collect_watch_paths;
    use crate::core::skill_store::{ProjectRecord, SkillStore};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn linked_workspace_watch_paths_only_include_selected_roots() {
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("watcher.db");
        let skills_root = tmp.path().join("external").join("skills");
        let disabled_root = tmp.path().join("external").join("skills-disabled");
        fs::create_dir_all(&skills_root).unwrap();
        fs::create_dir_all(&disabled_root).unwrap();

        let store = SkillStore::new(&db_path).unwrap();
        store
            .insert_project(&ProjectRecord {
                id: "linked-1".to_string(),
                name: "External".to_string(),
                path: skills_root.to_string_lossy().to_string(),
                workspace_type: "linked".to_string(),
                linked_agent_key: Some("external".to_string()),
                linked_agent_name: Some("External".to_string()),
                disabled_path: Some(disabled_root.to_string_lossy().to_string()),
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();

        let paths = collect_watch_paths(&store);
        assert!(paths.contains(&skills_root));
        assert!(paths.contains(&disabled_root));
        assert!(!paths.contains(&skills_root.parent().unwrap().to_path_buf()));
        assert!(!paths.contains(&disabled_root.parent().unwrap().to_path_buf()));
    }
}
