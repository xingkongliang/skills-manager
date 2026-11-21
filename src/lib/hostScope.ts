import type { InvokeArgs } from "@tauri-apps/api/core";

import { isString } from "../utils";

/**
 * Which commands follow the active host. Everything else always runs on this
 * computer: window, tray, app updates, backup, skills.sh browsing and the
 * remote host list itself.
 */

/** Commands a host runs. Must equal `host_dispatch::COMMANDS` in Rust. */
export const HOST_SCOPED_COMMANDS: ReadonlySet<string> = new Set([
  "operation_request",
  "operation_results_request",
  "instructions_request",
  "mcps_request",
  "project_setup_request",
  "resource_sync_request",
  // Tools
  "get_tool_status",
  "set_tool_enabled",
  "set_all_tools_enabled",
  "get_tool_order_cmd",
  "set_tool_order_cmd",
  "set_custom_tool_path",
  "reset_custom_tool_path",
  "set_custom_tool_project_path",
  "reset_custom_tool_project_path",
  "add_custom_tool",
  "remove_custom_tool",
  // Skills
  "get_managed_skills",
  "get_skills_for_preset",
  "get_skill_document",
  "get_source_skill_document",
  "get_skill_source_diff",
  "delete_managed_skill",
  "delete_managed_skills",
  "create_skill",
  "install_local",
  "install_git",
  "preview_git_install",
  "confirm_git_install",
  "cancel_git_preview",
  "install_from_skillssh",
  "check_skill_update",
  "check_all_skill_updates",
  "update_skill",
  "batch_update_skills",
  "reimport_local_skill",
  "relink_local_skill_source",
  "detach_local_skill_source",
  "get_all_tags",
  "set_skill_tags",
  "rename_tag",
  "delete_tag",
  "cancel_install",
  "batch_import_folder",
  // Sync
  "sync_skill_to_tool",
  "unsync_skill_from_tool",
  "resolve_edited_copy",
  "get_skill_tool_toggles",
  "set_skill_tool_toggle",
  // Sync (SID-110): a host's Sync, run over its session
  "sync_access_key",
  "sync_access_setup",
  "sync_access_check",
  "sync_join_preview",
  "sync_join",
  "sync_undo_join",
  "sync_leave",
  "sync_pre_join_copies",
  "sync_delete_pre_join_copy",
  "sync_agent_log",
  "sync_status",
  "sync_now",
  "sync_history",
  "sync_conflicts",
  "sync_review",
  "sync_resolve",
  "sync_resolve_preset",
  "sync_resolve_resource",
  "sync_restore",
  "sync_pause",
  "sync_resume",
  // Scan
  "scan_local_skills",
  "import_existing_skill",
  "import_all_discovered",
  // Agent local workspace
  "get_global_local_skills",
  "get_global_local_skill_document",
  "import_global_local_skill_to_center",
  "update_global_local_skill_from_center",
  "delete_global_local_skill",
  // Projects
  "project_git_request",
  "get_projects",
  "add_project",
  "add_linked_workspace",
  "remove_project",
  "reorder_projects",
  "scan_projects",
  "get_project_agent_targets",
  "get_project_skills",
  "get_project_skill_document",
  "import_project_skill_to_center",
  "update_project_skill_to_center",
  "export_skill_to_project",
  "update_project_skill_from_center",
  "toggle_project_skill",
  "delete_project_skill",
  "set_project_agent_keys",
  "preview_project_agent_change",
  "apply_project_agent_change",
  "set_project_skill_agents",
  "clear_project_skill_agents",
  "preview_project_retired_links",
  "apply_project_retired_links",
  "set_project_deploy_mode",
  "preview_project_convert_to_copy",
  "apply_project_convert_to_copy",
  // Host file system
  "list_directory",
  // Presets
  "get_presets",
  "get_active_preset",
  "create_preset",
  "update_preset",
  "delete_preset",
  "switch_preset",
  "apply_preset_to_default",
  "preview_preset_switch",
  "apply_preset_to_coding_agents",
  "add_skill_to_preset",
  "remove_skill_from_preset",
  "reorder_presets",
  "get_preset_skill_order",
  "reorder_preset_skills",
  // Settings (routed by key, see below)
  "get_settings",
  "set_settings",
  "get_central_repo_path",
  "get_central_repo_path_override",
  "get_central_repo_pending_path",
  "get_central_repo_warnings",
  "set_central_repo_path",
]);

/**
 * Settings that describe the app you sit at: window, tray, language and
 * backup. A host refuses to change them. Must equal `LOCAL_ONLY_SETTINGS` and
 * `LOCAL_ONLY_SETTING_PREFIXES` in `host_dispatch.rs`.
 */
export const LOCAL_ONLY_SETTING_KEYS: readonly string[] = [
  "theme",
  "language",
  "text_size",
  "close_action",
  "show_tray_icon",
];

export const LOCAL_ONLY_SETTING_PREFIXES: readonly string[] = ["backup_", "git_backup_", "github_"];

/** How this app lays out the library. They stay with the app, like the theme. */
export const VIEW_SETTING_KEYS: readonly string[] = ["library_group_by", "library_sort_by"];

/** Settings that describe the managed machine, so they follow the active host. */
export const HOST_SCOPED_SETTING_KEYS: ReadonlySet<string> = new Set([
  "sync_mode",
  "default_project_deploy_mode",
  "auto_update_check_interval",
  "auto_update_apply",
  "auto_update_last_run_at",
  "preset_switch_previewed",
  "update_check_ttl_minutes",
  "disabled_tools",
  "tool_order",
  "custom_tools",
  "custom_tool_paths",
  "custom_tool_project_paths",
  "proxy_url",
  "agent_control_setup_prompt",
]);

/** Per-project settings; projects live on the host. */
export const HOST_SCOPED_SETTING_PREFIXES: readonly string[] = ["project_last_used_export_agents:"];

export function isHostScopedSetting(key: string): boolean {
  return (
    HOST_SCOPED_SETTING_KEYS.has(key) ||
    HOST_SCOPED_SETTING_PREFIXES.some((prefix) => key.startsWith(prefix))
  );
}

/** Whether `command` with these `args` runs on the active host. */
export function isHostScoped(command: string, args?: InvokeArgs): boolean {
  if (!HOST_SCOPED_COMMANDS.has(command)) return false;

  if (command === "get_settings" || command === "set_settings") {
    const key = args && "key" in args ? args.key : undefined;

    return isString(key) && isHostScopedSetting(key);
  }

  return true;
}
