//! Merge protocol markers (merge-engine design §6).
//!
//! Every commit the app creates carries a `Skills-Manager-Protocol: 2`
//! trailer and guarantees `.skills-manager/protocol.json` exists in the tree
//! (sticky — restoring a pre-protocol snapshot self-heals on the next
//! commit). Together these let the object-merge engine detect writes made by
//! clients that do not understand the pairing rules: a commit whose tree has
//! `protocol.json` but whose message lacks the trailer was written by an old
//! client.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Object-merge protocol generation. Bump when merge semantics change in a
/// way old clients must not mix with.
pub const MERGE_PROTOCOL_VERSION: u32 = 2;
const PROTOCOL_SCHEMA_VERSION: u32 = 1;

pub const TRAILER_PROTOCOL: &str = "Skills-Manager-Protocol";
pub const TRAILER_CONFLICTS: &str = "Skills-Manager-Conflicts";
pub const TRAILER_RESOLVED: &str = "Skills-Manager-Resolved";

/// Repo-relative path of the protocol marker file.
pub const PROTOCOL_FILE_REL: &str = ".skills-manager/protocol.json";

/// Cap on ids in a single `Skills-Manager-Conflicts:` trailer (§4). Overflow
/// is recorded as `+N` and blocks the automatic path.
pub const CONFLICTS_TRAILER_CAP: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolFile {
    pub schema_version: u32,
    pub merge_protocol: u32,
}

impl Default for ProtocolFile {
    fn default() -> Self {
        Self {
            schema_version: PROTOCOL_SCHEMA_VERSION,
            merge_protocol: MERGE_PROTOCOL_VERSION,
        }
    }
}

/// Canonical serialized bytes for `protocol.json`. Both sides of a merge must
/// produce byte-identical blobs for the tree-OID convergence guarantee (§10).
pub fn protocol_file_bytes(file: &ProtocolFile) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(file)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Make sure `protocol.json` exists in the working tree with at least the
/// current protocol version (sticky: never lowers an existing version).
/// Called before every app commit so a restore of a pre-protocol snapshot
/// self-heals.
pub fn ensure_protocol_file(skills_dir: &Path) -> Result<()> {
    let path = skills_dir.join(PROTOCOL_FILE_REL);
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(existing) = serde_json::from_str::<ProtocolFile>(&raw) {
            if existing.merge_protocol >= MERGE_PROTOCOL_VERSION {
                return Ok(());
            }
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, protocol_file_bytes(&ProtocolFile::default())?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Append the protocol trailer to a commit message (`app_commit`, §6). Every
/// commit the app makes — manual backup, auto backup, init, restore, merge,
/// conflict resolution — goes through this.
pub fn app_commit_message(message: &str) -> String {
    format!(
        "{}\n\n{}: {}",
        message.trim_end(),
        TRAILER_PROTOCOL,
        MERGE_PROTOCOL_VERSION
    )
}

/// Whether a commit message carries the protocol trailer.
pub fn has_protocol_trailer(message: &str) -> bool {
    message
        .lines()
        .any(|line| line.trim_start().starts_with(&format!("{TRAILER_PROTOCOL}:")))
}

/// Parse the comma-separated skill ids of a `key: id1, id2` trailer line.
/// Overflow markers (`+N`) are skipped by the id filter.
pub fn parse_trailer_ids(message: &str, key: &str) -> Vec<String> {
    let prefix = format!("{key}:");
    let mut ids = Vec::new();
    for line in message.lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix(&prefix) {
            for id in rest.split(',') {
                let id = id.trim();
                if !id.is_empty() && !id.starts_with('+') {
                    ids.push(id.to_string());
                }
            }
        }
    }
    ids
}

/// Format the conflicts trailer line for a merge commit, honoring the id cap.
/// Returns `(line, overflowed)`; callers must block the automatic path when
/// `overflowed` is true (§4 — practically unreachable).
pub fn format_conflicts_trailer(ids: &[String]) -> Option<(String, bool)> {
    if ids.is_empty() {
        return None;
    }
    let mut sorted: Vec<&String> = ids.iter().collect();
    sorted.sort();
    let shown: Vec<&str> = sorted
        .iter()
        .take(CONFLICTS_TRAILER_CAP)
        .map(|s| s.as_str())
        .collect();
    let overflow = sorted.len().saturating_sub(CONFLICTS_TRAILER_CAP);
    let mut line = format!("{TRAILER_CONFLICTS}: {}", shown.join(", "));
    if overflow > 0 {
        line.push_str(&format!(", +{overflow}"));
    }
    Some((line, overflow > 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_protocol_file_creates_and_is_sticky() {
        let tmp = tempfile::tempdir().unwrap();
        ensure_protocol_file(tmp.path()).unwrap();
        let raw = std::fs::read_to_string(tmp.path().join(PROTOCOL_FILE_REL)).unwrap();
        let parsed: ProtocolFile = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.merge_protocol, MERGE_PROTOCOL_VERSION);

        // A future/higher version is never lowered.
        std::fs::write(
            tmp.path().join(PROTOCOL_FILE_REL),
            r#"{"schema_version":1,"merge_protocol":99}"#,
        )
        .unwrap();
        ensure_protocol_file(tmp.path()).unwrap();
        let raw = std::fs::read_to_string(tmp.path().join(PROTOCOL_FILE_REL)).unwrap();
        let parsed: ProtocolFile = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.merge_protocol, 99);

        // A corrupt file is rewritten.
        std::fs::write(tmp.path().join(PROTOCOL_FILE_REL), "not json").unwrap();
        ensure_protocol_file(tmp.path()).unwrap();
        let raw = std::fs::read_to_string(tmp.path().join(PROTOCOL_FILE_REL)).unwrap();
        assert!(serde_json::from_str::<ProtocolFile>(&raw).is_ok());
    }

    #[test]
    fn app_commit_message_appends_trailer_once() {
        let msg = app_commit_message("backup");
        assert_eq!(msg, "backup\n\nSkills-Manager-Protocol: 2");
        assert!(has_protocol_trailer(&msg));
        assert!(!has_protocol_trailer("backup"));
    }

    #[test]
    fn parse_trailer_ids_splits_and_skips_overflow_marker() {
        let msg = "sync: merged\n\nSkills-Manager-Protocol: 2\nSkills-Manager-Conflicts: a, b, +3";
        assert_eq!(parse_trailer_ids(msg, TRAILER_CONFLICTS), vec!["a", "b"]);
        assert!(parse_trailer_ids(msg, TRAILER_RESOLVED).is_empty());
    }

    #[test]
    fn conflicts_trailer_caps_at_twenty_ids() {
        let ids: Vec<String> = (0..25).map(|i| format!("id-{i:02}")).collect();
        let (line, overflowed) = format_conflicts_trailer(&ids).unwrap();
        assert!(overflowed);
        assert!(line.contains("+5"));
        assert_eq!(line.matches("id-").count(), 20);
        assert!(format_conflicts_trailer(&[]).is_none());

        let (line, overflowed) =
            format_conflicts_trailer(&["b".to_string(), "a".to_string()]).unwrap();
        assert!(!overflowed);
        // Sorted for determinism across devices.
        assert_eq!(line, "Skills-Manager-Conflicts: a, b");
    }
}
