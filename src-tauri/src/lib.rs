use std::sync::Arc;
use tauri::Manager;

mod commands;
mod core;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Ensure central repo exists
    core::central_repo::ensure_central_repo().expect("Failed to create central repo");

    // Initialize database
    let db_path = core::central_repo::db_path();
    let store = Arc::new(
        core::skill_store::SkillStore::new(&db_path).expect("Failed to initialize database"),
    );
    initialize_startup_scenario(&store).expect("Failed to initialize startup scenario");

    let cancel_registry = Arc::new(core::install_cancel::InstallCancelRegistry::new());

    tauri::Builder::default()
        .manage(store)
        .manage(cancel_registry)
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Tools
            commands::tools::get_tool_status,
            // Skills
            commands::skills::get_managed_skills,
            commands::skills::get_skills_for_scenario,
            commands::skills::get_skill_document,
            commands::skills::delete_managed_skill,
            commands::skills::install_local,
            commands::skills::install_git,
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
            // Git Backup
            commands::git_backup::git_backup_status,
            commands::git_backup::git_backup_init,
            commands::git_backup::git_backup_set_remote,
            commands::git_backup::git_backup_commit,
            commands::git_backup::git_backup_push,
            commands::git_backup::git_backup_pull,
            commands::git_backup::git_backup_clone,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn initialize_startup_scenario(store: &Arc<core::skill_store::SkillStore>) -> Result<(), String> {
    let scenarios = store.get_all_scenarios().map_err(|e| e.to_string())?;
    if scenarios.is_empty() {
        return Ok(());
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
            commands::scenarios::unsync_scenario_skills(store, old_active)?;
        }

        store
            .set_active_scenario(&desired_active)
            .map_err(|e| e.to_string())?;
    }

    commands::scenarios::sync_scenario_skills(store, &desired_active)?;
    Ok(())
}
