use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::commands::skills::{
    check_skill_update_internal_with_remote, prefetch_skill_remote, update_git_skill_internal,
};
use crate::core::repo_lock::RepoLock;
use crate::core::skill_store::SkillStore;

const SETTING_INTERVAL: &str = "auto_update_check_interval";
const SETTING_LAST_RUN: &str = "auto_update_last_run_at";
const SETTING_APPLY: &str = "auto_update_apply";
const EVENT_AUTO_UPDATED: &str = "skills-auto-updated";

/// Initial delay before the first scheduler tick. Gives the app a chance to
/// finish startup work (file watcher, tray, window paint) before the scheduler
/// starts hitting the network / git.
const INITIAL_DELAY: Duration = Duration::from_secs(60);

/// Polling cadence — we wake every 15 minutes to re-read settings and decide
/// whether a round is due. Kept well below the shortest (1h) interval so an
/// "hourly" setting is honoured reasonably promptly; also the cadence at which
/// a changed interval setting takes effect.
const POLL_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// Brief pause between per-skill checks. Each check holds the central-repo lock
/// for a network round-trip; without a gap, this loop re-acquires the lock so
/// quickly that a waiting user-initiated operation can be starved for the whole
/// round. The pause must exceed the foreground poll cadence in `repo_lock`
/// (50ms) so a foreground waiter reliably wins the lock during the gap.
const FOREGROUND_YIELD: Duration = Duration::from_millis(200);

#[derive(Serialize, Clone)]
struct AutoUpdatePayload {
    ran_at: String,
}

pub fn start<R: Runtime>(app: AppHandle<R>, store: Arc<SkillStore>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(INITIAL_DELAY).await;
        loop {
            if let Some(interval) = read_interval(&store) {
                if is_due(read_last_run(&store), interval) {
                    match run_round(&app, &store).await {
                        Ok(()) => record_round_completion(&app, &store),
                        Err(err) => {
                            log::warn!("skill auto-updater: round errored: {err}")
                        }
                    }
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

/// Single source of truth for "a check round just completed": persist
/// `auto_update_last_run_at`, emit `skills-auto-updated` with the standard
/// payload so the frontend Settings listener can update its "last checked"
/// label, and refresh the tray menu so the updates badge reflects new state.
///
/// Called by both the background scheduler and the tray's manual
/// "Check for skill updates" so the user-visible bookkeeping stays in sync
/// regardless of which surface triggered the check.
pub fn record_round_completion<R: Runtime>(app: &AppHandle<R>, store: &SkillStore) {
    let now = Utc::now();
    write_last_run(store, now);
    let payload = AutoUpdatePayload {
        ran_at: now.to_rfc3339(),
    };
    if let Err(err) = app.emit(EVENT_AUTO_UPDATED, payload) {
        log::debug!("skill auto-updater: emit failed: {err}");
    }
    if let Err(err) = crate::refresh_tray_menu(app) {
        log::debug!("skill auto-updater: refresh_tray_menu failed: {err}");
    }
}

fn read_interval(store: &SkillStore) -> Option<Duration> {
    let raw = store.get_setting(SETTING_INTERVAL).ok().flatten()?;
    parse_interval(raw.trim())
}

fn parse_interval(raw: &str) -> Option<Duration> {
    match raw.to_ascii_lowercase().as_str() {
        "" | "off" | "manual" | "disabled" => None,
        "1h" | "hourly" => Some(Duration::from_secs(60 * 60)),
        "6h" => Some(Duration::from_secs(6 * 60 * 60)),
        "24h" | "1d" | "daily" => Some(Duration::from_secs(24 * 60 * 60)),
        _ => None,
    }
}

fn read_last_run(store: &SkillStore) -> Option<DateTime<Utc>> {
    let raw = store.get_setting(SETTING_LAST_RUN).ok().flatten()?;
    DateTime::parse_from_rfc3339(raw.trim())
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn write_last_run(store: &SkillStore, at: DateTime<Utc>) {
    if let Err(err) = store.set_setting(SETTING_LAST_RUN, &at.to_rfc3339()) {
        log::warn!("skill auto-updater: failed to persist {SETTING_LAST_RUN}: {err}");
    }
}

fn is_due(last_run: Option<DateTime<Utc>>, interval: Duration) -> bool {
    let Some(last) = last_run else {
        return true;
    };
    let elapsed = Utc::now().signed_duration_since(last);
    // If we can't represent the interval as chrono::Duration (unrealistic for
    // our 6h–7d values), prefer "not due" so we don't accidentally run a
    // round on every tick.
    let Some(interval_chrono) = chrono::Duration::from_std(interval).ok() else {
        log::warn!(
            "skill auto-updater: failed to convert interval to chrono::Duration ({}s)",
            interval.as_secs()
        );
        return false;
    };
    elapsed >= interval_chrono
}

async fn run_round<R: Runtime>(_app: &AppHandle<R>, store: &Arc<SkillStore>) -> Result<(), String> {
    let store_for_task = store.clone();
    tauri::async_runtime::spawn_blocking(move || run_round_blocking(&store_for_task))
        .await
        .map_err(|err| format!("join error: {err}"))??;
    Ok(())
}

/// Whether the user has opted in to applying updates automatically (vs. only
/// checking and surfacing the badge).
fn apply_enabled(store: &SkillStore) -> bool {
    matches!(
        store.get_setting(SETTING_APPLY).ok().flatten().as_deref(),
        Some("on")
    )
}

fn run_round_blocking(store: &SkillStore) -> Result<(), String> {
    let proxy = store.proxy_url();
    let apply = apply_enabled(store);
    let ids: Vec<String> = store
        .get_all_skills()
        .map_err(|err| format!("get_all_skills failed: {err}"))?
        .into_iter()
        .map(|s| s.id)
        .collect();

    // Take and release the central-repo lock around each individual skill
    // check. This bounds the worst-case wait for any user-initiated manual
    // operation to a single skill's network round-trip (rather than the
    // entire round). A skill whose lock is busy — a manual install/update is
    // running — is simply skipped; the next scheduled round picks it up.
    let (mut checked, mut available, mut updated, mut held_back, mut failed) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    for skill_id in ids {
        // Yield the lock to any waiting user-initiated operation before taking
        // it again for the next skill (see FOREGROUND_YIELD).
        std::thread::sleep(FOREGROUND_YIELD);
        checked += 1;

        // Resolve the remote before taking the lock: the lock must never be
        // held across a network round-trip, or a slow remote fails every
        // concurrent user-initiated operation with a 20s "busy" (#315).
        let prefetched = prefetch_skill_remote(store, &skill_id, true, proxy.as_deref());

        // The check holds the repo lock; it must be released before applying,
        // because update_git_skill_internal acquires the lock itself.
        let status = {
            let _lock = match RepoLock::acquire("auto-update check") {
                Ok(lock) => lock,
                Err(_) => {
                    failed += 1;
                    log::info!("skill auto-updater: skipping {skill_id} (repo busy)");
                    continue;
                }
            };
            match check_skill_update_internal_with_remote(store, &skill_id, true, prefetched) {
                Ok(dto) => dto.update_status,
                Err(err) => {
                    failed += 1;
                    log::warn!("skill auto-updater: check failed for {skill_id}: {err}");
                    continue;
                }
            }
        };

        if status != "update_available" {
            continue;
        }
        available += 1;

        if apply {
            match update_git_skill_internal(store, &skill_id, proxy.as_deref(), None, None) {
                Ok(result) if !result.pending_removals.is_empty() => {
                    held_back += 1;
                    log::info!(
                        "skill auto-updater: holding back {skill_id} — updating would remove {} \
                         path(s) the new version does not have; update it by hand to review",
                        result.pending_removals.len()
                    );
                }
                Ok(_) => updated += 1,
                Err(err) => {
                    failed += 1;
                    log::warn!(
                        "skill auto-updater: update failed for {skill_id}: {}",
                        err.message
                    );
                }
            }
        }
    }
    log::info!(
        "skill auto-updater: round done — checked={checked} available={available} updated={updated} held_back={held_back} failed={failed}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_interval_known_values() {
        assert_eq!(parse_interval("off"), None);
        assert_eq!(parse_interval(""), None);
        assert_eq!(parse_interval("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_interval("hourly"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_interval("6h"), Some(Duration::from_secs(6 * 3600)));
        assert_eq!(parse_interval("24h"), Some(Duration::from_secs(86_400)));
        assert_eq!(parse_interval("daily"), Some(Duration::from_secs(86_400)));
        assert_eq!(parse_interval("7d"), None);
        assert_eq!(parse_interval("nonsense"), None);
    }

    #[test]
    fn is_due_when_no_history() {
        assert!(is_due(None, Duration::from_secs(60)));
    }

    #[test]
    fn is_due_after_interval() {
        let past = Utc::now() - chrono::Duration::hours(7);
        assert!(is_due(Some(past), Duration::from_secs(6 * 3600)));
    }

    #[test]
    fn not_due_within_interval() {
        let past = Utc::now() - chrono::Duration::hours(1);
        assert!(!is_due(Some(past), Duration::from_secs(6 * 3600)));
    }

    #[test]
    fn is_due_returns_false_when_interval_overflow() {
        // Duration::MAX is far larger than chrono::Duration can represent in
        // milliseconds, so the conversion fails. We must NOT then run on
        // every tick — the fallback should be "not due".
        let past = Utc::now() - chrono::Duration::hours(1);
        assert!(!is_due(Some(past), Duration::MAX));
    }
}
