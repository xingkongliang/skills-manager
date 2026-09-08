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
///
/// A `dst` that is itself a link is judged by its own location (parent + name),
/// never by what it resolves to. Resolving it would report our own deployed
/// link as "inside the source" and refuse — which is what broke switching an
/// already-deployed skill from symlink to copy mode. Removing that link cannot
/// touch the source (links are unlinked, not followed), so its pointee is
/// irrelevant to both hazards this guard exists for.
pub(crate) fn ensure_dst_not_inside_src(src: &Path, dst: &Path) -> Result<()> {
    // Same path, before any link-aware special casing below: if `src` is itself
    // a link, judging `dst` lexically and `src` canonically would compare a link
    // against its pointee and miss the equality. Unlinking `dst` would then
    // destroy `src`, and the copy would "succeed" from the empty directory it
    // just created in its place.
    if src == dst {
        anyhow::bail!(
            "Source and destination are the same path {:?}; refusing to copy",
            src
        );
    }
    let src_canon = src
        .canonicalize()
        .with_context(|| format!("Source {:?} does not exist or is not accessible", src))?;
    let dst_is_link = std::fs::symlink_metadata(dst)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false);
    let lexical_dst = || -> Option<PathBuf> {
        let parent = dst.parent()?.canonicalize().ok()?;
        let name = dst.file_name()?;
        Some(parent.join(name))
    };
    let dst_canon: Option<PathBuf> = if dst_is_link {
        lexical_dst()
    } else {
        dst.canonicalize().ok().or_else(lexical_dst)
    };
    // The same link reached by two spellings (`./x` vs `x`) is still the same
    // object, and the lexical/canonical mismatch above would hide it.
    if dst_is_link {
        let lexical_src = || -> Option<PathBuf> {
            let parent = src.parent()?.canonicalize().ok()?;
            Some(parent.join(src.file_name()?))
        };
        if let (Some(a), Some(b)) = (lexical_src(), dst_canon.as_ref()) {
            if a == *b {
                anyhow::bail!(
                    "Source and destination are the same path {:?}; refusing to copy",
                    src
                );
            }
        }
    }
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

/// What is actually sitting at a deployment target right now.
///
/// Classified from `symlink_metadata` only — never `is_dir()`, which follows
/// links and would report a Windows junction or a directory symlink as a real
/// directory, sending it down the `remove_dir_all` path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetState {
    /// Nothing there. Deploying cannot destroy anything.
    Absent,
    /// A link that resolves to the source we are about to deploy — ours by
    /// construction, and unlinking it cannot touch the source.
    LinkToSource,
    /// A link pointing somewhere else, or a dangling one. The link itself is
    /// user configuration even when its pointee is gone.
    ForeignLink,
    /// A real directory. Indistinguishable from a copy-mode deployment by
    /// filesystem inspection alone.
    RealDir,
    /// A real file. We never deploy a bare file, so this is always the user's.
    RealFile,
}

/// What the caller is authorized to destroy at the target.
///
/// This is an enum rather than a bool so that adding it to `sync_skill` breaks
/// every call site and forces each one to state its intent — the property worth
/// having in a fix for silent data loss (issue #363).
#[derive(Debug, Clone, Copy)]
pub enum ReplacePolicy<'a> {
    /// Only proceed when nothing of the user's can be lost. Anything we cannot
    /// prove is ours is refused.
    NoClobber,
    /// The caller found a `skill_targets` row claiming exactly this path.
    ///
    /// The row is historical intent, not proof of what is on disk now, so it
    /// only authorizes removing an object whose *current* type matches the
    /// recorded mode: a row saying `symlink` must never authorize deleting a
    /// real directory that replaced our link.
    Recorded { mode: &'a str },
    /// Explicit user intent: adopt an existing directory into the library,
    /// `--force`, or "update this copy from center".
    UserConfirmed,
}

impl ReplacePolicy<'_> {
    /// Whether this policy authorizes destroying `state`. See the module tests
    /// for the full decision table.
    fn permits(&self, state: TargetState) -> bool {
        match state {
            // Nothing to lose either way.
            TargetState::Absent | TargetState::LinkToSource => true,
            TargetState::ForeignLink => match self {
                ReplacePolicy::NoClobber => false,
                // Our own link can dangle after the library moves, so a
                // symlink row still authorizes replacing a link. A copy row
                // does not: something turned our directory into a link.
                ReplacePolicy::Recorded { mode } => *mode == "symlink",
                ReplacePolicy::UserConfirmed => true,
            },
            TargetState::RealDir => match self {
                ReplacePolicy::NoClobber => false,
                ReplacePolicy::Recorded { mode } => *mode == "copy",
                ReplacePolicy::UserConfirmed => true,
            },
            // We never deploy a bare file, so no record can vouch for one.
            TargetState::RealFile => matches!(self, ReplacePolicy::UserConfirmed),
        }
    }

    fn describe(&self) -> &'static str {
        match self {
            ReplacePolicy::NoClobber => "is not managed by Skills Manager",
            ReplacePolicy::Recorded { .. } => "does not match its recorded deployment",
            ReplacePolicy::UserConfirmed => "cannot be replaced",
        }
    }
}

/// A write was refused because the target is not ours to destroy.
///
/// A distinct type so callers can tell an ownership refusal — where the right
/// response is to report the conflict to the user — from an ordinary IO error,
/// which reconcile paths have always tolerated and logged.
#[derive(Debug)]
pub struct ReplaceRefused {
    pub target: PathBuf,
    pub reason: &'static str,
}

/// The one place this refusal is worded. `ReplaceRefused` and the
/// `TargetConflict` that carries it out to callers both render through here so
/// the two cannot drift apart.
pub fn refusal_message(target: &Path, reason: &str) -> String {
    format!(
        "Refusing to replace {target:?}: it {reason}. The existing content was left untouched — \
         import it into the library, or move it aside, and try again."
    )
}

impl std::fmt::Display for ReplaceRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&refusal_message(&self.target, self.reason))
    }
}

impl std::error::Error for ReplaceRefused {}

/// Classify `target` and check `policy` against it, returning the state that
/// may then be removed. The single place the refusal decision and its wording
/// live, so `sync_skill` and [`preflight_replace`] can never disagree.
fn authorize_replacement(
    source: &Path,
    target: &Path,
    policy: ReplacePolicy<'_>,
) -> Result<TargetState> {
    let state = classify_target(target, Some(source))?;
    if !policy.permits(state) {
        return Err(ReplaceRefused {
            target: target.to_path_buf(),
            reason: policy.describe(),
        }
        .into());
    }
    Ok(state)
}

/// Whether [`sync_skill`] would be allowed to write here, without touching
/// anything. For batch callers that must refuse the whole operation before
/// mutating any of it (#363, expectation 4).
///
/// This is a preflight, not an authorization: `sync_skill` re-checks
/// immediately before it removes anything, because the filesystem can change
/// in between.
pub fn preflight_replace(
    source: &Path,
    target: &Path,
    mode: SyncMode,
    policy: ReplacePolicy<'_>,
) -> Result<()> {
    if is_target_current(source, target, mode, None, None) {
        return Ok(());
    }
    authorize_replacement(source, target, policy).map(|_| ())
}

/// Classify what is at `target` without following links.
///
/// `source` is the deployment source, used only to recognize a link that already
/// points at it. Pass `None` when there is no source to compare against.
pub fn classify_target(target: &Path, source: Option<&Path>) -> Result<TargetState> {
    let metadata = match std::fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(TargetState::Absent),
        // A target we cannot even stat is not evidence that it is ours.
        Err(err) => {
            return Err(anyhow::Error::from(err)
                .context(format!("Cannot inspect existing target {:?}", target)));
        }
    };

    if metadata.file_type().is_symlink() {
        // `LinkToSource` is universally permitted, so it must only be reached
        // with a real source to compare against. Callers with no source (undeploy
        // and friends) pass `None`: an empty path would canonicalize to the
        // process CWD and turn a user's link into "ours".
        if source.is_some_and(|source| symlink_points_to(target, source)) {
            return Ok(TargetState::LinkToSource);
        }
        return Ok(TargetState::ForeignLink);
    }
    if metadata.is_dir() {
        Ok(TargetState::RealDir)
    } else {
        Ok(TargetState::RealFile)
    }
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

pub fn sync_skill(
    source: &Path,
    target: &Path,
    mode: SyncMode,
    policy: ReplacePolicy<'_>,
) -> Result<SyncMode> {
    // Internal self-check uses no hash context, so Copy mode always
    // proceeds — the caller (e.g. `sync_desired_targets`) is the place
    // that knows about freshness and can short-circuit.
    if is_target_current(source, target, mode, None, None) {
        return Ok(mode);
    }

    // Decide what we are allowed to destroy before touching anything. This
    // runs here, immediately before the removal, rather than only in a caller
    // preflight: a preflight result cannot authorize a later deletion (#363).
    let state = authorize_replacement(source, target, policy)?;

    // A real write to a watched dir follows: mute the watcher so this
    // app-initiated change doesn't echo back as a redundant refresh (#248).
    crate::core::file_watcher::mute_self_writes(target);

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent dir {:?}", parent))?;
    }

    ensure_dst_not_inside_src(source, target)?;

    // Remove exactly what we classified. Passing `state` keeps a directory
    // that appeared since the check from being swallowed by `remove_dir_all`:
    // the removal is type-specific, so it fails instead. Errors propagate —
    // the old `.ok()` could leave the target in place and then create over it.
    remove_classified_target(target, state)
        .with_context(|| format!("Failed to remove existing target {:?}", target))?;

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

/// Remove a target that was already classified, using an operation that only
/// works on that kind of object.
///
/// This is the difference between "we checked, then deleted whatever we found"
/// and "we deleted the thing we checked". If a real directory replaces a link
/// between the classification and this call, `remove_file` fails with an error
/// instead of `remove_dir_all` quietly eating it — the exact race that makes a
/// check-then-`remove_target` sequence still unsafe.
pub fn remove_classified_target(target: &Path, state: TargetState) -> Result<()> {
    match state {
        TargetState::Absent => Ok(()),
        TargetState::LinkToSource | TargetState::ForeignLink => {
            crate::core::file_watcher::mute_self_writes(target);
            remove_link(target)
        }
        TargetState::RealFile => {
            crate::core::file_watcher::mute_self_writes(target);
            std::fs::remove_file(target).map_err(Into::into)
        }
        TargetState::RealDir => {
            crate::core::file_watcher::mute_self_writes(target);
            // Only reachable via `Recorded { mode: "copy" }` or UserConfirmed.
            std::fs::remove_dir_all(target).map_err(Into::into)
        }
    }
}

/// Remove a deployment target only while what is on disk still matches the
/// deployment we recorded. Returns `false` when the object was preserved
/// because it no longer does.
///
/// Undeploy and cleanup paths reach a path through a `skill_targets` row, and
/// a row is a statement about the past: if the user replaced our link with a
/// real directory of their own, the row still points at it. Removing on the
/// row's word alone deletes that directory (#363, expectation 5).
///
/// Link-vs-source identity is irrelevant here — a `symlink` row authorizes
/// removing a link whether or not it still resolves to the skill we deployed
/// (the library may have moved) — so no source path is required.
pub fn remove_recorded_target(target: &Path, recorded_mode: &str) -> Result<bool> {
    let state = classify_target(target, None)?;
    let policy = ReplacePolicy::Recorded {
        mode: recorded_mode,
    };
    if !policy.permits(state) {
        return Ok(false);
    }
    remove_classified_target(target, state)?;
    Ok(true)
}

/// Whether what is at `target` is still consistent with a deployment recorded
/// as `recorded_mode`. `false` means something else took the path over, which
/// is why an undeploy may leave a path behind on purpose.
pub fn matches_recorded_deployment(target: &Path, recorded_mode: &str) -> Result<bool> {
    let state = classify_target(target, None)?;
    if state == TargetState::Absent {
        return Ok(false);
    }
    let policy = ReplacePolicy::Recorded {
        mode: recorded_mode,
    };
    Ok(policy.permits(state))
}

/// Unlink a symlink (or, on Windows, a directory symlink / junction) without
/// following it.
fn remove_link(target: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        let metadata = match std::fs::symlink_metadata(target) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        if !metadata.file_type().is_symlink() {
            anyhow::bail!(
                "Refusing to remove {:?}: expected a link but found a real file or directory",
                target
            );
        }
        // `remove_dir` on a directory link removes the link, not its contents.
        if metadata.file_type().is_symlink_dir() {
            std::fs::remove_dir(target)?;
        } else {
            std::fs::remove_file(target)?;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        // `remove_file` unlinks a symlink of either kind and refuses a real
        // directory outright, which is precisely the guarantee wanted here.
        match std::fs::remove_file(target) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }
}

/// Unconditional removal of whatever is at `target`.
///
/// **Unchecked**: this will `remove_dir_all` a real directory regardless of
/// whose it is. Deployment paths must go through [`remove_classified_target`]
/// (or [`sync_skill`]) instead; this remains for callers that have already
/// established ownership another way.
pub fn remove_target(target: &Path) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };

    // An actual removal from a watched dir follows: mute the self-write echo.
    crate::core::file_watcher::mute_self_writes(target);

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

        let mode = sync_skill(&src, &tgt, SyncMode::Copy, ReplacePolicy::NoClobber).unwrap();
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

        let mode = sync_skill(&src, &tgt, SyncMode::Symlink, ReplacePolicy::NoClobber).unwrap();
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

        let mode = sync_skill(&src, &tgt, SyncMode::Symlink, ReplacePolicy::NoClobber).unwrap();
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
        let mode = sync_skill(&src, &tgt, SyncMode::Symlink, ReplacePolicy::NoClobber).unwrap();
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

    /// The whole authorization contract in one place. Written out exhaustively
    /// so that widening any cell has to be a deliberate edit to this table
    /// rather than a side effect of touching `permits`.
    #[test]
    fn replace_policy_decision_table() {
        use TargetState::*;
        let copy = ReplacePolicy::Recorded { mode: "copy" };
        let link = ReplacePolicy::Recorded { mode: "symlink" };

        for state in [Absent, LinkToSource] {
            // Nothing of the user's exists at the target either way.
            assert!(ReplacePolicy::NoClobber.permits(state), "{state:?}");
            assert!(copy.permits(state), "{state:?}");
            assert!(link.permits(state), "{state:?}");
            assert!(ReplacePolicy::UserConfirmed.permits(state), "{state:?}");
        }

        // A foreign or dangling link is user configuration; only a symlink
        // record (ours, possibly dangling after a library move) or explicit
        // intent may replace it.
        assert!(!ReplacePolicy::NoClobber.permits(ForeignLink));
        assert!(!copy.permits(ForeignLink));
        assert!(link.permits(ForeignLink));
        assert!(ReplacePolicy::UserConfirmed.permits(ForeignLink));

        // A real directory is only ever ours under a copy record.
        assert!(!ReplacePolicy::NoClobber.permits(RealDir));
        assert!(copy.permits(RealDir));
        assert!(!link.permits(RealDir));
        assert!(ReplacePolicy::UserConfirmed.permits(RealDir));

        // We never deploy a bare file, so no record can vouch for one.
        assert!(!ReplacePolicy::NoClobber.permits(RealFile));
        assert!(!copy.permits(RealFile));
        assert!(!link.permits(RealFile));
        assert!(ReplacePolicy::UserConfirmed.permits(RealFile));
    }

    /// Without a source there is nothing a link can be "ours by pointing at",
    /// and `LinkToSource` is permitted by every policy — so an empty path here
    /// (which canonicalizes to the process CWD) would hand over a user's link.
    #[cfg(unix)]
    #[test]
    fn classify_target_without_source_never_reports_link_to_source() {
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("real");
        let link = tmp.path().join("link");
        fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        assert_eq!(
            classify_target(&link, None).unwrap(),
            TargetState::ForeignLink
        );
    }

    /// The old contract was "replace whatever is at the target". That is the
    /// data loss in #363: an unmanaged directory is not ours to delete, and no
    /// record vouches for this one.
    #[test]
    fn sync_skill_refuses_unmanaged_existing_directory() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("new.md"), "new").unwrap();

        fs::create_dir_all(&tgt).unwrap();
        fs::write(tgt.join("unmanaged.txt"), "DO_NOT_OVERWRITE").unwrap();

        let err = sync_skill(&src, &tgt, SyncMode::Copy, ReplacePolicy::NoClobber).unwrap_err();
        assert!(err.to_string().contains("Refusing to replace"), "{err}");
        // Byte-for-byte preservation is the whole point.
        assert_eq!(
            fs::read_to_string(tgt.join("unmanaged.txt")).unwrap(),
            "DO_NOT_OVERWRITE"
        );
        assert!(!tgt.join("new.md").exists());
    }

    /// A copy-mode row is what makes replacing a real directory legitimate:
    /// this is how an ordinary copy-mode re-sync updates its own target.
    #[test]
    fn sync_skill_replaces_recorded_copy_directory() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("new.md"), "new").unwrap();

        fs::create_dir_all(&tgt).unwrap();
        fs::write(tgt.join("old.md"), "old").unwrap();

        sync_skill(
            &src,
            &tgt,
            SyncMode::Copy,
            ReplacePolicy::Recorded { mode: "copy" },
        )
        .unwrap();
        assert!(tgt.join("new.md").exists());
        assert!(!tgt.join("old.md").exists());
    }

    /// A row saying "symlink" is evidence about a link, not about the real
    /// directory that replaced it. Trusting the row alone would delete content
    /// the user put there after our link went away.
    #[test]
    fn sync_skill_refuses_directory_that_replaced_a_recorded_symlink() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();

        fs::create_dir_all(&tgt).unwrap();
        fs::write(tgt.join("mine.txt"), "user content").unwrap();

        let err = sync_skill(
            &src,
            &tgt,
            SyncMode::Symlink,
            ReplacePolicy::Recorded { mode: "symlink" },
        )
        .unwrap_err();
        assert!(err.to_string().contains("Refusing to replace"), "{err}");
        assert_eq!(
            fs::read_to_string(tgt.join("mine.txt")).unwrap(),
            "user content"
        );
    }

    /// The main "does this break existing users" question. A rebuilt database
    /// has no `skill_targets` rows (the metadata snapshot does not carry them),
    /// so every already-deployed skill re-syncs with NoClobber. Symlink mode —
    /// the default — must stay fine, because the link already points at the
    /// source and nothing has to be destroyed.
    #[cfg(unix)]
    #[test]
    fn sync_skill_noclobber_accepts_our_own_existing_link_without_a_record() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();
        std::os::unix::fs::symlink(&src, &tgt).unwrap();

        assert_eq!(
            classify_target(&tgt, Some(&src)).unwrap(),
            TargetState::LinkToSource
        );
        let mode = sync_skill(&src, &tgt, SyncMode::Symlink, ReplacePolicy::NoClobber).unwrap();
        assert!(matches!(mode, SyncMode::Symlink));
        assert_eq!(fs::read_link(&tgt).unwrap(), src);
    }

    /// Adoption and `--force` are the escape hatch, and they must actually work
    /// or the refusal above would be a dead end.
    #[test]
    fn sync_skill_user_confirmed_replaces_unmanaged_directory() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("new.md"), "new").unwrap();

        fs::create_dir_all(&tgt).unwrap();
        fs::write(tgt.join("old.md"), "old").unwrap();

        sync_skill(&src, &tgt, SyncMode::Copy, ReplacePolicy::UserConfirmed).unwrap();
        assert!(tgt.join("new.md").exists());
        assert!(!tgt.join("old.md").exists());
    }

    #[test]
    fn sync_skill_refuses_unmanaged_existing_file() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();
        fs::write(&tgt, "user file").unwrap();

        // Not even a matching record can vouch for a bare file.
        for policy in [
            ReplacePolicy::NoClobber,
            ReplacePolicy::Recorded { mode: "copy" },
            ReplacePolicy::Recorded { mode: "symlink" },
        ] {
            let err = sync_skill(&src, &tgt, SyncMode::Copy, policy).unwrap_err();
            assert!(err.to_string().contains("Refusing to replace"), "{err}");
        }
        assert_eq!(fs::read_to_string(&tgt).unwrap(), "user file");
    }

    #[cfg(unix)]
    #[test]
    fn sync_skill_refuses_foreign_symlink_and_keeps_its_pointee() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let elsewhere = tmp.path().join("elsewhere");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join("mine.md"), "user content").unwrap();
        std::os::unix::fs::symlink(&elsewhere, &tgt).unwrap();

        let err =
            sync_skill(&src, &tgt, SyncMode::Symlink, ReplacePolicy::NoClobber).unwrap_err();
        assert!(err.to_string().contains("Refusing to replace"), "{err}");
        assert_eq!(fs::read_link(&tgt).unwrap(), elsewhere);
        assert!(elsewhere.join("mine.md").exists());
    }

    /// A link with nothing behind it is still the user's configuration, so
    /// NoClobber leaves it alone — but our own record may replace it, which is
    /// what happens after the library moves and our links dangle.
    #[cfg(unix)]
    #[test]
    fn sync_skill_dangling_link_refused_unmanaged_replaced_when_recorded() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let gone = tmp.path().join("gone");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();
        std::os::unix::fs::symlink(&gone, &tgt).unwrap();

        let err =
            sync_skill(&src, &tgt, SyncMode::Symlink, ReplacePolicy::NoClobber).unwrap_err();
        assert!(err.to_string().contains("Refusing to replace"), "{err}");

        sync_skill(
            &src,
            &tgt,
            SyncMode::Symlink,
            ReplacePolicy::Recorded { mode: "symlink" },
        )
        .unwrap();
        assert_eq!(fs::read_link(&tgt).unwrap(), src);
    }

    /// Switching an already-deployed skill from symlink to copy used to fail:
    /// `ensure_dst_not_inside_src` canonicalized our own link back to the
    /// source and called it infinite recursion.
    #[cfg(unix)]
    #[test]
    fn sync_skill_switches_own_link_to_copy_mode() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("source");
        let tgt = tmp.path().join("target");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "# hello").unwrap();
        std::os::unix::fs::symlink(&src, &tgt).unwrap();

        let mode = sync_skill(&src, &tgt, SyncMode::Copy, ReplacePolicy::NoClobber).unwrap();
        assert!(matches!(mode, SyncMode::Copy));
        assert!(!tgt.is_symlink());
        assert_eq!(fs::read_to_string(tgt.join("SKILL.md")).unwrap(), "# hello");
        // The source must survive having its own link removed.
        assert!(src.join("SKILL.md").exists());
    }

    /// The classify-then-remove sequence must not degrade into "delete whatever
    /// is there now" if the object changes type in between.
    #[test]
    fn remove_classified_target_will_not_delete_a_directory_it_classified_as_a_link() {
        let tmp = tempdir().unwrap();
        let swapped = tmp.path().join("swapped");
        fs::create_dir_all(&swapped).unwrap();
        fs::write(swapped.join("precious.txt"), "data").unwrap();

        // Classified as a link earlier; a real directory is there now.
        let err = remove_classified_target(&swapped, TargetState::ForeignLink).unwrap_err();
        assert!(swapped.join("precious.txt").exists(), "{err}");
    }

    #[test]
    fn remove_recorded_target_preserves_content_that_replaced_a_link() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("target");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("mine.txt"), "user content").unwrap();

        // Row says we deployed a symlink; a real directory is there now.
        assert!(!remove_recorded_target(&path, "symlink").unwrap());
        assert_eq!(
            fs::read_to_string(path.join("mine.txt")).unwrap(),
            "user content"
        );

        // A copy row does authorize removing its own directory.
        assert!(remove_recorded_target(&path, "copy").unwrap());
        assert!(!path.exists());
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
        let mode = sync_skill(&src, &tgt, SyncMode::Symlink, ReplacePolicy::NoClobber).unwrap();

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

        // Still rejected, now by the explicit same-path guard that runs before
        // the containment checks (it is what catches src == dst when both are
        // the same symlink, where lexical/canonical forms differ).
        let err = ensure_dst_not_inside_src(&src, &src).unwrap_err();
        assert!(err.to_string().contains("same path"), "{err}");
    }

    /// The symlink case the lexical-dst change opened up: unlinking `dst` would
    /// destroy `src`, and the copy would then "succeed" from the empty
    /// directory it created in its place.
    #[cfg(unix)]
    #[test]
    fn sync_skill_refuses_when_source_and_target_are_the_same_link() {
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("real");
        let link = tmp.path().join("link");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("SKILL.md"), "# hello").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let err = sync_skill(&link, &link, SyncMode::Copy, ReplacePolicy::UserConfirmed)
            .unwrap_err();
        assert!(err.to_string().contains("same path"), "{err}");
        // The link and everything behind it must survive.
        assert!(link.is_symlink());
        assert!(real.join("SKILL.md").exists());

        // Two spellings of that same link must also be caught. `..` is used
        // rather than `.` because Rust's path comparison already normalizes a
        // non-leading `.` away, so `tmp/./link == tmp/link` would exit through
        // the plain `src == dst` guard and never reach the lexical comparison.
        let aliased = tmp.path().join("real").join("..").join("link");
        assert_ne!(aliased, link, "alias must not be trivially equal");
        let err = sync_skill(&aliased, &link, SyncMode::Copy, ReplacePolicy::UserConfirmed)
            .unwrap_err();
        assert!(err.to_string().contains("same path"), "{err}");
        assert!(link.is_symlink());
        assert!(real.join("SKILL.md").exists());
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

        let err = sync_skill(&src, &tgt, SyncMode::Copy, ReplacePolicy::NoClobber).unwrap_err();
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
