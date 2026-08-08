use std::backtrace::Backtrace;
use std::fs;
use std::io::Write;
use std::panic;
use std::path::PathBuf;
use std::sync::OnceLock;

use tauri::{AppHandle, Manager};

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn last_panic_path(app: &AppHandle) -> Option<PathBuf> {
    LOG_DIR
        .get()
        .cloned()
        .or_else(|| app.path().app_log_dir().ok())
        .map(|dir| dir.join("last_panic.log"))
}

pub fn install_panic_hook(app: AppHandle) {
    if let Ok(dir) = app.path().app_log_dir() {
        let _ = fs::create_dir_all(&dir);
        let _ = LOG_DIR.set(dir);
    }

    let prev = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let backtrace = Backtrace::capture();
        let timestamp = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z");
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".into());
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        };

        let body =
            format!("[{timestamp}] PANIC at {location}\n{payload}\n\nBacktrace:\n{backtrace}\n");

        log::error!("panic: {payload} at {location}");

        if let Some(dir) = LOG_DIR.get() {
            let path = dir.join("last_panic.log");
            if let Ok(mut f) = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
            {
                let _ = f.write_all(body.as_bytes());
            }
        }

        prev(info);
    }));
}
