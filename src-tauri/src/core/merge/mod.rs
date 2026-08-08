//! Object-level three-way merge of the skills library (merge-engine design,
//! `docs/merge-engine-design.md`). Phase 3d-α introduced protocol markers on
//! every app commit plus the engine itself; 3d-β makes the object merge the
//! default for manual sync, with `merge_engine=system` as the opt-out escape
//! hatch back to the legacy line-level git merge.

pub mod apply;
pub mod decision;
#[cfg(test)]
mod integration_tests;
pub mod pending;
pub mod protocol;
pub mod resolve;
pub mod snapshot;
pub mod treebuild;
pub mod validate;

pub use apply::{
    object_merge_pull_unlocked, recover_on_startup, remote_touches_pending, MergeSummary,
};

/// Settings key of the engine switch (§9). Since 3d-β the object merge is
/// the default; an explicit "system" value is the escape hatch back to the
/// legacy line-level git merge.
pub const SETTING_MERGE_ENGINE: &str = "merge_engine";

pub fn object_merge_enabled(store: &crate::core::skill_store::SkillStore) -> bool {
    store
        .get_setting(SETTING_MERGE_ENGINE)
        .ok()
        .flatten()
        .map(|v| v.trim() != "system")
        .unwrap_or(true)
}

/// Engine-gated pull, shared by the GUI sync command and the CLI `git pull`.
/// Caller holds the repo lock and has applied the device identity. Note the
/// legacy path must stay reachable: with the object engine a *line-level*
/// merge commit from our own tooling would read as an old-client violation
/// (§6) on every other device, so every in-app pull goes through this gate.
pub fn gated_pull_unlocked(
    store: &crate::core::skill_store::SkillStore,
    skills_dir: &std::path::Path,
) -> anyhow::Result<MergeSummary> {
    if object_merge_enabled(store) {
        object_merge_pull_unlocked(store, skills_dir)
    } else {
        crate::core::git_backup::pull_unlocked(skills_dir)?;
        Ok(MergeSummary {
            engine: "system".to_string(),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;

    #[test]
    fn object_merge_is_default_and_system_is_the_escape_hatch() {
        let tmp = tempfile::tempdir().unwrap();
        let store = crate::core::skill_store::SkillStore::new(&tmp.path().join("t.db")).unwrap();
        // 3d-β: on by default (no setting saved).
        assert!(object_merge_enabled(&store));
        // Users who opted in during 3d-α stay on.
        store.set_setting(SETTING_MERGE_ENGINE, "object").unwrap();
        assert!(object_merge_enabled(&store));
        // Explicit escape hatch back to the line-level merge.
        store.set_setting(SETTING_MERGE_ENGINE, "system").unwrap();
        assert!(!object_merge_enabled(&store));
    }
}
