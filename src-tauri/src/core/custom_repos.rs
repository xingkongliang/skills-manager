//! Persistent list of user-added custom skill repositories ("My Repos").
//!
//! Storage is a single settings row (`CUSTOM_REPOS_SETTING_KEY`) holding a JSON
//! array of [`CustomRepoRecord`]. Adding a repository never touches the network:
//! the URL is validated and canonicalized locally, and an already-known
//! canonical URL resolves to the existing record, so `owner/repo`,
//! `owner/repo.git` and the full https spelling all map to one entry. Removing
//! a record only drops the bookmark — installed skills are independent rows and
//! are never touched.

use crate::core::error::AppError;
use crate::core::git_fetcher::{parse_git_source, validate_git_url};
use crate::core::skill_store::SkillStore;
use serde::{Deserialize, Serialize};

/// Settings key holding the JSON array of custom repositories.
pub const CUSTOM_REPOS_SETTING_KEY: &str = "custom_skill_repos";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomRepoRecord {
    pub id: String,
    /// Canonical clone URL — the dedup key, and what scanning/installing uses.
    pub url: String,
    /// Display label ("owner/repo"), derived once at add time.
    pub label: String,
    /// Unix epoch milliseconds.
    pub added_at: i64,
    /// Unix epoch milliseconds of the last *successful network fetch* of this
    /// repository (a refresh preview). Absent while never fetched — including
    /// every record stored before these fields existed, hence the serde
    /// defaults: old JSON without them still parses as None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fetch_at: Option<u64>,
    /// Skill count that fetch saw. Same lifecycle as `last_fetch_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fetch_count: Option<u32>,
}

/// All saved repositories, in insertion order.
pub fn list(store: &SkillStore) -> Result<Vec<CustomRepoRecord>, AppError> {
    read_records(store)
}

/// Validate, canonicalize, and persist a repository URL. Offline by design.
///
/// Idempotent on the canonical URL: re-adding any spelling of an already-known
/// repository returns the existing record unchanged.
pub fn add(store: &SkillStore, url: &str) -> Result<CustomRepoRecord, AppError> {
    let canonical = canonical_repo_url(url)?;
    let mut records = read_records(store)?;
    if let Some(existing) = records.iter().find(|record| record.url == canonical) {
        return Ok(existing.clone());
    }
    let record = CustomRepoRecord {
        id: uuid::Uuid::new_v4().to_string(),
        url: canonical.clone(),
        label: derive_label(&canonical),
        added_at: now_millis(),
        // New records have never been fetched — the first scan fills these in.
        last_fetch_at: None,
        last_fetch_count: None,
    };
    records.push(record.clone());
    write_records(store, &records)?;
    Ok(record)
}

/// Remove one repository bookmark by id. Unknown ids are a loud
/// [`ErrorKind::NotFound`](crate::core::error::ErrorKind::NotFound).
pub fn remove(store: &SkillStore, id: &str) -> Result<(), AppError> {
    let mut records = read_records(store)?;
    let before = records.len();
    records.retain(|record| record.id != id);
    if records.len() == before {
        return Err(AppError::not_found(format!(
            "Custom repository '{id}' not found"
        )));
    }
    write_records(store, &records)
}

/// Record fetch metadata on the repository whose canonical URL matches `url`.
///
/// A best-effort annotation written after a refresh preview succeeded: the
/// input is canonicalized with the same rule [`add`] uses, so any spelling of
/// the repository finds the record. Invalid input and unknown URLs are no-op
/// successes rather than errors — the scan the caller is annotating already
/// succeeded, and failing it over bookkeeping would trade a working feature
/// for a timestamp. Storage errors still propagate (a genuinely broken store
/// is worth hearing about from somewhere louder).
pub fn note_fetch(store: &SkillStore, url: &str, count: u32) -> Result<(), AppError> {
    let Ok(canonical) = canonical_repo_url(url) else {
        return Ok(());
    };
    let mut records = read_records(store)?;
    let Some(record) = records.iter_mut().find(|record| record.url == canonical) else {
        return Ok(());
    };
    record.last_fetch_at = Some(now_millis().max(0) as u64);
    record.last_fetch_count = Some(count);
    write_records(store, &records)
}

/// Overwrite the saved repository list with a fresh empty one.
///
/// The escape hatch for a corrupted `custom_skill_repos` value: [`list`],
/// [`add`] and [`remove`] all fail loudly (by design) while the stored JSON
/// does not parse, so without this the UI could only offer a retry that can
/// never succeed. This deliberately does NOT read the old value first —
/// reading is exactly what is broken — so it works even then. Saved bookmarks
/// are lost; installed skills are independent rows and are never touched.
pub fn reset(store: &SkillStore) -> Result<(), AppError> {
    store
        .set_setting(CUSTOM_REPOS_SETTING_KEY, "[]")
        .map_err(AppError::db)
}

fn read_records(store: &SkillStore) -> Result<Vec<CustomRepoRecord>, AppError> {
    let raw = store
        .get_setting(CUSTOM_REPOS_SETTING_KEY)
        .map_err(AppError::db)?;
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&raw).map_err(|err| {
        // Loud failure on corruption. The tempting alternative — treat the
        // unparsable value as "no repositories" and overwrite it on the next
        // add — would silently delete the user's saved list.
        AppError::internal(format!(
            "Custom repository list under '{CUSTOM_REPOS_SETTING_KEY}' is corrupted: {err}"
        ))
    })
}

fn write_records(store: &SkillStore, records: &[CustomRepoRecord]) -> Result<(), AppError> {
    let serialized = serde_json::to_string(records)
        .map_err(|err| AppError::internal(format!("Failed to serialize custom repos: {err}")))?;
    store
        .set_setting(CUSTOM_REPOS_SETTING_KEY, &serialized)
        .map_err(AppError::db)
}

/// Canonicalize a user-supplied repository reference to the URL stored as the
/// record's identity.
///
/// `validate_git_url` gates the input, then `parse_git_source` resolves
/// `owner/repo` shorthand (with or without a trailing `.git`) and GitHub
/// `tree/<branch>/<path>` URLs down to a repo-root clone URL — branch and
/// subpath are dropped, since a source always points at the whole repository.
/// The trailing `.git` is stripped afterwards so the shorthand expansion
/// (`.../repo.git`) and a plain full URL (`.../repo`) of one repository share
/// a single record.
fn canonical_repo_url(input: &str) -> Result<String, AppError> {
    validate_git_url(input)
        .map_err(|err| AppError::invalid_input(format!("Invalid repository URL: {err}")))?;

    let trimmed = input.trim().trim_end_matches('/');
    let clone_url = parse_git_source(trimmed).clone_url;
    let clone_url = clone_url.trim_end_matches('/');
    Ok(clone_url
        .strip_suffix(".git")
        .unwrap_or(clone_url)
        .to_string())
}

/// Derive the display label ("owner/repo") from a canonical clone URL: drop
/// the scheme, drop the host (or the `git@host:` prefix), then keep the last
/// two path segments. Deeply nested hosts (e.g. GitLab subgroups) show only
/// the final two segments; anything without two segments falls back to the
/// full URL rather than inventing a label.
fn derive_label(canonical_url: &str) -> String {
    let without_scheme = canonical_url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(canonical_url);
    let path = without_scheme
        .split_once(':')
        .map(|(_, rest)| rest)
        .unwrap_or(without_scheme);
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match segments.as_slice() {
        [.., owner, repo] => format!("{owner}/{repo}"),
        _ => canonical_url.to_string(),
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// The TempDir must outlive the store, so tests keep both.
    fn test_store() -> (TempDir, SkillStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = SkillStore::new(&tmp.path().join("test.db")).unwrap();
        (tmp, store)
    }

    #[test]
    fn add_dedupes_equivalent_url_spellings() {
        let (_tmp, store) = test_store();

        let first = add(&store, "owner/repo").unwrap();
        assert_eq!(first.url, "https://github.com/owner/repo");

        for spelling in [
            "owner/repo",
            "owner/repo.git",
            "https://github.com/owner/repo",
            "https://github.com/owner/repo.git",
            "https://github.com/owner/repo/",
            // A tree URL degrades to the repository root.
            "https://github.com/owner/repo/tree/main/skills",
        ] {
            let again = add(&store, spelling).unwrap();
            assert_eq!(again.id, first.id, "spelling '{spelling}' must dedupe");
        }

        assert_eq!(list(&store).unwrap().len(), 1);
    }

    #[test]
    fn add_derives_label_from_canonical_url() {
        let (_tmp, store) = test_store();

        let record = add(&store, "doccker/cc-use-exp").unwrap();
        assert_eq!(record.label, "doccker/cc-use-exp");
        assert!(!record.id.is_empty());
        assert!(record.added_at > 0);

        // GitLab-style nested groups show only the last two segments.
        let nested = add(&store, "https://gitlab.com/group/sub/repo").unwrap();
        assert_eq!(nested.label, "sub/repo");

        // SCP-style URLs keep their transport but still get a "owner/repo" label.
        let scp = add(&store, "git@github.com:owner/scp-repo.git").unwrap();
        assert_eq!(scp.url, "git@github.com:owner/scp-repo");
        assert_eq!(scp.label, "owner/scp-repo");

        // ssh:// is preserved verbatim (parse_git_source would mangle it).
        let ssh = add(&store, "ssh://git@github.com/owner/ssh-repo.git").unwrap();
        assert_eq!(ssh.url, "ssh://git@github.com/owner/ssh-repo");
        assert_eq!(ssh.label, "owner/ssh-repo");
    }

    #[test]
    fn add_rejects_invalid_urls_without_persisting() {
        let (_tmp, store) = test_store();

        for bad in ["", "not-a-url", "../relative/path", "/absolute/path"] {
            let err = add(&store, bad).unwrap_err();
            assert!(
                matches!(err.kind, crate::core::error::ErrorKind::InvalidInput),
                "'{bad}' should be rejected as invalid input, got {:?}",
                err.kind
            );
        }

        assert!(list(&store).unwrap().is_empty());
    }

    #[test]
    fn remove_missing_id_returns_not_found() {
        let (_tmp, store) = test_store();

        let err = remove(&store, "no-such-id").unwrap_err();
        assert!(matches!(err.kind, crate::core::error::ErrorKind::NotFound));
    }

    #[test]
    fn remove_deletes_only_the_target_record() {
        let (_tmp, store) = test_store();

        let keep = add(&store, "owner/keep").unwrap();
        let drop = add(&store, "owner/drop").unwrap();

        remove(&store, &drop.id).unwrap();

        let remaining = list(&store).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, keep.id);

        // Removing again is a not-found, not a silent no-op.
        let err = remove(&store, &drop.id).unwrap_err();
        assert!(matches!(err.kind, crate::core::error::ErrorKind::NotFound));
    }

    #[test]
    fn corrupted_settings_fail_loudly_and_are_never_cleared() {
        let (_tmp, store) = test_store();
        add(&store, "owner/saved").unwrap();
        let poisoned = "{not valid json";
        store
            .set_setting(CUSTOM_REPOS_SETTING_KEY, poisoned)
            .unwrap();

        for kind in ["list", "add", "remove"] {
            let err = match kind {
                "list" => list(&store).unwrap_err(),
                "add" => add(&store, "owner/other").unwrap_err(),
                _ => remove(&store, "any-id").unwrap_err(),
            };
            assert!(
                matches!(err.kind, crate::core::error::ErrorKind::Internal),
                "{kind} on corrupted settings must fail as internal, got {:?}",
                err.kind
            );
        }

        // The broken value is still there — nothing overwrote (cleared) it.
        assert_eq!(
            store.get_setting(CUSTOM_REPOS_SETTING_KEY).unwrap().as_deref(),
            Some(poisoned)
        );
    }

    #[test]
    fn reset_recovers_a_corrupted_list() {
        let (_tmp, store) = test_store();
        add(&store, "owner/saved").unwrap();
        store
            .set_setting(CUSTOM_REPOS_SETTING_KEY, "{not valid json")
            .unwrap();

        // While the value is poisoned every entry point fails — and a retry
        // alone can never fix that, which is exactly why reset exists.
        assert!(list(&store).is_err());
        assert!(add(&store, "owner/other").is_err());

        reset(&store).unwrap();
        assert!(list(&store).unwrap().is_empty());

        // The store is writable again.
        let record = add(&store, "owner/after-reset").unwrap();
        let records = list(&store).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, record.id);
    }

    #[test]
    fn blank_setting_reads_as_empty_list() {
        let (_tmp, store) = test_store();
        store.set_setting(CUSTOM_REPOS_SETTING_KEY, "").unwrap();

        assert!(list(&store).unwrap().is_empty());

        // And adding on top of a blank value starts a fresh list.
        let record = add(&store, "owner/first").unwrap();
        let records = list(&store).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, record.id);
    }

    #[test]
    fn records_stored_before_fetch_metadata_parse_with_none() {
        let (_tmp, store) = test_store();
        // The exact shape written by versions before `last_fetch_at` /
        // `last_fetch_count` existed: no such keys at all.
        let legacy = r#"[{"id":"legacy-1","url":"https://github.com/owner/old","label":"owner/old","added_at":1700000000000}]"#;
        store.set_setting(CUSTOM_REPOS_SETTING_KEY, legacy).unwrap();

        let records = list(&store).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].last_fetch_at, None);
        assert_eq!(records[0].last_fetch_count, None);

        // And the record is still fully usable: adding through the normal path
        // round-trips it without losing the legacy fields.
        add(&store, "owner/new").unwrap();
        let records = list(&store).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].url, "https://github.com/owner/old");
        assert_eq!(records[0].last_fetch_at, None);
    }

    #[test]
    fn note_fetch_updates_the_matching_record_any_spelling() {
        let (_tmp, store) = test_store();
        let target = add(&store, "owner/target").unwrap();
        let other = add(&store, "owner/other").unwrap();
        // Never fetched: the baseline is absent, not zero.
        assert_eq!(list(&store).unwrap()[0].last_fetch_at, None);

        // The scan uses whatever URL form the user typed; note_fetch resolves
        // it through the same canonicalization as add.
        note_fetch(&store, "https://github.com/owner/target.git", 24).unwrap();

        let records = list(&store).unwrap();
        let updated = records.iter().find(|r| r.id == target.id).unwrap();
        assert!(updated.last_fetch_at.unwrap() > 0);
        assert_eq!(updated.last_fetch_count, Some(24));
        let untouched = records.iter().find(|r| r.id == other.id).unwrap();
        assert_eq!(untouched.last_fetch_at, None);
        assert_eq!(untouched.last_fetch_count, None);

        // A later fetch overwrites, not accumulates.
        note_fetch(&store, "owner/target", 25).unwrap();
        let updated = list(&store).unwrap()[0].clone();
        assert_eq!(updated.last_fetch_count, Some(25));
    }

    #[test]
    fn note_fetch_is_a_noop_for_unknown_or_invalid_urls() {
        let (_tmp, store) = test_store();
        add(&store, "owner/saved").unwrap();
        let before = list(&store).unwrap();

        // Unknown repository: nothing to annotate, still a success.
        note_fetch(&store, "owner/never-added", 3).unwrap();
        // Invalid input cannot be canonicalized — same best-effort contract.
        note_fetch(&store, "not a url", 3).unwrap();

        assert_eq!(list(&store).unwrap(), before);
    }
}
