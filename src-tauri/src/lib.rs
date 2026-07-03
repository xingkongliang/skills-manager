use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

pub mod commands;
pub mod core;

/// Shared flag: when true, CloseRequested should NOT be prevented.
pub static QUITTING: AtomicBool = AtomicBool::new(false);

/// Guards concurrent preset apply/remove from the tray so a quick double-click
/// can't fire two batches at once. Intentionally separate from the
/// `TRAY_CHECK_UPDATES_RUNNING` flag — update checks only touch
/// `update_status` while preset apply touches `skill_targets`, so the two are
/// orthogonal and shouldn't block each other. Sharing the lock would silently
/// drop preset clicks during long-running update checks.
static TRAY_PRESET_APPLY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Tracks whether a manual "Check for skill updates" is currently running so the
/// tray menu can render a disabled "Checking for updates..." label.
static TRAY_CHECK_UPDATES_RUNNING: AtomicBool = AtomicBool::new(false);

const MAIN_TRAY_ID: &str = "main-tray";
const TRAY_PRESET_ADD_PREFIX: &str = "tray-preset-add:";
const TRAY_PRESET_REMOVE_PREFIX: &str = "tray-preset-remove:";
const TRAY_OPEN_UPDATES_ID: &str = "tray-open-updates";
const TRAY_OPEN_FOLDER_ID: &str = "tray-open-folder";
const TRAY_CHECK_UPDATES_ID: &str = "tray-check-updates";
const TRAY_OPEN_UPDATES_EVENT: &str = "tray-open-updates";

#[cfg(target_os = "macos")]
const CUSTOM_TRAY_ICON_BYTES: &[u8] = include_bytes!("../icons/tray/tray-icon-32.png");
#[cfg(not(target_os = "macos"))]
const CUSTOM_TRAY_ICON_BYTES: &[u8] = include_bytes!("../icons/tray/tray-icon-color-32.png");

fn parse_bool_setting(value: Option<String>, default: bool) -> bool {
    match value.as_deref().map(str::trim).map(str::to_ascii_lowercase) {
        Some(v) if matches!(v.as_str(), "true" | "1" | "yes" | "on") => true,
        Some(v) if matches!(v.as_str(), "false" | "0" | "no" | "off") => false,
        _ => default,
    }
}

fn is_tray_icon_enabled(store: &Arc<core::skill_store::SkillStore>) -> bool {
    let value = store.get_setting("show_tray_icon").ok().flatten();
    parse_bool_setting(value, true)
}

fn restore_main_window(app: &tauri::AppHandle) {
    let app_for_main = app.clone();
    if let Err(err) = app.run_on_main_thread(move || {
        #[cfg(target_os = "macos")]
        {
            if let Err(err) = app_for_main.set_dock_visibility(true) {
                log::error!("Failed to show Dock icon on macOS: {err}");
            }
            if let Err(err) = app_for_main.set_activation_policy(tauri::ActivationPolicy::Regular) {
                log::error!("Failed to set activation policy to Regular on macOS: {err}");
            }
            if let Err(err) = app_for_main.show() {
                log::error!("Failed to show app on macOS: {err}");
            }
        }

        if let Some(w) = app_for_main.get_webview_window("main") {
            if let Err(err) = w.show() {
                log::error!("Failed to show main window: {err}");
            }
            if let Err(err) = w.unminimize() {
                log::error!("Failed to unminimize main window: {err}");
            }
            if let Err(err) = w.set_focus() {
                log::error!("Failed to focus main window: {err}");
            }
        } else {
            log::error!("Main window not found while restoring from tray");
        }
    }) {
        log::error!("Failed to schedule restore_main_window on main thread: {err}");
    }
}

fn request_quit(app: &tauri::AppHandle) {
    let app_for_main = app.clone();
    if let Err(err) = app.run_on_main_thread(move || {
        quit_app(&app_for_main);
    }) {
        log::error!("Failed to schedule quit on main thread: {err}");
        // Fallback: attempt quit anyway.
        quit_app(app);
    }
}

fn load_custom_tray_icon() -> Option<tauri::image::Image<'static>> {
    let img = image::load_from_memory_with_format(CUSTOM_TRAY_ICON_BYTES, image::ImageFormat::Png)
        .ok()?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some(tauri::image::Image::new_owned(
        rgba.into_raw(),
        width,
        height,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayPresetStatus {
    Empty,
    Inactive,
    Partial,
    Active,
}

#[derive(Debug, Clone)]
struct TrayPresetEntry {
    id: String,
    name: String,
    skill_count: usize,
    synced_pairs: usize,
    total_pairs: usize,
}

impl TrayPresetEntry {
    fn status(&self) -> TrayPresetStatus {
        if self.total_pairs == 0 {
            TrayPresetStatus::Empty
        } else if self.synced_pairs == 0 {
            TrayPresetStatus::Inactive
        } else if self.synced_pairs >= self.total_pairs {
            TrayPresetStatus::Active
        } else {
            TrayPresetStatus::Partial
        }
    }
}

#[derive(Debug, Clone)]
struct TrayMenuData {
    total_skills: usize,
    coding_agent_count: usize,
    update_count: usize,
    presets: Vec<TrayPresetEntry>,
    check_updates_running: bool,
}

fn collect_tray_menu_data(store: &core::skill_store::SkillStore) -> TrayMenuData {
    let all_skills = store.get_all_skills().unwrap_or_default();
    let total_skills = all_skills.len();
    let update_count = all_skills
        .iter()
        .filter(|s| s.update_status == "update_available")
        .count();

    let coding_keys: Vec<String> = core::tool_adapters::enabled_installed_adapters(store)
        .into_iter()
        .filter(|adapter| matches!(adapter.category, core::tool_adapters::ToolCategory::Coding))
        .map(|adapter| adapter.key)
        .collect();
    let coding_agent_count = coding_keys.len();
    let coding_set: HashSet<&str> = coding_keys.iter().map(String::as_str).collect();

    let synced_pairs_set: HashSet<(String, String)> = store
        .get_all_targets()
        .unwrap_or_default()
        .into_iter()
        .filter(|target| coding_set.contains(target.tool.as_str()))
        .map(|target| (target.skill_id, target.tool))
        .collect();

    let scenarios = store.get_all_scenarios().unwrap_or_default();
    let mut presets = Vec::with_capacity(scenarios.len());
    for scenario in scenarios {
        let skill_ids = store
            .get_skill_ids_for_scenario(&scenario.id)
            .unwrap_or_default();
        let skill_count = skill_ids.len();
        let total_pairs = skill_count * coding_agent_count;
        let mut synced_pairs = 0usize;
        if total_pairs > 0 {
            for sid in &skill_ids {
                for tk in &coding_keys {
                    if synced_pairs_set.contains(&(sid.clone(), tk.clone())) {
                        synced_pairs += 1;
                    }
                }
            }
        }
        presets.push(TrayPresetEntry {
            id: scenario.id,
            name: scenario.name,
            skill_count,
            synced_pairs,
            total_pairs,
        });
    }

    TrayMenuData {
        total_skills,
        coding_agent_count,
        update_count,
        presets,
        check_updates_running: TRAY_CHECK_UPDATES_RUNNING.load(Ordering::SeqCst),
    }
}

fn format_status_line(data: &TrayMenuData) -> String {
    let skill_label = if data.total_skills == 1 { "skill" } else { "skills" };
    let agent_label = if data.coding_agent_count == 1 { "agent" } else { "agents" };
    format!(
        "{} {} · {} {} connected",
        data.total_skills, skill_label, data.coding_agent_count, agent_label
    )
}

fn format_tooltip(data: &TrayMenuData) -> String {
    if data.update_count > 0 {
        format!(
            "Skills Manager · {} skills · {} agents · {} updates",
            data.total_skills, data.coding_agent_count, data.update_count
        )
    } else {
        format!(
            "Skills Manager · {} skills · {} agents",
            data.total_skills, data.coding_agent_count
        )
    }
}

fn preset_menu_item_id(preset: &TrayPresetEntry) -> (String, &'static str) {
    // E1 semantics: only a fully-active preset removes on click. Partial
    // and inactive both fill missing pairs up to fully active.
    match preset.status() {
        TrayPresetStatus::Active => (
            format!("{TRAY_PRESET_REMOVE_PREFIX}{}", preset.id),
            "remove",
        ),
        _ => (format!("{TRAY_PRESET_ADD_PREFIX}{}", preset.id), "add"),
    }
}

fn preset_menu_label(preset: &TrayPresetEntry) -> String {
    let unit = if preset.skill_count == 1 { "skill" } else { "skills" };
    match preset.status() {
        TrayPresetStatus::Active => format!("✓ {} ({} {unit})", preset.name, preset.skill_count),
        TrayPresetStatus::Partial => format!(
            "{} ({}/{} synced)",
            preset.name, preset.synced_pairs, preset.total_pairs
        ),
        _ => format!("{} ({} {unit})", preset.name, preset.skill_count),
    }
}

fn preset_id_from_menu_id(menu_id: &str) -> Option<(&str, scenario_service_alias::BatchApplyMode)> {
    if let Some(id) = menu_id.strip_prefix(TRAY_PRESET_ADD_PREFIX) {
        return Some((id, scenario_service_alias::BatchApplyMode::Add));
    }
    if let Some(id) = menu_id.strip_prefix(TRAY_PRESET_REMOVE_PREFIX) {
        return Some((id, scenario_service_alias::BatchApplyMode::Remove));
    }
    None
}

mod scenario_service_alias {
    pub use crate::core::scenario_service::BatchApplyMode;
}

fn build_tray_menu<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &Arc<core::skill_store::SkillStore>,
) -> tauri::Result<(tauri::menu::Menu<R>, String)> {
    let data = collect_tray_menu_data(store);
    build_tray_menu_from_data(app, &data)
}

fn build_tray_menu_from_data<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    data: &TrayMenuData,
) -> tauri::Result<(tauri::menu::Menu<R>, String)> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};

    let menu = Menu::new(app)?;

    let app_name = MenuItem::with_id(app, "tray-app-name", "Skills Manager", false, None::<&str>)?;
    menu.append(&app_name)?;

    let status_line = MenuItem::with_id(
        app,
        "tray-status-line",
        format_status_line(data),
        false,
        None::<&str>,
    )?;
    menu.append(&status_line)?;

    if data.update_count > 0 {
        let updates_label = if data.update_count == 1 {
            "1 skill update available".to_string()
        } else {
            format!("{} skill updates available", data.update_count)
        };
        let updates_item =
            MenuItem::with_id(app, TRAY_OPEN_UPDATES_ID, updates_label, true, None::<&str>)?;
        menu.append(&updates_item)?;
    }

    menu.append(&PredefinedMenuItem::separator(app)?)?;

    let presets_submenu = Submenu::new(app, "Presets", true)?;
    if data.coding_agent_count == 0 {
        let no_agents = MenuItem::with_id(
            app,
            "tray-presets-no-agents",
            "No coding agents connected",
            false,
            None::<&str>,
        )?;
        presets_submenu.append(&no_agents)?;
    } else {
        let visible: Vec<_> = data
            .presets
            .iter()
            .filter(|p| !matches!(p.status(), TrayPresetStatus::Empty))
            .collect();
        if visible.is_empty() {
            let empty = MenuItem::with_id(
                app,
                "tray-presets-empty",
                "No presets with skills",
                false,
                None::<&str>,
            )?;
            presets_submenu.append(&empty)?;
        } else {
            for preset in visible {
                let (id, _action) = preset_menu_item_id(preset);
                let label = preset_menu_label(preset);
                let item = MenuItem::with_id(app, id, label, true, None::<&str>)?;
                presets_submenu.append(&item)?;
            }
        }
    }
    menu.append(&presets_submenu)?;

    menu.append(&PredefinedMenuItem::separator(app)?)?;

    let show_item = MenuItem::with_id(app, "show", "Open Skills Manager", true, None::<&str>)?;
    menu.append(&show_item)?;

    let check_label = if data.check_updates_running {
        "Checking for updates..."
    } else {
        "Check for skill updates"
    };
    let check_item = MenuItem::with_id(
        app,
        TRAY_CHECK_UPDATES_ID,
        check_label,
        !data.check_updates_running,
        None::<&str>,
    )?;
    menu.append(&check_item)?;

    let folder_item = MenuItem::with_id(
        app,
        TRAY_OPEN_FOLDER_ID,
        "Open Skills Folder",
        true,
        None::<&str>,
    )?;
    menu.append(&folder_item)?;

    menu.append(&PredefinedMenuItem::separator(app)?)?;

    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    menu.append(&quit_item)?;

    Ok((menu, format_tooltip(data)))
}

pub(crate) fn refresh_tray_menu<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    if app.tray_by_id(MAIN_TRAY_ID).is_none() {
        return Ok(());
    }
    let store = app
        .state::<Arc<core::skill_store::SkillStore>>()
        .inner()
        .clone();
    // Collect data off the main thread (DB reads are cheap, no FFI). The
    // actual `Menu::new`, `set_menu`, `set_tooltip` calls hit native macOS
    // (NSMenu / NSStatusItem) APIs and MUST run on the main thread — calling
    // them from a worker thread is UB and crashed the app with a
    // `slice::from_raw_parts` alignment panic during repeated tray actions.
    let data = collect_tray_menu_data(&store);
    let app_for_main = app.clone();
    app.run_on_main_thread(move || {
        let Some(tray) = app_for_main.tray_by_id(MAIN_TRAY_ID) else {
            return;
        };
        match build_tray_menu_from_data(&app_for_main, &data) {
            Ok((menu, tooltip)) => {
                if let Err(err) = tray.set_menu(Some(menu)) {
                    log::warn!("tray set_menu failed: {err}");
                }
                if let Err(err) = tray.set_tooltip(Some(&tooltip)) {
                    log::warn!("tray set_tooltip failed: {err}");
                }
            }
            Err(err) => log::warn!("build_tray_menu_from_data failed: {err}"),
        }
    })
    .map_err(|e| e.to_string())
}

/// Coalesce bursts into at most one tray rebuild per 300 ms window. Avoids
/// rebuilding the menu N times when `PresetBar` loops `skill × agent` calls
/// through `sync_skill_to_tool`. The first request schedules the rebuild;
/// any further requests during the wait window are absorbed.
static TRAY_REFRESH_PENDING: AtomicBool = AtomicBool::new(false);
const TRAY_REFRESH_DEBOUNCE: Duration = Duration::from_millis(300);

pub(crate) fn schedule_tray_refresh(app: &tauri::AppHandle) {
    if TRAY_REFRESH_PENDING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(TRAY_REFRESH_DEBOUNCE).await;
        TRAY_REFRESH_PENDING.store(false, Ordering::SeqCst);
        if let Err(err) = refresh_tray_menu(&app) {
            log::debug!("debounced tray refresh failed: {err}");
        }
    });
}

fn apply_preset_from_tray<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    preset_id: &str,
    mode: core::scenario_service::BatchApplyMode,
) {
    let store = app
        .state::<Arc<core::skill_store::SkillStore>>()
        .inner()
        .clone();
    let app = app.clone();
    let preset_id = preset_id.to_string();

    tauri::async_runtime::spawn(async move {
        let store_for_task = store.clone();
        let preset_id_for_task = preset_id.clone();
        // Result variants:
        //   Ok(true)  — the batch actually ran (do success-side effects)
        //   Ok(false) — skipped because another apply is in-flight or the
        //               preset/agent set was empty (no real work happened, so
        //               do NOT emit app-files-changed: that would lie to the
        //               frontend about state changes that didn't occur)
        //   Err(_)    — failure inside the batch
        let result = tauri::async_runtime::spawn_blocking(move || -> Result<bool, String> {
            let _guard = match TRAY_PRESET_APPLY_LOCK.try_lock() {
                Ok(guard) => guard,
                Err(_) => {
                    log::debug!(
                        "Another preset apply in flight, ignoring tray click for {preset_id_for_task}"
                    );
                    return Ok(false);
                }
            };
            core::scenario_service::ensure_scenario_exists(&store_for_task, &preset_id_for_task)
                .map_err(|e| e.to_string())?;
            let skill_ids = store_for_task
                .get_skill_ids_for_scenario(&preset_id_for_task)
                .map_err(|e| e.to_string())?;
            if skill_ids.is_empty() {
                return Ok(false);
            }
            let tool_keys: Vec<String> =
                core::tool_adapters::enabled_installed_adapters(&store_for_task)
                    .into_iter()
                    .filter(|adapter| {
                        matches!(adapter.category, core::tool_adapters::ToolCategory::Coding)
                    })
                    .map(|adapter| adapter.key)
                    .collect();
            if tool_keys.is_empty() {
                return Ok(false);
            }
            core::scenario_service::apply_skills_to_tools(
                &store_for_task,
                &skill_ids,
                &tool_keys,
                mode,
            )
            .map_err(|e| e.to_string())?;
            Ok(true)
        })
        .await;

        match result {
            Ok(Ok(true)) => {
                if let Err(err) = refresh_tray_menu(&app) {
                    log::warn!("Failed to refresh tray menu after preset apply: {err}");
                }
                if let Err(err) = app.emit("app-files-changed", ()) {
                    log::warn!("Failed to emit app-files-changed after tray preset apply: {err}");
                }
            }
            Ok(Ok(false)) => {
                // Refresh the menu so the user still sees fresh status (no
                // app-files-changed because nothing actually changed on disk).
                if let Err(err) = refresh_tray_menu(&app) {
                    log::debug!("Failed to refresh tray menu after skipped preset apply: {err}");
                }
            }
            Ok(Err(err)) => log::error!("Tray preset apply failed for {preset_id}: {err}"),
            Err(err) => log::error!("Tray preset apply task panicked: {err}"),
        }
    });
}

fn check_updates_from_tray<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if TRAY_CHECK_UPDATES_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::debug!("Skill update check already running, ignoring tray trigger");
        return;
    }
    if let Err(err) = refresh_tray_menu(app) {
        log::warn!("Failed to refresh tray menu before update check: {err}");
    }

    let store = app
        .state::<Arc<core::skill_store::SkillStore>>()
        .inner()
        .clone();
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        let store_for_task = store.clone();
        // Note: this intentionally does NOT take TRAY_PRESET_APPLY_LOCK.
        // Update checks only mutate `update_status` columns; preset apply
        // mutates `skill_targets`. They're orthogonal, and sharing a lock
        // would silently drop preset clicks made during a long-running check.
        let result = tauri::async_runtime::spawn_blocking(move || {
            let proxy_url = store_for_task.proxy_url();
            let ids: Vec<String> = store_for_task
                .get_all_skills()
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|skill| skill.id)
                .collect();
            for skill_id in ids {
                // Yield the lock between checks so a waiting user-initiated
                // operation isn't starved by this loop re-acquiring it
                // immediately (mirrors the auto-updater's FOREGROUND_YIELD).
                std::thread::sleep(std::time::Duration::from_millis(200));
                let _repo_lock = match core::repo_lock::RepoLock::acquire("tray check skill update")
                {
                    Ok(lock) => lock,
                    Err(err) => {
                        log::warn!("Tray update check: failed to acquire repo lock for {skill_id}: {err}");
                        continue;
                    }
                };
                if let Err(err) = commands::skills::check_skill_update_internal(
                    &store_for_task,
                    &skill_id,
                    true,
                    proxy_url.as_deref(),
                ) {
                    log::warn!("Tray update check failed for {skill_id}: {err}");
                }
            }
            Ok::<(), String>(())
        })
        .await;

        TRAY_CHECK_UPDATES_RUNNING.store(false, Ordering::SeqCst);

        match result {
            Ok(Ok(())) => {
                // Route through the shared completion helper so the manual
                // tray check writes `auto_update_last_run_at`, emits the same
                // `AutoUpdatePayload { ran_at }` the Settings listener
                // expects, and prevents the background scheduler from firing
                // a redundant round immediately after.
                core::skill_auto_updater::record_round_completion(&app_handle, &store);
            }
            Ok(Err(err)) => {
                log::error!("Tray update check failed: {err}");
                if let Err(err) = refresh_tray_menu(&app_handle) {
                    log::warn!("Failed to refresh tray menu after failed update check: {err}");
                }
            }
            Err(err) => {
                log::error!("Tray update check task panicked: {err}");
                if let Err(err) = refresh_tray_menu(&app_handle) {
                    log::warn!("Failed to refresh tray menu after update check panic: {err}");
                }
            }
        }
    });
}

fn open_skills_folder_from_tray<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let app_handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let repo_path = core::central_repo::base_dir();

        #[cfg(target_os = "macos")]
        let mut cmd = std::process::Command::new("open");
        #[cfg(target_os = "windows")]
        let mut cmd = {
            let mut c = std::process::Command::new("explorer");
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x08000000);
            c
        };
        #[cfg(target_os = "linux")]
        let mut cmd = std::process::Command::new("xdg-open");

        let status = cmd.arg(&repo_path).status();
        match status {
            Ok(_status) => {
                #[cfg(not(target_os = "windows"))]
                if !_status.success() {
                    log::warn!(
                        "Tray open folder: file manager exited with status: {}",
                        _status
                    );
                }
            }
            Err(err) => log::warn!("Tray open folder failed: {err}"),
        }
        let _ = app_handle;
    });
}

fn open_updates_from_tray(app: &tauri::AppHandle) {
    restore_main_window(app);
    if let Err(err) = app.emit(TRAY_OPEN_UPDATES_EVENT, ()) {
        log::warn!("Failed to emit {TRAY_OPEN_UPDATES_EVENT}: {err}");
    }
}

fn ensure_tray_icon(app: &tauri::AppHandle) -> tauri::Result<()> {
    if app.tray_by_id(MAIN_TRAY_ID).is_some() {
        return Ok(());
    }

    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let store = app
        .state::<Arc<core::skill_store::SkillStore>>()
        .inner()
        .clone();
    let (menu, tooltip) = build_tray_menu(app, &store)?;

    let mut builder = TrayIconBuilder::with_id(MAIN_TRAY_ID)
        .tooltip(tooltip)
        .menu(&menu)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            match id {
                "show" => {
                    log::debug!("Tray menu clicked: show");
                    restore_main_window(app)
                }
                "quit" => {
                    log::debug!("Tray menu clicked: quit");
                    request_quit(app)
                }
                TRAY_OPEN_UPDATES_ID => {
                    log::debug!("Tray menu clicked: open updates");
                    open_updates_from_tray(app);
                }
                TRAY_OPEN_FOLDER_ID => {
                    log::debug!("Tray menu clicked: open skills folder");
                    open_skills_folder_from_tray(app);
                }
                TRAY_CHECK_UPDATES_ID => {
                    log::debug!("Tray menu clicked: check for updates");
                    check_updates_from_tray(app);
                }
                other => {
                    if let Some((preset_id, mode)) = preset_id_from_menu_id(other) {
                        log::debug!("Tray menu clicked: preset {preset_id} mode {:?}", mode);
                        apply_preset_from_tray(app, preset_id, mode);
                    }
                }
            }
        });

    if let Some(icon) = load_custom_tray_icon().or_else(|| app.default_window_icon().cloned()) {
        builder = builder.icon(icon);
    }

    #[cfg(target_os = "macos")]
    {
        // Render the original white PNG directly for maximum brightness.
        builder = builder.icon_as_template(false);
    }

    // On macOS, left-click on tray icon opens the menu by default;
    // on Windows/Linux, left-click restores the window directly.
    if !cfg!(target_os = "macos") {
        builder = builder
            .show_menu_on_left_click(false)
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    restore_main_window(tray.app_handle());
                }
            });
    }

    let _tray = builder.build(app)?;
    log::debug!("Tray icon created");
    Ok(())
}

pub fn set_tray_icon_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let app_for_main = app.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let result = if enabled {
            ensure_tray_icon(&app_for_main).map_err(|e| e.to_string())
        } else {
            let _ = app_for_main.remove_tray_by_id(MAIN_TRAY_ID);
            log::debug!("Tray icon removed");
            Ok(())
        };
        let _ = tx.send(result);
    })
    .map_err(|e| e.to_string())?;

    rx.recv()
        .map_err(|e| format!("Failed to receive tray update result: {e}"))?
}

/// Quit the application cleanly: destroy the main window, then exit.
///
/// Do NOT signal our process group here (e.g. `kill(-pgid, SIGTERM)`).
/// On Linux the app inherits the launcher's pgid — that may be the user's
/// desktop session (issue #47, tearing down GNOME) or the developer's shell
/// (terminating the parent terminal and its sibling jobs). Either is
/// catastrophic and not worth the convenience of auto-cleaning a stray
/// `tauri dev` vite process.
pub fn quit_app(app: &tauri::AppHandle) {
    QUITTING.store(true, Ordering::SeqCst);
    if let Some(w) = app.get_webview_window("main") {
        if let Err(err) = w.destroy() {
            log::error!("Failed to destroy main window while quitting: {err}");
        }
    }
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let pre_builder_start = Instant::now();
    let (store, startup_timings) =
        core::app_state::initialize_store().expect("Failed to initialize app state");
    let pre_builder_ms = pre_builder_start.elapsed().as_millis();
    let store_for_setup = store.clone();

    let cancel_registry = Arc::new(core::install_cancel::InstallCancelRegistry::new());

    let builder_start = Instant::now();
    tauri::Builder::default()
        .manage(store)
        .manage(cancel_registry)
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            restore_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            // Snapshot the builder->setup gap BEFORE doing any work in setup,
            // so the label reflects only the time Tauri spent constructing
            // the App between Builder::default() and invoking this callback.
            let builder_to_setup_ms = builder_start.elapsed().as_millis();
            let setup_start = Instant::now();
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .level_for("tao", log::LevelFilter::Warn)
                    .level_for("wry", log::LevelFilter::Warn)
                    .level_for("hyper", log::LevelFilter::Warn)
                    .level_for("reqwest", log::LevelFilter::Warn)
                    .level_for("rustls", log::LevelFilter::Warn)
                    .max_file_size(5 * 1024 * 1024)
                    .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(3))
                    .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                    .format(|out, message, record| {
                        out.finish(format_args!(
                            "{} {:5} [{}] {}",
                            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z"),
                            record.level(),
                            record.target(),
                            message
                        ))
                    })
                    .build(),
            )?;

            core::panic_log::install_panic_hook(app.handle().clone());
            log::info!(
                "app start: version={} os={} arch={}",
                app.config().version.clone().unwrap_or_default(),
                std::env::consts::OS,
                std::env::consts::ARCH
            );
            log::info!(
                "startup: pre_builder {} ms, builder_to_setup {} ms",
                pre_builder_ms,
                builder_to_setup_ms
            );
            startup_timings.log();

            // One-time repair for skills uploaded before sync targets were
            // registered on import: they have a center record but no target,
            // leaving them button-less in the workspace. Idempotent and cheap
            // once repaired.
            let step = Instant::now();
            let repaired =
                commands::agent_workspace::backfill_stranded_agent_targets(&store_for_setup);
            if repaired > 0 {
                log::info!(
                    "startup: backfilled {} stranded agent skill target(s) in {} ms",
                    repaired,
                    step.elapsed().as_millis()
                );
            }

            let step = Instant::now();
            if is_tray_icon_enabled(&store_for_setup) {
                ensure_tray_icon(app.handle())?;
                log::info!(
                    "startup: ensure_tray_icon done in {} ms",
                    step.elapsed().as_millis()
                );
            } else {
                log::info!("startup: tray icon disabled");
            }

            let step = Instant::now();
            core::file_watcher::start_file_watcher(app.handle().clone(), store_for_setup.clone());
            log::info!(
                "startup: start_file_watcher done in {} ms",
                step.elapsed().as_millis()
            );

            let step = Instant::now();
            core::skill_auto_updater::start(app.handle().clone(), store_for_setup.clone());
            log::info!(
                "startup: skill auto-updater spawned in {} ms",
                step.elapsed().as_millis()
            );

            // Intercept window close — let frontend decide (close vs hide to tray)
            // When QUITTING is set, allow the close to proceed so the process fully exits.
            let step = Instant::now();
            let win = app.get_webview_window("main").unwrap();
            let win_for_event = win.clone();
            win.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if QUITTING.load(Ordering::SeqCst) {
                        return; // allow close
                    }
                    win_for_event.emit("window-close-requested", ()).ok();
                    api.prevent_close();
                }
            });
            log::info!(
                "startup: window handle + close hook in {} ms",
                step.elapsed().as_millis()
            );

            log::info!(
                "startup: setup() body total {} ms",
                setup_start.elapsed().as_millis()
            );

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Tools
            commands::tools::get_tool_status,
            commands::tools::set_tool_enabled,
            commands::tools::set_all_tools_enabled,
            commands::tools::get_tool_order_cmd,
            commands::tools::set_tool_order_cmd,
            commands::tools::set_custom_tool_path,
            commands::tools::reset_custom_tool_path,
            commands::tools::set_custom_tool_project_path,
            commands::tools::reset_custom_tool_project_path,
            commands::tools::add_custom_tool,
            commands::tools::remove_custom_tool,
            // Skills
            commands::skills::get_managed_skills,
            commands::skills::get_skills_for_preset,
            commands::skills::get_skill_document,
            commands::skills::get_source_skill_document,
            commands::skills::get_skill_source_diff,
            commands::skills::delete_managed_skill,
            commands::skills::delete_managed_skills,
            commands::skills::install_local,
            commands::skills::install_git,
            commands::skills::preview_git_install,
            commands::skills::confirm_git_install,
            commands::skills::cancel_git_preview,
            commands::skills::install_from_skillssh,
            commands::skills::check_skill_update,
            commands::skills::check_all_skill_updates,
            commands::skills::update_skill,
            commands::skills::batch_update_skills,
            commands::skills::reimport_local_skill,
            commands::skills::relink_local_skill_source,
            commands::skills::detach_local_skill_source,
            commands::skills::get_all_tags,
            commands::skills::set_skill_tags,
            commands::skills::rename_tag,
            commands::skills::delete_tag,
            commands::skills::cancel_install,
            commands::skills::batch_import_folder,
            // Sync
            commands::sync::sync_skill_to_tool,
            commands::sync::unsync_skill_from_tool,
            commands::sync::get_skill_tool_toggles,
            commands::sync::set_skill_tool_toggle,
            // Scan
            commands::scan::scan_local_skills,
            commands::scan::import_existing_skill,
            commands::scan::import_all_discovered,
            // Browse
            commands::browse::fetch_leaderboard,
            commands::browse::search_skillssh,
            // Settings
            commands::settings::get_settings,
            commands::settings::set_settings,
            commands::settings::get_central_repo_path,
            commands::settings::get_central_repo_path_override,
            commands::settings::get_central_repo_warnings,
            commands::settings::set_central_repo_path,
            commands::settings::open_central_repo_folder,
            commands::settings::check_app_update,
            commands::settings::get_diagnostic_info,
            commands::settings::get_recent_log_excerpt,
            commands::settings::export_logs_zip,
            commands::settings::log_startup_event,
            commands::settings::check_last_panic,
            commands::settings::clear_last_panic,
            commands::settings::app_exit,
            commands::settings::hide_to_tray,
            // Git Backup
            commands::git_backup::git_backup_fetch,
            commands::git_backup::git_backup_status,
            commands::git_backup::git_backup_init,
            commands::git_backup::git_backup_set_remote,
            commands::git_backup::git_backup_remove_remote,
            commands::git_backup::git_backup_commit,
            commands::git_backup::git_backup_push,
            commands::git_backup::git_backup_pull,
            commands::git_backup::git_backup_clone,
            commands::git_backup::git_backup_reclone,
            commands::git_backup::git_backup_create_snapshot,
            commands::git_backup::git_backup_list_versions,
            commands::git_backup::git_backup_restore_version,
            // Projects
            commands::projects::get_projects,
            commands::projects::add_project,
            commands::projects::add_linked_workspace,
            commands::projects::remove_project,
            commands::projects::scan_projects,
            commands::projects::get_project_agent_targets,
            commands::projects::get_project_skills,
            commands::projects::get_project_skill_document,
            commands::projects::import_project_skill_to_center,
            commands::projects::export_skill_to_project,
            commands::projects::update_project_skill_to_center,
            commands::projects::update_project_skill_from_center,
            commands::projects::toggle_project_skill,
            commands::projects::delete_project_skill,
            commands::projects::slugify_skill_names,
            // Agent local workspace
            commands::agent_workspace::get_global_local_skills,
            commands::agent_workspace::get_global_local_skill_document,
            commands::agent_workspace::import_global_local_skill_to_center,
            commands::agent_workspace::update_global_local_skill_from_center,
            commands::agent_workspace::delete_global_local_skill,
            // Presets
            commands::presets::get_presets,
            commands::presets::get_active_preset,
            commands::presets::create_preset,
            commands::presets::update_preset,
            commands::presets::delete_preset,
            commands::presets::switch_preset,
            commands::presets::apply_preset_to_default,
            commands::presets::apply_preset_to_coding_agents,
            commands::presets::add_skill_to_preset,
            commands::presets::remove_skill_from_preset,
            commands::presets::reorder_presets,
            commands::projects::reorder_projects,
            commands::presets::get_preset_skill_order,
            commands::presets::reorder_preset_skills,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
