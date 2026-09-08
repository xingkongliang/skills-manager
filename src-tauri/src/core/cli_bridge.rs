//! The copy of `skills-manager-cli` that agents are told to run.
//!
//! The CLI already ships inside the desktop bundle — Tauri packages every
//! `[[bin]]` of the crate — but nothing puts it anywhere an agent can find it.
//! On Linux the `.deb`/`.rpm` land it in `/usr/bin`, which is already on PATH;
//! on macOS it sits inside the `.app`, and on Windows inside the install
//! directory. Neither is on PATH, and putting it there is not ours to do:
//! `/usr/local/bin` needs an admin prompt, `~/.local/bin` is not on the default
//! macOS PATH at all, and Windows reserves symlink creation for administrators.
//!
//! So the app maintains its own copy at a fixed, predictable path instead, and
//! the `manage-skills` skill looks there first. A copy, not a symlink: an
//! AppImage is mounted at a temporary path, and a `.app` the user drags
//! elsewhere would leave a dangling link.
//!
//! ## Why the stamp file
//!
//! The copy can fail — on Windows the old binary may be running (an agent
//! mid-command) or held by a virus scanner, and then the rename cannot replace
//! it. A stale bridge is not merely out of date: before #363 the deploy path
//! deleted a user's unmanaged directory outright, so an old binary can be one
//! that destroys data a new one refuses to touch. The database's own
//! `user_version` gate does not catch this, because such a fix carries no
//! schema change.
//!
//! The stamp is therefore written last and removed first. Its presence means
//! "a copy of exactly this version completed and ran"; its absence means the
//! bridge must not be used, whether or not a binary is sitting there. It is a
//! small text file, so it can still be removed when the locked binary cannot.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::central_repo;

const BRIDGE_BIN_NAME: &str = if cfg!(windows) {
    "skills-manager-cli.exe"
} else {
    "skills-manager-cli"
};

/// Where the bridge lives. Deliberately the home directory rather than
/// `central_repo::base_dir()`: the library can be relocated to anywhere the
/// user likes, and the skill has to be able to name this path without asking.
pub fn bridge_dir() -> PathBuf {
    central_repo::home_base_dir().join("bin")
}

pub fn bridge_path() -> PathBuf {
    bridge_dir().join(BRIDGE_BIN_NAME)
}

fn stamp_path() -> PathBuf {
    bridge_dir().join(".version")
}

/// The CLI that shipped alongside the running app binary.
fn bundled_cli() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("cannot locate the running executable")?;
    let dir = exe
        .parent()
        .context("the running executable has no parent directory")?;
    let candidate = dir.join(BRIDGE_BIN_NAME);
    if !candidate.is_file() {
        bail!(
            "this build does not ship {BRIDGE_BIN_NAME} next to the app binary ({})",
            dir.display()
        );
    }
    Ok(candidate)
}

/// An empty stamp counts as no stamp — that is how `invalidate_bridge` disables
/// a file it cannot delete. The skill's own check uses `-s` for the same reason.
fn read_stamp() -> Option<String> {
    std::fs::read_to_string(stamp_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Make the current bridge untrusted before touching the binary.
///
/// A stamp that survives a failed republish is the one way the whole scheme
/// breaks: the old binary and its old stamp agree with each other, so nothing
/// downstream can tell they are stale. Three fallbacks, cheapest first, each
/// enough on its own to satisfy "an unstamped or absent binary is never run":
///
/// 1. unlink the stamp — the normal path;
/// 2. truncate it — a file that cannot be removed but can still be written
///    (the reader treats an empty stamp as no stamp);
/// 3. remove the binary itself — on Windows a locked or read-only `.version`
///    can defeat both of the above while the executable beside it is still
///    removable, and no binary is as good as no stamp.
///
/// If none of them works, the publish is abandoned rather than run with a
/// stamp that lies.
fn invalidate_bridge() -> Result<()> {
    let stamp = stamp_path();
    match std::fs::remove_file(&stamp) {
        Ok(()) => return Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => log::warn!("cli bridge: could not remove {}: {e}", stamp.display()),
    }
    if std::fs::write(&stamp, b"").is_ok() {
        return Ok(());
    }
    log::warn!("cli bridge: could not truncate {}", stamp.display());
    let binary = bridge_path();
    match std::fs::remove_file(&binary) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(anyhow::Error::new(e).context(format!(
            "cannot invalidate the published CLI: neither {} nor {} could be removed",
            stamp.display(),
            binary.display()
        ))),
    }
}

/// Build the command that runs the published CLI, without a console window.
///
/// The CLI is a console-subsystem binary and the app is not, so spawning it
/// plainly makes Windows open a console window for it — a black flash on the
/// first launch after every install (#413). Every other spawn in this crate
/// hides it the same way; a new one here has to as well.
fn cli_command(path: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Run the freshly copied binary. A copy that cannot report its own version is
/// truncated, blocked, or built for another architecture — publishing it would
/// hand agents a binary that fails on first use.
fn verify(path: &Path, expected_version: &str) -> Result<()> {
    let output = cli_command(path)
        .arg("--version")
        .output()
        .with_context(|| format!("could not run {}", path.display()))?;
    if !output.status.success() {
        bail!("{} --version exited with {}", path.display(), output.status);
    }
    // Whole-token equality, not `contains`: 1.35.1 would otherwise accept a
    // binary reporting 1.35.10, and catching exactly that mismatch is the
    // reason this check exists.
    let reported = String::from_utf8_lossy(&output.stdout);
    let version = reported.split_whitespace().last().unwrap_or_default();
    if version != expected_version {
        bail!(
            "{} reports {:?}, expected version {expected_version}",
            path.display(),
            reported.trim()
        );
    }
    Ok(())
}

/// Publish the bundled CLI to the bridge path, replacing whatever is there.
///
/// Best-effort by contract: every failure leaves the stamp absent and is
/// logged, and the caller carries on. The app must start whether or not an
/// agent can drive it.
pub fn ensure_bridge(app_version: &str) {
    match ensure_bridge_inner(app_version) {
        Ok(path) => log::info!("cli bridge: ready at {}", path.display()),
        Err(e) => log::warn!("cli bridge: not available: {e:#}"),
    }
}

fn ensure_bridge_inner(app_version: &str) -> Result<PathBuf> {
    // Fast path: this version already published and still present. Checked
    // before anything is removed so an ordinary launch does not spend 15 MB of
    // copying, and so a running agent's binary is not disturbed for no reason.
    if read_stamp().as_deref() == Some(app_version) && bridge_path().is_file() {
        return Ok(bridge_path());
    }
    publish_from(&bundled_cli()?, app_version)
}

fn publish_from(source: &Path, app_version: &str) -> Result<PathBuf> {
    let target = bridge_path();

    // From here the bridge is not to be trusted until the new copy lands.
    invalidate_bridge()?;

    let dir = bridge_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("could not create {}", dir.display()))?;

    // Copy beside the target so the rename stays on one filesystem.
    let staged = dir.join(format!(".{BRIDGE_BIN_NAME}.staged"));
    let _ = std::fs::remove_file(&staged);
    std::fs::copy(source, &staged)
        .with_context(|| format!("could not copy {} to {}", source.display(), staged.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .context("could not make the staged CLI executable")?;
    }

    if let Err(e) = verify(&staged, app_version) {
        let _ = std::fs::remove_file(&staged);
        return Err(e);
    }

    // On Windows this fails while the old binary is running or held open by a
    // scanner. That is the case the stamp exists for: the stale binary stays,
    // unusable, rather than being silently presented as current.
    std::fs::rename(&staged, &target).with_context(|| {
        format!(
            "could not replace {} (it may be in use)",
            target.display()
        )
    })?;

    std::fs::write(stamp_path(), app_version)
        .with_context(|| format!("could not write {}", stamp_path().display()))?;

    log::info!(
        "cli bridge: published {app_version} to {}",
        target.display()
    );
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// A fake "bundled CLI" that reports the version it is told to. The copy,
    /// verify, rename and stamp sequence is what this module owns; which real
    /// binary it copies is not.
    #[cfg(unix)]
    fn fake_cli(dir: &Path, reports: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("skills-manager-cli");
        std::fs::write(&path, format!("#!/bin/sh\necho 'skills-manager-cli {reports}'\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn publishing_stamps_only_after_the_copy_runs() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        central_repo::set_test_home_dir_override(Some(tmp.path().to_path_buf()));
        let src_dir = tmp.path().join("bundle");
        std::fs::create_dir_all(&src_dir).unwrap();

        let source = fake_cli(&src_dir, "9.9.9");
        publish_from(&source, "9.9.9").expect("a runnable copy must publish");

        assert!(bridge_path().is_file());
        assert_eq!(read_stamp().as_deref(), Some("9.9.9"));

        central_repo::set_test_home_dir_override(None);
    }

    /// The publish must abandon rather than proceed when the old bridge cannot
    /// be made untrusted — otherwise it would leave a stamp vouching for a
    /// binary that is no longer the one the app ships.
    ///
    /// Calls `invalidate_bridge` directly: going through `publish_from` would
    /// also fail at the staged copy, and an assertion that cannot tell the two
    /// apart proves nothing about the step under test.
    #[cfg(unix)]
    #[test]
    fn a_bridge_that_cannot_be_invalidated_blocks_the_publish() {
        use std::os::unix::fs::PermissionsExt;
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        central_repo::set_test_home_dir_override(Some(tmp.path().to_path_buf()));
        let src_dir = tmp.path().join("bundle");
        std::fs::create_dir_all(&src_dir).unwrap();

        publish_from(&fake_cli(&src_dir, "9.9.9"), "9.9.9").unwrap();
        let stamp = bridge_dir().join(".version");

        // Nothing here can be removed (the directory denies writes) and the
        // stamp itself cannot be rewritten, so all three fallbacks are closed.
        std::fs::set_permissions(&stamp, std::fs::Permissions::from_mode(0o400)).unwrap();
        std::fs::set_permissions(bridge_dir(), std::fs::Permissions::from_mode(0o500)).unwrap();

        let result = invalidate_bridge();
        let published_still_there = bridge_path().is_file() && stamp.is_file();

        std::fs::set_permissions(bridge_dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&stamp, std::fs::Permissions::from_mode(0o600)).unwrap();

        result.expect_err("invalidation must fail when no fallback can take effect");
        assert!(
            published_still_there,
            "precondition: the stale pair is what could not be removed"
        );
        // And the publish that would follow must not run.
        std::fs::set_permissions(bridge_dir(), std::fs::Permissions::from_mode(0o500)).unwrap();
        let publish = publish_from(&fake_cli(&src_dir, "9.9.10"), "9.9.10");
        std::fs::set_permissions(bridge_dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
        publish.expect_err("publishing must abandon rather than leave a lying stamp");

        central_repo::set_test_home_dir_override(None);
    }

    /// The invariant the stamp exists for: when a replace cannot complete, the
    /// binary left behind is not merely old, it may be one that deletes a
    /// user's directory where the current one refuses (#363). It must not be
    /// reported as usable, and the schema gate cannot catch this because such
    /// a fix carries no migration.
    #[cfg(unix)]
    #[test]
    fn a_failed_republish_leaves_the_previous_bridge_unusable() {
        let _lock = central_repo::test_base_dir_lock();
        let tmp = tempdir().unwrap();
        central_repo::set_test_home_dir_override(Some(tmp.path().to_path_buf()));
        let src_dir = tmp.path().join("bundle");
        std::fs::create_dir_all(&src_dir).unwrap();

        publish_from(&fake_cli(&src_dir, "9.9.9"), "9.9.9").unwrap();
        assert_eq!(
            read_stamp().as_deref(),
            Some("9.9.9"),
            "precondition: a good bridge exists"
        );

        // The new app version ships a CLI that cannot run — a truncated copy,
        // a blocked binary, the wrong architecture.
        let broken = src_dir.join("broken-cli");
        std::fs::write(&broken, "not executable").unwrap();
        publish_from(&broken, "9.9.10").expect_err("an unrunnable copy must not publish");

        assert!(
            read_stamp().is_none(),
            "a bridge that failed to republish must be left unstamped, so the \
             skill refuses it even though a binary is still on disk"
        );
        assert!(
            bridge_path().is_file(),
            "the old binary may well be unremovable; the stamp is what gates it"
        );

        central_repo::set_test_home_dir_override(None);
    }

}
