import { invoke } from "@tauri-apps/api/core";

// ── Types ──

export interface ToolInfo {
  key: string;
  display_name: string;
  installed: boolean;
  skills_dir: string;
  enabled: boolean;
  is_custom: boolean;
  has_path_override: boolean;
  project_relative_skills_dir: string | null;
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
  scenario_ids: string[];
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

export interface Scenario {
  id: string;
  name: string;
  description: string | null;
  icon: string | null;
  sort_order: number;
  skill_count: number;
  created_at: number;
  updated_at: number;
  disclosure_mode: string;
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

export const setCustomToolPath = (key: string, path: string) =>
  invoke<void>("set_custom_tool_path", { key, path });

export const resetCustomToolPath = (key: string) =>
  invoke<void>("reset_custom_tool_path", { key });

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

export const getSkillsForScenario = (scenarioId: string) =>
  invoke<ManagedSkill[]>("get_skills_for_scenario", {
    scenarioId,
  });

export const getSkillDocument = (skillId: string) =>
  invoke<SkillDocument>("get_skill_document", { skillId });

export const getSourceSkillDocument = (skillId: string) =>
  invoke<SourceSkillDocument>("get_source_skill_document", { skillId });

export const deleteManagedSkill = (skillId: string) =>
  invoke<void>("delete_managed_skill", { skillId });

export const installLocal = (sourcePath: string, name?: string) =>
  invoke<void>("install_local", { sourcePath, name: name || null });

export const installGit = (repoUrl: string, name?: string) =>
  invoke<void>("install_git", { repoUrl, name: name || null });

export interface GitSkillPreview {
  dir_name: string;
  name: string;
  description: string | null;
}

export interface GitPreviewResult {
  temp_dir: string;
  skills: GitSkillPreview[];
}

export interface SkillInstallItem {
  dir_name: string;
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

// ── Sync ──

export const syncSkillToTool = (skillId: string, tool: string) =>
  invoke<void>("sync_skill_to_tool", { skillId, tool });

export const unsyncSkillFromTool = (skillId: string, tool: string) =>
  invoke<void>("unsync_skill_from_tool", { skillId, tool });

export const getSkillToolToggles = (skillId: string, scenarioId: string) =>
  invoke<SkillToolToggle[]>("get_skill_tool_toggles", { skillId, scenarioId });

export const setSkillToolToggle = (
  skillId: string,
  scenarioId: string,
  tool: string,
  enabled: boolean
) =>
  invoke<void>("set_skill_tool_toggle", { skillId, scenarioId, tool, enabled });

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

export const searchSkillsmp = (
  query: string,
  ai?: boolean,
  page?: number,
  limit?: number,
) =>
  invoke<SkillsShSkill[]>("search_skillsmp", {
    query,
    ai: ai ?? null,
    page: page ?? null,
    limit: limit ?? null,
  });

// ── Settings ──

export const getSettings = (key: string) =>
  invoke<string | null>("get_settings", { key });

export const setSettings = (key: string, value: string) =>
  invoke<void>("set_settings", { key, value });

export const getCentralRepoPath = () =>
  invoke<string>("get_central_repo_path");

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

// ── Git Backup ──

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
}

export interface GitBackupVersion {
  tag: string;
  commit: string;
  message: string;
  committed_at: string;
}

export const gitBackupStatus = () =>
  invoke<GitBackupStatus>("git_backup_status");

export const gitBackupInit = () => invoke<void>("git_backup_init");

export const gitBackupSetRemote = (url: string) =>
  invoke<void>("git_backup_set_remote", { url });

export const gitBackupCommit = (message: string) =>
  invoke<void>("git_backup_commit", { message });

export const gitBackupPush = () => invoke<void>("git_backup_push");

export const gitBackupPull = () => invoke<void>("git_backup_pull");

export const gitBackupClone = (url: string) =>
  invoke<void>("git_backup_clone", { url });

export const gitBackupCreateSnapshot = () =>
  invoke<string>("git_backup_create_snapshot");

export const gitBackupListVersions = (limit?: number) =>
  invoke<GitBackupVersion[]>("git_backup_list_versions", {
    limit: typeof limit === "number" ? limit : null,
  });

export const gitBackupRestoreVersion = (tag: string) =>
  invoke<void>("git_backup_restore_version", { tag });

// ── Scenarios ──

export const getScenarios = () => invoke<Scenario[]>("get_scenarios");

export const getActiveScenario = () =>
  invoke<Scenario | null>("get_active_scenario");

export const createScenario = (name: string, description?: string, icon?: string) =>
  invoke<Scenario>("create_scenario", {
    name,
    description: description || null,
    icon: icon || null,
  });

export const updateScenario = (
  id: string,
  name: string,
  description?: string,
  icon?: string
) =>
  invoke<void>("update_scenario", {
    id,
    name,
    description: description || null,
    icon: icon || null,
  });

export const deleteScenario = (id: string) =>
  invoke<void>("delete_scenario", { id });

export const switchScenario = (id: string) =>
  invoke<void>("switch_scenario", { id });

export const addSkillToScenario = (skillId: string, scenarioId: string) =>
  invoke<void>("add_skill_to_scenario", { skillId, scenarioId });

export const removeSkillFromScenario = (skillId: string, scenarioId: string) =>
  invoke<void>("remove_skill_from_scenario", { skillId, scenarioId });

export const reorderScenarios = (ids: string[]) =>
  invoke<void>("reorder_scenarios", { ids });

export const reorderProjects = (ids: string[]) =>
  invoke<void>("reorder_projects", { ids });

export const getScenarioSkillOrder = (scenarioId: string) =>
  invoke<string[]>("get_scenario_skill_order", { scenarioId });

export const reorderScenarioSkills = (scenarioId: string, skillIds: string[]) =>
  invoke<void>("reorder_scenario_skills", { scenarioId, skillIds });

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

// ── Packs ──

export interface PackRecord {
  id: string;
  name: string;
  description: string | null;
  icon: string | null;
  color: string | null;
  sort_order: number;
  created_at: number;
  updated_at: number;
  router_description: string | null;
  router_body: string | null;
  is_essential: boolean;
  router_updated_at: number | null;
}

/** Lightweight skill record returned by pack queries (not the full ManagedSkill). */
export interface PackSkillRecord {
  id: string;
  name: string;
  description: string | null;
  source_type: string;
  central_path: string;
  enabled: boolean;
  status: string;
}

export const getAllPacks = () =>
  invoke<PackRecord[]>("get_all_packs");

export const getPackById = (id: string) =>
  invoke<PackRecord | null>("get_pack_by_id", { id });

export const createPack = (
  name: string,
  description?: string,
  icon?: string,
  color?: string,
) =>
  invoke<PackRecord>("create_pack", {
    name,
    description: description ?? null,
    icon: icon ?? null,
    color: color ?? null,
  });

export const updatePack = (
  id: string,
  name: string,
  description?: string,
  icon?: string,
  color?: string,
) =>
  invoke<void>("update_pack", {
    id,
    name,
    description: description ?? null,
    icon: icon ?? null,
    color: color ?? null,
  });

export const deletePack = (id: string) =>
  invoke<void>("delete_pack", { id });

export const addSkillToPack = (packId: string, skillId: string) =>
  invoke<void>("add_skill_to_pack", { packId, skillId });

export const removeSkillFromPack = (packId: string, skillId: string) =>
  invoke<void>("remove_skill_from_pack", { packId, skillId });

export const getSkillsForPack = (packId: string) =>
  invoke<PackSkillRecord[]>("get_skills_for_pack", { packId });

export const getPacksForScenario = (scenarioId: string) =>
  invoke<PackRecord[]>("get_packs_for_scenario", { scenarioId });

export const addPackToScenario = (scenarioId: string, packId: string) =>
  invoke<void>("add_pack_to_scenario", { scenarioId, packId });

export const removePackFromScenario = (scenarioId: string, packId: string) =>
  invoke<void>("remove_pack_from_scenario", { scenarioId, packId });

export const getEffectiveSkillsForScenario = (scenarioId: string) =>
  invoke<PackSkillRecord[]>("get_effective_skills_for_scenario", { scenarioId });

// ── Agent Configs (Phase 5 — per-agent scenario assignment) ──

export interface AgentConfigDto {
  tool_key: string;
  display_name: string;
  scenario_id: string | null;
  scenario_name: string | null;
  managed: boolean;
  installed: boolean;
  effective_skill_count: number;
  extra_pack_count: number;
}

export const getAllAgentConfigs = () =>
  invoke<AgentConfigDto[]>("get_all_agent_configs");

export const getAgentConfig = (toolKey: string) =>
  invoke<AgentConfigDto | null>("get_agent_config", { toolKey });

export const setAgentScenario = (toolKey: string, scenarioId: string) =>
  invoke<void>("set_agent_scenario", { toolKey, scenarioId });

export const setAgentManaged = (toolKey: string, managed: boolean) =>
  invoke<void>("set_agent_managed", { toolKey, managed });

export const addAgentExtraPack = (toolKey: string, packId: string) =>
  invoke<void>("add_agent_extra_pack", { toolKey, packId });

export const removeAgentExtraPack = (toolKey: string, packId: string) =>
  invoke<void>("remove_agent_extra_pack", { toolKey, packId });

export const getEffectiveSkillsForAgent = (toolKey: string) =>
  invoke<PackSkillRecord[]>("get_effective_skills_for_agent", { toolKey });

export const getAgentExtraPacks = (toolKey: string) =>
  invoke<PackRecord[]>("get_agent_extra_packs", { toolKey });

export interface AgentSkillOwnership {
  managed: PackSkillRecord[];
  discovered: {
    id: string;
    tool: string;
    found_path: string;
    name_guess: string | null;
    fingerprint: string | null;
    found_at: number;
    imported_skill_id: string | null;
    is_native: boolean;
  }[];
  native: string[];
}

export const getAgentSkillOwnership = (toolKey: string) =>
  invoke<AgentSkillOwnership>("get_agent_skill_ownership", { toolKey });

export interface DedupResult {
  already_linked: string[];
  replaced_with_symlink: string[];
  marked_native: string[];
  skipped_unknown: string[];
  errors: string[];
}

export const dedupAgentSkills = (toolKey: string) =>
  invoke<DedupResult>("dedup_agent_skills", { toolKey });

export const importDiscoveredSkill = (discoveredId: string) =>
  invoke<void>("import_discovered_skill", { discoveredId });

export const markSkillAsNative = (discoveredId: string) =>
  invoke<void>("mark_skill_as_native", { discoveredId });

export const unmarkSkillAsNative = (discoveredId: string) =>
  invoke<void>("unmark_skill_as_native", { discoveredId });

// ── Plugins (Phase 3 — stubs until backend is merged) ──

export interface ManagedPlugin {
  id: string;
  plugin_key: string;
  display_name: string | null;
  plugin_data: string;
  created_at: number;
  updated_at: number;
}

export interface ScenarioPlugin {
  plugin: ManagedPlugin;
  enabled: boolean;
}

export const getManagedPlugins = () =>
  invoke<ManagedPlugin[]>("get_managed_plugins");

export const scanPlugins = () =>
  invoke<void>("scan_plugins");

export const getScenarioPlugins = (scenarioId: string) =>
  invoke<ScenarioPlugin[]>("get_scenario_plugins", { scenarioId });

export const setScenarioPluginEnabled = (
  scenarioId: string,
  pluginId: string,
  enabled: boolean,
) =>
  invoke<void>("set_scenario_plugin_enabled", { scenarioId, pluginId, enabled });
