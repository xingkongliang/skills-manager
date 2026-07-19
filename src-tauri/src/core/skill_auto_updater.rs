use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::commands::skills::{check_skill_update_internal, update_git_skill_internal};
use crate::core::error::AppError;
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

/// Maximum number of retries for a single skill when a transient network error
/// is detected (e.g. TCP Broken pipe, connection reset). The total number of
/// attempts is `MAX_NETWORK_RETRIES + 1` (initial + retries).
const MAX_NETWORK_RETRIES: usize = 3;

/// Base back-off duration for the first retry. Subsequent retries double this
/// (capped at 30 s) — 1 s, 2 s, 4 s, …
const RETRY_BASE_SECS: u64 = 1;

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
    let (mut checked, mut available, mut updated, mut failed) =
        (0usize, 0usize, 0usize, 0usize);
    for skill_id in ids {
        // Yield the lock to any waiting user-initiated operation before taking
        // it again for the next skill (see FOREGROUND_YIELD).
        std::thread::sleep(FOREGROUND_YIELD);
        checked += 1;

        // The check holds the repo lock; it must be released before applying,
        // because update_git_skill_internal acquires the lock itself.
        // Transient network errors (Broken pipe, connection reset) are retried
        // with exponential back-off; hard failures skip the skill immediately.
        let status = match check_with_retry(store, &skill_id, proxy.as_deref()) {
            Some(s) => s,
            None => {
                failed += 1;
                continue;
            }
        };

        if status != "update_available" {
            continue;
        }
        available += 1;

        if apply {
            if update_with_retry(store, &skill_id, proxy.as_deref()) {
                updated += 1;
            } else {
                failed += 1;
            }
        }
    }
    log::info!(
        "skill auto-updater: round done — checked={checked} available={available} updated={updated} failed={failed}"
    );
    Ok(())
}

/// Attempt `check_skill_update_internal` for one skill, retrying up to
/// [`MAX_NETWORK_RETRIES`] times on transient network errors with exponential
/// back-off. Returns the `update_status` string on success, or `None` on
/// failure (caller should increment the `failed` counter).
///
/// The central-repo lock is acquired fresh on each attempt and released
/// between attempts so foreground operations can interleave during the delay.
fn check_with_retry(store: &SkillStore, skill_id: &str, proxy: Option<&str>) -> Option<String> {
    for attempt in 0..=MAX_NETWORK_RETRIES {
        if attempt > 0 {
            let delay = retry_delay(attempt - 1);
            log::info!(
                "skill auto-updater: retrying check for {skill_id} \
                 (attempt {attempt}/{MAX_NETWORK_RETRIES}, delay={}s)",
                delay.as_secs()
            );
            std::thread::sleep(delay);
        }

        let _lock = match RepoLock::acquire("auto-update check") {
            Ok(lock) => lock,
            Err(_) => {
                log::info!("skill auto-updater: skipping {skill_id} (repo busy)");
                return None;
            }
        };

        match check_skill_update_internal(store, skill_id, true, proxy) {
            Ok(dto) => return Some(dto.update_status),
            Err(ref err) if attempt < MAX_NETWORK_RETRIES && err.is_transient() => {
                log::info!(
                    "skill auto-updater: transient error checking {skill_id} \
                     (attempt {}): {}",
                    attempt + 1,
                    err.message
                );
                // Lock is dropped here; the next iteration sleeps before re-acquiring.
            }
            Err(err) => {
                log::warn!(
                    "skill auto-updater: check failed for {skill_id}: {}",
                    err.message
                );
                return None;
            }
        }
    }
    // All retries exhausted.
    log::warn!("skill auto-updater: check exhausted {MAX_NETWORK_RETRIES} retries for {skill_id}");
    None
}

/// Attempt `update_git_skill_internal` for one skill, retrying up to
/// [`MAX_NETWORK_RETRIES`] times on transient network errors with exponential
/// back-off. Returns `true` on success, `false` on failure (caller should
/// increment the `failed` counter).
fn update_with_retry(store: &SkillStore, skill_id: &str, proxy: Option<&str>) -> bool {
    for attempt in 0..=MAX_NETWORK_RETRIES {
        if attempt > 0 {
            let delay = retry_delay(attempt - 1);
            log::info!(
                "skill auto-updater: retrying update for {skill_id} \
                 (attempt {attempt}/{MAX_NETWORK_RETRIES}, delay={}s)",
                delay.as_secs()
            );
            std::thread::sleep(delay);
        }

        match update_git_skill_internal(store, skill_id, proxy, None) {
            Ok(_) => return true,
            Err(ref err) if attempt < MAX_NETWORK_RETRIES && err.is_transient() => {
                log::info!(
                    "skill auto-updater: transient error updating {skill_id} \
                     (attempt {}): {}",
                    attempt + 1,
                    err.message
                );
            }
            Err(err) => {
                log::warn!(
                    "skill auto-updater: update failed for {skill_id}: {}",
                    err.message
                );
                return false;
            }
        }
    }
    // All retries exhausted.
    log::warn!(
        "skill auto-updater: update exhausted {MAX_NETWORK_RETRIES} retries for {skill_id}"
    );
    false
}

/// Exponential back-off delay for retry `attempt` (0-indexed).
/// Sequence: 1 s, 2 s, 4 s, … capped at 30 s.
/// Uses a checked left-shift so that large `attempt` values saturate at the
/// cap rather than overflowing.
fn retry_delay(attempt: usize) -> Duration {
    let secs = RETRY_BASE_SECS
        .checked_shl(attempt as u32)
        .unwrap_or(u64::MAX)
        .min(30);
    Duration::from_secs(secs)
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

    // ── retry_delay ──

    #[test]
    fn retry_delay_attempt_0_is_base() {
        assert_eq!(retry_delay(0), Duration::from_secs(RETRY_BASE_SECS));
    }

    #[test]
    fn retry_delay_doubles_each_attempt() {
        assert_eq!(retry_delay(1), Duration::from_secs(2));
        assert_eq!(retry_delay(2), Duration::from_secs(4));
        assert_eq!(retry_delay(3), Duration::from_secs(8));
    }

    #[test]
    fn retry_delay_caps_at_30s() {
        // At attempt 5 the uncapped value would be 32 s; must be capped at 30.
        assert_eq!(retry_delay(5), Duration::from_secs(30));
        // Large attempt numbers must not overflow.
        assert_eq!(retry_delay(100), Duration::from_secs(30));
    }
}
