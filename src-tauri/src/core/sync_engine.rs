use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Refuse to sync when `src` and `dst` overlap in either direction (equal,
/// dst inside src, or src inside dst). Otherwise the recursive copy walks
/// into the freshly-created `dst` and produces unbounded nesting (issue #61),
/// or the pre-copy removal of `dst` deletes the source along with it (#199).
///
/// `src` must exist and canonicalize — every caller is about to read from it,
/// and all destructive steps (remove target / remove destination) happen after
/// this check, so failing here protects the existing target. A missing `dst`
/// is the normal case for a fresh install and is judged via its parent.
pub(crate) fn ensure_dst_not_inside_src(src: &Path, dst: &Path) -> Result<()> {
    let src_canon = src
        .canonicalize()
        .with_context(|| format!("Source {:?} does not exist or is not accessible", src))?;
    let dst_canon: Option<PathBuf> = dst.canonicalize().ok().or_else(|| {
        let parent = dst.parent()?.canonicalize().ok()?;
        let name = dst.file_name()?;
        Some(parent.join(name))
    });
    if let Some(dst_canon) = dst_canon {
        if dst_canon.starts_with(&src_canon) {
            anyhow::bail!(
                "Destination {:?} is inside source {:?}; refusing to copy to avoid infinite recursion",
                dst,
                src
            );
        }
        if src_canon.starts_with(&dst_canon) {
            anyhow::bail!(
                "Source {:?} is inside destination {:?}; refusing to avoid deleting the source",
                src,
                dst
            );
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum SyncMode {
    Symlink,
    Copy,
}

impl SyncMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncMode::Symlink => "symlink",
            SyncMode::Copy => "copy",
        }
    }
}

pub fn sync_mode_for_tool(_tool_key: &str, configured_mode: Option<&str>) -> SyncMode {
    match configured_mode {
        Some("copy") => SyncMode::Copy,
        Some("symlink") => SyncMode::Symlink,
        _ => SyncMode::Symlink,
    }
}

pub fn target_dir_name(central_path: &Path, skill_name: &str) -> String {
    central_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| skill_name.to_string())
}

pub fn sync_skill(source: &Path, target: &Path, mode: SyncMode) -> Result<SyncMode> {
    // Internal self-check uses no hash context, so Copy mode always
    // proceeds — the caller (e.g. `sync_desired_targets`) is the place
    // that knows about freshness and can short-circuit.
    if is_target_current(source, target, mode, None, None) {
        return Ok(mode);
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent dir {:?}", parent))?;
    }

    ensure_dst_not_inside_src(source, target)?;

    // Remove existing target
    remove_target(target).ok();

    match mode {
        SyncMode::Symlink => {
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(source, target).with_context(|| {
                    format!("Failed to create symlink {:?} -> {:?}", target, source)
                })?;
                Ok(SyncMode::Symlink)
            }
            #[cfg(windows)]
            {
                match std::os::windows::fs::symlink_dir(source, target) {
                    Ok(()) => Ok(SyncMode::Symlink),
                    Err(err) => {
                        // Typical causes: missing SeCreateSymbolicLinkPrivilege,
                        // Developer Mode disabled, or non-NTFS target volume.
                        // A directory junction needs no privilege on local NTFS
                        // volumes and is equivalent for our purposes (issue #126),
                        // so try that before degrading to a full copy. Junctions
                        // cannot point at remote/UNC paths (e.g. \\wsl.localhost),
                        // which is where the copy fallback still applies.
                        //
                        // A junction is reported back as `SyncMode::Symlink`:
                        // std treats mount points as symlinks (`is_symlink()`,
                        // `read_link`), so freshness checks and removal handle
                        // it exactly like a real directory symlink.
                        match junction::create(source, target) {
                            Ok(()) => {
                                log::info!(
                                    "symlink_dir {:?} -> {:?} failed ({err}); created directory junction instead",
                                    target,
                                    source
                                );
                                Ok(SyncMode::Symlink)
                            }
                            Err(junction_err) => {
                                log::warn!(
                                    "symlink_dir ({err}) and junction ({junction_err}) both failed for {:?} -> {:?}, falling back to copy",
                                    target,
                                    source
                                );
                                copy_dir_recursive(source, target)?;
                                Ok(SyncMode::Copy)
                            }
                        }
                    }
                }
            }
            #[cfg(all(not(unix), not(windows)))]
            {
                copy_dir_recursive(source, target)?;
                Ok(SyncMode::Copy)
            }
        }
        SyncMode::Copy => {
            copy_dir_recursive(source, target)?;
            Ok(SyncMode::Copy)
        }
    }
}

/// Decide whether the existing target is already in the desired state.
///
/// - **Symlink mode**: the target must be a symlink pointing at `source`.
/// - **Copy mode**: the target must still exist on disk **and** the
///   previously synced source hash must equal the current source hash
///   (both must be `Some`). The existence check protects against a
///   user manually deleting the synced directory between sessions —
///   without it a stale hash would cause us to skip a re-copy the
///   user needs. Callers without hash context should pass `None`,
///   which preserves the historical "always recopy" behavior. See
///   `SkillTargetRecord.source_hash` and issue #153 for context.
pub fn is_target_current(
    source: &Path,
    target: &Path,
    mode: SyncMode,
    last_synced_source_hash: Option<&str>,
    current_source_hash: Option<&str>,
) -> bool {
    match mode {
        SyncMode::Symlink => symlink_points_to(target, source),
        SyncMode::Copy => match (last_synced_source_hash, current_source_hash) {
            (Some(stored), Some(current)) if stored == current => {
                std::fs::symlink_metadata(target).is_ok()
            }
            _ => false,
        },
    }
}

fn symlink_points_to(target: &Path, source: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(target) else {
        return false;
    };
    if !metadata.file_type().is_symlink() {
        return false;
    }

    let Ok(link_target) = std::fs::read_link(target) else {
        return false;
    };
    let resolved_link_target = if link_target.is_absolute() {
        link_target
    } else {
        target
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(link_target)
    };

    if resolved_link_target == source {
        return true;
    }

    match (resolved_link_target.canonicalize(), source.canonicalize()) {
        (Ok(link), Ok(src)) => link == src,
        _ => false,
    }
}

pub fn remove_target(target: &Path) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };

    if metadata.file_type().is_symlink() {
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileTypeExt;
            // Decide from the link's own metadata: `target.is_dir()` follows
            // the link, so a dangling directory symlink/junction would be
            // misclassified as a file and `remove_file` would fail, leaving
            // a broken link behind.
            if metadata.file_type().is_symlink_dir() {
                std::fs::remove_dir(target)?;
            } else {
                std::fs::remove_file(target)?;
            }
        }
        #[cfg(not(windows))]
        {
            std::fs::remove_file(target)?;
        }
    } else if metadata.is_dir() {
        std::fs::remove_dir_all(target)?;
    } else {
        std::fs::remove_file(target)?;
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ft.is_dir() {
            let name = entry.file_name();
            if name == ".git" {
                continue;
            }
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // ── sync_mode_for_tool ──

    #[test]
    fn sync_mode_defaults_to_symlink() {
        assert!(matches!(
            sync_mode_for_tool("claude-code", None),
            SyncMode::Symlink
        ));
    }

    #[test]
    fn sync_mode_cursor_defaults_to_symlink() {
        assert!(matches!(
            sync_mode_for_tool("cursor", None),
            SyncMode::Symlink
        ));
    }

    #[test]
    fn sync_mode_explicit_copy_overrides_default() {
        assert!(matches!(
            sync_mode_for_tool("claude-code", Some("copy")),
            SyncMode::Copy
        ));
    }

    #[test]
    fn sync_mode_explicit_symlink_overrides_cursor_default() {
        assert!(matches!(
            sync_mode_for_tool("cursor", Some("symlink")),
            SyncMode::Symlink
        ));
    }

    #[test]
    fn sync_mode_unknown_config_falls_back_to_tool_default() {
        assert!(matches!(
            sync_mode_for_tool("cursor", Some("invalid")),
            SyncMode::Symlink
        ));
        assert!(matches!(
            sync_mode_for_tool("claude-code", Some("invalid")),
            SyncMode::Symlink
        ));
    }

    #[test]
    fn sync_mode_as_str() {
        assert_eq!(SyncMode::Symlink.as_str(), "symlink");
        assert_eq!(SyncMode::Copy.as_str(), "copy");
    }

    #[test]
    fn target_dir_name_uses_central_directory_name() {
        let central_path = Path::new("/central/skill123-2");

        assert_eq!(target_dir_name(central_path, "skill123"), "skill123-2");
    }

    #[test]
    fn target_dir_name_falls_back_to_skill_name() {
        assert_eq!(target_dir_name(Path::new(""), "skill123"), "skill123");
    }

    // ── sync_skill (filesystem) ──

    #[test]
    fn sync_skill_copy_creates_directory_with_files() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();

        let mode = sync_skill(&src, &tgt, SyncMode::Copy).unwrap();
        assert!(matches!(mode, SyncMode::Copy));
        assert!(tgt.join("SKILL.md").exists());
        assert_eq!(fs::read_to_string(tgt.join("SKILL.md")).unwrap(), "# hello");
    }

    #[cfg(unix)]
    #[test]
    fn sync_skill_symlink_creates_symlink() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();

        let mode = sync_skill(&src, &tgt, SyncMode::Symlink).unwrap();
        assert!(matches!(mode, SyncMode::Symlink));
        assert!(tgt.is_symlink());
    }

    #[cfg(windows)]
    #[test]
    fn sync_skill_symlink_creates_symlink_on_windows() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();

        let mode = sync_skill(&src, &tgt, SyncMode::Symlink).unwrap();
        assert!(matches!(mode, SyncMode::Symlink));
        assert!(tgt.is_symlink());
    }

    #[cfg(windows)]
    #[test]
    fn junction_target_is_recognized_and_removable() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();
        junction::create(&src, &tgt).unwrap();

        // A junction must satisfy the symlink-mode freshness check so
        // later syncs skip instead of re-creating it on every startup.
        assert!(is_target_current(&src, &tgt, SyncMode::Symlink, None, None));
        assert!(tgt.join("SKILL.md").exists());

        // And sync_skill must treat it as already current.
        let mode = sync_skill(&src, &tgt, SyncMode::Symlink).unwrap();
        assert!(matches!(mode, SyncMode::Symlink));

        remove_target(&tgt).unwrap();
        assert!(fs::symlink_metadata(&tgt).is_err());
        assert!(src.join("SKILL.md").exists());
    }

    #[cfg(windows)]
    #[test]
    fn remove_target_removes_dangling_junction() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        junction::create(&src, &tgt).unwrap();

        // Delete the junction's target so the link dangles.
        fs::remove_dir_all(&src).unwrap();

        remove_target(&tgt).unwrap();
        assert!(fs::symlink_metadata(&tgt).is_err());
    }

    #[test]
    fn sync_skill_replaces_existing_target() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("new.md"), "new").unwrap();

        // Pre-existing target directory
        fs::create_dir_all(&tgt).unwrap();
        fs::write(tgt.join("old.md"), "old").unwrap();

        sync_skill(&src, &tgt, SyncMode::Copy).unwrap();
        assert!(tgt.join("new.md").exists());
        assert!(!tgt.join("old.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn sync_skill_symlink_skips_existing_correct_link() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();
        std::os::unix::fs::symlink(&src, &tgt).unwrap();

        let before = fs::symlink_metadata(&tgt).unwrap().modified().unwrap();
        let mode = sync_skill(&src, &tgt, SyncMode::Symlink).unwrap();

        assert!(matches!(mode, SyncMode::Symlink));
        assert_eq!(fs::read_link(&tgt).unwrap(), src);
        assert_eq!(
            fs::symlink_metadata(&tgt).unwrap().modified().unwrap(),
            before
        );
    }

    // ── copy_dir_recursive ──

    #[test]
    fn copy_dir_recursive_skips_dot_git() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join(".git")).unwrap();
        fs::write(src.join(".git/config"), "git config").unwrap();
        fs::create_dir_all(src.join("subdir")).unwrap();
        fs::write(src.join("subdir/file.md"), "content").unwrap();
        fs::write(src.join("root.md"), "root").unwrap();

        let dst = tmp.path().join("dst");
        copy_dir_recursive(&src, &dst).unwrap();

        assert!(!dst.join(".git").exists());
        assert!(dst.join("subdir/file.md").exists());
        assert!(dst.join("root.md").exists());
    }

    // ── ensure_dst_not_inside_src ──

    #[test]
    fn ensure_dst_not_inside_src_rejects_subdirectory() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("skills");
        fs::create_dir_all(&src).unwrap();
        let dst = src.join("skills");

        let err = ensure_dst_not_inside_src(&src, &dst).unwrap_err();
        assert!(err.to_string().contains("infinite recursion"), "{err}");
    }

    #[test]
    fn ensure_dst_not_inside_src_rejects_same_path() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("skills");
        fs::create_dir_all(&src).unwrap();

        let err = ensure_dst_not_inside_src(&src, &src).unwrap_err();
        assert!(err.to_string().contains("infinite recursion"), "{err}");
    }

    #[test]
    fn ensure_dst_not_inside_src_allows_disjoint_paths() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("skills");
        let dst = tmp.path().join("other").join("skills");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(dst.parent().unwrap()).unwrap();

        ensure_dst_not_inside_src(&src, &dst).unwrap();
    }

    #[test]
    fn ensure_dst_not_inside_src_allows_sibling_dst() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("skills");
        let dst = tmp.path().join("skills-disabled");
        fs::create_dir_all(&src).unwrap();

        ensure_dst_not_inside_src(&src, &dst).unwrap();
    }

    #[test]
    fn ensure_dst_not_inside_src_rejects_missing_source() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("does-not-exist");
        let dst = tmp.path().join("target");
        fs::create_dir_all(&dst).unwrap();

        let err = ensure_dst_not_inside_src(&src, &dst).unwrap_err();
        assert!(err.to_string().contains("not accessible"), "{err}");
    }

    #[test]
    fn ensure_dst_not_inside_src_rejects_source_inside_destination() {
        let tmp = tempdir().unwrap();
        let dst = tmp.path().join("skills");
        let src = dst.join("nested");
        fs::create_dir_all(&src).unwrap();

        let err = ensure_dst_not_inside_src(&src, &dst).unwrap_err();
        assert!(err.to_string().contains("deleting the source"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dst_not_inside_src_rejects_dangling_symlink_source() {
        // A dangling symlink source used to slip past the guard (canonicalize
        // failure returned Ok) and let the caller delete the destination
        // before the copy inevitably failed (#199 hardening).
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("dangling");
        std::os::unix::fs::symlink(tmp.path().join("gone"), &src).unwrap();
        let dst = tmp.path().join("target");
        fs::create_dir_all(&dst).unwrap();

        let err = ensure_dst_not_inside_src(&src, &dst).unwrap_err();
        assert!(err.to_string().contains("not accessible"), "{err}");
    }

    #[test]
    fn sync_skill_refuses_target_inside_source() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("skills");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();
        let tgt = src.join("skills");

        let err = sync_skill(&src, &tgt, SyncMode::Copy).unwrap_err();
        assert!(err.to_string().contains("infinite recursion"), "{err}");
        // Source must be untouched after the rejection.
        assert!(src.join("SKILL.md").exists());
    }

    // ── remove_target ──

    #[test]
    fn remove_target_removes_directory() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("to_remove");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("file.txt"), "data").unwrap();

        remove_target(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn remove_target_removes_file() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("file.txt");
        fs::write(&file, "data").unwrap();

        remove_target(&file).unwrap();
        assert!(!file.exists());
    }

    #[cfg(unix)]
    #[test]
    fn remove_target_removes_symlink() {
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("real");
        fs::create_dir_all(&real).unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        remove_target(&link).unwrap();
        assert!(!link.exists());
        assert!(real.exists()); // original untouched
    }

    #[cfg(windows)]
    #[test]
    fn remove_target_removes_directory_symlink() {
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("real");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("SKILL.md"), "# hello").unwrap();
        let link = tmp.path().join("link");
        std::os::windows::fs::symlink_dir(&real, &link).unwrap();

        remove_target(&link).unwrap();
        assert!(!link.exists());
        assert!(real.exists());
        assert!(real.join("SKILL.md").exists());
    }

    #[test]
    fn remove_target_nonexistent_is_ok() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("does_not_exist");
        assert!(remove_target(&path).is_ok());
    }

    // ── is_target_current copy-mode freshness (issue #153) ──

    #[test]
    fn is_target_current_copy_skips_when_hashes_match_and_target_exists() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&tgt).unwrap();
        assert!(is_target_current(
            &src,
            &tgt,
            SyncMode::Copy,
            Some("hash-abc"),
            Some("hash-abc"),
        ));
    }

    #[test]
    fn is_target_current_copy_resyncs_when_target_missing_even_if_hashes_match() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target-that-was-deleted");
        // User deleted the synced directory manually; we must re-copy.
        assert!(!is_target_current(
            &src,
            &tgt,
            SyncMode::Copy,
            Some("hash-abc"),
            Some("hash-abc"),
        ));
    }

    #[test]
    fn is_target_current_copy_resyncs_when_hashes_differ() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&tgt).unwrap();
        assert!(!is_target_current(
            &src,
            &tgt,
            SyncMode::Copy,
            Some("hash-old"),
            Some("hash-new"),
        ));
    }

    #[test]
    fn is_target_current_copy_resyncs_when_either_hash_missing() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&tgt).unwrap();
        // No previously recorded hash → must resync (e.g. row predates v6).
        assert!(!is_target_current(
            &src,
            &tgt,
            SyncMode::Copy,
            None,
            Some("hash-abc"),
        ));
        // Source has no current hash → must resync (defensive).
        assert!(!is_target_current(
            &src,
            &tgt,
            SyncMode::Copy,
            Some("hash-abc"),
            None,
        ));
        // Both missing → must resync.
        assert!(!is_target_current(&src, &tgt, SyncMode::Copy, None, None));
    }
}
