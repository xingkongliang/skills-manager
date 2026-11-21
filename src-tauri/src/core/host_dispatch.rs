//! Runs a host-scoped command by name, outside Tauri.
//!
//! `args` is the same camelCase object the UI passes to `invoke`, read the way
//! Tauri reads it: a missing required key or a value of the wrong type is
//! `invalid_input`, a missing or null optional key is `None`, and unknown keys
//! are ignored. A name outside [`COMMANDS`] is `not_found`.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use crate::commands::{
    agent_workspace, directory, presets, projects, scan, settings, skills, sync, tools,
};
use crate::core::error::AppError;
use crate::core::host::HostCtx;
use crate::core::{sync_access, sync_join, sync_service, sync_timer};

/// Every command [`dispatch`] accepts.
pub const COMMANDS: &[&str] = &[
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
    // Sync (SID-110): the app runs a host's Sync over its session.
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
    // Settings
    "get_settings",
    "set_settings",
    "get_central_repo_path",
    "get_central_repo_path_override",
    "get_central_repo_pending_path",
    "get_central_repo_warnings",
    "set_central_repo_path",
];

/// Settings that describe the app you sit at rather than the machine it
/// manages: window, tray, language and backup. They are never written on a host.
const LOCAL_ONLY_SETTINGS: &[&str] = &[
    "theme",
    "language",
    "text_size",
    "close_action",
    "show_tray_icon",
];
const LOCAL_ONLY_SETTING_PREFIXES: &[&str] = &["backup_", "git_backup_", "github_"];

pub fn is_local_only_setting(key: &str) -> bool {
    LOCAL_ONLY_SETTINGS.contains(&key)
        || LOCAL_ONLY_SETTING_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
}

struct Args<'a>(&'a Value);

impl Args<'_> {
    fn req<T: DeserializeOwned>(&self, key: &str) -> Result<T, AppError> {
        let value = self
            .0
            .get(key)
            .ok_or_else(|| AppError::invalid_input(format!("Missing argument: {key}")))?;
        T::deserialize(value)
            .map_err(|e| AppError::invalid_input(format!("Invalid argument {key}: {e}")))
    }

    fn opt<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, AppError> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => self.req(key).map(Some),
        }
    }
}

fn reply<T: Serialize>(result: Result<T, AppError>) -> Result<Value, AppError> {
    serde_json::to_value(result?).map_err(AppError::internal)
}

pub fn dispatch(ctx: &HostCtx, command: &str, args: &Value) -> Result<Value, AppError> {
    let a = Args(args);
    match command {
        "operation_results_request" => {
            crate::core::operations::results::dispatch(ctx, a.req("request")?)
        }
        "operation_request" => crate::core::operations::dispatch(ctx, a.req("request")?),
        "project_git_request" => projects::project_git_request_core(
            ctx,
            &a.req::<String>("projectId")?,
            a.req("request")?,
        ),
        "instructions_request" => crate::core::instructions::dispatch(ctx, a.req("request")?),
        "mcps_request" => crate::core::mcps::dispatch(ctx, a.req("request")?),
        "project_setup_request" => crate::core::project_setup::dispatch(ctx, a.req("request")?),
        "resource_sync_request" => crate::core::resource_sync::dispatch(ctx, a.req("request")?),
        // Tools
        "get_tool_status" => reply(tools::get_tool_status_core(ctx)),
        "set_tool_enabled" => reply(tools::set_tool_enabled_core(
            ctx,
            a.req("key")?,
            a.req("enabled")?,
        )),
        "set_all_tools_enabled" => reply(tools::set_all_tools_enabled_core(ctx, a.req("enabled")?)),
        "get_tool_order_cmd" => reply(tools::get_tool_order_cmd_core(ctx)),
        "set_tool_order_cmd" => reply(tools::set_tool_order_cmd_core(ctx, a.req("order")?)),
        "set_custom_tool_path" => reply(tools::set_custom_tool_path_core(
            ctx,
            a.req("key")?,
            a.req("path")?,
        )),
        "reset_custom_tool_path" => reply(tools::reset_custom_tool_path_core(ctx, a.req("key")?)),
        "set_custom_tool_project_path" => reply(tools::set_custom_tool_project_path_core(
            ctx,
            a.req("key")?,
            a.opt("projectRelativeSkillsDir")?,
        )),
        "reset_custom_tool_project_path" => reply(tools::reset_custom_tool_project_path_core(
            ctx,
            a.req("key")?,
        )),
        "add_custom_tool" => reply(tools::add_custom_tool_core(
            &ctx.store,
            a.req("key")?,
            a.req("displayName")?,
            a.req("skillsDir")?,
            a.opt("projectRelativeSkillsDir")?,
        )),
        "remove_custom_tool" => reply(tools::remove_custom_tool_core(ctx, a.req("key")?)),
        // Skills
        "get_managed_skills" => reply(skills::get_managed_skills_core(ctx)),
        "get_skills_for_preset" => {
            reply(skills::get_skills_for_preset_core(ctx, a.req("presetId")?))
        }
        "get_skill_document" => reply(skills::get_skill_document_core(ctx, a.req("skillId")?)),
        "get_source_skill_document" => reply(skills::get_source_skill_document_core(
            ctx,
            a.req("skillId")?,
        )),
        "get_skill_source_diff" => {
            reply(skills::get_skill_source_diff_core(ctx, a.req("skillId")?))
        }
        "delete_managed_skill" => reply(skills::delete_managed_skill_core(ctx, a.req("skillId")?)),
        "delete_managed_skills" => {
            reply(skills::delete_managed_skills_core(ctx, a.req("skillIds")?))
        }
        "create_skill" => reply(skills::create_skill_core(ctx, a.req("request")?)),
        "install_local" => reply(skills::install_local_core(
            ctx,
            a.req("sourcePath")?,
            a.opt("name")?,
        )),
        "install_git" => reply(skills::install_git_core(
            ctx,
            a.req("repoUrl")?,
            a.opt("name")?,
        )),
        "preview_git_install" => reply(skills::preview_git_install_core(ctx, a.req("repoUrl")?)),
        "confirm_git_install" => reply(skills::confirm_git_install_core(
            ctx,
            a.req("repoUrl")?,
            a.req("tempDir")?,
            a.req("items")?,
        )),
        "cancel_git_preview" => reply(skills::cancel_git_preview_core(a.req("tempDir")?)),
        "install_from_skillssh" => reply(skills::install_from_skillssh_core(
            ctx,
            a.req("source")?,
            a.req("skillId")?,
        )),
        "check_skill_update" => reply(skills::check_skill_update_core(
            ctx,
            a.req("skillId")?,
            a.opt("force")?,
        )),
        "check_all_skill_updates" => {
            reply(skills::check_all_skill_updates_core(ctx, a.opt("force")?))
        }
        "update_skill" => reply(skills::update_skill_core(
            ctx,
            a.req("skillId")?,
            a.opt("approvedRemovals")?,
        )),
        "batch_update_skills" => reply(skills::batch_update_skills_core(ctx, a.req("skillIds")?)),
        "reimport_local_skill" => reply(skills::reimport_local_skill_core(
            ctx,
            a.req("skillId")?,
            a.opt("approvedRemovals")?,
        )),
        "relink_local_skill_source" => reply(skills::relink_local_skill_source_core(
            ctx,
            a.req("skillId")?,
            a.req("sourcePath")?,
            a.opt("approvedRemovals")?,
        )),
        "detach_local_skill_source" => reply(skills::detach_local_skill_source_core(
            ctx,
            a.req("skillId")?,
        )),
        "get_all_tags" => reply(skills::get_all_tags_core(ctx)),
        "set_skill_tags" => reply(skills::set_skill_tags_core(
            ctx,
            a.req("skillId")?,
            a.req("tags")?,
        )),
        "rename_tag" => reply(skills::rename_tag_core(
            ctx,
            a.req("oldName")?,
            a.req("newName")?,
        )),
        "delete_tag" => reply(skills::delete_tag_core(ctx, a.req("name")?)),
        "cancel_install" => reply(skills::cancel_install_core(ctx, a.req("key")?)),
        "batch_import_folder" => reply(skills::batch_import_folder_core(ctx, a.req("folderPath")?)),
        // Sync
        "sync_skill_to_tool" => reply(sync::sync_skill_to_tool_core(
            ctx,
            a.req("skillId")?,
            a.req("tool")?,
        )),
        "unsync_skill_from_tool" => reply(sync::unsync_skill_from_tool_core(
            ctx,
            a.req("skillId")?,
            a.req("tool")?,
        )),
        "resolve_edited_copy" => reply((|| {
            let skill: String = a.req("skillId")?;
            let tool: String = a.req("tool")?;
            let review: Option<String> = a.opt("review")?;
            let _lock = crate::core::repo_lock::RepoLock::acquire_foreground("resolve edited copy")
                .map_err(AppError::internal)?;
            crate::core::change_review::check(
                &ctx.store,
                review.as_deref(),
                "edited",
                &skill,
                Some(&tool),
            )
            .map_err(AppError::internal)?;
            sync::resolve_edited_copy_core(ctx, skill, tool, a.req("keepMine")?)
        })()),
        "sync_access_key" => reply(sync_access::public_key(&a.req::<String>("comment")?)),
        "sync_access_setup" => reply(sync_access::write_known_hosts(
            &a.req::<Vec<String>>("hostKeys")?,
        )),
        "sync_access_check" => reply(sync_access::check(&a.req::<String>("url")?)),
        "sync_join_preview" => reply(sync_join::preview(&ctx.store, &a.req::<String>("url")?)),
        "sync_join" => reply(
            sync_join::join(ctx, &a.req::<String>("url")?)
                .map(|preview| json!({ "preview": preview, "timer": sync_timer::install() })),
        ),
        "sync_undo_join" => reply(sync_join::undo_join(
            ctx,
            a.opt::<String>("copy")?.as_deref(),
        )),
        "sync_leave" => reply(sync_join::leave(&ctx.store).map(|()| sync_timer::remove())),
        "sync_pre_join_copies" => reply(Ok(sync_join::pre_join_copies())),
        "sync_delete_pre_join_copy" => {
            reply(sync_join::delete_pre_join_copy(&a.req::<String>("name")?))
        }
        "sync_agent_log" => reply(sync_service::agent_log(&ctx.store)),
        "sync_status" => reply(Ok(sync_service::sync_status(&ctx.store))),
        "sync_now" => reply(sync_service::sync_now(
            ctx,
            sync_service::SYNC_COMMIT_MESSAGE,
        )),
        "sync_history" => reply(sync_service::history(a.opt("limit")?)),
        "sync_review" => reply(
            crate::core::change_review::preview(
                &ctx.store,
                &a.req::<String>("kind")?,
                &a.req::<String>("key")?,
                a.opt::<String>("tool")?.as_deref(),
            )
            .map_err(AppError::internal),
        ),
        "sync_conflicts" => reply(Ok(sync_service::conflicts(&ctx.store))),
        "sync_resolve" => reply((|| {
            let keep: Option<String> = a.opt("keep")?;
            let (skill_id, action) = sync_service::resolve_choice(
                &ctx.store,
                &a.req::<String>("skill")?,
                keep.as_deref(),
                a.opt("both")?.unwrap_or(false),
            )?;
            sync_service::resolve_reviewed(
                ctx,
                &skill_id,
                action,
                a.opt::<String>("review")?.as_deref(),
            )
        })()),
        "sync_resolve_preset" => reply((|| {
            let choice = match (a.opt::<String>("keep")?, a.opt("combine")?.unwrap_or(false)) {
                (Some(id), _) => sync_service::PresetChoice::Keep(id),
                (None, true) => sync_service::PresetChoice::Combine,
                (None, false) => sync_service::PresetChoice::KeepBoth,
            };
            sync_service::resolve_preset_reviewed(
                &ctx.store,
                &a.req::<String>("preset")?,
                choice,
                a.opt::<String>("review")?.as_deref(),
            )
        })()),
        "sync_resolve_resource" => reply(
            crate::core::resource_sync::resolve_reviewed(
                ctx,
                &a.req::<String>("key")?,
                a.opt("other")?.unwrap_or(false),
                a.opt::<String>("review")?.as_deref(),
            )
            .map_err(|e| AppError::classify_git_error(format!("{e:#}"))),
        ),
        "sync_restore" => reply(sync_service::restore(ctx, &a.req::<String>("point")?)),
        "sync_pause" => reply(sync_service::set_paused(&ctx.store, true)),
        "sync_resume" => reply(sync_service::set_paused(&ctx.store, false)),
        "get_skill_tool_toggles" => reply(sync::get_skill_tool_toggles_core(
            ctx,
            a.req("skillId")?,
            a.req("presetId")?,
        )),
        "set_skill_tool_toggle" => reply(sync::set_skill_tool_toggle_core(
            ctx,
            a.req("skillId")?,
            a.req("presetId")?,
            a.req("tool")?,
            a.req("enabled")?,
        )),
        // Scan
        "scan_local_skills" => reply(scan::scan_local_skills_core(ctx)),
        "import_existing_skill" => reply(scan::import_existing_skill_core(
            ctx,
            a.req("sourcePath")?,
            a.opt("name")?,
        )),
        "import_all_discovered" => reply(scan::import_all_discovered_core(ctx)),
        // Agent local workspace
        "get_global_local_skills" => reply(agent_workspace::get_global_local_skills_core(
            ctx,
            a.req("agent")?,
        )),
        "get_global_local_skill_document" => {
            reply(agent_workspace::get_global_local_skill_document_core(
                ctx,
                a.req("agent")?,
                a.req("skillRelativePath")?,
            ))
        }
        "import_global_local_skill_to_center" => {
            reply(agent_workspace::import_global_local_skill_to_center_core(
                ctx,
                a.req("agent")?,
                a.req("skillRelativePath")?,
            ))
        }
        "update_global_local_skill_from_center" => {
            reply(agent_workspace::update_global_local_skill_from_center_core(
                ctx,
                a.req("agent")?,
                a.req("skillRelativePath")?,
            ))
        }
        "delete_global_local_skill" => reply(agent_workspace::delete_global_local_skill_core(
            ctx,
            a.req("agent")?,
            a.req("skillRelativePath")?,
        )),
        // Projects
        "get_projects" => reply(projects::get_projects_core(ctx)),
        "add_project" => reply(projects::add_project_core(
            ctx,
            a.req("path")?,
            a.opt("deployMode")?,
        )),
        "add_linked_workspace" => reply(projects::add_linked_workspace_core(
            ctx,
            a.req("name")?,
            a.req("path")?,
            a.opt("disabledPath")?,
        )),
        "remove_project" => reply(projects::remove_project_core(ctx, a.req("id")?)),
        "reorder_projects" => reply(projects::reorder_projects_core(ctx, a.req("ids")?)),
        "scan_projects" => reply(projects::scan_projects_core(ctx, a.req("root")?)),
        "get_project_agent_targets" => reply(projects::get_project_agent_targets_core(
            ctx,
            a.req("projectId")?,
        )),
        "get_project_skills" => reply(projects::get_project_skills_core(ctx, a.req("projectId")?)),
        "get_project_skill_document" => reply(projects::get_project_skill_document_core(
            ctx,
            a.req("projectId")?,
            a.req("skillRelativePath")?,
            a.req("agent")?,
        )),
        "import_project_skill_to_center" | "update_project_skill_to_center" => {
            reply(projects::import_project_skill_to_center_core(
                ctx,
                a.req("projectId")?,
                a.req("skillRelativePath")?,
                a.req("agent")?,
            ))
        }
        "export_skill_to_project" => reply(projects::export_skill_to_project_core(
            ctx,
            a.req("skillId")?,
            a.req("projectId")?,
            a.opt("agents")?,
        )),
        "update_project_skill_from_center" => {
            reply(projects::update_project_skill_from_center_core(
                ctx,
                a.req("projectId")?,
                a.req("skillRelativePath")?,
                a.req("agent")?,
            ))
        }
        "toggle_project_skill" => reply(projects::toggle_project_skill_core(
            ctx,
            a.req("projectId")?,
            a.req("skillRelativePath")?,
            a.req("agent")?,
            a.req("enabled")?,
        )),
        "delete_project_skill" => reply(projects::delete_project_skill_core(
            ctx,
            a.req("projectId")?,
            a.req("skillRelativePath")?,
            a.req("agent")?,
            a.opt("wholeSkill")?,
        )),
        "set_project_agent_keys" => reply(projects::set_project_agent_keys_core(
            ctx,
            a.req("projectId")?,
            a.opt("agentKeys")?,
        )),
        "preview_project_agent_change" => reply(projects::preview_project_agent_change_core(
            ctx,
            a.req("projectId")?,
            a.req("agentKeys")?,
        )),
        "apply_project_agent_change" => reply(projects::apply_project_agent_change_core(
            ctx,
            a.req("projectId")?,
            a.req("agentKeys")?,
        )),
        "set_project_skill_agents" => reply(projects::set_project_skill_agents_core(
            ctx,
            a.req("projectId")?,
            a.req("skillRelativePath")?,
            a.req("agentKeys")?,
        )),
        "clear_project_skill_agents" => reply(projects::clear_project_skill_agents_core(
            ctx,
            a.req("projectId")?,
            a.req("skillRelativePath")?,
        )),
        "preview_project_retired_links" => reply(projects::preview_project_retired_links_core(
            ctx,
            a.req("projectId")?,
        )),
        "apply_project_retired_links" => reply(projects::apply_project_retired_links_core(
            ctx,
            a.req("projectId")?,
        )),
        "set_project_deploy_mode" => reply(projects::set_project_deploy_mode_core(
            ctx,
            a.req("projectId")?,
            a.req("deployMode")?,
        )),
        "preview_project_convert_to_copy" => reply(projects::preview_project_convert_to_copy_core(
            ctx,
            a.req("projectId")?,
        )),
        "apply_project_convert_to_copy" => reply(projects::apply_project_convert_to_copy_core(
            ctx,
            a.req("projectId")?,
        )),
        // Host file system
        "list_directory" => reply(directory::list_directory_core(a.opt("path")?)),
        // Presets
        "get_presets" => reply(presets::get_presets_core(ctx)),
        "get_active_preset" => reply(presets::get_active_preset_core(ctx)),
        "create_preset" => reply(presets::create_preset_core(
            ctx,
            a.req("name")?,
            a.opt("description")?,
            a.opt("icon")?,
        )),
        "update_preset" => reply(presets::update_preset_core(
            ctx,
            a.req("id")?,
            a.req("name")?,
            a.opt("description")?,
            a.opt("icon")?,
        )),
        "delete_preset" => reply(presets::delete_preset_core(ctx, a.req("id")?)),
        "preview_preset_switch" => reply(presets::preview_preset_switch_core(ctx, a.req("id")?)),
        "switch_preset" | "apply_preset_to_default" => {
            let id: String = a.req("id")?;
            let review: Option<String> = a.opt("review")?;
            reply(
                crate::core::scenario_service::apply_scenario_reviewed(
                    &ctx.store,
                    &id,
                    review.as_deref(),
                )
                .and_then(crate::core::scenario_service::refusals_to_error),
            )
        }
        "apply_preset_to_coding_agents" => reply(presets::apply_preset_to_coding_agents_core(
            ctx,
            a.req("presetId")?,
            a.req("mode")?,
        )),
        "add_skill_to_preset" => reply(presets::add_skill_to_preset_core(
            ctx,
            a.req("skillId")?,
            a.req("presetId")?,
        )),
        "remove_skill_from_preset" => reply(presets::remove_skill_from_preset_core(
            ctx,
            a.req("skillId")?,
            a.req("presetId")?,
        )),
        "reorder_presets" => reply(presets::reorder_presets_core(ctx, a.req("ids")?)),
        "get_preset_skill_order" => reply(presets::get_preset_skill_order_core(
            ctx,
            a.req("presetId")?,
        )),
        "reorder_preset_skills" => reply(presets::reorder_preset_skills_core(
            ctx,
            a.req("presetId")?,
            a.req("skillIds")?,
        )),
        // Settings
        "get_settings" => reply(settings::get_settings_core(ctx, a.req("key")?)),
        "set_settings" => {
            let key: String = a.req("key")?;
            if is_local_only_setting(&key) {
                return Err(AppError::invalid_input(format!(
                    "Setting {key} belongs to this computer and is not changed on a host"
                )));
            }
            reply(settings::set_settings_core(ctx, key, a.req("value")?))
        }
        "get_central_repo_path" => reply(Ok(settings::get_central_repo_path())),
        "get_central_repo_path_override" => reply(Ok(settings::get_central_repo_path_override())),
        "get_central_repo_pending_path" => reply(Ok(settings::get_central_repo_pending_path())),
        "get_central_repo_warnings" => reply(Ok(settings::get_central_repo_warnings())),
        "set_central_repo_path" => reply(settings::set_central_repo_path_core(a.opt("path")?)),
        _ => Err(AppError::not_found(format!("Unknown command: {command}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::ErrorKind;
    use crate::core::host::{HostEvents, NoopEvents, RecordingEvents};
    use crate::core::skill_store::SkillStore;
    use serde_json::json;
    use std::sync::{Arc, MutexGuard};
    use tempfile::TempDir;

    /// A host on a temp store and a temp central repo. The repo override is
    /// process-wide, so the guard is held for the test's lifetime.
    struct TestHost {
        ctx: HostCtx,
        tmp: TempDir,
        _lock: MutexGuard<'static, ()>,
    }

    impl Drop for TestHost {
        fn drop(&mut self) {
            crate::core::central_repo::set_test_base_dir_override(None);
        }
    }

    fn test_host() -> TestHost {
        test_host_with(Arc::new(NoopEvents))
    }

    fn test_host_with(events: Arc<dyn HostEvents>) -> TestHost {
        let lock = crate::core::central_repo::test_base_dir_lock();
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("repo");
        crate::core::central_repo::set_test_base_dir_override(Some(base.clone()));
        std::fs::create_dir_all(crate::core::central_repo::skills_dir()).unwrap();
        let store = SkillStore::new(&base.join("test.db")).unwrap();
        TestHost {
            ctx: HostCtx::for_tests(store, events),
            tmp,
            _lock: lock,
        }
    }

    fn is_unknown_command(err: &AppError) -> bool {
        err.kind == ErrorKind::NotFound && err.message.starts_with("Unknown command")
    }

    /// Every listed command reaches its own arm. Each runs on a fresh temp
    /// host so one command's writes can't feed the next (a scan followed by
    /// import-all would copy this machine's agent skills). The mistyped
    /// `path` stops path-taking commands before they touch the disk —
    /// `set_central_repo_path` would otherwise rewrite this user's config.
    /// The two scan commands are left out: they read the agent folders under
    /// this machine's real home.
    #[test]
    fn every_listed_command_is_dispatched() {
        let reads_real_home = ["scan_local_skills", "import_all_discovered"];
        for command in COMMANDS.iter().filter(|c| !reads_real_home.contains(c)) {
            let host = test_host();
            if let Err(err) = dispatch(&host.ctx, command, &json!({ "path": 0 })) {
                assert!(
                    !is_unknown_command(&err),
                    "{command} is listed but not routed"
                );
            }
        }
    }

    #[test]
    fn a_late_remote_seal_or_resume_cannot_override_a_newer_shutdown_decision() {
        let host = test_host();
        let project = operation_project(&host, "late-seal");
        let request = |action, epoch| {
            dispatch(
                &host.ctx,
                "operation_request",
                &json!({"request":{"action":action,"epoch":epoch}}),
            )
            .unwrap()
        };
        assert_eq!(request("shutdown_seal", 1)["ready"], true);
        request("shutdown_resume", 2);
        assert_eq!(request("shutdown_seal", 1)["ready"], false);
        submit_operation(&host, "after-late-seal", &project);
        settled_operation(&host, "after-late-seal");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while request("shutdown_seal", 3)["ready"] != true {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        request("shutdown_resume", 2);
        assert!(dispatch(&host.ctx,"operation_request", &json!({"request":{"action":"submit_project_update","operation_id":"after-late-resume","project_id":project["id"]}})).is_err());
        request("shutdown_resume", 4);
    }

    #[test]
    fn shutdown_cancel_drops_waiting_work_and_retains_completed_changes() {
        let host = test_host();
        let done = operation_project(&host, "done");
        submit_operation(&host, "done", &done);
        settled_operation(&host, "done");
        let held = crate::core::repo_lock::RepoLock::try_exclusive("shutdown fixture")
            .unwrap()
            .unwrap();
        let next = operation_project(&host, "next");
        submit_operation(&host, "next", &next);
        let result = dispatch(
            &host.ctx,
            "operation_request",
            &json!({"request":{"action":"shutdown_cancel"}}),
        )
        .unwrap();
        let snapshots = result["snapshots"].as_array().unwrap();
        assert_eq!(snapshots[0]["items"][0]["phase"], "succeeded");
        assert_eq!(snapshots[1]["items"][0]["phase"], "canceled");
        assert_eq!(
            std::fs::read_to_string(host.tmp.path().join("done/.claude/skills/x/SKILL.md"))
                .unwrap(),
            "library version"
        );
        assert_eq!(
            std::fs::read_to_string(host.tmp.path().join("next/.claude/skills/x/SKILL.md"))
                .unwrap(),
            "old copy"
        );
        drop(held);
        settled_operation(&host, "next");
    }

    #[test]
    fn shutdown_seal_waits_for_real_work_and_blocks_later_admission_until_resumed() {
        let (entered, receiver) = std::sync::mpsc::channel();
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let host = test_host_with(Arc::new(HeldOperation {
            target_path: None,
            entered_once: std::sync::atomic::AtomicBool::new(false),
            entered,
            release: release.clone(),
        }));
        let release = ReleaseOnDrop(release);
        let project = operation_project(&host, "shutdown");
        submit_operation(&host, "held", &project);
        receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        let shutdown = |action| {
            dispatch(
                &host.ctx,
                "operation_request",
                &json!({"request":{"action":action}}),
            )
            .unwrap()
        };
        assert_eq!(shutdown("shutdown_seal")["ready"], false);
        // Automatic quit must include work admitted after the first idle check.
        let second = operation_project(&host, "later");
        submit_operation(&host, "later", &second);
        drop(release);
        assert_eq!(
            settled_operation(&host, "held")["items"][0]["phase"],
            "succeeded"
        );
        settled_operation(&host, "later");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while shutdown("shutdown_seal")["ready"] != true {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(dispatch(&host.ctx, "operation_request", &json!({"request":{"action":"submit_project_update","operation_id":"after-seal","project_id":project["id"]}})).is_err());
        shutdown("shutdown_resume");
        assert!(dispatch(&host.ctx, "operation_request", &json!({"request":{"action":"submit_project_update","operation_id":"after-resume","project_id":project["id"]}})).is_ok());
        settled_operation(&host, "after-resume");
    }

    #[test]
    fn project_operation_captures_scope_and_reports_real_updates() {
        let host = test_host();
        let project = host.tmp.path().join("project");
        let target = project.join(".claude/skills/x");
        let source = crate::core::central_repo::skills_dir().join("x");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(target.join("SKILL.md"), "old copy").unwrap();
        std::fs::write(source.join("SKILL.md"), "library version").unwrap();
        std::fs::File::options()
            .write(true)
            .open(source.join("SKILL.md"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3))
            .unwrap();
        host.ctx
            .store
            .insert_skill(&crate::core::test_support::sample_skill("x", "x", &source))
            .unwrap();
        let project = dispatch(&host.ctx, "add_project", &json!({"path": project})).unwrap();
        let request = json!({"request": {"action":"submit_project_update", "operation_id":"update-1", "host_id":null, "project_id":project["id"]}});
        let submitted = dispatch(&host.ctx, "operation_request", &request).unwrap();
        assert_eq!(submitted["project_id"], project["id"]);
        assert_eq!(submitted["items"].as_array().unwrap().len(), 1);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let completed = loop {
            let status = dispatch(
                &host.ctx,
                "operation_request",
                &json!({"request":{"action":"read", "operation_id":"update-1"}}),
            )
            .unwrap();
            if status["settled"] == true {
                break status;
            }
            assert!(std::time::Instant::now() < deadline, "{status}");
            std::thread::yield_now();
        };
        assert_eq!(completed["items"][0]["phase"], "succeeded");
        assert_eq!(
            std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "library version"
        );
        let repeated = dispatch(&host.ctx, "operation_request", &request).unwrap();
        assert_eq!(repeated["attempt_id"], submitted["attempt_id"]);
        let receipt = dispatch(
            &host.ctx,
            "operation_request",
            &json!({"request":{"action":"reconcile", "operation_id":"update-1"}}),
        )
        .unwrap();
        assert_eq!(receipt["items"][0]["phase"], "succeeded");
    }

    #[test]
    fn retained_results_reopen_locally_without_resuming_work_or_success_history() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("results.db");
        let ctx = HostCtx::headless(Arc::new(SkillStore::new(&db).unwrap()));
        let result = |id: &str, phase: &str| {
            json!({
                "operation_id":id,"attempt_id":"first","host_id":"offline",
                "work":"installation","project_id":null,"project_path":null,
                "destination":{"kind":"library","path":"/original/library"},
                "origin":{"host_name":"Build","ssh_target":"dev@original","host_created_at":1},
                "sources":[{"kind":"git","url":"https://user:SECRET@fixture.invalid/repo?token=SECRET#SECRET"}],
                "preview":[{"rel_path":"unselected","name":"unselected","description":"SECRET"}],
                "revision":3,"settled":true,"cancel_requested":false,
                "items":[{"id":"one","attempt_id":"first","relative_path":"one","agent":null,"phase":phase,"message":"raw SECRET exception"},
                    {"id":"done","attempt_id":"first","relative_path":"done","agent":null,"phase":"succeeded","message":null}]
            })
        };
        let saved = dispatch(&ctx,"operation_results_request", &json!({"request":{"action":"write","version":0,"results":[result("partial","failed"),result("success","succeeded")]}})).unwrap();
        assert_eq!(saved["version"], 1);
        drop(ctx);
        let ctx = HostCtx::headless(Arc::new(SkillStore::new(&db).unwrap()));
        let restored = dispatch(
            &ctx,
            "operation_results_request",
            &json!({"request":{"action":"read"}}),
        )
        .unwrap();
        assert_eq!(restored["results"].as_array().unwrap().len(), 1);
        assert_eq!(
            restored["results"][0]["origin"]["ssh_target"],
            "dev@original"
        );
        assert_eq!(restored["results"][0]["items"][1]["phase"], "succeeded");
        assert!(restored["results"][0]["preview"].is_null());
        assert!(!restored.to_string().contains("SECRET"));
        assert_eq!(
            dispatch(
                &ctx,
                "operation_request",
                &json!({"request":{"action":"list"}})
            )
            .unwrap(),
            json!([])
        );
    }

    #[test]
    fn retained_results_reject_stale_writes_and_keep_lost_admission_correlation() {
        let host = test_host();
        let result = json!({"operation_id":"lost","attempt_id":"old","recovery":"checking","recovery_attempt":"confirm-new",
            "host_id":"removed","project_id":"same-id","project_path":"/original/project",
            "destination":{"kind":"project","id":"same-id","path":"/original/project"},
            "revision":4,"settled":false,"cancel_requested":false,"items":[]});
        let write = |version: u64, results: Value| json!({"request":{"action":"write","version":version,"results":results}});
        let saved = dispatch(
            &host.ctx,
            "operation_results_request",
            &write(0, json!([result])),
        )
        .unwrap();
        assert_eq!(saved["results"][0]["recovery"], "unknown");
        assert_eq!(saved["results"][0]["recovery_attempt"], "confirm-new");
        assert_eq!(saved["results"][0]["settled"], true);
        dispatch(&host.ctx, "operation_results_request", &write(1, json!([]))).unwrap();
        assert!(dispatch(
            &host.ctx,
            "operation_results_request",
            &write(1, saved["results"].clone())
        )
        .is_err());
        assert_eq!(
            dispatch(
                &host.ctx,
                "operation_results_request",
                &json!({"request":{"action":"read"}})
            )
            .unwrap()["results"],
            json!([])
        );
    }

    #[test]
    fn retained_pending_confirmation_preserves_expected_attempt_as_unknown_not_review_authority() {
        let host = test_host();
        let result = json!({"operation_id":"pending-confirm","attempt_id":"preview","host_id":null,
            "project_id":null,"project_path":null,"work":"installation",
            "admission":{"action":"confirm_install","expected_attempt":"confirmation"},
            "revision":2,"settled":true,"cancel_requested":false,
            "items":[{"id":"one","attempt_id":"preview","relative_path":"one","agent":null,"phase":"needs_review","message":null}]});
        let saved = dispatch(
            &host.ctx,
            "operation_results_request",
            &json!({"request":{"action":"write","version":0,"results":[result]}}),
        )
        .unwrap();
        assert_eq!(saved["results"][0]["recovery"], "unknown");
        assert_eq!(saved["results"][0]["recovery_attempt"], "confirmation");
        assert_eq!(saved["results"][0]["items"][0]["phase"], "outcome_unknown");
        assert!(saved["results"][0]["admission"].is_null());
    }

    #[test]
    fn result_storage_failure_is_sanitized_and_retry_saves_the_same_result() {
        let host = test_host();
        let database = rusqlite::Connection::open(host.tmp.path().join("repo/test.db")).unwrap();
        database.execute_batch("CREATE TRIGGER fail_results BEFORE INSERT ON resource_state WHEN NEW.kind = 'operation_results' BEGIN SELECT RAISE(FAIL, 'SECRET exception'); END;").unwrap();
        let write = json!({"request":{"action":"write","version":0,"results":[{
            "operation_id":"keep-memory","attempt_id":"first","host_id":null,"project_id":null,"project_path":null,
            "revision":3,"settled":true,"cancel_requested":false,
            "items":[{"id":"one","attempt_id":"first","relative_path":"one","agent":null,"phase":"failed","message":"SECRET stderr"}]
        }]}});
        let error = dispatch(&host.ctx, "operation_results_request", &write).unwrap_err();
        assert!(!error.message.contains("SECRET"));
        let empty = dispatch(
            &host.ctx,
            "operation_results_request",
            &json!({"request":{"action":"read"}}),
        )
        .unwrap();
        assert_eq!(empty["version"], 0);
        database.execute_batch("DROP TRIGGER fail_results").unwrap();
        let saved = dispatch(&host.ctx, "operation_results_request", &write).unwrap();
        assert_eq!(saved["results"][0]["operation_id"], "keep-memory");
        assert_eq!(saved["results"][0]["items"][0]["phase"], "failed");
        assert!(!saved.to_string().contains("SECRET"));
    }

    #[test]
    fn library_operation_installs_local_without_a_project_or_agent() {
        let host = test_host();
        let source = host.tmp.path().join("source");
        crate::core::test_support::write_skill(&source, "writing");
        let request = json!({"request":{"action":"submit_install","operation_id":"local-install","host_id":null,"sources":[{"kind":"local","path":source,"name":"writing"}]}});
        let submitted = dispatch(&host.ctx, "operation_request", &request).unwrap();
        assert_eq!(submitted["destination"]["kind"], "library");
        assert!(submitted["project_id"].is_null());
        let result = settled_operation(&host, "local-install");
        assert_eq!(result["items"][0]["phase"], "succeeded", "{result}");
        assert!(crate::core::central_repo::skills_dir()
            .join("writing/SKILL.md")
            .exists());
        let skills = dispatch(&host.ctx, "get_managed_skills", &json!({})).unwrap();
        assert_eq!(skills.as_array().unwrap().len(), 1);
        assert_eq!(
            dispatch(&host.ctx, "operation_request", &request).unwrap()["attempt_id"],
            submitted["attempt_id"]
        );
    }

    #[test]
    fn reconcile_verifies_install_content_and_ownership_without_replaying() {
        let host = test_host();
        let source = host.tmp.path().join("source");
        crate::core::test_support::write_skill(&source, "writing");
        let held = crate::core::repo_lock::RepoLock::try_shared()
            .unwrap()
            .unwrap();
        dispatch(&host.ctx, "operation_request", &json!({"request":{"action":"submit_install","operation_id":"lost-install","host_id":null,"sources":[{"kind":"local","path":source,"name":"writing"}]}})).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while read_operation(&host, "lost-install")["items"][0]["phase"] != "waiting" {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let intent = host
            .ctx
            .store
            .resource_state_get("operation_receipt", "lost-install")
            .unwrap()
            .unwrap();
        drop(held);
        settled_operation(&host, "lost-install");
        host.ctx
            .store
            .resource_state_put("operation_receipt", "lost-install", &intent)
            .unwrap();
        let reader = HostCtx::headless(host.ctx.store.clone());
        let reconcile = || {
            dispatch(
                &reader,
                "operation_request",
                &json!({"request":{"action":"reconcile","operation_id":"lost-install"}}),
            )
            .unwrap()
        };
        assert_eq!(reconcile()["items"][0]["phase"], "unchanged");
        let original_source = std::fs::read(source.join("SKILL.md")).unwrap();
        std::fs::write(source.join("SKILL.md"), "source changed").unwrap();
        assert_eq!(reconcile()["items"][0]["phase"], "outcome_unknown");
        std::fs::write(source.join("SKILL.md"), original_source).unwrap();
        let target = crate::core::central_repo::skills_dir().join("writing/SKILL.md");
        std::fs::write(&target, "edited copy").unwrap();
        assert_eq!(reconcile()["items"][0]["phase"], "outcome_unknown");
        assert_eq!(std::fs::read_to_string(target).unwrap(), "edited copy");
        assert_eq!(
            dispatch(&reader, "get_managed_skills", &json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn library_wait_yields_to_project_and_changed_local_source_requires_review() {
        let host = test_host();
        let project = operation_project(&host, "independent");
        let source = host.tmp.path().join("source");
        crate::core::test_support::write_skill(&source, "writing");
        let held = crate::core::repo_lock::RepoLock::try_shared()
            .unwrap()
            .unwrap();
        dispatch(&host.ctx, "operation_request", &json!({"request":{"action":"submit_install","operation_id":"waiting-install","host_id":null,"sources":[{"kind":"local","path":source,"name":"writing"}]}})).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while read_operation(&host, "waiting-install")["items"][0]["phase"] != "waiting" {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        submit_operation(&host, "independent", &project);
        assert_eq!(
            settled_operation(&host, "independent")["items"][0]["phase"],
            "succeeded"
        );
        std::fs::write(source.join("SKILL.md"), "changed after admission").unwrap();
        drop(held);
        let result = settled_operation(&host, "waiting-install");
        assert_eq!(result["items"][0]["phase"], "needs_review", "{result}");
        assert!(!crate::core::central_repo::skills_dir()
            .join("writing")
            .exists());
    }

    #[test]
    fn git_operation_previews_without_writing_and_installs_only_confirmed_selection() {
        git_operation_review(false, false);
    }

    #[test]
    fn restored_git_review_recreates_missing_preview_and_installs_only_fresh_selection() {
        git_operation_review(true, false);
    }

    #[test]
    fn restored_selected_git_failure_never_adds_unselected_preview_entries() {
        git_operation_review(true, true);
    }

    fn git_operation_review(restored: bool, failed_selection: bool) {
        let mut host = test_host();
        let repo = host.tmp.path().join("git-source");
        crate::core::test_support::write_skill(&repo.join("skills/one"), "one");
        crate::core::test_support::write_skill(&repo.join("skills/two"), "two");
        for args in [
            vec!["init"],
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-m",
                "fixture",
            ],
        ] {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
        }
        let config = host.tmp.path().join("gitconfig");
        std::fs::write(
            &config,
            format!(
                "[url \"{}\"]\n    insteadOf = https://fixture.invalid/skills.git\n",
                tauri::Url::from_directory_path(&repo).unwrap()
            ),
        )
        .unwrap();
        let _config = GitConfigGuard::new(&config);
        let request = json!({"request":{"action":"submit_install","operation_id":"git-preview","host_id":null,"sources":[{"kind":"git","url":"https://fixture-user:fixture-secret@fixture.invalid/skills.git?token=fixture-secret#fragment"}]}});
        dispatch(&host.ctx, "operation_request", &request).unwrap();
        let preview = settled_operation(&host, "git-preview");
        assert_eq!(
            preview["items"][0]["source"]["url"],
            "https://fixture.invalid/skills.git"
        );
        let reader = HostCtx::headless(host.ctx.store.clone());
        let receipt = dispatch(
            &reader,
            "operation_request",
            &json!({"request":{"action":"reconcile","operation_id":"git-preview"}}),
        )
        .unwrap();
        assert!(!receipt.to_string().contains("fixture-secret"));
        assert!(dispatch(&reader, "operation_request", &json!({"request":{"action":"confirm_install","operation_id":"git-preview","attempt_id":"stale","items":[{"rel_path":"one","name":"one"}]}})).is_err());
        assert_eq!(preview["items"][0]["phase"], "needs_review", "{preview}");
        assert_eq!(preview["preview"].as_array().unwrap().len(), 2);
        assert!(!crate::core::central_repo::skills_dir().join("one").exists());
        if restored && !failed_selection {
            drop(reader);
            host.ctx = HostCtx::headless(Arc::new(
                SkillStore::new(&host.tmp.path().join("repo/test.db")).unwrap(),
            ));
            dispatch(&host.ctx, "operation_request", &json!({"request":{"action":"prepare_review","operation_id":"git-preview","attempt_id":"recreated"}})).unwrap();
            let recreated = settled_operation(&host, "git-preview");
            assert_eq!(recreated["preview"].as_array().unwrap().len(), 2);
            assert!(!crate::core::central_repo::skills_dir()
                .join("chosen")
                .exists());
        }
        let mut selection = json!([{"rel_path":"one","name":"chosen"}]);
        if restored && !failed_selection {
            selection
                .as_array_mut()
                .unwrap()
                .push(json!({"rel_path":"two","name":"second"}));
        }
        let database = rusqlite::Connection::open(host.tmp.path().join("repo/test.db")).unwrap();
        if failed_selection {
            database.execute_batch("CREATE TRIGGER fail_selected BEFORE INSERT ON skills BEGIN SELECT RAISE(FAIL, 'fixture failure'); END;").unwrap();
        }
        dispatch(&host.ctx, "operation_request", &json!({"request":{"action":"confirm_install","operation_id":"git-preview","attempt_id":"selected","items":selection}})).unwrap();
        let installed = settled_operation(&host, "git-preview");
        if failed_selection {
            assert_eq!(installed["items"][0]["phase"], "failed", "{installed}");
            database
                .execute_batch("DROP TRIGGER fail_selected")
                .unwrap();
            host.ctx = HostCtx::headless(Arc::new(
                SkillStore::new(&host.tmp.path().join("repo/test.db")).unwrap(),
            ));
            dispatch(&host.ctx, "operation_request", &json!({"request":{"action":"prepare_review","operation_id":"git-preview","attempt_id":"selected-review"}})).unwrap();
            let review = settled_operation(&host, "git-preview");
            assert_eq!(review["preview"].as_array().unwrap().len(), 1, "{review}");
            assert_eq!(review["preview"][0]["rel_path"], "one");
            assert_eq!(review["preview"][0]["name"], "chosen");
            assert!(!crate::core::central_repo::skills_dir().join("two").exists());
            return;
        }
        assert_eq!(
            installed["items"].as_array().unwrap().len(),
            if restored { 2 } else { 1 }
        );
        assert_eq!(installed["items"][0]["phase"], "succeeded", "{installed}");
        assert!(crate::core::central_repo::skills_dir()
            .join("chosen/SKILL.md")
            .exists());
        assert!(!crate::core::central_repo::skills_dir().join("two").exists());
    }

    #[test]
    fn marketplace_operation_uses_local_git_and_keeps_edited_library_skill() {
        let mut host = test_host();
        let repo = host.tmp.path().join("git-source");
        crate::core::test_support::write_skill(&repo.join("skills/one"), "one");
        for args in [
            vec!["init"],
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-m",
                "fixture",
            ],
        ] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap()
                .status
                .success());
        }
        let config = host.tmp.path().join("gitconfig");
        std::fs::write(
            &config,
            format!(
                "[url \"{}\"]\n    insteadOf = https://github.com/fixture/skills.git\n",
                tauri::Url::from_directory_path(&repo).unwrap()
            ),
        )
        .unwrap();
        let _config = GitConfigGuard::new(&config);
        let request = |id: &str| json!({"request":{"action":"submit_install","operation_id":id,"host_id":null,"sources":[{"kind":"marketplace","source":"fixture/skills","skill_id":"one"}]}});
        dispatch(&host.ctx, "operation_request", &request("market-one")).unwrap();
        assert_eq!(
            settled_operation(&host, "market-one")["items"][0]["phase"],
            "succeeded"
        );
        let installed = crate::core::central_repo::skills_dir().join("one/SKILL.md");
        std::fs::write(&installed, "my edit").unwrap();
        dispatch(&host.ctx, "operation_request", &request("market-again")).unwrap();
        let result = settled_operation(&host, "market-again");
        assert_eq!(result["items"][0]["phase"], "needs_review", "{result}");
        assert_eq!(std::fs::read_to_string(&installed).unwrap(), "my edit");
        dispatch(
            &host.ctx,
            "operation_results_request",
            &json!({"request":{"action":"write","version":0,"results":[result]}}),
        )
        .unwrap();
        host.ctx = HostCtx::headless(Arc::new(
            SkillStore::new(&host.tmp.path().join("repo/test.db")).unwrap(),
        ));
        let retained = dispatch(
            &host.ctx,
            "operation_results_request",
            &json!({"request":{"action":"read"}}),
        )
        .unwrap();
        assert_eq!(retained["results"][0]["sources"][0]["kind"], "marketplace");
        dispatch(&host.ctx,"operation_request", &json!({"request":{"action":"prepare_review","operation_id":"market-again","attempt_id":"fresh-market"}})).unwrap();
        let review = settled_operation(&host, "market-again");
        assert_eq!(review["items"][0]["phase"], "needs_review");
        assert!(review["preview"].is_null());
        assert_eq!(std::fs::read_to_string(installed).unwrap(), "my edit");
    }

    #[test]
    fn cache_waits_release_execution_slots_and_cancel_without_library_writes() {
        use fs2::FileExt;
        let events = Arc::new(RecordingEvents::default());
        let host = test_host_with(events.clone());
        let repo = host.tmp.path().join("git-source");
        crate::core::test_support::write_skill(&repo.join("skills/one"), "one");
        for args in [
            vec!["init"],
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-m",
                "fixture",
            ],
        ] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap()
                .status
                .success());
        }
        let config = host.tmp.path().join("gitconfig");
        std::fs::write(
            &config,
            format!(
                "[url \"{}\"]\n    insteadOf = https://github.com/fixture/skills.git\n",
                tauri::Url::from_directory_path(&repo).unwrap()
            ),
        )
        .unwrap();
        let _config = GitConfigGuard::new(&config);
        let request = |id: &str, skill: &str| json!({"request":{"action":"submit_install","operation_id":id,"host_id":null,"sources":[{"kind":"marketplace","source":"fixture/skills","skill_id":skill}]}});
        dispatch(&host.ctx, "operation_request", &request("prime", "one")).unwrap();
        assert_eq!(
            settled_operation(&host, "prime")["items"][0]["phase"],
            "succeeded"
        );
        let lock_path = std::fs::read_dir(crate::core::central_repo::cache_dir().join("repos"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "lock"))
            .unwrap();
        let cache = std::fs::File::options()
            .read(true)
            .write(true)
            .open(lock_path)
            .unwrap();
        cache.lock_exclusive().unwrap();
        for (id, skill) in [("cache-one", "two"), ("cache-two", "three")] {
            dispatch(&host.ctx, "operation_request", &request(id, skill)).unwrap();
        }
        let waiting_observed = || {
            let events = events.0.lock().unwrap();
            ["cache-one", "cache-two"].iter().all(|id| {
                events.iter().any(|(event, snapshot)| {
                    event == "operation-changed"
                        && snapshot["operation_id"] == *id
                        && snapshot["items"][0]["phase"] == "waiting"
                })
            })
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !waiting_observed() && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        let waited = waiting_observed();
        let project = operation_project(&host, "unrelated-cache");
        submit_operation(&host, "unrelated-cache", &project);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while read_operation(&host, "unrelated-cache")["settled"] != true
            && std::time::Instant::now() < deadline
        {
            std::thread::yield_now();
        }
        let progressed =
            read_operation(&host, "unrelated-cache")["items"][0]["phase"] == "succeeded";
        for id in ["cache-one", "cache-two"] {
            dispatch(
                &host.ctx,
                "operation_request",
                &json!({"request":{"action":"cancel","operation_id":id}}),
            )
            .unwrap();
        }
        drop(cache);
        settled_operation(&host, "cache-one");
        settled_operation(&host, "cache-two");
        settled_operation(&host, "unrelated-cache");
        assert!(progressed, "a cache wait consumed both execution slots");
        assert!(waited, "both cache-blocked operations must report waiting");
        assert!(!crate::core::central_repo::skills_dir().join("two").exists());
    }

    #[test]
    fn archive_batch_continues_and_retries_only_failed_items_with_original_evidence() {
        archive_batch_retry_keeps_evidence(false);
    }

    #[test]
    fn changed_failed_install_source_returns_to_review_without_repeating_successes() {
        archive_batch_retry_keeps_evidence(true);
    }

    fn archive_batch_retry_keeps_evidence(changed: bool) {
        use std::io::Write;
        let host = test_host();
        let archive = host.tmp.path().join("writing.skill");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        zip.start_file("writing/SKILL.md", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"---\nname: writing\n---\narchive content")
            .unwrap();
        zip.finish().unwrap();
        let source = host.tmp.path().join("disappearing");
        crate::core::test_support::write_skill(&source, "research");
        let original = std::fs::read(source.join("SKILL.md")).unwrap();
        let held = crate::core::repo_lock::RepoLock::try_shared()
            .unwrap()
            .unwrap();
        dispatch(&host.ctx, "operation_request", &json!({"request":{"action":"submit_install","operation_id":"archive-batch","host_id":null,"sources":[{"kind":"local","path":source,"name":"research"},{"kind":"local","path":archive}]}})).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let status = read_operation(&host, "archive-batch");
            if status["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["phase"] == "waiting")
            {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "{status}");
            std::thread::yield_now();
        }
        std::fs::remove_dir_all(&source).unwrap();
        drop(held);
        let partial = settled_operation(&host, "archive-batch");
        assert_eq!(partial["items"][0]["phase"], "failed", "{partial}");
        assert_eq!(partial["items"][1]["phase"], "succeeded", "{partial}");
        assert!(crate::core::central_repo::skills_dir()
            .join("writing/SKILL.md")
            .exists());
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("SKILL.md"),
            if changed {
                b"changed since the failed attempt".to_vec()
            } else {
                original
            },
        )
        .unwrap();
        dispatch(&host.ctx, "operation_request", &json!({"request":{"action":"retry_failed","operation_id":"archive-batch","attempt_id":"retry-local"}})).unwrap();
        let retried = settled_operation(&host, "archive-batch");
        assert_eq!(
            retried["items"][0]["phase"],
            if changed { "needs_review" } else { "succeeded" },
            "{retried}"
        );
        assert_eq!(retried["items"][1], partial["items"][1]);
        assert_eq!(
            dispatch(&host.ctx, "get_managed_skills", &json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            if changed { 1 } else { 2 }
        );
    }

    #[test]
    fn restored_install_recreates_review_without_writing_or_repeating_successful_siblings() {
        restored_install_review(false);
    }

    #[test]
    fn restored_install_confirmation_rechecks_target_after_fresh_review() {
        restored_install_review(true);
    }

    fn restored_install_review(changed_target: bool) {
        let mut host = test_host();
        let good = host.tmp.path().join("good");
        let missing = host.tmp.path().join("missing");
        crate::core::test_support::write_skill(&good, "good");
        dispatch(&host.ctx,"operation_request", &json!({"request":{"action":"submit_install","operation_id":"restore-install","host_id":null,"sources":[{"kind":"local","path":good},{"kind":"local","path":missing}]}})).unwrap();
        let partial = settled_operation(&host, "restore-install");
        assert_eq!(partial["items"][0]["phase"], "succeeded");
        assert_eq!(partial["items"][1]["phase"], "failed");
        host.ctx = HostCtx::headless(Arc::new(
            SkillStore::new(&host.tmp.path().join("repo/test.db")).unwrap(),
        ));
        crate::core::test_support::write_skill(&missing, "recreated");
        dispatch(&host.ctx,"operation_request", &json!({"request":{"action":"prepare_review","operation_id":"restore-install","attempt_id":"fresh-review"}})).unwrap();
        let review = settled_operation(&host, "restore-install");
        assert_eq!(review["items"][0], partial["items"][0]);
        assert_eq!(review["items"][1]["phase"], "needs_review");
        assert!(!crate::core::central_repo::skills_dir()
            .join("recreated")
            .exists());
        assert_eq!(review["preview"].as_array().unwrap().len(), 1);
        let selection = json!([{"rel_path":review["preview"][0]["rel_path"],"name":"recreated"}]);
        let target = crate::core::central_repo::skills_dir().join("recreated");
        if changed_target {
            crate::core::test_support::write_skill(&target, "someone-else");
            std::fs::write(target.join("SKILL.md"), "my edit after preview").unwrap();
        }
        dispatch(&host.ctx,"operation_request", &json!({"request":{"action":"confirm_install","operation_id":"restore-install","attempt_id":"confirmed","items":selection}})).unwrap();
        let done = settled_operation(&host, "restore-install");
        assert_eq!(done["items"][0], partial["items"][0]);
        if changed_target {
            assert_eq!(done["items"][1]["phase"], "needs_review", "{done}");
            assert_eq!(
                std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
                "my edit after preview"
            );
            assert!(!crate::core::central_repo::skills_dir()
                .join("recreated-2")
                .exists());
            return;
        }
        assert_eq!(done["items"][1]["phase"], "succeeded");
        assert!(crate::core::central_repo::skills_dir()
            .join("recreated/SKILL.md")
            .exists());
    }

    #[test]
    fn restored_local_variants_reopen_real_stores_and_require_fresh_review() {
        use std::io::Write;
        for kind in ["folder", "archive", "batch", "discovered"] {
            let mut host = test_host();
            let path = host.tmp.path().join("source");
            let source = match kind {
                "archive" => {
                    let archive = host.tmp.path().join("writing.skill");
                    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
                    zip.start_file("writing/SKILL.md", zip::write::SimpleFileOptions::default())
                        .unwrap();
                    zip.write_all(b"---\nname: writing\n---\narchive content")
                        .unwrap();
                    zip.finish().unwrap();
                    json!({"kind":"local","path":archive})
                }
                "batch" => {
                    crate::core::test_support::write_skill(&path.join("one"), "one");
                    crate::core::test_support::write_skill(&path.join("two"), "two");
                    json!({"kind":"local_batch","path":path})
                }
                _ => {
                    crate::core::test_support::write_skill(&path, "writing");
                    json!({"kind":"local","path":path,"origin":if kind == "discovered" {json!("discovered")} else {Value::Null}})
                }
            };
            let database =
                rusqlite::Connection::open(host.tmp.path().join("repo/test.db")).unwrap();
            database.execute_batch("CREATE TRIGGER fail_install BEFORE INSERT ON skills BEGIN SELECT RAISE(FAIL, 'fixture failure'); END;").unwrap();
            dispatch(&host.ctx,"operation_request", &json!({"request":{"action":"submit_install","operation_id":"variant","host_id":null,"sources":[source]}})).unwrap();
            let failed = settled_operation(&host, "variant");
            assert!(
                failed["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|i| i["phase"] == "failed"),
                "{kind}: {failed}"
            );
            dispatch(
                &host.ctx,
                "operation_results_request",
                &json!({"request":{"action":"write","version":0,"results":[failed]}}),
            )
            .unwrap();
            host.ctx = HostCtx::headless(Arc::new(
                SkillStore::new(&host.tmp.path().join("repo/test.db")).unwrap(),
            ));
            let retained = dispatch(
                &host.ctx,
                "operation_results_request",
                &json!({"request":{"action":"read"}}),
            )
            .unwrap();
            assert_eq!(
                retained["results"][0]["sources"][0]["kind"], source["kind"],
                "{kind}"
            );
            assert_eq!(
                retained["results"][0]["sources"][0]["path"], source["path"],
                "{kind}"
            );
            assert_eq!(
                retained["results"][0]["sources"][0]["origin"], source["origin"],
                "{kind}"
            );
            assert_eq!(
                dispatch(
                    &host.ctx,
                    "operation_request",
                    &json!({"request":{"action":"list"}})
                )
                .unwrap(),
                json!([])
            );
            assert!(dispatch(&host.ctx,"operation_request", &json!({"request":{"action":"retry_failed","operation_id":"variant","attempt_id":"stale-shortcut"}})).is_err());
            database.execute_batch("DROP TRIGGER fail_install").unwrap();
            dispatch(&host.ctx,"operation_request", &json!({"request":{"action":"prepare_review","operation_id":"variant","attempt_id":"fresh-review","item_ids":[failed["items"][0]["id"]]}})).unwrap();
            let review = settled_operation(&host, "variant");
            assert!(
                review["items"][0]["phase"] == "needs_review",
                "{kind}: {review}"
            );
            assert_eq!(
                dispatch(&host.ctx, "get_managed_skills", &json!({})).unwrap(),
                json!([])
            );
            let choice = &review["preview"][0];
            dispatch(&host.ctx,"operation_request", &json!({"request":{"action":"confirm_install","operation_id":"variant","attempt_id":"confirmed","items":[{"rel_path":choice["rel_path"],"name":choice["name"]}]}})).unwrap();
            let done = settled_operation(&host, "variant");
            assert_eq!(done["items"][0]["phase"], "succeeded", "{kind}: {done}");
            assert_eq!(
                dispatch(&host.ctx, "get_managed_skills", &json!({}))
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            if kind == "batch" {
                assert_eq!(done["items"][1]["phase"], "failed");
                assert_eq!(
                    done["items"][1]["attempt_id"],
                    failed["items"][1]["attempt_id"]
                );
            }
        }
    }

    #[test]
    fn queued_install_does_not_follow_a_changed_library_path() {
        let host = test_host();
        let original = crate::core::central_repo::base_dir();
        let source = host.tmp.path().join("source");
        crate::core::test_support::write_skill(&source, "writing");
        let held = crate::core::repo_lock::RepoLock::try_shared()
            .unwrap()
            .unwrap();
        dispatch(&host.ctx, "operation_request", &json!({"request":{"action":"submit_install","operation_id":"original-library","host_id":null,"sources":[{"kind":"local","path":source,"name":"writing"}]}})).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while read_operation(&host, "original-library")["items"][0]["phase"] != "waiting" {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let changed = host.tmp.path().join("changed-library");
        crate::core::central_repo::set_test_base_dir_override(Some(changed.clone()));
        std::fs::create_dir_all(crate::core::central_repo::skills_dir()).unwrap();
        drop(held);
        let result = settled_operation(&host, "original-library");
        crate::core::central_repo::set_test_base_dir_override(Some(original.clone()));
        assert_eq!(result["items"][0]["phase"], "needs_review", "{result}");
        assert!(!original.join("skills/writing").exists());
        assert!(!changed.join("skills/writing").exists());
    }

    #[test]
    fn empty_local_batches_keep_their_own_context_while_unrelated_work_waits() {
        for project_work in [true, false] {
            let host = test_host();
            let project = operation_project(&host, "blocked-project");
            let batch = host.tmp.path().join("batch");
            crate::core::test_support::write_skill(&batch.join("one"), "one");
            let submit_batch = |id: &str, path: &std::path::Path| {
                dispatch(
                    &host.ctx,
                    "operation_request",
                    &json!({"request":{
                        "action":"submit_install", "operation_id":id, "host_id":"original-host",
                        "sources":[{"kind":"local_batch", "path":path}]
                    }}),
                )
                .unwrap()
            };
            submit_batch("initial-batch", &batch);
            settled_operation(&host, "initial-batch");
            let skills = dispatch(&host.ctx, "get_managed_skills", &json!({})).unwrap();
            let installed = crate::core::central_repo::skills_dir().join("one/SKILL.md");
            let before = std::fs::metadata(&installed).unwrap().modified().unwrap();
            let source = host.tmp.path().join("unrelated");
            crate::core::test_support::write_skill(&source, "unrelated");
            let guard = crate::core::repo_lock::RepoLock::acquire("external writer").unwrap();
            if project_work {
                submit_operation(&host, "held", &project);
            } else {
                dispatch(
                    &host.ctx,
                    "operation_request",
                    &json!({"request":{
                        "action":"submit_install", "operation_id":"held", "host_id":null,
                        "sources":[{"kind":"local", "path":source}]
                    }}),
                )
                .unwrap();
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while read_operation(&host, "held")["items"][0]["phase"] != "waiting" {
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            let held = read_operation(&host, "held");
            let empty = host.tmp.path().join("empty");
            std::fs::create_dir_all(empty.join("not-a-skill")).unwrap();
            for (id, path) in [("all-installed", &batch), ("empty-batch", &empty)] {
                let snapshot = submit_batch(id, path);
                assert_eq!(snapshot["operation_id"], id, "{snapshot}");
                assert_eq!(snapshot["host_id"], "original-host");
                assert_eq!(snapshot["work"], "installation");
                assert_eq!(
                    snapshot["destination"],
                    json!({"kind":"library", "path":
                    crate::core::central_repo::skills_dir().canonicalize().unwrap()})
                );
                assert!(snapshot["project_id"].is_null());
                assert!(snapshot["project_path"].is_null());
                assert_eq!(
                    snapshot["sources"],
                    json!([{"kind":"local_batch","path":path}])
                );
                assert_eq!(snapshot["items"], json!([]));
                assert_eq!(snapshot["settled"], true);
                assert_eq!(read_operation(&host, id), snapshot);
                assert_eq!(submit_batch(id, path), snapshot);
                dispatch(
                    &host.ctx,
                    "operation_request",
                    &json!({"request":{
                        "action":"cancel", "operation_id":id
                    }}),
                )
                .unwrap();
                assert_eq!(read_operation(&host, "held"), held);
            }
            assert_eq!(
                std::fs::metadata(&installed).unwrap().modified().unwrap(),
                before
            );
            assert!(!crate::core::central_repo::skills_dir()
                .join("unrelated")
                .exists());
            assert_eq!(
                std::fs::read_to_string(
                    host.tmp
                        .path()
                        .join("blocked-project/.claude/skills/x/SKILL.md")
                )
                .unwrap(),
                "old copy"
            );
            drop(guard);
            assert_eq!(
                settled_operation(&host, "held")["items"][0]["phase"],
                "succeeded"
            );
            if project_work {
                assert_eq!(
                    dispatch(&host.ctx, "get_managed_skills", &json!({})).unwrap(),
                    skills
                );
            }
        }
    }

    #[test]
    fn local_batch_uses_installation_work_and_never_claims_an_agent_destination() {
        let host = test_host();
        let source = host.tmp.path().join("team");
        crate::core::test_support::write_skill(&source.join("one"), "one");
        crate::core::test_support::write_skill(&source.join("two"), "two");
        std::fs::create_dir_all(source.join("not-a-skill")).unwrap();
        dispatch(&host.ctx, "operation_request", &json!({"request":{"action":"submit_install","operation_id":"local-batch","host_id":null,"sources":[{"kind":"local_batch","path":source}]}})).unwrap();
        let result = settled_operation(&host, "local-batch");
        assert_eq!(result["work"], "installation");
        assert_eq!(result["items"].as_array().unwrap().len(), 2);
        for item in result["items"].as_array().unwrap() {
            assert!(item["agent"].is_null());
            assert_eq!(item["phase"], "succeeded", "{result}");
        }
        assert_eq!(
            dispatch(&host.ctx, "get_managed_skills", &json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[cfg(unix)]
    #[test]
    fn changed_local_source_alias_requires_fresh_review_even_with_identical_content() {
        let host = test_host();
        let source = host.tmp.path().join("source");
        let other = host.tmp.path().join("other");
        crate::core::test_support::write_skill(&source, "writing");
        crate::core::test_support::write_skill(&other, "writing");
        let held = crate::core::repo_lock::RepoLock::try_shared()
            .unwrap()
            .unwrap();
        dispatch(&host.ctx, "operation_request", &json!({"request":{"action":"submit_install","operation_id":"source-alias","host_id":null,"sources":[{"kind":"local","path":source,"name":"writing"}]}})).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while read_operation(&host, "source-alias")["items"][0]["phase"] != "waiting" {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        std::fs::rename(&source, host.tmp.path().join("original-source")).unwrap();
        std::os::unix::fs::symlink(&other, &source).unwrap();
        drop(held);
        let result = settled_operation(&host, "source-alias");
        assert_eq!(result["items"][0]["phase"], "needs_review", "{result}");
        assert!(!crate::core::central_repo::skills_dir()
            .join("writing")
            .exists());
    }

    #[test]
    fn changed_library_install_target_requires_review_instead_of_choosing_a_new_destination() {
        let host = test_host();
        let source = host.tmp.path().join("source");
        crate::core::test_support::write_skill(&source, "writing");
        let held = crate::core::repo_lock::RepoLock::try_shared()
            .unwrap()
            .unwrap();
        dispatch(&host.ctx, "operation_request", &json!({"request":{"action":"submit_install","operation_id":"target-change","host_id":null,"sources":[{"kind":"local","path":source,"name":"writing"}]}})).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while read_operation(&host, "target-change")["items"][0]["phase"] != "waiting" {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let target = crate::core::central_repo::skills_dir().join("writing");
        crate::core::test_support::write_skill(&target, "writing");
        std::fs::write(target.join("SKILL.md"), "owned by someone else").unwrap();
        drop(held);
        let result = settled_operation(&host, "target-change");
        assert_eq!(result["items"][0]["phase"], "needs_review", "{result}");
        assert_eq!(
            std::fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "owned by someone else"
        );
        assert!(!crate::core::central_repo::skills_dir()
            .join("writing-2")
            .exists());
    }

    #[test]
    fn equivalent_library_submission_reattaches_and_running_cancel_stops_before_apply() {
        let (entered, receiver) = std::sync::mpsc::channel();
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let host = test_host_with(Arc::new(HeldOperation {
            target_path: None,
            entered_once: std::sync::atomic::AtomicBool::new(false),
            entered,
            release: release.clone(),
        }));
        let release = ReleaseOnDrop(release);
        let source = host.tmp.path().join("source");
        crate::core::test_support::write_skill(&source, "writing");
        let request = |id: &str| json!({"request":{"action":"submit_install","operation_id":id,"host_id":null,"sources":[{"kind":"local","path":source,"name":"writing"}]}});
        dispatch(&host.ctx, "operation_request", &request("held")).unwrap();
        receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        assert_eq!(
            dispatch(&host.ctx, "operation_request", &request("equivalent")).unwrap()
                ["operation_id"],
            "held"
        );
        let stopping = dispatch(
            &host.ctx,
            "operation_request",
            &json!({"request":{"action":"cancel","operation_id":"held"}}),
        )
        .unwrap();
        assert_eq!(stopping["settled"], false);
        assert_eq!(stopping["items"][0]["phase"], "stopping");
        drop(release);
        assert_eq!(
            settled_operation(&host, "held")["items"][0]["phase"],
            "canceled"
        );
        assert!(!crate::core::central_repo::skills_dir()
            .join("writing")
            .exists());
    }

    struct GitConfigGuard(Option<std::ffi::OsString>);
    impl GitConfigGuard {
        fn new(path: &std::path::Path) -> Self {
            let old = std::env::var_os("GIT_CONFIG_GLOBAL");
            std::env::set_var("GIT_CONFIG_GLOBAL", path);
            Self(old)
        }
    }
    impl Drop for GitConfigGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("GIT_CONFIG_GLOBAL", value),
                None => std::env::remove_var("GIT_CONFIG_GLOBAL"),
            }
        }
    }

    struct HeldOperation {
        target_path: Option<&'static str>,
        entered_once: std::sync::atomic::AtomicBool,
        entered: std::sync::mpsc::Sender<()>,
        release: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    }
    impl HostEvents for HeldOperation {
        fn emit(&self, event: &str, payload: Value) {
            if event == "operation-changed"
                && payload["operation_id"] == "held"
                && payload["items"].as_array().is_some_and(|items| {
                    // A targeted barrier needs the item's initial snapshot:
                    // sibling callbacks can also report that item as applying.
                    items.iter().any(|item| {
                        item["phase"] == "applying"
                            && self
                                .target_path
                                .is_none_or(|path| item["relative_path"] == path)
                    }) && self.target_path.is_none_or(|path| {
                        items.iter().all(|item| {
                            item["relative_path"] == path
                                || item["phase"] == "queued"
                                || item["phase"] == "needs_review"
                        })
                    })
                })
                && !self
                    .entered_once
                    .swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                self.entered.send(()).unwrap();
                let (lock, wake) = &*self.release;
                let _guard = wake
                    .wait_while(lock.lock().unwrap(), |released| !*released)
                    .unwrap();
            }
        }
    }
    struct ReleaseOnDrop(Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>);
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            *self.0 .0.lock().unwrap() = true;
            self.0 .1.notify_all();
        }
    }
    fn operation_project(host: &TestHost, name: &str) -> Value {
        let project = host.tmp.path().join(name);
        let target = project.join(".claude/skills/x");
        let source = crate::core::central_repo::skills_dir().join("x");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(target.join("SKILL.md"), "old copy").unwrap();
        std::fs::write(source.join("SKILL.md"), "library version").unwrap();
        std::fs::File::options()
            .write(true)
            .open(source.join("SKILL.md"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3))
            .unwrap();
        if host.ctx.store.get_skill_by_id("x").unwrap().is_none() {
            host.ctx
                .store
                .insert_skill(&crate::core::test_support::sample_skill("x", "x", &source))
                .unwrap();
        }
        dispatch(&host.ctx, "add_project", &json!({"path":project})).unwrap()
    }
    fn submit_operation(host: &TestHost, id: &str, project: &Value) -> Value {
        dispatch(&host.ctx, "operation_request", &json!({"request":{"action":"submit_project_update","operation_id":id,"project_id":project["id"],"host_id":null}})).unwrap()
    }
    fn read_operation(host: &TestHost, id: &str) -> Value {
        dispatch(
            &host.ctx,
            "operation_request",
            &json!({"request":{"action":"read","operation_id":id}}),
        )
        .unwrap()
    }
    fn settled_operation(host: &TestHost, id: &str) -> Value {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let status = read_operation(host, id);
            if status["settled"] == true {
                return status;
            }
            assert!(std::time::Instant::now() < deadline, "{status}");
            std::thread::yield_now();
        }
    }

    #[cfg(unix)]
    #[test]
    fn project_operations_serialize_aliases_without_starving_independent_projects() {
        let (entered, receiver) = std::sync::mpsc::channel();
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let host = test_host_with(Arc::new(HeldOperation {
            target_path: None,
            entered_once: std::sync::atomic::AtomicBool::new(false),
            entered,
            release: release.clone(),
        }));
        let release = ReleaseOnDrop(release);
        let a = operation_project(&host, "a");
        let c = operation_project(&host, "c");
        submit_operation(&host, "held", &a);
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let duplicate = submit_operation(&host, "duplicate", &a);
        assert_eq!(
            duplicate["operation_id"], "held",
            "equivalent active work reattaches"
        );
        // A separate linked workspace reaches the same physical folder.
        let alias = dispatch(&host.ctx, "add_linked_workspace", &json!({"path": host.tmp.path().join("a/.claude/skills"),"agentKey":"claude_code","name":"Alias"})).unwrap();
        submit_operation(&host, "alias", &alias);
        submit_operation(&host, "independent", &c);
        assert_eq!(
            settled_operation(&host, "independent")["items"][0]["phase"],
            "succeeded"
        );
        assert_ne!(read_operation(&host, "alias")["settled"], true);
        assert_eq!(
            std::fs::read_to_string(host.tmp.path().join("a/.claude/skills/x/SKILL.md")).unwrap(),
            "old copy"
        );
        drop(release);
        assert_eq!(
            settled_operation(&host, "held")["items"][0]["phase"],
            "succeeded"
        );
        assert_eq!(
            settled_operation(&host, "alias")["items"][0]["phase"],
            "unchanged"
        );
    }

    #[test]
    fn cancel_queued_project_work_never_applies_a_write() {
        let host = test_host();
        let project = operation_project(&host, "queued");
        let guard = crate::core::repo_lock::RepoLock::acquire("external library writer").unwrap();
        submit_operation(&host, "cancel-me", &project);
        let canceled = dispatch(
            &host.ctx,
            "operation_request",
            &json!({"request":{"action":"cancel","operation_id":"cancel-me"}}),
        )
        .unwrap();
        assert_eq!(canceled["items"][0]["phase"], "canceled");
        drop(guard);
        assert_eq!(
            settled_operation(&host, "cancel-me")["items"][0]["phase"],
            "canceled"
        );
        assert_eq!(
            std::fs::read_to_string(host.tmp.path().join("queued/.claude/skills/x/SKILL.md"))
                .unwrap(),
            "old copy"
        );
    }

    #[test]
    fn failed_only_retry_keeps_successes_and_review_items() {
        project_partial_retry(false);
    }

    #[test]
    fn restored_failed_project_retry_keeps_successes_and_review_items() {
        project_partial_retry(true);
    }

    fn project_partial_retry(restored: bool) {
        let (entered, receiver) = std::sync::mpsc::channel();
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let mut host = test_host_with(Arc::new(HeldOperation {
            target_path: Some("x"),
            entered_once: std::sync::atomic::AtomicBool::new(false),
            entered,
            release: release.clone(),
        }));
        let release = ReleaseOnDrop(release);
        let project = operation_project(&host, "retry");
        for name in ["y", "review"] {
            let target = host.tmp.path().join(format!("retry/.claude/skills/{name}"));
            let source = crate::core::central_repo::skills_dir().join(name);
            std::fs::create_dir_all(&target).unwrap();
            std::fs::create_dir_all(&source).unwrap();
            std::fs::write(target.join("SKILL.md"), format!("project {name}")).unwrap();
            std::fs::write(source.join("SKILL.md"), format!("library {name}")).unwrap();
            let newer = if name == "review" { &target } else { &source };
            std::fs::File::options()
                .write(true)
                .open(newer.join("SKILL.md"))
                .unwrap()
                .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3))
                .unwrap();
            host.ctx
                .store
                .insert_skill(&crate::core::test_support::sample_skill(
                    name, name, &source,
                ))
                .unwrap();
        }
        submit_operation(&host, "held", &project);
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let source = crate::core::central_repo::skills_dir().join("x");
        let saved = host.tmp.path().join("saved-source");
        std::fs::rename(&source, &saved).unwrap();
        drop(release);
        let finished = settled_operation(&host, "held");
        let phases: std::collections::HashMap<_, _> = finished["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| {
                (
                    item["relative_path"].as_str().unwrap(),
                    item["phase"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(phases["x"], "failed");
        assert_eq!(phases["y"], "succeeded");
        assert_eq!(phases["review"], "needs_review");
        if restored {
            dispatch(
                &host.ctx,
                "operation_results_request",
                &json!({"request":{"action":"write","version":0,"results":[finished]}}),
            )
            .unwrap();
            host.ctx = HostCtx::headless(Arc::new(
                SkillStore::new(&host.tmp.path().join("repo/test.db")).unwrap(),
            ));
            let retained = dispatch(
                &host.ctx,
                "operation_results_request",
                &json!({"request":{"action":"read"}}),
            )
            .unwrap();
            assert_eq!(retained["results"][0]["project_path"], project["path"]);
            assert_eq!(retained["results"][0]["items"].as_array().unwrap().len(), 3);
        }
        std::fs::rename(saved, source).unwrap();
        let request = json!({"request":{"action":"retry_failed","operation_id":"held","attempt_id":"retry-attempt"}});
        dispatch(&host.ctx, "operation_request", &request).unwrap();
        let retried = settled_operation(&host, "held");
        let items = retried["items"].as_array().unwrap();
        assert_eq!(
            items.iter().find(|i| i["relative_path"] == "x").unwrap()["phase"],
            "succeeded"
        );
        let y = items.iter().find(|i| i["relative_path"] == "y").unwrap();
        assert_eq!(y["phase"], "succeeded");
        assert_eq!(y["attempt_id"], finished["attempt_id"]);
        assert_eq!(
            items
                .iter()
                .find(|i| i["relative_path"] == "review")
                .unwrap()["phase"],
            "needs_review"
        );
        assert_eq!(
            dispatch(&host.ctx, "operation_request", &request).unwrap()["revision"],
            retried["revision"]
        );
    }

    #[test]
    fn restored_project_retry_keeps_siblings_and_revalidates_original_evidence() {
        let (entered, receiver) = std::sync::mpsc::channel();
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let mut host = test_host_with(Arc::new(HeldOperation {
            target_path: None,
            entered_once: std::sync::atomic::AtomicBool::new(false),
            entered,
            release: release.clone(),
        }));
        let release = ReleaseOnDrop(release);
        let project = operation_project(&host, "restored-project");
        submit_operation(&host, "held", &project);
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let source = crate::core::central_repo::skills_dir().join("x");
        let saved = host.tmp.path().join("saved-source");
        std::fs::rename(&source, &saved).unwrap();
        drop(release);
        assert_eq!(
            settled_operation(&host, "held")["items"][0]["phase"],
            "failed"
        );
        // Close the original operation service/store, then reopen the same isolated database.
        host.ctx = HostCtx::headless(Arc::new(
            SkillStore::new(&host.tmp.path().join("repo/test.db")).unwrap(),
        ));
        std::fs::rename(&saved, &source).unwrap();
        std::fs::write(source.join("SKILL.md"), "changed after the failure").unwrap();
        dispatch(&host.ctx,"operation_request", &json!({"request":{"action":"retry_failed","operation_id":"held","attempt_id":"restored-attempt"}})).unwrap();
        let result = settled_operation(&host, "held");
        assert_eq!(result["items"][0]["phase"], "needs_review", "{result}");
        assert_eq!(
            std::fs::read_to_string(
                host.tmp
                    .path()
                    .join("restored-project/.claude/skills/x/SKILL.md")
            )
            .unwrap(),
            "old copy"
        );
    }

    #[test]
    fn reconcile_uses_receipts_without_replaying_an_unowned_apply() {
        let (entered, receiver) = std::sync::mpsc::channel();
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let host = test_host_with(Arc::new(HeldOperation {
            target_path: None,
            entered_once: std::sync::atomic::AtomicBool::new(false),
            entered,
            release: release.clone(),
        }));
        let release = ReleaseOnDrop(release);
        let project = operation_project(&host, "receipt");
        submit_operation(&host, "held", &project);
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let reader = HostCtx::headless(host.ctx.store.clone());
        let reattached = dispatch(&reader, "operation_request", &json!({"request":{"action":"submit_project_update","operation_id":"held","project_id":project["id"],"host_id":null}})).unwrap();
        assert_eq!(reattached["items"][0]["phase"], "outcome_unknown");
        let response = dispatch(
            &reader,
            "operation_request",
            &json!({"request":{"action":"reconcile","operation_id":"held"}}),
        )
        .unwrap();
        assert_eq!(response["items"][0]["phase"], "outcome_unknown");
        assert_eq!(
            std::fs::read_to_string(host.tmp.path().join("receipt/.claude/skills/x/SKILL.md"))
                .unwrap(),
            "old copy"
        );
        drop(release);
        settled_operation(&host, "held");
        let response = dispatch(
            &reader,
            "operation_request",
            &json!({"request":{"action":"reconcile","operation_id":"held"}}),
        )
        .unwrap();
        assert_eq!(response["items"][0]["phase"], "succeeded");
    }

    #[test]
    fn reconciliation_repairs_a_live_unknown_after_result_storage_recovers() {
        let host = test_host();
        host.ctx.store.set_setting("sync_mode", "copy").unwrap();
        let project = operation_project(&host, "storage-loss");
        let database = rusqlite::Connection::open(host.tmp.path().join("repo/test.db")).unwrap();
        database.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON resource_state WHEN NEW.kind = 'operation_receipt' AND json_extract(NEW.value, '$.snapshot.settled') = 1 BEGIN SELECT RAISE(FAIL, 'lost result storage'); END;").unwrap();
        submit_operation(&host, "storage-loss", &project);
        assert_eq!(
            settled_operation(&host, "storage-loss")["items"][0]["phase"],
            "outcome_unknown"
        );
        database.execute_batch("DROP TRIGGER lose_result").unwrap();
        let recovered = dispatch(
            &host.ctx,
            "operation_request",
            &json!({"request":{"action":"reconcile","operation_id":"storage-loss"}}),
        )
        .unwrap();
        assert_eq!(recovered["items"][0]["phase"], "unchanged");
        assert_eq!(
            read_operation(&host, "storage-loss")["items"][0]["phase"],
            "unchanged"
        );
        assert_eq!(
            std::fs::read_to_string(
                host.tmp
                    .path()
                    .join("storage-loss/.claude/skills/x/SKILL.md")
            )
            .unwrap(),
            "library version"
        );
    }

    #[test]
    fn reconcile_verifies_a_project_destination_after_a_lost_result_receipt() {
        let (entered, receiver) = std::sync::mpsc::channel();
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let host = test_host_with(Arc::new(HeldOperation {
            target_path: None,
            entered_once: std::sync::atomic::AtomicBool::new(false),
            entered,
            release: release.clone(),
        }));
        let release = ReleaseOnDrop(release);
        host.ctx.store.set_setting("sync_mode", "copy").unwrap();
        let project = operation_project(&host, "lost-result");
        submit_operation(&host, "held", &project);
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        // Storage/transport stand-in: the write finishes, but only its prior
        // intent survives. Reconciliation must inspect actual files, not replay.
        let intent = host
            .ctx
            .store
            .resource_state_get("operation_receipt", "held")
            .unwrap()
            .unwrap();
        drop(release);
        settled_operation(&host, "held");
        host.ctx
            .store
            .resource_state_put("operation_receipt", "held", &intent)
            .unwrap();
        let reader = HostCtx::headless(host.ctx.store.clone());
        let reconcile = || {
            dispatch(
                &reader,
                "operation_request",
                &json!({"request":{"action":"reconcile","operation_id":"held"}}),
            )
            .unwrap()
        };
        assert_eq!(reconcile()["items"][0]["phase"], "unchanged");
        let source = crate::core::central_repo::skills_dir().join("x/SKILL.md");
        std::fs::write(&source, "source changed after write").unwrap();
        assert_eq!(reconcile()["items"][0]["phase"], "outcome_unknown");
        std::fs::write(source, "library version").unwrap();
        let target = host
            .tmp
            .path()
            .join("lost-result/.claude/skills/x/SKILL.md");
        std::fs::write(&target, "edited after response loss").unwrap();
        assert_eq!(reconcile()["items"][0]["phase"], "outcome_unknown");
        assert_eq!(
            std::fs::read_to_string(target).unwrap(),
            "edited after response loss"
        );
    }

    #[test]
    fn changed_project_update_intent_returns_to_review() {
        let (entered, receiver) = std::sync::mpsc::channel();
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let host = test_host_with(Arc::new(HeldOperation {
            target_path: None,
            entered_once: std::sync::atomic::AtomicBool::new(false),
            entered,
            release: release.clone(),
        }));
        let release = ReleaseOnDrop(release);
        let project = operation_project(&host, "changed");
        dispatch(
            &host.ctx,
            "set_settings",
            &json!({"key":"sync_mode","value":"copy"}),
        )
        .unwrap();
        submit_operation(&host, "held", &project);
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        dispatch(
            &host.ctx,
            "set_settings",
            &json!({"key":"sync_mode","value":"symlink"}),
        )
        .unwrap();
        drop(release);
        assert_eq!(
            settled_operation(&host, "held")["items"][0]["phase"],
            "needs_review"
        );
        assert_eq!(
            std::fs::read_to_string(host.tmp.path().join("changed/.claude/skills/x/SKILL.md"))
                .unwrap(),
            "old copy"
        );
    }

    struct HoldEveryApply {
        entered: std::sync::mpsc::Sender<()>,
        release: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    }
    impl HostEvents for HoldEveryApply {
        fn emit(&self, event: &str, payload: Value) {
            if event == "operation-changed" && payload["items"][0]["phase"] == "applying" {
                self.entered.send(()).unwrap();
                let (lock, wake) = &*self.release;
                let _guard = wake
                    .wait_while(lock.lock().unwrap(), |released| !*released)
                    .unwrap();
            }
        }
    }
    #[test]
    fn project_operation_admission_is_bounded_and_running_cancel_waits_for_a_safe_point() {
        let (entered, receiver) = std::sync::mpsc::channel();
        let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let host = test_host_with(Arc::new(HoldEveryApply {
            entered,
            release: release.clone(),
        }));
        let release = ReleaseOnDrop(release);
        for name in ["first", "second", "third"] {
            let project = operation_project(&host, name);
            submit_operation(&host, name, &project);
        }
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        // The first two real workers are held at the public progress boundary.
        // A third may not enter until a slot is released.
        assert!(receiver
            .recv_timeout(std::time::Duration::from_millis(200))
            .is_err());
        let list = dispatch(
            &host.ctx,
            "operation_request",
            &json!({"request":{"action":"list"}}),
        )
        .unwrap();
        assert_eq!(
            list.as_array()
                .unwrap()
                .iter()
                .filter(|o| o["items"][0]["phase"] == "applying")
                .count(),
            2
        );
        let stopping = dispatch(
            &host.ctx,
            "operation_request",
            &json!({"request":{"action":"cancel","operation_id":"first"}}),
        )
        .unwrap();
        assert_eq!(stopping["items"][0]["phase"], "stopping");
        assert_eq!(
            stopping["settled"], false,
            "cancel acknowledgement is not completion"
        );
        drop(release);
        assert_eq!(
            settled_operation(&host, "first")["items"][0]["phase"],
            "canceled"
        );
        assert_eq!(
            settled_operation(&host, "second")["items"][0]["phase"],
            "succeeded"
        );
        assert_eq!(
            settled_operation(&host, "third")["items"][0]["phase"],
            "succeeded"
        );
        assert_eq!(
            std::fs::read_to_string(host.tmp.path().join("first/.claude/skills/x/SKILL.md"))
                .unwrap(),
            "old copy"
        );
        assert_eq!(
            std::fs::read_to_string(host.tmp.path().join("second/.claude/skills/x/SKILL.md"))
                .unwrap(),
            "library version"
        );
    }

    #[test]
    fn unknown_command_is_not_found() {
        let host = test_host();
        let err = dispatch(&host.ctx, "git_backup_push", &json!({})).unwrap_err();
        assert!(is_unknown_command(&err));
    }

    #[test]
    fn missing_or_mistyped_argument_is_invalid_input() {
        let host = test_host();
        let missing =
            dispatch(&host.ctx, "set_tool_enabled", &json!({ "key": "claude" })).unwrap_err();
        assert_eq!(missing.kind, ErrorKind::InvalidInput);
        assert_eq!(missing.message, "Missing argument: enabled");

        let mistyped = dispatch(
            &host.ctx,
            "set_tool_enabled",
            &json!({ "key": "claude", "enabled": "yes" }),
        )
        .unwrap_err();
        assert_eq!(mistyped.kind, ErrorKind::InvalidInput);
        assert!(mistyped.message.starts_with("Invalid argument enabled"));
    }

    #[test]
    fn get_tool_status_lists_the_builtin_agents() {
        let host = test_host();
        let result = dispatch(&host.ctx, "get_tool_status", &json!({})).unwrap();
        let keys: Vec<&str> = result
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["key"].as_str().unwrap())
            .collect();
        assert!(keys.contains(&"claude_code"), "got {keys:?}");
    }

    #[test]
    fn created_preset_is_listed() {
        let host = test_host();
        let created = dispatch(
            &host.ctx,
            "create_preset",
            &json!({ "name": "Work", "description": null }),
        )
        .unwrap();
        assert_eq!(created["name"], "Work");

        let presets = dispatch(&host.ctx, "get_presets", &json!({})).unwrap();
        let listed = presets.as_array().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["id"], created["id"]);
    }

    #[test]
    fn host_settings_round_trip_and_local_ones_are_refused() {
        let host = test_host();
        dispatch(
            &host.ctx,
            "set_settings",
            &json!({ "key": "sync_mode", "value": "copy" }),
        )
        .unwrap();
        let value = dispatch(&host.ctx, "get_settings", &json!({ "key": "sync_mode" })).unwrap();
        assert_eq!(value, "copy");

        for key in [
            "theme",
            "show_tray_icon",
            "git_backup_remote_url",
            "github_auth_method",
        ] {
            let err = dispatch(
                &host.ctx,
                "set_settings",
                &json!({ "key": key, "value": "x" }),
            )
            .unwrap_err();
            assert_eq!(err.kind, ErrorKind::InvalidInput, "{key}");
            assert_eq!(host.ctx.store.get_setting(key).unwrap(), None, "{key}");
        }
    }

    #[test]
    fn batch_import_reports_progress_per_skill() {
        let events = Arc::new(RecordingEvents::default());
        let host = test_host_with(events.clone());
        let folder = host.tmp.path().join("import");
        for name in ["alpha", "beta"] {
            let dir = folder.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: test\n---\n"),
            )
            .unwrap();
        }

        let result = dispatch(
            &host.ctx,
            "batch_import_folder",
            &json!({ "folderPath": folder.to_string_lossy() }),
        )
        .unwrap();
        assert_eq!(result["imported"], 2);

        let recorded = events.0.lock().unwrap();
        let progress: Vec<(u64, u64)> = recorded
            .iter()
            .filter(|(event, _)| event == "batch-import-progress")
            .map(|(_, p)| (p["current"].as_u64().unwrap(), p["total"].as_u64().unwrap()))
            .collect();
        assert_eq!(progress, vec![(1, 2), (2, 2)]);
    }

    #[test]
    fn added_project_is_listed() {
        let host = test_host();
        let dir = host.tmp.path().join("my-project");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.to_string_lossy();

        let added = dispatch(
            &host.ctx,
            "add_project",
            &json!({ "path": path, "deployMode": null }),
        )
        .unwrap();
        assert_eq!(added["name"], "my-project");

        let projects = dispatch(&host.ctx, "get_projects", &json!({})).unwrap();
        let listed = projects.as_array().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["id"], added["id"]);
        assert_eq!(listed[0]["path"], path.as_ref());
    }
}
