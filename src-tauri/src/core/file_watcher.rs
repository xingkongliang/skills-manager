use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{mpsc::Sender, Mutex, OnceLock};
use std::sync::Arc;
use std::time::Duration;

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::Emitter;

use super::{central_repo, skill_store::SkillStore, tool_adapters, tool_adapters::ToolAdapter};

const APP_FS_CHANGED_EVENT: &str = "app-files-changed";
const WATCH_EMIT_DEBOUNCE: Duration = Duration::from_millis(500);

static WATCHER_CONTROL: OnceLock<Mutex<Option<Sender<WatcherMessage>>>> = OnceLock::new();

enum WatcherMessage {
    FsEvent(Result<Event, notify::Error>),
    Resync,
}

fn watcher_control() -> &'static Mutex<Option<Sender<WatcherMessage>>> {
    WATCHER_CONTROL.get_or_init(|| Mutex::new(None))
}

fn set_watcher_control(sender: Sender<WatcherMessage>) {
    if let Ok(mut slot) = watcher_control().lock() {
        *slot = Some(sender);
    }
}

pub fn request_watch_set_resync() {
    // Maintenance rule: if you change any data source consumed by
    // `collect_watch_paths()` (tool adapters, custom tool paths/defs, projects,
    // linked workspace roots, disabled paths), trigger this after the write
    // succeeds so the live watcher set stays in sync.
    if let Ok(slot) = watcher_control().lock() {
        if let Some(sender) = slot.as_ref() {
            let _ = sender.send(WatcherMessage::Resync);
        }
    }
}

fn collect_watch_paths(store: &SkillStore) -> Vec<PathBuf> {
    collect_watch_paths_for_adapters(store, tool_adapters::all_tool_adapters(store))
}

fn collect_watch_paths_for_adapters(
    store: &SkillStore,
    adapters: Vec<ToolAdapter>,
) -> Vec<PathBuf> {
    let mut paths = vec![central_repo::skills_dir(), central_repo::scenarios_dir()];

    for adapter in adapters {
        paths.push(adapter.skills_dir());
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
        set_watcher_control(tx.clone());
        let mut watcher = match RecommendedWatcher::new(
            move |result| {
                let _ = tx.send(WatcherMessage::FsEvent(result));
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
        let mut last_emit = std::time::Instant::now() - WATCH_EMIT_DEBOUNCE;
        sync_watch_set(&mut watcher, &mut watched, &store);

        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(WatcherMessage::Resync) => {
                    sync_watch_set(&mut watcher, &mut watched, &store);
                }
                Ok(WatcherMessage::FsEvent(Ok(event))) => {
                    if !should_emit(&event) || last_emit.elapsed() < WATCH_EMIT_DEBOUNCE {
                        continue;
                    }
                    if let Err(err) = app.emit(APP_FS_CHANGED_EVENT, ()) {
                        log::debug!("Failed to emit app-files-changed: {err}");
                    } else {
                        last_emit = std::time::Instant::now();
                    }
                }
                Ok(WatcherMessage::FsEvent(Err(err))) => {
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
    use super::{collect_watch_paths, collect_watch_paths_for_adapters};
    use crate::core::skill_store::{ProjectRecord, SkillStore};
    use crate::core::tool_adapters::ToolAdapter;
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

    #[test]
    fn watch_paths_ignore_additional_scan_dirs() {
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("watcher.db");
        let primary_dir = tmp.path().join("agent-skills");
        let noisy_scan_dir = tmp.path().join("plugin-cache");
        fs::create_dir_all(&primary_dir).unwrap();
        fs::create_dir_all(&noisy_scan_dir).unwrap();

        let store = SkillStore::new(&db_path).unwrap();
        let adapters = vec![ToolAdapter {
            key: "custom".to_string(),
            display_name: "Custom".to_string(),
            relative_skills_dir: String::new(),
            relative_detect_dir: String::new(),
            additional_scan_dirs: vec![noisy_scan_dir.to_string_lossy().to_string()],
            override_skills_dir: Some(primary_dir.to_string_lossy().to_string()),
            is_custom: true,
        }];

        let paths = collect_watch_paths_for_adapters(&store, adapters);
        assert!(paths.contains(&primary_dir));
        assert!(!paths.contains(&noisy_scan_dir));
    }
}
