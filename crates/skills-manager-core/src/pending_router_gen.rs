//! Pending router-gen marker I/O.
//!
//! When a CLI user runs `sm pack gen-router <pack>`, we drop a small JSON
//! marker under `<sm_root>/pending-router-gen/<pack_id>.json`. The
//! `pack-router-gen` Claude Code skill picks these up on the next session,
//! generates a router description/body, writes it back via the store API,
//! and deletes the marker.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// A single pending router-generation request.
///
/// `skills` carries the minimum context needed for the generator skill to
/// write a useful router description without re-querying the store.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PendingMarker {
    pub pack_id: String,
    pub pack_name: String,
    pub created_at: i64,
    /// (skill name, optional description) pairs for skills in the pack.
    pub skills: Vec<(String, Option<String>)>,
}

/// Directory where pending markers are stored, given the SM root
/// (typically `~/.skills-manager`).
pub fn markers_dir(root: &Path) -> PathBuf {
    root.join("pending-router-gen")
}

/// Write a marker for the given pack. Creates the directory if needed.
pub fn write_marker(root: &Path, marker: &PendingMarker) -> Result<()> {
    let dir = markers_dir(root);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", marker.pack_id));
    fs::write(path, serde_json::to_string_pretty(marker)?)?;
    Ok(())
}

/// List all currently-pending markers. Returns an empty vec if the
/// directory does not exist.
pub fn list_markers(root: &Path) -> Result<Vec<PendingMarker>> {
    let dir = markers_dir(root);
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.path().extension().and_then(|s| s.to_str()) == Some("json") {
            let m: PendingMarker = serde_json::from_str(&fs::read_to_string(entry.path())?)?;
            out.push(m);
        }
    }
    Ok(out)
}

/// Delete the marker for `pack_id`. No-op if it does not exist.
pub fn delete_marker(root: &Path, pack_id: &str) -> Result<()> {
    let path = markers_dir(root).join(format!("{pack_id}.json"));
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let m = PendingMarker {
            pack_id: "p1".into(),
            pack_name: "mkt-seo".into(),
            created_at: 1,
            skills: vec![("seo-audit".into(), Some("Audit".into()))],
        };
        write_marker(tmp.path(), &m).unwrap();
        let list = list_markers(tmp.path()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].pack_name, "mkt-seo");
        delete_marker(tmp.path(), "p1").unwrap();
        assert_eq!(list_markers(tmp.path()).unwrap().len(), 0);
    }

    #[test]
    fn missing_dir_yields_empty_list() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(list_markers(tmp.path()).unwrap().len(), 0);
    }

    #[test]
    fn delete_marker_missing_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        // Does not error when dir/file are absent.
        delete_marker(tmp.path(), "nonexistent").unwrap();
    }
}
