use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Manager};

mod commands;
mod core;

/// Shared flag: when true, CloseRequested should NOT be prevented.
pub static QUITTING: AtomicBool = AtomicBool::new(false);
static TRAY_SCENARIO_SWITCH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
const MAIN_TRAY_ID: &str = "main-tray";
const TRAY_SCENARIO_ITEM_PREFIX: &str = "tray-scenario:";
const CUSTOM_TRAY_ICON_BYTES: &[u8] = include_bytes!("../icons/tray/tray-icon-32.png");

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

fn tray_scenario_item_id(scenario_id: &str) -> String {
    format!("{TRAY_SCENARIO_ITEM_PREFIX}{scenario_id}")
}

fn scenario_id_from_tray_item(menu_id: &str) -> Option<&str> {
    menu_id.strip_prefix(TRAY_SCENARIO_ITEM_PREFIX)
}

fn build_tray_menu<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: &Arc<core::skill_store::SkillStore>,
) -> tauri::Result<tauri::menu::Menu<R>> {
    use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

    let menu = Menu::new(app)?;
    let app_name = MenuItem::with_id(app, "tray-app-name", "Skills Manager", false, None::<&str>)?;
    menu.append(&app_name)?;

    let active_id = store.get_active_scenario_id().ok().flatten();
    let scenarios = store.get_all_scenarios().unwrap_or_default();
    let active_name = active_id.as_deref().and_then(|id| {
        scenarios
            .iter()
            .find(|scenario| scenario.id == id)
            .map(|scenario| scenario.name.as_str())
    });
    let active_label = MenuItem::with_id(
        app,
        "tray-active-scenario",
        format!("Current: {}", active_name.unwrap_or("None")),
        false,
        None::<&str>,
    )?;
    menu.append(&active_label)?;

    let first_separator = PredefinedMenuItem::separator(app)?;
    menu.append(&first_separator)?;

    let scenario_submenu = Submenu::new(app, "Switch Scenario", true)?;
    if scenarios.is_empty() {
        let empty_item = MenuItem::with_id(
            app,
            "tray-no-scenarios",
            "No scenarios",
            false,
            None::<&str>,
        )?;
        scenario_submenu.append(&empty_item)?;
    } else {
        for scenario in scenarios {
            let checked = active_id.as_deref() == Some(scenario.id.as_str());
            let scenario_item = CheckMenuItem::with_id(
                app,
                tray_scenario_item_id(&scenario.id),
                scenario.name,
                true,
                checked,
                None::<&str>,
            )?;
            scenario_submenu.append(&scenario_item)?;
        }
    }
    menu.append(&scenario_submenu)?;

    let second_separator = PredefinedMenuItem::separator(app)?;
    menu.append(&second_separator)?;

    let show_item = MenuItem::with_id(app, "show", "Open Skills Manager", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    menu.append(&show_item)?;
    menu.append(&quit_item)?;

    Ok(menu)
}

pub(crate) fn refresh_tray_menu<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    let Some(tray) = app.tray_by_id(MAIN_TRAY_ID) else {
        return Ok(());
    };
    let store = app
        .state::<Arc<core::skill_store::SkillStore>>()
        .inner()
        .clone();
    let menu = build_tray_menu(app, &store).map_err(|e| e.to_string())?;
    tray.set_menu(Some(menu)).map_err(|e| e.to_string())
}

fn switch_scenario_from_tray<R: tauri::Runtime>(app: &tauri::AppHandle<R>, scenario_id: &str) {
    let store = app
        .state::<Arc<core::skill_store::SkillStore>>()
        .inner()
        .clone();
    let app = app.clone();
    let scenario_id = scenario_id.to_string();

    tauri::async_runtime::spawn(async move {
        let store_for_task = store.clone();
        let scenario_id_for_task = scenario_id.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let _switch_guard = TRAY_SCENARIO_SWITCH_LOCK
                .lock()
                .map_err(|_| "Tray scenario switch lock poisoned".to_string())?;
            let scenario_exists = store_for_task
                .get_all_scenarios()
                .map_err(|e| e.to_string())?
                .iter()
                .any(|scenario| scenario.id == scenario_id_for_task);
            if !scenario_exists {
                return Err("Scenario not found".to_string());
            }
            let current_active = store_for_task
                .get_active_scenario_id()
                .map_err(|e| e.to_string())?;
            if current_active.as_deref() == Some(&scenario_id_for_task) {
                return Ok(false);
            }
            if let Some(old_id) = current_active.as_deref() {
                commands::scenarios::unsync_scenario_skills(&store_for_task, old_id)
                    .map_err(|e| e.to_string())?;
            }
            store_for_task
                .set_active_scenario(&scenario_id_for_task)
                .map_err(|e| e.to_string())?;
            commands::scenarios::sync_scenario_skills(&store_for_task, &scenario_id_for_task)
                .map_err(|e| e.to_string())?;
            Ok::<bool, String>(true)
        })
        .await;

        match result {
            Ok(Ok(changed)) => {
                if changed {
                    if let Err(err) = refresh_tray_menu(&app) {
                        log::warn!("Failed to refresh tray menu after tray scenario switch: {err}");
                    }
                    if let Err(err) = app.emit("tray-scenario-switched", scenario_id) {
                        log::warn!("Failed to emit tray-scenario-switched: {err}");
                    }
                }
            }
            Ok(Err(err)) => log::error!("Failed to switch scenario from tray: {err}"),
            Err(err) => log::error!("Scenario switch task panicked: {err}"),
        }
    });
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
    let menu = build_tray_menu(app, &store)?;

    let mut builder = TrayIconBuilder::with_id(MAIN_TRAY_ID)
        .tooltip("Skills Manager")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                log::info!("Tray menu clicked: show");
                restore_main_window(app)
            }
            "quit" => {
                log::info!("Tray menu clicked: quit");
                request_quit(app)
            }
            id => {
                if let Some(scenario_id) = scenario_id_from_tray_item(id) {
                    log::info!("Tray menu clicked: switch scenario to {scenario_id}");
                    switch_scenario_from_tray(app, scenario_id);
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
    log::info!("Tray icon created");
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
            log::info!("Tray icon removed");
            Ok(())
        };
        let _ = tx.send(result);
    })
    .map_err(|e| e.to_string())?;

    rx.recv()
        .map_err(|e| format!("Failed to receive tray update result: {e}"))?
}

/// Quit the application cleanly: destroy the main window, then exit.
/// In dev mode, also kill sibling processes in the same process group
/// so that `tauri dev`'s beforeDevCommand (vite) gets cleaned up.
pub fn quit_app(app: &tauri::AppHandle) {
    QUITTING.store(true, Ordering::SeqCst);
    if let Some(w) = app.get_webview_window("main") {
        if let Err(err) = w.destroy() {
            log::error!("Failed to destroy main window while quitting: {err}");
        }
    }
    // In dev mode, kill sibling processes (vite dev server) by signaling the process group.
    // Uses libc directly to avoid platform-specific `kill` command syntax differences.
    #[cfg(unix)]
    unsafe {
        // getpgrp() returns our process group ID; kill(-pgid, SIGTERM) sends to all in the group.
        let pgid = libc::getpgrp();
        libc::kill(-pgid, libc::SIGTERM);
    }
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Ensure central repo exists
    core::central_repo::ensure_central_repo().expect("Failed to create central repo");

    // Initialize database
    let db_path = core::central_repo::db_path();
    let store = Arc::new(
        core::skill_store::SkillStore::new(&db_path).expect("Failed to initialize database"),
    );
    let store_for_setup = store.clone();
    initialize_startup_scenario(&store).expect("Failed to initialize startup scenario");

    let cancel_registry = Arc::new(core::install_cancel::InstallCancelRegistry::new());

    tauri::Builder::default()
        .manage(store)
        .manage(cancel_registry)
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            restore_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            if is_tray_icon_enabled(&store_for_setup) {
                ensure_tray_icon(app.handle())?;
            }

            // Intercept window close — let frontend decide (close vs hide to tray)
            // When QUITTING is set, allow the close to proceed so the process fully exits.
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

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Tools
            commands::tools::get_tool_status,
            commands::tools::set_tool_enabled,
            commands::tools::set_all_tools_enabled,
            // Skills
            commands::skills::get_managed_skills,
            commands::skills::get_skills_for_scenario,
            commands::skills::get_skill_document,
            commands::skills::delete_managed_skill,
            commands::skills::install_local,
            commands::skills::install_git,
            commands::skills::preview_git_install,
            commands::skills::confirm_git_install,
            commands::skills::cancel_git_preview,
            commands::skills::install_from_skillssh,
            commands::skills::check_skill_update,
            commands::skills::check_all_skill_updates,
            commands::skills::update_skill,
            commands::skills::reimport_local_skill,
            commands::skills::get_all_tags,
            commands::skills::set_skill_tags,
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
            commands::settings::open_central_repo_folder,
            commands::settings::check_app_update,
            commands::settings::app_exit,
            commands::settings::hide_to_tray,
            // Git Backup
            commands::git_backup::git_backup_status,
            commands::git_backup::git_backup_init,
            commands::git_backup::git_backup_set_remote,
            commands::git_backup::git_backup_commit,
            commands::git_backup::git_backup_push,
            commands::git_backup::git_backup_pull,
            commands::git_backup::git_backup_clone,
            commands::git_backup::git_backup_create_snapshot,
            commands::git_backup::git_backup_list_versions,
            commands::git_backup::git_backup_restore_version,
            // Projects
            commands::projects::get_projects,
            commands::projects::add_project,
            commands::projects::remove_project,
            commands::projects::scan_projects,
            commands::projects::get_project_skills,
            commands::projects::get_project_skill_document,
            commands::projects::import_project_skill_to_center,
            commands::projects::export_skill_to_project,
            commands::projects::update_project_skill_to_center,
            commands::projects::update_project_skill_from_center,
            commands::projects::toggle_project_skill,
            commands::projects::delete_project_skill,
            commands::projects::slugify_skill_names,
            // Scenarios
            commands::scenarios::get_scenarios,
            commands::scenarios::get_active_scenario,
            commands::scenarios::create_scenario,
            commands::scenarios::update_scenario,
            commands::scenarios::delete_scenario,
            commands::scenarios::switch_scenario,
            commands::scenarios::add_skill_to_scenario,
            commands::scenarios::remove_skill_from_scenario,
            commands::scenarios::reorder_scenarios,
            commands::projects::reorder_projects,
            commands::scenarios::get_scenario_skill_order,
            commands::scenarios::reorder_scenario_skills,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn initialize_startup_scenario(store: &Arc<core::skill_store::SkillStore>) -> Result<(), String> {
    let mut scenarios = store.get_all_scenarios().map_err(|e| e.to_string())?;
    if scenarios.is_empty() {
        let now = chrono::Utc::now().timestamp_millis();
        let default_scenario = core::skill_store::ScenarioRecord {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Default".to_string(),
            description: Some("Default startup scenario".to_string()),
            icon: None,
            sort_order: 0,
            created_at: now,
            updated_at: now,
        };
        store
            .insert_scenario(&default_scenario)
            .map_err(|e| e.to_string())?;
        scenarios.push(default_scenario);
    }

    let current_active = store.get_active_scenario_id().map_err(|e| e.to_string())?;
    let preferred_default = store.get_setting("default_scenario").ok().flatten();

    let desired_active = preferred_default
        .filter(|id| scenarios.iter().any(|scenario| scenario.id == *id))
        .or_else(|| {
            current_active
                .clone()
                .filter(|id| scenarios.iter().any(|scenario| scenario.id == *id))
        })
        .unwrap_or_else(|| scenarios[0].id.clone());

    if current_active.as_deref() != Some(desired_active.as_str()) {
        if let Some(old_active) = current_active.as_deref() {
            commands::scenarios::unsync_scenario_skills(store, old_active)
                .map_err(|e| e.to_string())?;
        }

        store
            .set_active_scenario(&desired_active)
            .map_err(|e| e.to_string())?;
    }

    commands::scenarios::sync_scenario_skills(store, &desired_active).map_err(|e| e.to_string())?;
    Ok(())
}
