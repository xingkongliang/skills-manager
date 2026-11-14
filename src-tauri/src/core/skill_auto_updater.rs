use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::commands::skills::{
    check_skill_update_internal_with_remote, prefetch_skill_remote, update_git_skill_internal,
};
use crate::core::repo_lock::RepoLock;
use crate::core::skill_store::{is_terminal_update_status, SkillStore, SOURCE_GONE};

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

/// What one round did, in the terms the log and the tests both need.
///
/// `gone` and `skipped_gone` are deliberately separate: one counts skills this
/// round discovered to be deleted upstream, the other skills already known to
/// be — a library with 17 of them must not read as if the round had just found
/// 18 (#655).
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub struct RoundSummary {
    pub checked: usize,
    pub available: usize,
    pub updated: usize,
    pub held_back: usize,
    pub failed: usize,
    /// Deleted upstream, discovered by this round's apply.
    pub gone: usize,
    /// Already known to be deleted upstream: skipped without a fetch.
    pub skipped_gone: usize,
}

fn run_round_blocking(store: &SkillStore) -> Result<RoundSummary, String> {
    let proxy = store.proxy_url();
    let apply = apply_enabled(store);
    let skills: Vec<(String, String)> = store
        .get_all_skills()
        .map_err(|err| format!("get_all_skills failed: {err}"))?
        .into_iter()
        .map(|s| (s.id, s.update_status))
        .collect();

    // Take and release the central-repo lock around each individual skill
    // check. This bounds the worst-case wait for any user-initiated manual
    // operation to a single skill's network round-trip (rather than the
    // entire round). A skill whose lock is busy — a manual install/update is
    // running — is simply skipped; the next scheduled round picks it up.
    let mut round = RoundSummary::default();
    for (skill_id, status) in skills {
        // Nothing this round does can clear a settled "the Source deleted it":
        // no fetch, no update attempt, and deliberately no failure count (#655).
        if is_terminal_update_status(&status) {
            round.skipped_gone += 1;
            continue;
        }
        // Yield the lock to any waiting user-initiated operation before taking
        // it again for the next skill (see FOREGROUND_YIELD).
        std::thread::sleep(FOREGROUND_YIELD);
        round.checked += 1;

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
                    round.failed += 1;
                    log::info!("skill auto-updater: skipping {skill_id} (repo busy)");
                    continue;
                }
            };
            match check_skill_update_internal_with_remote(store, &skill_id, true, prefetched) {
                Ok(dto) => dto.update_status,
                Err(err) => {
                    round.failed += 1;
                    log::warn!("skill auto-updater: check failed for {skill_id}: {err}");
                    continue;
                }
            }
        };

        if status != "update_available" {
            // A skills.sh skill settles into `source_gone` only through the
            // update below, so this is not where a deletion is discovered.
            continue;
        }
        round.available += 1;

        if apply {
            match update_git_skill_internal(store, &skill_id, proxy.as_deref(), None, None) {
                // Nothing to update and nothing to retry: the Source deleted
                // this skill (#655).
                Ok(result) if result.skill.update_status == SOURCE_GONE => {
                    round.gone += 1;
                }
                Ok(result) if !result.pending_removals.is_empty() => {
                    round.held_back += 1;
                    log::info!(
                        "skill auto-updater: holding back {skill_id} — updating would remove {} \
                         path(s) the new version does not have; update it by hand to review",
                        result.pending_removals.len()
                    );
                }
                Ok(_) => round.updated += 1,
                Err(err) => {
                    round.failed += 1;
                    log::warn!(
                        "skill auto-updater: update failed for {skill_id}: {}",
                        err.message
                    );
                }
            }
        }
    }
    log::info!(
        "skill auto-updater: round done — checked={} available={} updated={} held_back={} failed={} gone={} skipped_gone={}",
        round.checked,
        round.available,
        round.updated,
        round.held_back,
        round.failed,
        round.gone,
        round.skipped_gone
    );
    Ok(round)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::skill_store::SkillRecord;
    use tempfile::tempdir;

    /// A Source that deleted a skill will not put it back on its own, so a round
    /// must not spend a fetch or a retry on it — and must not count it as a
    /// failure. The remote here cannot be reached, so any attempt would land as
    /// `error` (#655).
    #[test]
    fn a_round_skips_a_skill_its_source_deleted() {
        let tmp = tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("skills.db")).unwrap();
        let unreachable = "file:///nonexistent/skills-manager-test/repo";
        store
            .insert_skill(&SkillRecord {
                id: "gone-skill".to_string(),
                name: "gone-skill".to_string(),
                description: None,
                source_type: "git".to_string(),
                source_ref: Some(unreachable.to_string()),
                source_ref_resolved: Some(unreachable.to_string()),
                source_subpath: Some("skills/gone-skill".to_string()),
                source_branch: None,
                source_revision: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
                remote_revision: None,
                central_path: tmp
                    .path()
                    .join("central/gone-skill")
                    .to_string_lossy()
                    .to_string(),
                content_hash: None,
                enabled: true,
                created_at: 1,
                updated_at: 1,
                status: "ok".to_string(),
                update_status: "source_gone".to_string(),
                last_checked_at: None,
                last_check_error: None,
            })
            .unwrap();

        let round = run_round_blocking(&store).unwrap();

        let stored = store.get_skill_by_id("gone-skill").unwrap().unwrap();
        assert_eq!(stored.update_status, "source_gone");
        assert_eq!(stored.last_checked_at, None, "no fetch, no check");
        assert_eq!(stored.last_check_error, None);
        assert_eq!(
            round,
            RoundSummary {
                skipped_gone: 1,
                ..RoundSummary::default()
            },
            "skipped, not checked, not failed, not updated"
        );
    }

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
