import { invoke } from "@tauri-apps/api/core";

// ── Types ──

export interface ToolInfo {
  key: string;
  display_name: string;
  installed: boolean;
  skills_dir: string;
  enabled: boolean;
}

export interface ManagedSkill {
  id: string;
  name: string;
  description: string | null;
  source_type: string;
  source_ref: string | null;
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

export interface Scenario {
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

export interface Project {
  id: string;
  name: string;
  path: string;
  sort_order: number;
  skill_count: number;
  created_at: number;
  updated_at: number;
}

export interface ProjectSkill {
  name: string;
  dir_name: string;
  description: string | null;
  path: string;
  files: string[];
  enabled: boolean;
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

// ── Skills ──

export const getManagedSkills = () =>
  invoke<ManagedSkill[]>("get_managed_skills");

export const getSkillsForScenario = (scenarioId: string) =>
  invoke<ManagedSkill[]>("get_skills_for_scenario", {
    scenarioId,
  });

export const getSkillDocument = (skillId: string) =>
  invoke<SkillDocument>("get_skill_document", { skillId });

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

export const updateSkill = (skillId: string) =>
  invoke<ManagedSkill>("update_skill", { skillId });

export const reimportLocalSkill = (skillId: string) =>
  invoke<ManagedSkill>("reimport_local_skill", { skillId });

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

export const removeProject = (id: string) =>
  invoke<void>("remove_project", { id });

export const scanProjects = (root: string) =>
  invoke<string[]>("scan_projects", { root });

export const getProjectSkills = (projectId: string) =>
  invoke<ProjectSkill[]>("get_project_skills", { projectId });

export const getProjectSkillDocument = (projectPath: string, skillDirName: string) =>
  invoke<ProjectSkillDocument>("get_project_skill_document", { projectPath, skillDirName });

export const importProjectSkillToCenter = (projectId: string, skillDirName: string) =>
  invoke<void>("import_project_skill_to_center", { projectId, skillDirName });

export const exportSkillToProject = (skillId: string, projectId: string) =>
  invoke<void>("export_skill_to_project", { skillId, projectId });

export const updateProjectSkillToCenter = (projectId: string, skillDirName: string) =>
  invoke<void>("update_project_skill_to_center", { projectId, skillDirName });

export const updateProjectSkillFromCenter = (projectId: string, skillDirName: string) =>
  invoke<void>("update_project_skill_from_center", { projectId, skillDirName });

export const toggleProjectSkill = (projectId: string, skillDirName: string, enabled: boolean) =>
  invoke<void>("toggle_project_skill", { projectId, skillDirName, enabled });

export const deleteProjectSkill = (projectId: string, skillDirName: string) =>
  invoke<void>("delete_project_skill", { projectId, skillDirName });

export const slugifySkillNames = (names: string[]) =>
  invoke<string[]>("slugify_skill_names", { names });
