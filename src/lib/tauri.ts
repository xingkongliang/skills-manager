import { invoke } from "@tauri-apps/api/core";

// ── Types ──

export type ToolCategory = "coding" | "lobster";

export interface ToolInfo {
  key: string;
  display_name: string;
  installed: boolean;
  skills_dir: string;
  enabled: boolean;
  is_custom: boolean;
  has_path_override: boolean;
  project_relative_skills_dir: string | null;
  has_project_path_override: boolean;
  category: ToolCategory;
}

export interface ManagedSkill {
  id: string;
  name: string;
  description: string | null;
  source_type: string;
  source_ref: string | null;
  source_ref_resolved: string | null;
  source_subpath: string | null;
  source_branch: string | null;
  source_revision: string | null;
  remote_revision: string | null;
  update_status: string;
  last_checked_at: number | null;
  last_check_error: string | null;
  central_path: string;
  enabled: boolean;
  created_at: number;
  updated_at: number;
  status: string;
  targets: SkillTarget[];
  preset_ids: string[];
  tags: string[];
}

export interface SkillTarget {
  id: string;
  skill_id: string;
  tool: string;
  target_path: string;
  mode: string;
  status: string;
  synced_at: number | null;
}

export interface SkillToolToggle {
  tool: string;
  display_name: string;
  installed: boolean;
  globally_enabled: boolean;
  enabled: boolean;
}

export interface SkillDocument {
  skill_id: string;
  filename: string;
  content: string;
  central_path: string;
}

export interface SourceSkillDocument {
  skill_id: string;
  filename: string;
  content: string;
  source_label: string;
  revision: string;
}

export type SkillSourceDiffStatus = "added" | "removed" | "modified";
export type SkillSourceDiffContentKind =
  | "text"
  | "binary"
  | "too_large"
  | "permission_only";

export interface SkillSourceDiffEntry {
  relative_path: string;
  status: SkillSourceDiffStatus;
  content_kind: SkillSourceDiffContentKind;
  original_text: string | null;
  updated_text: string | null;
  executable_before: boolean;
  executable_after: boolean;
}

export interface SkillSourceDiff {
  skill_id: string;
  source_label: string;
  revision: string;
  entries: SkillSourceDiffEntry[];
}

export interface Preset {
  id: string;
  name: string;
  description: string | null;
  icon: string | null;
  sort_order: number;
  skill_count: number;
  created_at: number;
  updated_at: number;
}

export interface DiscoveredGroup {
  name: string;
  fingerprint: string | null;
  locations: { id: string; tool: string; found_path: string }[];
  imported: boolean;
  found_at: number;
}

export interface ScanResult {
  tools_scanned: number;
  skills_found: number;
  groups: DiscoveredGroup[];
}

export interface SkillsShSkill {
  id: string;
  skill_id: string;
  name: string;
  source: string;
  installs: number;
}

export interface SyncHealth {
  in_sync: number;
  project_newer: number;
  center_newer: number;
  diverged: number;
  project_only: number;
}

export interface Project {
  id: string;
  name: string;
  path: string;
  workspace_type: "project" | "linked";
  linked_agent_name: string | null;
  supports_skill_toggle: boolean;
  sort_order: number;
  skill_count: number;
  sync_health: SyncHealth;
  created_at: number;
  updated_at: number;
}

export interface ProjectAgentTarget {
  key: string;
  display_name: string;
  enabled: boolean;
  installed: boolean;
  is_custom: boolean;
}

export interface ProjectSkill {
  name: string;
  dir_name: string;
  relative_path: string;
  description: string | null;
  path: string;
  files: string[];
  enabled: boolean;
  agent: string;
  agent_display_name: string;
  tags: string[];
  in_center: boolean;
  sync_status: "project_only" | "in_sync" | "project_newer" | "center_newer" | "diverged";
  center_skill_id: string | null;
}

export interface ProjectSkillDocument {
  skill_name: string;
  filename: string;
  content: string;
}

// ── Tools ──

export const getToolStatus = () => invoke<ToolInfo[]>("get_tool_status");

export const setToolEnabled = (key: string, enabled: boolean) =>
  invoke<void>("set_tool_enabled", { key, enabled });

export const setAllToolsEnabled = (enabled: boolean) =>
  invoke<void>("set_all_tools_enabled", { enabled });

export const getToolOrder = () => invoke<string[]>("get_tool_order_cmd");

export const setToolOrder = (order: string[]) =>
  invoke<void>("set_tool_order_cmd", { order });

export const setCustomToolPath = (key: string, path: string) =>
  invoke<void>("set_custom_tool_path", { key, path });

export const resetCustomToolPath = (key: string) =>
  invoke<void>("reset_custom_tool_path", { key });

export const setCustomToolProjectPath = (
  key: string,
  projectRelativeSkillsDir: string | null,
) =>
  invoke<void>("set_custom_tool_project_path", {
    key,
    projectRelativeSkillsDir,
  });

export const resetCustomToolProjectPath = (key: string) =>
  invoke<void>("reset_custom_tool_project_path", { key });

export const addCustomTool = (
  key: string,
  displayName: string,
  skillsDir: string,
  projectRelativeSkillsDir?: string,
) =>
  invoke<void>("add_custom_tool", {
    key,
    displayName,
    skillsDir,
    projectRelativeSkillsDir: projectRelativeSkillsDir ?? null,
  });

export const removeCustomTool = (key: string) =>
  invoke<void>("remove_custom_tool", { key });

// ── Skills ──

export const getManagedSkills = () =>
  invoke<ManagedSkill[]>("get_managed_skills");

export const getSkillsForPreset = (presetId: string) =>
  invoke<ManagedSkill[]>("get_skills_for_preset", {
    presetId,
  });

export const getSkillDocument = (skillId: string) =>
  invoke<SkillDocument>("get_skill_document", { skillId });

export const getSourceSkillDocument = (skillId: string) =>
  invoke<SourceSkillDocument>("get_source_skill_document", { skillId });

export const getSkillSourceDiff = (skillId: string) =>
  invoke<SkillSourceDiff>("get_skill_source_diff", { skillId });

export const deleteManagedSkill = (skillId: string) =>
  invoke<void>("delete_managed_skill", { skillId });

export interface BatchDeleteSkillsResult {
  deleted: number;
  failed: string[];
}

export const deleteManagedSkills = (skillIds: string[]) =>
  invoke<BatchDeleteSkillsResult>("delete_managed_skills", { skillIds });

export const installLocal = (sourcePath: string, name?: string) =>
  invoke<void>("install_local", { sourcePath, name: name || null });

export const installGit = (repoUrl: string, name?: string) =>
  invoke<void>("install_git", { repoUrl, name: name || null });

export interface GitSkillPreview {
  /** Path relative to the resolved scan root, using `/` separators. Stable key. */
  rel_path: string;
  name: string;
  description: string | null;
}

export interface GitPreviewResult {
  temp_dir: string;
  skills: GitSkillPreview[];
}

export interface SkillInstallItem {
  rel_path: string;
  name: string;
}

export const previewGitInstall = (repoUrl: string) =>
  invoke<GitPreviewResult>("preview_git_install", { repoUrl });

export const confirmGitInstall = (repoUrl: string, tempDir: string, items: SkillInstallItem[]) =>
  invoke<void>("confirm_git_install", { repoUrl, tempDir, items });

export const cancelGitPreview = (tempDir: string) =>
  invoke<void>("cancel_git_preview", { tempDir });

export const installFromSkillssh = (source: string, skillId: string) =>
  invoke<void>("install_from_skillssh", { source, skillId });

export const cancelInstall = (key: string) =>
  invoke<boolean>("cancel_install", { key });

export const checkSkillUpdate = (skillId: string, force?: boolean) =>
  invoke<ManagedSkill>("check_skill_update", {
    skillId,
    force: force ?? false,
  });

export const checkAllSkillUpdates = (force?: boolean) =>
  invoke<void>("check_all_skill_updates", {
    force: force ?? false,
  });

export interface UpdateSkillResult {
  skill: ManagedSkill;
  /** False when a monorepo commit didn't touch this skill's subdirectory. */
  content_changed: boolean;
}

export const updateSkill = (skillId: string) =>
  invoke<UpdateSkillResult>("update_skill", { skillId });

export interface BatchUpdateSkillsResult {
  refreshed: number;
  unchanged: number;
  failed: string[];
}

export const batchUpdateSkills = (skillIds: string[]) =>
  invoke<BatchUpdateSkillsResult>("batch_update_skills", { skillIds });

export const reimportLocalSkill = (skillId: string) =>
  invoke<ManagedSkill>("reimport_local_skill", { skillId });

export const relinkLocalSkillSource = (skillId: string, sourcePath: string) =>
  invoke<ManagedSkill>("relink_local_skill_source", { skillId, sourcePath });

export const detachLocalSkillSource = (skillId: string) =>
  invoke<ManagedSkill>("detach_local_skill_source", { skillId });

export interface BatchImportResult {
  imported: number;
  skipped: number;
  errors: string[];
}

export const batchImportFolder = (folderPath: string) =>
  invoke<BatchImportResult>("batch_import_folder", { folderPath });

export const getAllTags = () => invoke<string[]>("get_all_tags");

export const setSkillTags = (skillId: string, tags: string[]) =>
  invoke<void>("set_skill_tags", { skillId, tags });

export const renameTag = (oldName: string, newName: string) =>
  invoke<void>("rename_tag", { oldName, newName });

export const deleteTag = (name: string) =>
  invoke<void>("delete_tag", { name });

// ── Sync ──

export const syncSkillToTool = (skillId: string, tool: string) =>
  invoke<void>("sync_skill_to_tool", { skillId, tool });

export const unsyncSkillFromTool = (skillId: string, tool: string) =>
  invoke<void>("unsync_skill_from_tool", { skillId, tool });

export const getSkillToolToggles = (skillId: string, presetId: string) =>
  invoke<SkillToolToggle[]>("get_skill_tool_toggles", { skillId, presetId });

export const setSkillToolToggle = (
  skillId: string,
  presetId: string,
  tool: string,
  enabled: boolean
) =>
  invoke<void>("set_skill_tool_toggle", { skillId, presetId, tool, enabled });

// ── Scan ──

export const scanLocalSkills = () => invoke<ScanResult>("scan_local_skills");

export const importExistingSkill = (sourcePath: string, name?: string) =>
  invoke<void>("import_existing_skill", { sourcePath, name: name || null });

export const importAllDiscovered = () =>
  invoke<void>("import_all_discovered");

// ── Browse ──

export const fetchLeaderboard = (board: string) =>
  invoke<SkillsShSkill[]>("fetch_leaderboard", { board });

export const searchSkillssh = (query: string, limit?: number) =>
  invoke<SkillsShSkill[]>("search_skillssh", {
    query,
    limit: limit ?? null,
  });

// ── Settings ──

export const getSettings = (key: string) =>
  invoke<string | null>("get_settings", { key });

export const setSettings = (key: string, value: string) =>
  invoke<void>("set_settings", { key, value });

export const getCentralRepoPath = () =>
  invoke<string>("get_central_repo_path");

export const getCentralRepoPathOverride = () =>
  invoke<string | null>("get_central_repo_path_override");

export const setCentralRepoPath = (path?: string | null) =>
  invoke<string>("set_central_repo_path", { path: path ?? null });

export const appExit = () => invoke<void>("app_exit");

export const hideToTray = () => invoke<void>("hide_to_tray");

export const openCentralRepoFolder = () =>
  invoke<void>("open_central_repo_folder");

export interface AppUpdateInfo {
  has_update: boolean;
  current_version: string;
  latest_version: string;
  release_url: string;
}

export const checkAppUpdate = () =>
  invoke<AppUpdateInfo>("check_app_update");

export interface DiagnosticInfo {
  app_version: string;
  os: string;
  os_version: string;
  arch: string;
  central_repo_path: string;
  central_repo_path_overridden: boolean;
}

export const getDiagnosticInfo = () =>
  invoke<DiagnosticInfo>("get_diagnostic_info");

export interface LogExcerpt {
  log_path: string;
  excerpt: string;
  line_count: number;
  has_warnings: boolean;
}

export const getRecentLogExcerpt = () =>
  invoke<LogExcerpt>("get_recent_log_excerpt");

export interface LogExportResult {
  zip_path: string;
  file_count: number;
}

export const exportLogsZip = () =>
  invoke<LogExportResult>("export_logs_zip");

export interface PanicInfo {
  timestamp: string;
  message: string;
}

export const checkLastPanic = () =>
  invoke<PanicInfo | null>("check_last_panic");

export const clearLastPanic = () =>
  invoke<void>("clear_last_panic");

/**
 * Diagnostic-only: write a named startup event with elapsed ms (from
 * performance.timeOrigin) into the backend log file. Used to correlate
 * WebView2 boot and frontend boot timing with Rust-side startup logs
 * when debugging slow launches (see issue #153).
 */
export const logStartupEvent = (label: string, elapsedMs: number) =>
  invoke<void>("log_startup_event", { label, elapsedMs: Math.round(elapsedMs) });

// ── Git Backup ──

export type GitUpstreamHealth =
  | "healthy"
  | "no_remote"
  | "no_upstream"
  | "unrelated_histories"
  | "detached";

export interface GitBackupStatus {
  is_repo: boolean;
  remote_url: string | null;
  branch: string | null;
  has_changes: boolean;
  ahead: number;
  behind: number;
  last_commit: string | null;
  last_commit_time: string | null;
  current_snapshot_tag: string | null;
  restored_from_tag: string | null;
  upstream_health: GitUpstreamHealth;
}

export interface GitBackupVersion {
  tag: string;
  commit: string;
  message: string;
  committed_at: string;
}

export const gitBackupStatus = () =>
  invoke<GitBackupStatus>("git_backup_status");

export const gitBackupFetch = () => invoke<void>("git_backup_fetch");

export const gitBackupInit = () => invoke<void>("git_backup_init");

export const gitBackupSetRemote = (url: string) =>
  invoke<void>("git_backup_set_remote", { url });

export const gitBackupCommit = (message: string) =>
  invoke<void>("git_backup_commit", { message });

export const gitBackupPush = () => invoke<void>("git_backup_push");

export const gitBackupPull = () => invoke<void>("git_backup_pull");

export const gitBackupClone = (url: string) =>
  invoke<void>("git_backup_clone", { url });

export const gitBackupReclone = (url: string) =>
  invoke<void>("git_backup_reclone", { url });

export const gitBackupCreateSnapshot = () =>
  invoke<string>("git_backup_create_snapshot");

export const gitBackupListVersions = (limit?: number) =>
  invoke<GitBackupVersion[]>("git_backup_list_versions", {
    limit: typeof limit === "number" ? limit : null,
  });

export const gitBackupRestoreVersion = (tag: string) =>
  invoke<void>("git_backup_restore_version", { tag });

// ── Presets ──

export const getPresets = () => invoke<Preset[]>("get_presets");

export const getActivePreset = () =>
  invoke<Preset | null>("get_active_preset");

export const createPreset = (name: string, description?: string, icon?: string) =>
  invoke<Preset>("create_preset", {
    name,
    description: description || null,
    icon: icon || null,
  });

export const updatePreset = (
  id: string,
  name: string,
  description?: string,
  icon?: string
) =>
  invoke<void>("update_preset", {
    id,
    name,
    description: description || null,
    icon: icon || null,
  });

export const deletePreset = (id: string) =>
  invoke<void>("delete_preset", { id });

/** @deprecated v1.16+: clicking a scene no longer applies. Use applyPresetToDefault. */
export const switchPreset = (id: string) =>
  invoke<void>("switch_preset", { id });

export const applyPresetToDefault = (id: string) =>
  invoke<void>("apply_preset_to_default", { id });

export const addSkillToPreset = (skillId: string, presetId: string) =>
  invoke<void>("add_skill_to_preset", { skillId, presetId });

export const removeSkillFromPreset = (skillId: string, presetId: string) =>
  invoke<void>("remove_skill_from_preset", { skillId, presetId });

export const reorderPresets = (ids: string[]) =>
  invoke<void>("reorder_presets", { ids });

export const reorderProjects = (ids: string[]) =>
  invoke<void>("reorder_projects", { ids });

export const getPresetSkillOrder = (presetId: string) =>
  invoke<string[]>("get_preset_skill_order", { presetId });

export const reorderPresetSkills = (presetId: string, skillIds: string[]) =>
  invoke<void>("reorder_preset_skills", { presetId, skillIds });

// ── Projects ──

export const getProjects = () => invoke<Project[]>("get_projects");

export const addProject = (path: string) =>
  invoke<Project>("add_project", { path });

export const addLinkedWorkspace = (name: string, path: string, disabledPath?: string) =>
  invoke<Project>("add_linked_workspace", {
    name,
    path,
    disabledPath: disabledPath ?? null,
  });

export const removeProject = (id: string) =>
  invoke<void>("remove_project", { id });

export const scanProjects = (root: string) =>
  invoke<string[]>("scan_projects", { root });

export const getProjectAgentTargets = (projectId: string) =>
  invoke<ProjectAgentTarget[]>("get_project_agent_targets", { projectId });

export const getProjectSkills = (projectId: string) =>
  invoke<ProjectSkill[]>("get_project_skills", { projectId });

export const getProjectSkillDocument = (projectId: string, skillRelativePath: string, agent: string) =>
  invoke<ProjectSkillDocument>("get_project_skill_document", { projectId, skillRelativePath, agent });

export const importProjectSkillToCenter = (projectId: string, skillRelativePath: string, agent: string) =>
  invoke<void>("import_project_skill_to_center", { projectId, skillRelativePath, agent });

export const exportSkillToProject = (skillId: string, projectId: string, agents?: string[]) =>
  invoke<void>("export_skill_to_project", { skillId, projectId, agents: agents ?? null });

export const updateProjectSkillToCenter = (projectId: string, skillRelativePath: string, agent: string) =>
  invoke<void>("update_project_skill_to_center", { projectId, skillRelativePath, agent });

export const updateProjectSkillFromCenter = (projectId: string, skillRelativePath: string, agent: string) =>
  invoke<void>("update_project_skill_from_center", { projectId, skillRelativePath, agent });

export const toggleProjectSkill = (projectId: string, skillRelativePath: string, agent: string, enabled: boolean) =>
  invoke<void>("toggle_project_skill", { projectId, skillRelativePath, agent, enabled });

export const deleteProjectSkill = (projectId: string, skillRelativePath: string, agent: string) =>
  invoke<void>("delete_project_skill", { projectId, skillRelativePath, agent });

export const slugifySkillNames = (names: string[]) =>
  invoke<string[]>("slugify_skill_names", { names });

// ── Agent Local Workspace ──

export const getGlobalLocalSkills = (agent: string) =>
  invoke<ProjectSkill[]>("get_global_local_skills", { agent });

export const getGlobalLocalSkillDocument = (agent: string, skillRelativePath: string) =>
  invoke<ProjectSkillDocument>("get_global_local_skill_document", { agent, skillRelativePath });

export const importGlobalLocalSkillToCenter = (agent: string, skillRelativePath: string) =>
  invoke<void>("import_global_local_skill_to_center", { agent, skillRelativePath });

export const updateGlobalLocalSkillFromCenter = (agent: string, skillRelativePath: string) =>
  invoke<void>("update_global_local_skill_from_center", { agent, skillRelativePath });

export const deleteGlobalLocalSkill = (agent: string, skillRelativePath: string) =>
  invoke<void>("delete_global_local_skill", { agent, skillRelativePath });
