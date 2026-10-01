//! Showing a path in the operating system's file manager.
//!
//! The three-way platform split lives here once. It already existed in three
//! places (the tray, the Settings button, and the log-export reveal), and a
//! fourth copy — for a project's folder — is how they drift apart.
//!
//! Command construction is pure so the branches can be asserted in tests
//! without launching a file manager on the machine running them.

use std::path::Path;
use std::process::{Command, ExitStatus};

/// What to do with the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileManagerAction {
    /// Show the directory's contents.
    OpenDir,
    /// Show the directory that contains this entry, with the entry selected.
    ///
    /// Not the same as `OpenDir` on a directory: it answers "where does this
    /// live?" rather than "what is in it?".
    RevealItem,
}

/// The program and arguments for an action, without running anything.
///
/// Arguments are returned unquoted. `Command` escapes them for the platform,
/// and pre-quoting would leave the quotes inside the path.
pub fn file_manager_command(path: &Path, action: FileManagerAction) -> (String, Vec<String>) {
    let shown = path.display().to_string();

    if cfg!(target_os = "macos") {
        match action {
            FileManagerAction::OpenDir => ("open".into(), vec![shown]),
            FileManagerAction::RevealItem => ("open".into(), vec!["-R".into(), shown]),
        }
    } else if cfg!(target_os = "windows") {
        match action {
            FileManagerAction::OpenDir => ("explorer".into(), vec![shown]),
            // explorer takes the selection as one argument, path included.
            FileManagerAction::RevealItem => {
                ("explorer".into(), vec![format!("/select,{shown}")])
            }
        }
    } else {
        match action {
            FileManagerAction::OpenDir => ("xdg-open".into(), vec![shown]),
            // xdg-open cannot select an entry, so the containing directory is
            // the closest thing to revealing one. A path with no parent is its
            // own location.
            FileManagerAction::RevealItem => {
                let target = path.parent().unwrap_or(path).display().to_string();
                ("xdg-open".into(), vec![target])
            }
        }
    }
}

/// Run the action.
///
/// Returns the exit status rather than judging it, because callers disagree on
/// what a failure means: a button reports it to the user, a tray item logs it,
/// and the reveal after an export ignores it. Windows `explorer.exe` exits
/// non-zero even on success, so callers on that platform must not test it.
///
/// Deliberately not covered by tests: running it would launch a file manager on
/// the machine running them.
pub fn run(path: &Path, action: FileManagerAction) -> std::io::Result<ExitStatus> {
    let (program, args) = file_manager_command(path, action);
    let mut command = Command::new(program);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: explorer would otherwise flash a console window.
        command.creation_flags(0x08000000);
    }

    command.args(args).status()
}

/// Show a directory's contents in the file manager.
pub fn open_dir(path: &Path) -> std::io::Result<ExitStatus> {
    run(path, FileManagerAction::OpenDir)
}

/// Show an entry in its containing directory, selected where the platform can.
pub fn reveal_item(path: &Path) -> std::io::Result<ExitStatus> {
    run(path, FileManagerAction::RevealItem)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path with spaces and non-ASCII characters must leave here as one
    /// unquoted argument: quoting is `Command`'s job, and doing it here would
    /// put the quotes in the path.
    #[test]
    fn opening_a_directory_passes_the_path_as_one_unquoted_argument() {
        let (program, args) = file_manager_command(
            Path::new("/tmp/My Projects/技能库"),
            FileManagerAction::OpenDir,
        );

        assert_eq!(args, vec!["/tmp/My Projects/技能库".to_string()]);
        assert_eq!(
            program,
            if cfg!(target_os = "macos") {
                "open"
            } else if cfg!(target_os = "windows") {
                "explorer"
            } else {
                "xdg-open"
            }
        );
    }

    #[test]
    fn revealing_targets_the_entry_not_its_contents() {
        let path = Path::new("/tmp/My Projects/技能库");
        let (program, args) = file_manager_command(path, FileManagerAction::RevealItem);

        if cfg!(target_os = "macos") {
            assert_eq!(program, "open");
            // -R must precede the path, or `open` reads it as a file to launch.
            assert_eq!(
                args,
                vec!["-R".to_string(), "/tmp/My Projects/技能库".to_string()]
            );
        } else if cfg!(target_os = "windows") {
            assert_eq!(program, "explorer");
            // explorer wants the path inside the single /select argument.
            assert_eq!(args, vec!["/select,/tmp/My Projects/技能库".to_string()]);
        } else {
            assert_eq!(program, "xdg-open");
            // No selection concept, so the containing directory is opened.
            assert_eq!(args, vec!["/tmp/My Projects".to_string()]);
        }
    }

    #[test]
    fn no_platform_produces_an_empty_argument() {
        // A path with no parent must still yield something runnable.
        let (_, args) = file_manager_command(Path::new("/"), FileManagerAction::RevealItem);

        assert!(!args.is_empty());
        assert!(args.iter().all(|arg| !arg.is_empty()), "{args:?}");
    }
}
