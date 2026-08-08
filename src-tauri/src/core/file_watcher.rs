use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::Emitter;

use super::{central_repo, skill_store::SkillStore, tool_adapters};

const APP_FS_CHANGED_EVENT: &str = "app-files-changed";
const WATCH_RESCAN_INTERVAL: Duration = Duration::from_secs(3);
const WATCH_EMIT_DEBOUNCE: Duration = Duration::from_millis(500);

/// How long a single app-initiated write suppresses watcher emits. Covers a
/// batch of back-to-back `sync_skill`/`remove_target` calls plus the latency
/// before the OS delivers their events. Kept short so a genuine external edit
/// made moments after a sync is still surfaced by the next event or rescan.
const SELF_WRITE_MUTE: Duration = Duration::from_millis(1200);

/// Paths the app is currently writing to, plus the monotonic-ms deadline
/// (relative to [`EPOCH`]) until which their watcher echoes are suppressed
/// (#248: the app's own sync writes echoed back as `app-files-changed`,
/// forcing a redundant full `refreshAppData` - and historically refresh
/// storms). Path-scoped so a genuine external edit landing inside the window
/// is still surfaced (deferred) instead of silently dropped. A monotonic base
/// avoids wall-clock jumps breaking the window.
static MUTE_STATE: OnceLock<Mutex<MuteState>> = OnceLock::new();
static EPOCH: OnceLock<Instant> = OnceLock::new();

#[derive(Default)]
struct MuteState {
    deadline_ms: u64,
    roots: Vec<PathBuf>,
}

fn mute_state() -> &'static Mutex<MuteState> {
    MUTE_STATE.get_or_init(|| Mutex::new(MuteState::default()))
}

fn now_ms() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Pure predicate split out so the mute window is unit-testable without touching
/// the process-global clock or state.
fn muted_at(now_ms: u64, suppress_until_ms: u64) -> bool {
    now_ms < suppress_until_ms
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum MuteVerdict {
    /// Outside any mute window.
    Live,
    /// Inside the window and every event path is under a dir the app itself
    /// just wrote: the echo the mute exists to swallow.
    SelfWrite,
    /// Inside the window but touching paths the app did NOT write (or a
    /// path-less rescan): someone else changed something while we were noisy.
    Foreign,
}

/// Pure classification split out for unit tests. `event_paths` empty means
/// "no path information" (watch-set rescan) — treated as Foreign so it defers
/// rather than vanishes.
fn classify_mute(
    now_ms: u64,
    deadline_ms: u64,
    roots: &[PathBuf],
    event_paths: &[PathBuf],
) -> MuteVerdict {
    if !muted_at(now_ms, deadline_ms) {
        return MuteVerdict::Live;
    }
    if !event_paths.is_empty()
        && event_paths
            .iter()
            .all(|path| roots.iter().any(|root| path.starts_with(root)))
    {
        MuteVerdict::SelfWrite
    } else {
        MuteVerdict::Foreign
    }
}

fn classify_event_paths(event_paths: &[PathBuf]) -> MuteVerdict {
    let state = mute_state().lock().unwrap();
    classify_mute(now_ms(), state.deadline_ms, &state.roots, event_paths)
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum EmitAction {
    /// Forward the change to the frontend now.
    Emit,
    /// A real change arrived during the self-write mute: remember it and emit
    /// once the window closes. Dropping it outright would leave the UI stale
    /// when a user/external edit lands within the mute window (the whole point
    /// of the mute is to hide OUR writes, not theirs).
    Defer,
    /// Nothing to forward (irrelevant event, our own write echo, or debounce
    /// coalesces it into an emit that already happened).
    Skip,
}

/// Pure routing for one observed change, split out for unit tests. `relevant`
/// = the event/rescan actually warrants a refresh; `debounced` = an emit fired
/// less than the debounce ago.
fn decide_emit(relevant: bool, mute: MuteVerdict, debounced: bool) -> EmitAction {
    if !relevant {
        return EmitAction::Skip;
    }
    match mute {
        // The frontend already refreshed after the user action that caused
        // our write; its echo is pure redundant work (#248).
        MuteVerdict::SelfWrite => EmitAction::Skip,
        MuteVerdict::Foreign => EmitAction::Defer,
        MuteVerdict::Live => {
            if debounced {
                EmitAction::Skip
            } else {
                EmitAction::Emit
            }
        }
    }
}

/// Suppress the watcher echo of an app write under `target` for
/// [`SELF_WRITE_MUTE`]. The frontend already refreshes after the user action
/// that triggered the write, so echoing it back is pure redundant work.
/// Extends (never shrinks) an in-flight window and accumulates the batch's
/// roots, so a burst of writes keeps its own echoes quiet until it settles;
/// once the window expires the root list resets. Only paths under a recorded
/// root are treated as self-writes — anything else observed during the window
/// is deferred, not dropped. A no-op when no watcher is running (e.g. the
/// CLI), since nothing reads the state.
pub fn mute_self_writes(target: &Path) {
    let now = now_ms();
    let mut state = mute_state().lock().unwrap();
    if !muted_at(now, state.deadline_ms) {
        state.roots.clear();
    }
    state.deadline_ms = state
        .deadline_ms
        .max(now.saturating_add(SELF_WRITE_MUTE.as_millis() as u64));
    if !state.roots.iter().any(|root| target.starts_with(root)) {
        state.roots.push(target.to_path_buf());
    }
}

fn self_write_muted() -> bool {
    muted_at(now_ms(), mute_state().lock().unwrap().deadline_ms)
}

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
                let project_dir = adapter.project_relative_skills_dir();
                if project_dir.is_empty() {
                    continue;
                }
                if !seen_dirs.insert(project_dir.to_string()) {
                    continue;
                }
                let skills_dir = project_path.join(project_dir);
                let disabled_dir = project_path.join(format!("{}-disabled", project_dir));
                // Only watch dirs that actually have skills inside. Watching the parent
                // or empty leaf dirs would hold OS handles (Windows ReadDirectoryChangesW)
                // and prevent users from deleting the agent-config folder (e.g. .codex)
                // after they remove all skills from it. Newly-populated dirs are picked
                // up by the polling rescan within WATCH_RESCAN_INTERVAL.
                if dir_has_entries(&skills_dir) {
                    paths.push(skills_dir);
                }
                if dir_has_entries(&disabled_dir) {
                    paths.push(disabled_dir);
                }
            }
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

fn watch_target(path: &Path) -> Option<PathBuf> {
    if path.exists() {
        Some(path.to_path_buf())
    } else {
        None
    }
}

fn dir_has_entries(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|mut iter| iter.next().is_some())
        .unwrap_or(false)
}

fn sync_watch_set(
    watcher: &mut RecommendedWatcher,
    watched: &mut HashSet<PathBuf>,
    store: &SkillStore,
) -> bool {
    let desired: HashSet<PathBuf> = collect_watch_paths(store)
        .into_iter()
        .filter_map(|path| watch_target(&path))
        .collect();
    let mut changed = false;

    for stale in watched.difference(&desired).cloned().collect::<Vec<_>>() {
        if let Err(err) = watcher.unwatch(&stale) {
            log::debug!("Failed to unwatch {}: {err}", stale.display());
        }
        watched.remove(&stale);
        changed = true;
    }

    for path in desired {
        if watched.contains(&path) {
            continue;
        }
        match watcher.watch(&path, RecursiveMode::Recursive) {
            Ok(()) => {
                watched.insert(path);
                changed = true;
            }
            Err(err) => {
                log::debug!("Failed to watch {}: {err}", path.display());
            }
        }
    }

    changed
}

fn should_emit(event: &Event) -> bool {
    if event.paths.is_empty() {
        return false;
    }
    // Drop events that come exclusively from `.git/` subtrees. `git fetch`
    // writes FETCH_HEAD/refs/packed-refs every time refreshGitStatus runs;
    // forwarding those to the UI causes refreshAppData → setManagedSkills →
    // refreshGitStatus to loop back into another fetch.
    event.paths.iter().any(|p| !is_in_git_dir(p))
}

fn is_in_git_dir(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str() == std::ffi::OsStr::new(".git"))
}

/// Whether the event touches the central repository's working tree (the part
/// auto-backup cares about). `.git` internals don't count — commits, fetches
/// and pushes must not re-arm the auto-backup debounce.
fn touches_central_repo(event: &Event) -> bool {
    let skills_dir = central_repo::skills_dir();
    event
        .paths
        .iter()
        .any(|p| p.starts_with(&skills_dir) && !is_in_git_dir(p))
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
        // A relevant change arrived while self-writes were muted; emit it once
        // the window closes (the loop ticks at least every 500ms, so the flush
        // below needs no extra timer).
        let mut pending_emit = false;

        let emit_now = |last_emit: &mut Instant| {
            if let Err(err) = app.emit(APP_FS_CHANGED_EVENT, ()) {
                log::debug!("Failed to emit app-files-changed: {err}");
            } else {
                *last_emit = Instant::now();
            }
        };

        loop {
            if last_sync.elapsed() >= WATCH_RESCAN_INTERVAL {
                let changed = sync_watch_set(&mut watcher, &mut watched, &store);
                // No path information here, so a mute window classifies as
                // Foreign: the change defers instead of vanishing.
                match decide_emit(
                    changed,
                    classify_event_paths(&[]),
                    last_emit.elapsed() < WATCH_EMIT_DEBOUNCE,
                ) {
                    EmitAction::Emit => emit_now(&mut last_emit),
                    EmitAction::Defer => pending_emit = true,
                    EmitAction::Skip => {}
                }
                last_sync = Instant::now();
            }

            // Flush a deferred emit once the mute window has closed.
            if pending_emit && !self_write_muted() && last_emit.elapsed() >= WATCH_EMIT_DEBOUNCE {
                pending_emit = false;
                emit_now(&mut last_emit);
            }

            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Ok(event)) => {
                    if touches_central_repo(&event) {
                        super::auto_backup::notify_central_change();
                    }
                    match decide_emit(
                        should_emit(&event),
                        classify_event_paths(&event.paths),
                        last_emit.elapsed() < WATCH_EMIT_DEBOUNCE,
                    ) {
                        EmitAction::Emit => emit_now(&mut last_emit),
                        EmitAction::Defer => pending_emit = true,
                        EmitAction::Skip => {}
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
    use super::{collect_watch_paths, muted_at};
    use crate::core::skill_store::{ProjectRecord, SkillStore};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn self_write_mute_window_is_half_open() {
        // Muted strictly before the deadline; the deadline instant itself and
        // anything after are live again, so the window can never wedge shut.
        assert!(muted_at(5, 10));
        assert!(!muted_at(10, 10));
        assert!(!muted_at(11, 10));
        // A zeroed deadline (never muted) is never active.
        assert!(!muted_at(0, 0));
    }

    #[test]
    fn mute_swallows_own_echo_but_defers_foreign_changes() {
        use super::{decide_emit, EmitAction, MuteVerdict};

        // Our own write echo is the thing the mute exists to swallow (#248).
        assert_eq!(
            decide_emit(true, MuteVerdict::SelfWrite, false),
            EmitAction::Skip
        );
        // A real foreign change during the window must survive as a deferred
        // emit — never vanish — regardless of debounce.
        assert_eq!(
            decide_emit(true, MuteVerdict::Foreign, false),
            EmitAction::Defer
        );
        assert_eq!(
            decide_emit(true, MuteVerdict::Foreign, true),
            EmitAction::Defer
        );
        // Live and quiet → emit; only debounce coalesces.
        assert_eq!(
            decide_emit(true, MuteVerdict::Live, false),
            EmitAction::Emit
        );
        assert_eq!(decide_emit(true, MuteVerdict::Live, true), EmitAction::Skip);
        // Irrelevant events never emit or defer, muted or not.
        assert_eq!(
            decide_emit(false, MuteVerdict::Foreign, false),
            EmitAction::Skip
        );
        assert_eq!(
            decide_emit(false, MuteVerdict::Live, false),
            EmitAction::Skip
        );
    }

    #[test]
    fn mute_classification_is_path_scoped() {
        use super::{classify_mute, MuteVerdict};
        use std::path::PathBuf;

        let roots = vec![PathBuf::from("/agents/claude/skills/foo")];
        let inside = vec![PathBuf::from("/agents/claude/skills/foo/SKILL.md")];
        let outside = vec![PathBuf::from("/agents/codex/skills/bar/SKILL.md")];
        let mixed = vec![inside[0].clone(), outside[0].clone()];

        // Outside the window everything is Live, whatever the paths say.
        assert_eq!(classify_mute(10, 10, &roots, &inside), MuteVerdict::Live);

        // Inside the window: only events fully under a written root are our
        // own echo; anything touching other paths is a foreign change.
        assert_eq!(
            classify_mute(5, 10, &roots, &inside),
            MuteVerdict::SelfWrite
        );
        assert_eq!(classify_mute(5, 10, &roots, &outside), MuteVerdict::Foreign);
        assert_eq!(classify_mute(5, 10, &roots, &mixed), MuteVerdict::Foreign);
        // No path info (watch-set rescan) defers rather than vanishes.
        assert_eq!(classify_mute(5, 10, &roots, &[]), MuteVerdict::Foreign);
    }

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

    fn insert_non_linked_project(store: &SkillStore, project_path: &std::path::Path) {
        store
            .insert_project(&ProjectRecord {
                id: "proj-1".to_string(),
                name: "proj-1".to_string(),
                path: project_path.to_string_lossy().to_string(),
                workspace_type: "project".to_string(),
                linked_agent_key: None,
                linked_agent_name: None,
                disabled_path: None,
                sort_order: 0,
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();
    }

    #[test]
    fn non_linked_project_skips_empty_skill_dirs() {
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("watcher.db");
        let project_path = tmp.path().join("proj");
        let skills_dir = project_path.join(".codex").join("skills");
        let disabled_dir = project_path.join(".codex").join("skills-disabled");
        let agent_dir = project_path.join(".codex");
        fs::create_dir_all(&skills_dir).unwrap();
        fs::create_dir_all(&disabled_dir).unwrap();

        let store = SkillStore::new(&db_path).unwrap();
        insert_non_linked_project(&store, &project_path);

        let paths = collect_watch_paths(&store);
        assert!(!paths.contains(&skills_dir), "empty skills dir watched");
        assert!(!paths.contains(&disabled_dir), "empty disabled dir watched");
        assert!(!paths.contains(&agent_dir), "agent parent dir watched");
    }

    #[test]
    fn non_linked_project_skips_missing_skill_dirs() {
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("watcher.db");
        let project_path = tmp.path().join("proj");
        fs::create_dir_all(&project_path).unwrap();
        let skills_dir = project_path.join(".codex").join("skills");
        let agent_dir = project_path.join(".codex");

        let store = SkillStore::new(&db_path).unwrap();
        insert_non_linked_project(&store, &project_path);

        let paths = collect_watch_paths(&store);
        assert!(!paths.contains(&skills_dir));
        assert!(!paths.contains(&agent_dir));
    }

    #[test]
    fn non_linked_project_watches_non_empty_skill_dirs() {
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("watcher.db");
        let project_path = tmp.path().join("proj");
        let skills_dir = project_path.join(".codex").join("skills");
        let agent_dir = project_path.join(".codex");
        fs::create_dir_all(skills_dir.join("hello")).unwrap();
        fs::write(
            skills_dir.join("hello").join("SKILL.md"),
            "---\nname: hello\n---\n",
        )
        .unwrap();

        let store = SkillStore::new(&db_path).unwrap();
        insert_non_linked_project(&store, &project_path);

        let paths = collect_watch_paths(&store);
        assert!(
            paths.contains(&skills_dir),
            "non-empty skills dir not watched"
        );
        assert!(!paths.contains(&agent_dir), "agent parent dir watched");
    }
}
