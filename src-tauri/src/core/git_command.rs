use std::process::Command;

pub(super) fn git_command() -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new("git");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPDIR").is_some() {
        cmd.env_remove("LD_LIBRARY_PATH");
    }
    cmd
}

#[cfg(all(test, target_os = "linux"))]
mod command_environment_tests {
    use super::git_command;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    #[test]
    fn git_child_environment() {
        let temp = tempfile::tempdir().unwrap();
        let git = temp.path().join("git");
        std::fs::write(&git, r#"#!/bin/sh
if [ "$EXPECT_LIBRARY_PATH" = absent ]; then
    [ "${LD_LIBRARY_PATH+x}" != x ] || { echo 'LD_LIBRARY_PATH unexpectedly inherited' >&2; exit 1; }
else
    [ "$LD_LIBRARY_PATH" = '/nonexistent/skills-manager-test:custom path' ] || exit 2
fi
[ "$HTTPS_PROXY" = 'http://proxy.invalid:8080' ] || exit 3
[ "$GIT_ASKPASS" = '/sentinel/askpass' ] || exit 4
[ "$GIT_CONFIG_COUNT" = 1 ] || exit 5
[ "$GIT_CONFIG_KEY_0" = 'credential.helper' ] || exit 6
[ "$GIT_CONFIG_VALUE_0" = '/sentinel/credential-helper' ] || exit 7
[ "$PATH" = "$EXPECTED_PATH" ] || exit 8
"#).unwrap();
        std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::join_paths(std::iter::once(temp.path().to_path_buf()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        for (appdir, appimage, library_path) in [
            (Some("/appdir"), None, true),
            (None, None, true),
            (None, Some("/app.AppImage"), true),
            (Some("/appdir"), None, false),
            (Some(""), None, true),
        ] {
            let mut child = Command::new(std::env::current_exe().unwrap());
            child
                .args([
                    "--exact",
                    concat!(module_path!(), "::git_environment_probe")
                        .split_once("::")
                        .unwrap()
                        .1,
                    "--nocapture",
                ])
                .env("SKILLS_MANAGER_GIT_ENV_PROBE", "1")
                .env("PATH", &path)
                .env("EXPECTED_PATH", &path)
                .env("HTTPS_PROXY", "http://proxy.invalid:8080")
                .env("GIT_ASKPASS", "/sentinel/askpass")
                .env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", "credential.helper")
                .env("GIT_CONFIG_VALUE_0", "/sentinel/credential-helper")
                .env(
                    "EXPECT_LIBRARY_PATH",
                    if appdir.is_some() || !library_path {
                        "absent"
                    } else {
                        "present"
                    },
                );
            for (key, value) in [
                ("APPDIR", appdir),
                ("APPIMAGE", appimage),
                (
                    "LD_LIBRARY_PATH",
                    library_path.then_some("/nonexistent/skills-manager-test:custom path"),
                ),
            ] {
                match value {
                    Some(value) => {
                        child.env(key, value);
                    }
                    None => {
                        child.env_remove(key);
                    }
                }
            }
            let output = child.output().unwrap();
            assert!(
                output.status.success(),
                "APPDIR={appdir:?}, APPIMAGE={appimage:?}, library_path={library_path}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn git_environment_probe() {
        if std::env::var_os("SKILLS_MANAGER_GIT_ENV_PROBE").is_none() {
            return;
        }
        let parent_library_path = std::env::var_os("LD_LIBRARY_PATH");
        let output = git_command().output().unwrap();
        assert_eq!(std::env::var_os("LD_LIBRARY_PATH"), parent_library_path);
        assert!(
            output.status.success(),
            "Git child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
