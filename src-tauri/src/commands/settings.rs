use semver::Version;
use std::process::Command;
use std::sync::Arc;
use tauri::{Manager, State};

use crate::core::{central_repo, error::AppError, log_sanitize, skill_store::SkillStore, skillssh_api};

#[derive(serde::Serialize)]
pub struct AppUpdateInfo {
    pub has_update: bool,
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
}

#[tauri::command]
pub async fn get_settings(
    key: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<Option<String>, AppError> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.get_setting(&key).map_err(AppError::db))
        .await?
}

/// Diagnostic-only: let the frontend write a named startup event with elapsed
/// milliseconds into the backend log file. Used to correlate WebView2 boot,
/// first paint, and refreshAppData timing with Rust-side startup logs when
/// debugging slow launches (see issue #153).
///
/// `label` is sanitized (control chars stripped, capped at 64 chars) so a
/// buggy or malicious caller cannot inject newlines that would corrupt the
/// log file layout.
#[tauri::command]
pub fn log_startup_event(label: String, elapsed_ms: u64) {
    let sanitized: String = label
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    let display = if sanitized.is_empty() {
        "(empty)".to_string()
    } else {
        sanitized
    };
    log::info!("frontend startup: {display} {elapsed_ms} ms");
}

#[tauri::command]
pub async fn set_settings(
    app: tauri::AppHandle,
    key: String,
    value: String,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let store = store.inner().clone();
    let key_for_store = key.clone();
    let value_for_store = value.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store
            .set_setting(&key_for_store, &value_for_store)
            .map_err(AppError::db)?;
        if key_for_store == "show_tray_icon" {
            let tray_enabled = matches!(
                value_for_store.trim().to_ascii_lowercase().as_str(),
                "true" | "1" | "yes" | "on"
            );
            if !tray_enabled {
                store
                    .set_setting("close_action", "close")
                    .map_err(AppError::db)?;
            }
        }
        Ok::<(), AppError>(())
    })
    .await??;

    if key == "show_tray_icon" {
        let enabled = matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes" | "on"
        );
        crate::set_tray_icon_enabled(&app, enabled).map_err(AppError::io)?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_central_repo_path() -> String {
    central_repo::base_dir().to_string_lossy().to_string()
}

#[tauri::command]
pub fn get_central_repo_path_override() -> Option<String> {
    central_repo::configured_base_dir().map(|path| path.to_string_lossy().to_string())
}

/// Warning codes recorded while resolving the central repository at startup
/// (e.g. unreadable config, invalid configured path). Non-empty means the app
/// fell back to the default location and the user should be told (#228).
#[tauri::command]
pub fn get_central_repo_warnings() -> Vec<String> {
    central_repo::startup_warnings()
}

#[tauri::command]
pub async fn set_central_repo_path(path: Option<String>) -> Result<String, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        central_repo::set_base_dir_override(path)
            .map(|resolved| resolved.to_string_lossy().to_string())
            .map_err(AppError::io)
    })
    .await?
}

#[tauri::command]
pub async fn open_central_repo_folder() -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(|| {
        let repo_path = central_repo::base_dir();

        #[cfg(target_os = "macos")]
        let mut cmd = Command::new("open");
        #[cfg(target_os = "windows")]
        let mut cmd = {
            let mut c = Command::new("explorer");
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x08000000); // CREATE_NO_WINDOW
            c
        };
        #[cfg(target_os = "linux")]
        let mut cmd = Command::new("xdg-open");

        let status = cmd
            .arg(&repo_path)
            .status()
            .map_err(|e| AppError::io(format!("Failed to open folder: {e}")))?;

        // Windows explorer.exe returns exit code 1 even on success
        #[cfg(not(target_os = "windows"))]
        if !status.success() {
            return Err(AppError::io(format!(
                "File manager exited with status: {status}"
            )));
        }

        let _ = status;
        Ok(())
    })
    .await?
}

#[tauri::command]
pub async fn check_app_update(
    app: tauri::AppHandle,
    store: State<'_, Arc<SkillStore>>,
) -> Result<AppUpdateInfo, AppError> {
    let current_version = app.config().version.clone().unwrap_or_default();
    let proxy_url = store.proxy_url();
    tauri::async_runtime::spawn_blocking(move || {
        let client = skillssh_api::build_http_client(proxy_url.as_deref(), 15);

        let resp: serde_json::Value = client
            .get("https://api.github.com/repos/xingkongliang/skills-manager/releases/latest")
            .send()
            .map_err(|e| AppError::network(format!("Network error: {e}")))?
            .json()
            .map_err(|e| AppError::network(format!("Failed to parse response: {e}")))?;

        let tag = resp["tag_name"]
            .as_str()
            .ok_or_else(|| AppError::network("No tag_name in response"))?;
        let latest_version = tag.strip_prefix('v').unwrap_or(tag).to_string();
        let release_url = resp["html_url"]
            .as_str()
            .unwrap_or("https://github.com/xingkongliang/skills-manager/releases")
            .to_string();

        let has_update = version_gt(&latest_version, &current_version);

        Ok(AppUpdateInfo {
            has_update,
            current_version,
            latest_version,
            release_url,
        })
    })
    .await?
}

#[derive(serde::Serialize)]
pub struct DiagnosticInfo {
    pub app_version: String,
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub central_repo_path: String,
    pub central_repo_path_overridden: bool,
}

#[tauri::command]
pub async fn get_diagnostic_info(app: tauri::AppHandle) -> Result<DiagnosticInfo, AppError> {
    let app_version = app.config().version.clone().unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        let os = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();
        let os_version = detect_os_version();
        let configured = central_repo::configured_base_dir();
        let central_repo_path = central_repo::base_dir().to_string_lossy().to_string();
        Ok(DiagnosticInfo {
            app_version,
            os,
            os_version,
            arch,
            central_repo_path,
            central_repo_path_overridden: configured.is_some(),
        })
    })
    .await?
}

fn detect_os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|out| {
                if out.status.success() {
                    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
                } else {
                    None
                }
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = Command::new("cmd");
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        cmd.args(["/C", "ver"])
            .output()
            .ok()
            .and_then(|out| {
                if out.status.success() {
                    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
                } else {
                    None
                }
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|content| {
                for line in content.lines() {
                    if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
                        return Some(rest.trim().trim_matches('"').to_string());
                    }
                }
                None
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        "unknown".to_string()
    }
}

#[derive(serde::Serialize)]
pub struct PanicInfo {
    pub timestamp: String,
    pub message: String,
}

#[tauri::command]
pub async fn check_last_panic(app: tauri::AppHandle) -> Result<Option<PanicInfo>, AppError> {
    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|e| AppError::io(format!("Failed to resolve log dir: {e}")))?;
    let panic_path = log_dir.join("last_panic.log");
    if !panic_path.exists() {
        return Ok(None);
    }
    tauri::async_runtime::spawn_blocking(move || {
        let metadata = std::fs::metadata(&panic_path).map_err(AppError::io)?;
        let modified = metadata
            .modified()
            .map_err(AppError::io)
            .unwrap_or(std::time::SystemTime::now());
        let datetime: chrono::DateTime<chrono::Local> = modified.into();
        let timestamp = datetime.format("%Y-%m-%d %H:%M:%S").to_string();
        let content = std::fs::read_to_string(&panic_path).unwrap_or_default();
        let first_lines: Vec<&str> = content.lines().take(3).collect();
        let message = log_sanitize::sanitize(&first_lines.join("\n"));
        Ok(Some(PanicInfo { timestamp, message }))
    })
    .await?
}

#[tauri::command]
pub async fn clear_last_panic(app: tauri::AppHandle) -> Result<(), AppError> {
    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|e| AppError::io(format!("Failed to resolve log dir: {e}")))?;
    let panic_path = log_dir.join("last_panic.log");
    if panic_path.exists() {
        std::fs::remove_file(&panic_path).map_err(AppError::io)?;
    }
    Ok(())
}

#[derive(serde::Serialize)]
pub struct LogExcerpt {
    pub log_path: String,
    pub excerpt: String,
    pub line_count: usize,
    pub has_warnings: bool,
}

#[tauri::command]
pub async fn get_recent_log_excerpt(app: tauri::AppHandle) -> Result<LogExcerpt, AppError> {
    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|e| AppError::io(format!("Failed to resolve log dir: {e}")))?;
    let app_name = &app.package_info().name;
    let log_path = log_dir.join(format!("{app_name}.log"));

    tauri::async_runtime::spawn_blocking(move || {
        let raw = std::fs::read_to_string(&log_path).unwrap_or_default();
        let display_path = log_sanitize::sanitize(&log_path.to_string_lossy());

        if raw.is_empty() {
            return Ok(LogExcerpt {
                log_path: display_path,
                excerpt: "(log file is empty or not yet created)".into(),
                line_count: 0,
                has_warnings: false,
            });
        }

        let lines: Vec<&str> = raw.lines().collect();
        let scan_window = 200usize.min(lines.len());
        let start = lines.len().saturating_sub(scan_window);
        let window = &lines[start..];

        let alerts: Vec<usize> = window
            .iter()
            .enumerate()
            .filter_map(|(i, line)| {
                if line.contains(" ERROR ") || line.contains(" WARN ") {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        let excerpt_raw = if alerts.is_empty() {
            let tail_n = 80usize.min(window.len());
            window[window.len() - tail_n..].join("\n")
        } else {
            let context = 10usize;
            let mut keep = vec![false; window.len()];
            for &idx in &alerts {
                let lo = idx.saturating_sub(context);
                let hi = (idx + context + 1).min(window.len());
                for k in lo..hi {
                    keep[k] = true;
                }
            }
            let mut out = String::new();
            let mut last_kept: Option<usize> = None;
            for (i, line) in window.iter().enumerate() {
                if keep[i] {
                    if let Some(prev) = last_kept {
                        if i > prev + 1 {
                            out.push_str("... (gap)\n");
                        }
                    }
                    out.push_str(line);
                    out.push('\n');
                    last_kept = Some(i);
                }
            }
            out.trim_end().to_string()
        };

        let collapsed = collapse_consecutive_repeats(&excerpt_raw);
        let excerpt = log_sanitize::sanitize(&collapsed);
        let line_count = excerpt.lines().count();

        Ok(LogExcerpt {
            log_path: display_path,
            excerpt,
            line_count,
            has_warnings: !alerts.is_empty(),
        })
    })
    .await?
}

/// Strip the leading timestamp so two log lines that differ only in time
/// produce identical fingerprints (used to detect repeated noise).
fn line_fingerprint(line: &str) -> &str {
    // New format: `2026-05-17T17:56:13.845+10:00 INFO  [app_lib] msg`
    //   → take everything starting at `INFO`/`WARN`/... (after the first space)
    // Old format: `[2026-03-22][04:20:02][app_lib][INFO] msg`
    //   → take everything after the second `]` (skips date+time, keeps module+level+msg)
    if line.starts_with('[') {
        // skip first two bracketed groups (date, time)
        let mut count = 0;
        for (i, ch) in line.char_indices() {
            if ch == ']' {
                count += 1;
                if count == 2 {
                    return line[i + 1..].trim_start();
                }
            }
        }
        line
    } else if let Some(idx) = line.find(' ') {
        line[idx + 1..].trim_start()
    } else {
        line
    }
}

fn collapse_consecutive_repeats(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let key = line_fingerprint(lines[i]);
        let mut j = i + 1;
        while j < lines.len() && line_fingerprint(lines[j]) == key {
            j += 1;
        }
        let count = j - i;
        out.push(lines[i].to_string());
        if count >= 3 {
            out.push(format!("... (line above repeated {} more times)", count - 1));
        } else if count == 2 {
            out.push(lines[i + 1].to_string());
        }
        i = j;
    }
    out.join("\n")
}

#[derive(serde::Serialize)]
pub struct LogExportResult {
    pub zip_path: String,
    pub file_count: usize,
}

#[tauri::command]
pub async fn export_logs_zip(
    app: tauri::AppHandle,
    store: State<'_, Arc<SkillStore>>,
) -> Result<LogExportResult, AppError> {
    use std::io::{Read, Write};

    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|e| AppError::io(format!("Failed to resolve log dir: {e}")))?;
    let app_name = app.package_info().name.clone();
    let app_version = app.config().version.clone().unwrap_or_default();
    let central_path = central_repo::base_dir().to_string_lossy().to_string();
    let central_overridden = central_repo::configured_base_dir().is_some();
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let os_version = detect_os_version();
    let store = store.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let downloads = dirs::download_dir()
            .or_else(dirs::home_dir)
            .ok_or_else(|| AppError::io("Failed to resolve Downloads directory"))?;
        std::fs::create_dir_all(&downloads).map_err(AppError::io)?;

        let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let zip_path = downloads.join(format!("{app_name}-log-{timestamp}.zip"));

        let mut log_files: Vec<std::path::PathBuf> = Vec::new();
        if log_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&log_dir) {
                let mut all: Vec<(std::path::PathBuf, std::time::SystemTime)> = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().map(|x| x == "log").unwrap_or(false))
                    .filter(|e| e.file_name().to_string_lossy() != "last_panic.log")
                    .filter_map(|e| {
                        e.metadata()
                            .and_then(|m| m.modified())
                            .ok()
                            .map(|t| (e.path(), t))
                    })
                    .collect();
                all.sort_by(|a, b| b.1.cmp(&a.1));
                for (path, _) in all.into_iter().take(3) {
                    log_files.push(path);
                }
            }
        }
        let panic_path = log_dir.join("last_panic.log");

        // Serialize the audit log to JSONL ahead of writing the zip so we can
        // report the entry count in diagnostics.md. String fields go through
        // log_sanitize so credentialed URLs / home paths captured in failure
        // details do not leak when the user attaches the zip to a public issue.
        let audit_entries = store.list_audit(None).unwrap_or_default();
        let mut audit_jsonl = String::with_capacity(audit_entries.len() * 96);
        for entry in &audit_entries {
            let mut sanitized = entry.clone();
            sanitized.skill_name = sanitized.skill_name.as_deref().map(log_sanitize::sanitize);
            sanitized.detail = sanitized.detail.as_deref().map(log_sanitize::sanitize);
            if let Ok(line) = serde_json::to_string(&sanitized) {
                audit_jsonl.push_str(&line);
                audit_jsonl.push('\n');
            }
        }

        let diagnostics = format!(
            "# Diagnostics\n\nExported: {ts}\n\n- App version: `{ver}`\n- OS: `{os} {osver} ({arch})`\n- Central repo: `{repo}`{custom}\n- Log files included: {count}\n- Audit entries included: {audit}\n",
            ts = chrono::Local::now().to_rfc3339(),
            ver = app_version,
            os = os,
            osver = os_version,
            arch = arch,
            repo = log_sanitize::sanitize(&central_path),
            custom = if central_overridden { " (custom path)" } else { "" },
            count = log_files.len() + if panic_path.exists() { 1 } else { 0 },
            audit = audit_entries.len(),
        );

        let file = std::fs::File::create(&zip_path).map_err(AppError::io)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);

        zip.start_file("diagnostics.md", opts)
            .map_err(|e| AppError::io(e.to_string()))?;
        zip.write_all(diagnostics.as_bytes()).map_err(AppError::io)?;

        let mut included = 0usize;
        for path in &log_files {
            if let Ok(mut f) = std::fs::File::open(path) {
                let mut content = String::new();
                if f.read_to_string(&mut content).is_ok() {
                    let collapsed = collapse_consecutive_repeats(&content);
                    let sanitized = log_sanitize::sanitize(&collapsed);
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "log.txt".into());
                    zip.start_file(name, opts)
                        .map_err(|e| AppError::io(e.to_string()))?;
                    zip.write_all(sanitized.as_bytes()).map_err(AppError::io)?;
                    included += 1;
                }
            }
        }

        if panic_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&panic_path) {
                let sanitized = log_sanitize::sanitize(&content);
                zip.start_file("last_panic.log", opts)
                    .map_err(|e| AppError::io(e.to_string()))?;
                zip.write_all(sanitized.as_bytes()).map_err(AppError::io)?;
                included += 1;
            }
        }

        if !audit_jsonl.is_empty() {
            zip.start_file("audit.jsonl", opts)
                .map_err(|e| AppError::io(e.to_string()))?;
            zip.write_all(audit_jsonl.as_bytes())
                .map_err(AppError::io)?;
        }

        zip.finish().map_err(|e| AppError::io(e.to_string()))?;

        reveal_in_file_manager(&zip_path);

        Ok(LogExportResult {
            zip_path: zip_path.to_string_lossy().to_string(),
            file_count: included,
        })
    })
    .await?
}

fn reveal_in_file_manager(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg("-R").arg(path).status();
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = Command::new("explorer");
        cmd.creation_flags(0x08000000);
        let arg = format!("/select,{}", path.display());
        let _ = cmd.arg(arg).status();
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = path.parent() {
            let _ = Command::new("xdg-open").arg(parent).status();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_repeated_tray_lines_old_format() {
        let log = "\
[2026-03-22][04:20:02][app_lib][INFO] Tray icon created
[2026-03-22][04:21:02][app_lib][INFO] Tray icon created
[2026-03-22][04:22:02][app_lib][INFO] Tray icon created
[2026-03-22][04:23:02][app_lib][INFO] Tray icon created
[2026-03-22][04:24:02][app_lib][INFO] Migrated legacy tool key X -> Y";
        let out = collapse_consecutive_repeats(log);
        assert!(out.contains("repeated 3 more times"), "got: {out}");
        assert!(out.contains("Migrated legacy tool key X -> Y"));
        assert_eq!(out.lines().count(), 3);
    }

    #[test]
    fn keeps_distinct_lines() {
        let log = "\
2026-05-17T17:56:13 INFO  [app_lib] app start
2026-05-17T17:56:14 ERROR [app_lib] something failed
2026-05-17T17:56:15 INFO  [app_lib] scan complete";
        let out = collapse_consecutive_repeats(log);
        assert_eq!(out.lines().count(), 3);
        assert!(out.contains("app start"));
        assert!(out.contains("something failed"));
    }

    #[test]
    fn pair_repeat_kept_as_is() {
        let log = "a INFO  [m] foo\nb INFO  [m] foo\nc INFO  [m] bar";
        let out = collapse_consecutive_repeats(log);
        assert_eq!(out.lines().count(), 3);
        assert!(!out.contains("repeated"));
    }
}

#[tauri::command]
pub async fn app_exit(app: tauri::AppHandle) {
    let app_for_main = app.clone();
    if let Err(err) = app.run_on_main_thread(move || crate::quit_app(&app_for_main)) {
        log::error!("Failed to schedule app_exit on main thread: {err}");
        crate::quit_app(&app);
    }
}

#[tauri::command]
pub async fn hide_to_tray(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    store: State<'_, Arc<SkillStore>>,
) -> Result<(), AppError> {
    let show_tray_icon = {
        let store = store.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            let value = store.get_setting("show_tray_icon").map_err(AppError::db)?;
            Ok::<bool, AppError>(!matches!(
                value.as_deref().map(str::trim).map(str::to_ascii_lowercase),
                Some(v) if matches!(v.as_str(), "false" | "0" | "no" | "off")
            ))
        })
        .await??
    };

    if !show_tray_icon {
        crate::quit_app(&app);
        return Ok(());
    }

    window.hide().map_err(|e| AppError::io(e.to_string()))?;
    // On macOS, avoid app.hide() (app-level hidden state can block restore in tray flow).
    // Keep app running and hide only the window + Dock icon.
    #[cfg(target_os = "macos")]
    {
        app.set_dock_visibility(false)
            .map_err(|e| AppError::io(format!("Failed to hide Dock icon on macOS: {e}")))?;
        app.set_activation_policy(tauri::ActivationPolicy::Accessory)
            .map_err(|e| {
                AppError::io(format!("Failed to set activation policy to Accessory: {e}"))
            })?;
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    Ok(())
}

fn version_gt(a: &str, b: &str) -> bool {
    // Prefer strict SemVer comparison (supports pre-release/build metadata).
    if let (Ok(a_ver), Ok(b_ver)) = (Version::parse(a), Version::parse(b)) {
        return a_ver > b_ver;
    }

    // Fallback for non-SemVer tags.
    let parse = |s: &str| -> Vec<u64> { s.split('.').filter_map(|p| p.parse().ok()).collect() };
    parse(a) > parse(b)
}
