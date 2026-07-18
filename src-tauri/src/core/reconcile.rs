//! Pure three-state reconcile decision for the folder-level sync path.
//!
//! Given the observable state of two sides (a content hash + an mtime each)
//! plus an optional "last-synced baseline" hash, decide who should overwrite
//! whom, or whether the two have genuinely diverged. No IO whatsoever — this is
//! a pure function so both ends reach the same verdict from the same inputs.
//!
//! Semantics deliberately mirror `core/merge/decision.rs`: the baseline decides
//! *who changed*, and newest-wins by mtime is used *only* to break a real
//! two-sided conflict. We never let the clock alone pick a winner when the
//! baseline can tell us only one side actually moved — wall clocks across
//! machines are unreliable, so "who changed relative to the last sync" is the
//! trustworthy signal and time is just the last-resort tie-break.

/// One side's observable state. `hash` / `mtime_ms` are `None` when they can't
/// be read (e.g. the directory is absent, or its mtime is unavailable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideState {
    pub hash: Option<String>,
    pub mtime_ms: Option<i64>,
}

/// The verdict. `conflict` marks the decisions that resolved a genuine
/// two-sided divergence (both changed vs baseline, or no baseline at all) as
/// opposed to a clean single-side fast-forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconcile {
    /// Both sides hold identical content — nothing to do.
    InSync,
    /// A is authoritative, overwrite B. `conflict=true` when both sides had
    /// changed relative to the baseline and A won the newest-wins tie-break.
    TakeA { conflict: bool },
    /// B is authoritative, overwrite A.
    TakeB { conflict: bool },
    /// Both sides changed but we can't tell who is newer (mtime tied within the
    /// threshold, or missing on either side) — leave it for a human.
    Diverged,
}

/// Decide how to reconcile two sides.
///
/// * `baseline_hash` — content hash recorded at the last successful sync, or
///   `None` for a first-ever (baseline-less) comparison.
/// * `threshold_ms` — mtime tie-break tolerance; a difference at or under this
///   is treated as "can't tell who is newer".
pub fn reconcile(
    baseline_hash: Option<&str>,
    a: &SideState,
    b: &SideState,
    threshold_ms: i64,
) -> Reconcile {
    // 1. Trivial agreement: identical content, or both absent → nothing to do.
    match (a.hash.as_deref(), b.hash.as_deref()) {
        (Some(ha), Some(hb)) if ha == hb => return Reconcile::InSync,
        (None, None) => return Reconcile::InSync,
        _ => {}
    }

    match baseline_hash {
        // 2. With a baseline we know *who changed*. A side whose hash differs
        //    from the baseline (including a missing hash) is "changed".
        Some(base) => {
            let a_changed = a.hash.as_deref() != Some(base);
            let b_changed = b.hash.as_deref() != Some(base);
            match (a_changed, b_changed) {
                // Only A moved → A is the new version, clean fast-forward onto B.
                (true, false) => Reconcile::TakeA { conflict: false },
                (false, true) => Reconcile::TakeB { conflict: false },
                // Neither moved. Logically unreachable (step 1 would have caught
                // equal hashes) — defensive InSync rather than a false conflict.
                (false, false) => Reconcile::InSync,
                // Both moved → genuine conflict. Only *here* does the clock get
                // to pick a winner, matching decision.rs's "newest-wins is for
                // double-change conflicts only".
                (true, true) => mtime_winner(a, b, threshold_ms),
            }
        }
        // 3. No baseline: we cannot attribute change, so degrade to pure
        //    newest-wins by mtime and always flag it as a conflict — the clock
        //    is the only signal we have and it is not authoritative.
        None => mtime_winner(a, b, threshold_ms),
    }
}

/// Break a two-sided conflict by mtime: the strictly-newer side (by more than
/// `threshold_ms`) wins; a tie within the threshold, or a missing mtime on
/// either side, is `Diverged`. Always flags `conflict=true` because it is only
/// ever reached from a genuine two-sided divergence.
fn mtime_winner(a: &SideState, b: &SideState, threshold_ms: i64) -> Reconcile {
    match (a.mtime_ms, b.mtime_ms) {
        (Some(ta), Some(tb)) => {
            // `saturating_sub` clamps instead of overflowing on extreme i64
            // mtimes (e.g. i64::MAX vs i64::MIN); the clamp only ever makes the
            // gap look at least as large, so a genuine winner still wins and a
            // tie still ties — the verdict is unchanged for realistic inputs.
            if ta.saturating_sub(tb) > threshold_ms {
                Reconcile::TakeA { conflict: true }
            } else if tb.saturating_sub(ta) > threshold_ms {
                Reconcile::TakeB { conflict: true }
            } else {
                Reconcile::Diverged
            }
        }
        _ => Reconcile::Diverged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(hash: Option<&str>, mtime_ms: Option<i64>) -> SideState {
        SideState {
            hash: hash.map(|s| s.to_string()),
            mtime_ms,
        }
    }

    // ── step 1: trivial agreement ──

    #[test]
    fn equal_hashes_are_in_sync() {
        let a = side(Some("h"), Some(10));
        let b = side(Some("h"), Some(9999));
        assert_eq!(reconcile(Some("base"), &a, &b, 0), Reconcile::InSync);
    }

    #[test]
    fn both_absent_is_in_sync() {
        let a = side(None, None);
        let b = side(None, None);
        assert_eq!(reconcile(Some("base"), &a, &b, 0), Reconcile::InSync);
    }

    // ── step 2: with a baseline, single-side change fast-forwards ──

    #[test]
    fn baseline_only_a_changed_takes_a_without_conflict() {
        let a = side(Some("new"), Some(100));
        let b = side(Some("base"), Some(100));
        assert_eq!(
            reconcile(Some("base"), &a, &b, 5),
            Reconcile::TakeA { conflict: false }
        );
    }

    #[test]
    fn baseline_only_b_changed_takes_b_without_conflict() {
        let a = side(Some("base"), Some(100));
        let b = side(Some("new"), Some(100));
        assert_eq!(
            reconcile(Some("base"), &a, &b, 5),
            Reconcile::TakeB { conflict: false }
        );
    }

    #[test]
    fn baseline_missing_hash_counts_as_changed_side() {
        // A absent (None) while B matches baseline → A "changed/absent", single
        // side moved → clean TakeA.
        let a = side(None, Some(100));
        let b = side(Some("base"), Some(100));
        assert_eq!(
            reconcile(Some("base"), &a, &b, 5),
            Reconcile::TakeA { conflict: false }
        );
    }

    // ── step 2: both changed → newest-wins conflict resolution ──

    #[test]
    fn baseline_both_changed_a_newer_wins_as_conflict() {
        let a = side(Some("a-new"), Some(200));
        let b = side(Some("b-new"), Some(100));
        assert_eq!(
            reconcile(Some("base"), &a, &b, 5),
            Reconcile::TakeA { conflict: true }
        );
    }

    #[test]
    fn baseline_both_changed_b_newer_wins_as_conflict() {
        let a = side(Some("a-new"), Some(100));
        let b = side(Some("b-new"), Some(200));
        assert_eq!(
            reconcile(Some("base"), &a, &b, 5),
            Reconcile::TakeB { conflict: true }
        );
    }

    #[test]
    fn baseline_both_changed_mtime_tie_within_threshold_diverges() {
        let a = side(Some("a-new"), Some(100));
        let b = side(Some("b-new"), Some(103));
        // |100 - 103| = 3 <= threshold 5 → can't tell who is newer.
        assert_eq!(reconcile(Some("base"), &a, &b, 5), Reconcile::Diverged);
    }

    #[test]
    fn baseline_both_changed_missing_mtime_diverges() {
        let a = side(Some("a-new"), None);
        let b = side(Some("b-new"), Some(200));
        assert_eq!(reconcile(Some("base"), &a, &b, 5), Reconcile::Diverged);
    }

    #[test]
    fn baseline_both_changed_diff_exactly_threshold_diverges() {
        // Boundary: a difference of exactly threshold_ms is NOT strictly newer
        // (the comparison is `>`, not `>=`) → Diverged. Guards against a `>=`
        // regression.
        let a = side(Some("a-new"), Some(100));
        let b = side(Some("b-new"), Some(105));
        assert_eq!(reconcile(Some("base"), &a, &b, 5), Reconcile::Diverged);
    }

    // ── step 3: no baseline → pure mtime, always conflict ──

    #[test]
    fn no_baseline_a_newer_wins_as_conflict() {
        let a = side(Some("a"), Some(300));
        let b = side(Some("b"), Some(100));
        assert_eq!(
            reconcile(None, &a, &b, 5),
            Reconcile::TakeA { conflict: true }
        );
    }

    #[test]
    fn no_baseline_b_newer_wins_as_conflict() {
        let a = side(Some("a"), Some(100));
        let b = side(Some("b"), Some(300));
        assert_eq!(
            reconcile(None, &a, &b, 5),
            Reconcile::TakeB { conflict: true }
        );
    }

    #[test]
    fn no_baseline_mtime_tie_diverges() {
        let a = side(Some("a"), Some(100));
        let b = side(Some("b"), Some(100));
        assert_eq!(reconcile(None, &a, &b, 5), Reconcile::Diverged);
    }

    #[test]
    fn no_baseline_diff_exactly_threshold_diverges() {
        // Same `>` vs `>=` boundary as the baseline case, on the no-baseline
        // (pure mtime) path.
        let a = side(Some("a"), Some(100));
        let b = side(Some("b"), Some(95));
        assert_eq!(reconcile(None, &a, &b, 5), Reconcile::Diverged);
    }

    #[test]
    fn no_baseline_missing_one_mtime_diverges() {
        // No baseline + one side's mtime unreadable → can't rank → Diverged,
        // never a silent one-sided overwrite.
        let a = side(Some("a"), None);
        let b = side(Some("b"), Some(300));
        assert_eq!(reconcile(None, &a, &b, 5), Reconcile::Diverged);
    }
}
