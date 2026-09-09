/// Handle the custom title bar like a native window title bar.
/// This synchronous command runs on the main thread, as required by AppKit.
#[tauri::command]
pub fn titlebar_double_click(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSWindow;
        use objc2_foundation::{ns_string, NSUserDefaults};

        let _main_thread =
            MainThreadMarker::new().ok_or("Title bar actions must run on the main thread")?;
        let defaults = NSUserDefaults::standardUserDefaults();
        let action = defaults
            .stringForKey(ns_string!("AppleActionOnDoubleClick"))
            .map(|value| value.to_string());
        let native_window = window.ns_window().map_err(|err| err.to_string())?;
        // SAFETY: Tauri owns this NSWindow and keeps it alive for this command.
        // Synchronous commands execute on the main thread.
        let native_window = unsafe { &*native_window.cast::<NSWindow>() };
        match action.as_deref() {
            Some("None") => {}
            Some("Minimize") => native_window.performMiniaturize(None),
            _ => {
                if window.is_resizable().map_err(|err| err.to_string())?
                    && !window.is_fullscreen().map_err(|err| err.to_string())?
                {
                    native_window.performZoom(None);
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    if window.is_resizable().map_err(|err| err.to_string())? {
        if window.is_maximized().map_err(|err| err.to_string())? {
            window.unmaximize().map_err(|err| err.to_string())?;
        } else {
            window.maximize().map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}
