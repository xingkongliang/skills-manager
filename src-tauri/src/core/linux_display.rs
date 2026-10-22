//! Display backend selection for Linux AppImage launches.
//!
//! The AppImage bundles its own WebKit/Wayland stack, which segfaults in EGL
//! init against host NVIDIA drivers on Wayland sessions (issue #568). Fall
//! back to X11 (XWayland) in exactly that setup, unless the user already
//! chose a backend via `GDK_BACKEND`.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

/// Snapshot of the display environment relevant to backend selection.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DisplayEnv<'a> {
    pub gdk_backend: Option<&'a str>,
    pub appdir: bool,
    pub wayland: bool,
    pub x11_available: bool,
    pub nvidia: bool,
}

/// Backend to force, if any, for the given environment.
pub(crate) fn backend_override(env: DisplayEnv) -> Option<&'static str> {
    if env.gdk_backend.is_some()
        || !env.appdir
        || !env.wayland
        || !env.x11_available
        || !env.nvidia
    {
        return None;
    }
    Some("x11")
}

/// Build a [`DisplayEnv`] from raw env values. Empty values count as unset:
/// `export VAR=` is a common accident, and GDK ignores an empty backend.
pub(crate) fn snapshot<'a>(
    gdk_backend: Option<&'a str>,
    appdir: bool,
    wayland_display: Option<&'a str>,
    session_type: Option<&'a str>,
    display: Option<&'a str>,
    nvidia: bool,
) -> DisplayEnv<'a> {
    DisplayEnv {
        gdk_backend: gdk_backend.filter(|v| !v.trim().is_empty()),
        appdir,
        wayland: wayland_display.is_some_and(|v| !v.is_empty())
            || session_type == Some("wayland"),
        x11_available: display.is_some_and(|v| !v.is_empty()),
        nvidia,
    }
}

/// Apply [`backend_override`] to the real process environment. Must run
/// before GTK initializes, i.e. at the top of `run()`.
pub(crate) fn ensure_display_backend() {
    #[cfg(target_os = "linux")]
    {
        let gdk_backend = std::env::var("GDK_BACKEND").ok();
        let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
        let session_type = std::env::var("XDG_SESSION_TYPE").ok();
        let display = std::env::var("DISPLAY").ok();
        let env = snapshot(
            gdk_backend.as_deref(),
            std::env::var_os("APPDIR").is_some_and(|v| !v.is_empty()),
            wayland_display.as_deref(),
            session_type.as_deref(),
            display.as_deref(),
            has_nvidia_gpu(),
        );
        if let Some(backend) = backend_override(env) {
            std::env::set_var("GDK_BACKEND", backend);
        }
    }
}

#[cfg(target_os = "linux")]
fn has_nvidia_gpu() -> bool {
    std::path::Path::new("/dev/nvidia0").exists()
        || std::fs::read_to_string("/proc/modules")
            .map(|modules| {
                modules.lines().any(|line| {
                    matches!(line.split_whitespace().next(),
                        Some("nvidia") | Some("nvidia_drm") | Some("nvidia_modeset"))
                })
            })
            .unwrap_or(false)
}

#[cfg(test)]
mod backend_selection_tests {
    use super::*;

    fn full_env(gdk_backend: Option<&str>) -> DisplayEnv<'_> {
        snapshot(gdk_backend, true, Some(":1"), Some("wayland"), Some(":0"), true)
    }

    #[test]
    fn forces_x11_only_for_appimage_nvidia_wayland_with_xwayland() {
        assert_eq!(backend_override(full_env(None)), Some("x11"));
        // Empty user values count as unset.
        assert_eq!(backend_override(full_env(Some(""))), Some("x11"));
        assert_eq!(backend_override(full_env(Some("   "))), Some("x11"));
        // Any real user choice wins, even one that keeps Wayland.
        assert_eq!(backend_override(full_env(Some("x11"))), None);
        assert_eq!(backend_override(full_env(Some("wayland"))), None);
        // Non-AppImage installs keep the system WebKit path untouched.
        assert_eq!(
            backend_override(snapshot(
                None,
                false,
                Some(":1"),
                Some("wayland"),
                Some(":0"),
                true
            )),
            None
        );
        // No Wayland, no XWayland, or no NVIDIA: nothing to work around.
        for env in [
            snapshot(None, true, None, Some("x11"), Some(":0"), true),
            snapshot(None, true, Some(":1"), Some("wayland"), Some(""), true),
            snapshot(None, true, Some(":1"), Some("wayland"), None, true),
            snapshot(None, true, Some(":1"), Some("wayland"), Some(":0"), false),
        ] {
            assert_eq!(backend_override(env), None);
        }
    }

    #[test]
    fn snapshot_treats_blank_values_as_unset() {
        let env = snapshot(Some(""), true, Some(""), Some(""), Some(""), true);
        assert_eq!(env.gdk_backend, None);
        assert!(!env.wayland);
        assert!(!env.x11_available);
        assert_eq!(backend_override(env), None);
    }
}
