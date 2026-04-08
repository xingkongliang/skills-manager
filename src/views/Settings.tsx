import { useState, useEffect, useCallback } from "react";
import {
  Folder,
  FolderOpen,
  RefreshCw,
  CheckCircle2,
  Circle,
  Globe,
  Layers,
  Link as LinkIcon,
  Copy,
  Settings2,
  Github,
  Loader2,
  ExternalLink,
  Sun,
  Moon,
  Monitor,
  BookOpen,
  Download,
  Type,
  Key,
  Pencil,
  RotateCcw,
  Plus,
  Trash2,
  X,
  Check,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { openUrl } from "@tauri-apps/plugin-opener";
import { check as checkUpdater } from "@tauri-apps/plugin-updater";
import { open as dialogOpen, confirm as dialogConfirm } from "@tauri-apps/plugin-dialog";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import { useThemeContext } from "../context/ThemeContext";
import * as api from "../lib/tauri";
import type { AppUpdateInfo } from "../lib/tauri";
import type { Theme } from "../hooks/useTheme";

const IS_WINDOWS = navigator.userAgent.includes("Windows");
const TEXT_SIZE_ZOOM_MAP: Record<string, string> = {
  small: "0.9",
  default: "1",
  large: "1.1",
  xlarge: "1.2",
};

function applyTextSize(size: string) {
  document.documentElement.style.zoom = TEXT_SIZE_ZOOM_MAP[size] || "1";
}

export function Settings() {
  const { t, i18n } = useTranslation();
  const { tools, scenarios, refreshTools, openHelp } = useApp();
  const [togglingTools, setTogglingTools] = useState<Set<string>>(new Set());
  const { theme, setTheme } = useThemeContext();
  const [syncMode, setSyncMode] = useState("symlink");
  const [syncScope, setSyncScope] = useState("scenario");
  const [defaultScenario, setDefaultScenario] = useState("");
  const [closeAction, setCloseAction] = useState("");
  const [showTrayIcon, setShowTrayIcon] = useState(true);
  const [developerMode, setDeveloperMode] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [openingRepo, setOpeningRepo] = useState(false);
  const [openingGithub, setOpeningGithub] = useState(false);
  const [centralRepoPath, setCentralRepoPath] = useState("");
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<AppUpdateInfo | null>(null);
  const [installing, setInstalling] = useState(false);
  const [gitRemoteInput, setGitRemoteInput] = useState("");
  const [gitRemoteSaving, setGitRemoteSaving] = useState(false);
  const [proxyInput, setProxyInput] = useState("");
  const [proxySaving, setProxySaving] = useState(false);
  const [textSize, setTextSize] = useState("default");
  const [skillsmpApiKey, setSkillsmpApiKey] = useState("");
  const [skillsmpSaving, setSkillsmpSaving] = useState(false);
  // Agent path editing
  const [editingPathKey, setEditingPathKey] = useState<string | null>(null);
  const [editingPathValue, setEditingPathValue] = useState("");
  // Custom agent dialog
  const [showAddCustom, setShowAddCustom] = useState(false);
  const [customName, setCustomName] = useState("");
  const [customPath, setCustomPath] = useState("");
  const [addingCustom, setAddingCustom] = useState(false);

  const GITHUB_URL = "https://github.com/xingkongliang/skills-manager";

  const startEditPath = useCallback((key: string, currentPath: string) => {
    setEditingPathKey(key);
    setEditingPathValue(currentPath);
  }, []);

  const handleSavePath = async () => {
    if (!editingPathKey || !editingPathValue.trim()) return;
    try {
      await api.setCustomToolPath(editingPathKey, editingPathValue.trim());
      await refreshTools();
      toast.success(t("settings.pathSaved"));
    } catch (e) {
      toast.error(String(e));
    } finally {
      setEditingPathKey(null);
    }
  };

  const handleResetPath = async (key: string) => {
    try {
      await api.resetCustomToolPath(key);
      await refreshTools();
      toast.success(t("settings.pathReset"));
    } catch {
      toast.error(t("common.error"));
    }
  };

  const handleBrowsePath = async (setter: (v: string) => void) => {
    const selected = await dialogOpen({ directory: true, multiple: false });
    if (selected && typeof selected === "string") {
      setter(selected);
    }
  };

  const generateCustomAgentKey = useCallback(
    (name: string) => {
      const base = name
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "_")
        .replace(/^_+|_+$/g, "");
      const seed = base || "agent";
      const existingKeys = new Set(tools.map((tool) => tool.key));
      if (!existingKeys.has(seed)) return seed;
      let n = 2;
      while (existingKeys.has(`${seed}_${n}`)) n += 1;
      return `${seed}_${n}`;
    },
    [tools]
  );

  const handleAddCustomAgent = async () => {
    const trimName = customName.trim();
    const trimPath = customPath.trim();
    if (!trimName || !trimPath) return;
    const trimKey = generateCustomAgentKey(trimName);
    setAddingCustom(true);
    try {
      await api.addCustomTool(trimKey, trimName, trimPath);
      await refreshTools();
      toast.success(t("settings.customAgentAdded"));
      setShowAddCustom(false);
      setCustomName("");
      setCustomPath("");
    } catch (e) {
      toast.error(String(e));
    } finally {
      setAddingCustom(false);
    }
  };

  const handleRemoveCustomAgent = async (key: string, name: string) => {
    const shouldRemove = await dialogConfirm(t("settings.removeCustomAgentConfirm", { name }));
    if (!shouldRemove) return;
    try {
      await api.removeCustomTool(key);
      await refreshTools();
      toast.success(t("settings.customAgentRemoved"));
    } catch {
      toast.error(t("common.error"));
    }
  };

  useEffect(() => {
    api.getSettings("sync_mode").then((v) => { if (v) setSyncMode(v); });
    api.getSettings("skill_sync_scope").then((v) => { if (v) setSyncScope(v); });
    api.getSettings("default_scenario").then((v) => { if (v) setDefaultScenario(v); });
    api.getSettings("proxy_url").then((v) => { setProxyInput(v ?? ""); });
    api.getSettings("close_action").then((v) => { setCloseAction(v ?? ""); });
    api.getSettings("show_tray_icon").then((v) => {
      const normalized = (v ?? "true").trim().toLowerCase();
      setShowTrayIcon(!(normalized === "false" || normalized === "0" || normalized === "no" || normalized === "off"));
    });
    api.getSettings("developer_mode").then((v) => {
      setDeveloperMode(v === "true");
    });
    api.getSettings("text_size").then((v) => { if (v) { setTextSize(v); applyTextSize(v); } });
    api.getSettings("skillsmp_api_key").then((v) => { if (v) setSkillsmpApiKey(v); });
    api.getCentralRepoPath().then(setCentralRepoPath).catch(() => {});

    (async () => {
      const savedRemote = (await api.getSettings("git_backup_remote_url").catch(() => null))?.trim() || "";
      if (savedRemote) {
        setGitRemoteInput(savedRemote);
        return;
      }

      // Fallback: if repo already has remote configured, auto-fill and persist it.
      const status = await api.gitBackupStatus().catch(() => null);
      const detectedRemote = status?.remote_url?.trim() || "";
      if (detectedRemote) {
        setGitRemoteInput(detectedRemote);
        api.setSettings("git_backup_remote_url", detectedRemote).catch(() => {});
      }
    })();
  }, []);

  const handleRefresh = async () => {
    setRefreshing(true);
    await refreshTools();
    setRefreshing(false);
    toast.success(t("common.success"));
  };

  const handleToggleTool = async (key: string, enabled: boolean) => {
    setTogglingTools((prev) => new Set(prev).add(key));
    try {
      await api.setToolEnabled(key, enabled);
      await refreshTools();
    } catch {
      toast.error(t("common.error"));
    } finally {
      setTogglingTools((prev) => {
        const next = new Set(prev);
        next.delete(key);
        return next;
      });
    }
  };

  const handleToggleAllTools = async (enabled: boolean) => {
    try {
      await api.setAllToolsEnabled(enabled);
      await refreshTools();
      toast.success(t("common.success"));
    } catch {
      toast.error(t("common.error"));
    }
  };

  const handleSyncModeChange = async (mode: string) => {
    setSyncMode(mode);
    await api.setSettings("sync_mode", mode);
  };

  const handleDefaultScenarioChange = async (id: string) => {
    setDefaultScenario(id);
    await api.setSettings("default_scenario", id);
  };

  const handleCloseActionChange = async (action: string) => {
    if (action === "hide" && !showTrayIcon) return;
    setCloseAction(action);
    await api.setSettings("close_action", action);
  };

  const handleShowTrayIconChange = async (enabled: boolean) => {
    setShowTrayIcon(enabled);
    await api.setSettings("show_tray_icon", enabled ? "true" : "false");
    if (!enabled && closeAction === "hide") {
      setCloseAction("close");
      await api.setSettings("close_action", "close");
    }
  };

  const handleDeveloperModeChange = async (enabled: boolean) => {
    setDeveloperMode(enabled);
    await api.setSettings("developer_mode", enabled ? "true" : "false");
  };

  const handleLanguageChange = (lng: string) => {
    localStorage.setItem("language", lng);
    i18n.changeLanguage(lng);
    api.setSettings("language", lng);
  };

  const handleTextSizeChange = (size: string) => {
    setTextSize(size);
    applyTextSize(size);
    api.setSettings("text_size", size);
  };

  const handleOpenRepoInFinder = async () => {
    try {
      setOpeningRepo(true);
      await api.openCentralRepoFolder();
    } catch (error) {
      console.error("Failed to open central repository folder", error);
      toast.error(t("common.error"));
    } finally {
      setOpeningRepo(false);
    }
  };

  const handleOpenGithub = async () => {
    try {
      setOpeningGithub(true);
      await openUrl(GITHUB_URL);
    } catch (error) {
      console.error("Failed to open GitHub repository", error);
      toast.error(t("common.error"));
    } finally {
      setOpeningGithub(false);
    }
  };

  const handleCheckUpdate = async () => {
    setCheckingUpdate(true);
    setUpdateInfo(null);
    try {
      const info = await api.checkAppUpdate();
      setUpdateInfo(info);
      if (info.has_update) {
        toast.info(t("settings.updateAvailable", { version: info.latest_version }));
      } else {
        toast.success(t("settings.noUpdate"));
      }
    } catch {
      toast.error(t("settings.updateError"));
    } finally {
      setCheckingUpdate(false);
    }
  };

  const handleAutoUpdate = async () => {
    setInstalling(true);
    try {
      const update = await checkUpdater();
      if (update) {
        toast.info(t("settings.installing"));
        await update.downloadAndInstall();
        toast.success(t("settings.restartToApply"));
      } else {
        toast.success(t("settings.noUpdate"));
      }
    } catch {
      toast.error(t("settings.updateError"));
      if (updateInfo?.release_url) {
        await openUrl(updateInfo.release_url);
      }
    } finally {
      setInstalling(false);
    }
  };

  const handleSaveSkillsmpApiKey = async () => {
    setSkillsmpSaving(true);
    try {
      await api.setSettings("skillsmp_api_key", skillsmpApiKey.trim());
      toast.success(t("common.success"));
    } catch {
      toast.error(t("common.error"));
    } finally {
      setSkillsmpSaving(false);
    }
  };

  const handleSaveGitRemote = async () => {
    setGitRemoteSaving(true);
    try {
      await api.setSettings("git_backup_remote_url", gitRemoteInput.trim());
      toast.success(t("settings.gitConfigSaved"));
    } catch {
      toast.error(t("common.error"));
    } finally {
      setGitRemoteSaving(false);
    }
  };

  const handleSaveProxy = async () => {
    const trimmed = proxyInput.trim();
    if (trimmed && !/^(https?|socks5):\/\//i.test(trimmed)) {
      toast.error(t("settings.proxyUrlInvalid"));
      return;
    }
    setProxySaving(true);
    try {
      await api.setSettings("proxy_url", trimmed);
      toast.success(t("settings.proxyUrlSaved"));
    } catch {
      toast.error(t("common.error"));
    } finally {
      setProxySaving(false);
    }
  };

  const fieldClass =
    "h-8 rounded-[4px] border border-border-subtle bg-background px-2.5 text-[13px] text-secondary outline-none transition-colors focus:border-border";
  const actionButtonClass =
    "inline-flex h-8 items-center gap-1.5 rounded-[4px] border px-2.5 text-[13px] font-medium transition-colors outline-none disabled:opacity-60";
  const segmentedButtonClass =
    "flex h-8 items-center gap-1.5 px-2.5 rounded-[3px] text-[13px] font-medium transition-colors outline-none";

  const themeOptions: Array<{ value: Theme; label: string; icon: typeof Sun }> = [
    { value: "light", label: t("settings.themeLight"), icon: Sun },
    { value: "dark", label: t("settings.themeDark"), icon: Moon },
    { value: "system", label: t("settings.themeSystem"), icon: Monitor },
  ];
  const displayedRepoPath = centralRepoPath
    ? centralRepoPath.replace(/\/Users\/[^/]+/, "~").replace(/\/home\/[^/]+/, "~").replace(/^[A-Za-z]:\\Users\\[^\\]+/, "~")
    : "~/.skills-manager/";

  return (
    <div className="app-page app-page-narrow">
      <div className="app-page-header">
        <h1 className="app-page-title flex items-center gap-2">
          <Settings2 className="w-4 h-4 text-accent" />
          {t("settings.title")}
        </h1>
      </div>

      <div className="space-y-6">
        {/* Agent status */}
        <section>
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <h2 className="app-section-title">
              {t("settings.supportedAgents")} ({tools.filter((t) => t.installed).length}/{tools.length})
            </h2>
            <div className="flex flex-wrap items-center gap-3">
              <button
                onClick={() => setShowAddCustom(true)}
                className="flex items-center gap-1 text-[13px] text-accent hover:text-accent-light transition-colors font-medium outline-none"
              >
                <Plus className="w-3.5 h-3.5" />
                {t("settings.addCustomAgent")}
              </button>
              <button
                onClick={() => handleToggleAllTools(true)}
                className="text-[13px] text-accent hover:text-accent-light transition-colors font-medium outline-none"
              >
                {t("settings.enableAll")}
              </button>
              <button
                onClick={() => handleToggleAllTools(false)}
                className="text-[13px] text-muted hover:text-secondary transition-colors font-medium outline-none"
              >
                {t("settings.disableAll")}
              </button>
              <button
                onClick={handleRefresh}
                disabled={refreshing}
                className="flex items-center gap-1.5 text-[13px] text-accent hover:text-accent-light transition-colors font-medium outline-none"
              >
                {refreshing ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                ) : (
                  <RefreshCw className="w-3.5 h-3.5" />
                )}
                {t("settings.refresh")}
              </button>
            </div>
          </div>

          {/* Add custom agent form */}
          {showAddCustom && (
            <div className="app-panel p-4 mb-3 space-y-2.5">
              <div className="flex items-center justify-between">
                <h3 className="text-[13px] font-medium text-secondary">{t("settings.addCustomAgent")}</h3>
                <button onClick={() => setShowAddCustom(false)} className="text-muted hover:text-secondary outline-none">
                  <X className="w-3.5 h-3.5" />
                </button>
              </div>
              <div>
                <label className="text-[12px] text-muted mb-1 block">{t("settings.agentName")}</label>
                <input
                  type="text"
                  value={customName}
                  onChange={(e) => setCustomName(e.target.value)}
                  placeholder={t("settings.agentNamePlaceholder")}
                  className={`${fieldClass} w-full`}
                />
              </div>
              <div>
                <label className="text-[12px] text-muted mb-1 block">{t("settings.skillsPath")}</label>
                <div className="flex flex-wrap items-center gap-2">
                  <input
                    type="text"
                    value={customPath}
                    onChange={(e) => setCustomPath(e.target.value)}
                    placeholder={t("settings.skillsPathPlaceholder")}
                    className={`${fieldClass} min-w-0 flex-1 font-mono`}
                  />
                  <button
                    onClick={() => handleBrowsePath(setCustomPath)}
                    className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
                  >
                    <FolderOpen className="w-3 h-3" />
                    {t("settings.selectFolder")}
                  </button>
                  <button
                    onClick={handleAddCustomAgent}
                    disabled={addingCustom || !customName.trim() || !customPath.trim()}
                    className={`${actionButtonClass} bg-accent text-white border-accent hover:opacity-90 disabled:opacity-50`}
                  >
                    {addingCustom ? <Loader2 className="w-3 h-3 animate-spin" /> : <Plus className="w-3 h-3" />}
                    {t("settings.addAgent")}
                  </button>
                </div>
              </div>
            </div>
          )}

          <div className="grid grid-cols-3 gap-2 md:grid-cols-4">
            {tools.map((agent, i) => (
              <div
                key={i}
                className={cn(
                  "group relative flex flex-col gap-1 p-2.5 rounded-[4px] border transition-colors",
                  agent.installed && agent.enabled
                    ? "bg-surface border-border-subtle hover:border-border"
                    : "bg-bg-secondary border-border-subtle opacity-50"
                )}
              >
                <div className="flex items-center gap-2">
                  {agent.installed ? (
                    <button
                      onClick={() => handleToggleTool(agent.key, !agent.enabled)}
                      disabled={togglingTools.has(agent.key)}
                      className="shrink-0 outline-none"
                      title={agent.enabled ? t("settings.disableAgent") : t("settings.enableAgent")}
                    >
                      {togglingTools.has(agent.key) ? (
                        <Loader2 className="w-3.5 h-3.5 animate-spin text-muted" />
                      ) : agent.enabled ? (
                        <CheckCircle2 className="w-3.5 h-3.5 text-emerald-500" />
                      ) : (
                        <Circle className="w-3.5 h-3.5 text-muted" />
                      )}
                    </button>
                  ) : (
                    <Circle className="w-3.5 h-3.5 text-faint shrink-0" />
                  )}
                  <h3 className={cn("text-[13px] font-medium truncate flex-1", agent.installed && agent.enabled ? "text-secondary" : "text-muted")}>
                    {agent.display_name}
                  </h3>
                  {/* Action buttons shown on hover */}
                  <div className="hidden group-hover:flex items-center gap-0.5 shrink-0">
                    {agent.has_path_override && !agent.is_custom && (
                      <button
                        onClick={() => handleResetPath(agent.key)}
                        className="p-0.5 text-muted hover:text-amber-500 outline-none"
                        title={t("settings.resetPath")}
                      >
                        <RotateCcw className="w-3 h-3" />
                      </button>
                    )}
                    <button
                      onClick={() => startEditPath(agent.key, agent.skills_dir)}
                      className="p-0.5 text-muted hover:text-accent outline-none"
                      title={t("settings.editPath")}
                    >
                      <Pencil className="w-3 h-3" />
                    </button>
                    {agent.is_custom && (
                      <button
                        onClick={() => handleRemoveCustomAgent(agent.key, agent.display_name)}
                        className="p-0.5 text-muted hover:text-red-500 outline-none"
                        title={t("settings.removeCustomAgent")}
                      >
                        <Trash2 className="w-3 h-3" />
                      </button>
                    )}
                  </div>
                </div>

                {/* Inline path editing */}
                {editingPathKey === agent.key ? (
                  <div className="flex items-center gap-1 mt-0.5">
                    <input
                      type="text"
                      value={editingPathValue}
                      onChange={(e) => setEditingPathValue(e.target.value)}
                      className="h-6 flex-1 rounded border border-border-subtle bg-background px-1.5 text-[12px] font-mono text-secondary outline-none focus:border-accent min-w-0"
                      autoFocus
                      onKeyDown={(e) => {
                        if (e.key === "Enter") handleSavePath();
                        if (e.key === "Escape") setEditingPathKey(null);
                      }}
                    />
                    <button
                      onClick={() => handleBrowsePath(setEditingPathValue)}
                      className="p-0.5 text-muted hover:text-accent outline-none shrink-0"
                      title={t("settings.selectFolder")}
                    >
                      <FolderOpen className="w-3 h-3" />
                    </button>
                    <button onClick={handleSavePath} className="p-0.5 text-emerald-500 hover:text-emerald-400 outline-none shrink-0">
                      <Check className="w-3 h-3" />
                    </button>
                    <button onClick={() => setEditingPathKey(null)} className="p-0.5 text-muted hover:text-secondary outline-none shrink-0">
                      <X className="w-3 h-3" />
                    </button>
                  </div>
                ) : (
                  <p className="text-[12px] text-muted truncate" title={agent.skills_dir}>
                    {agent.is_custom && (
                      <span className="text-[10px] text-amber-500 font-medium mr-1">{t("settings.customAgent")}</span>
                    )}
                    {agent.has_path_override && !agent.is_custom && (
                      <span className="text-[10px] text-amber-500 font-medium mr-1">{t("settings.pathOverridden")}</span>
                    )}
                    {agent.installed
                      ? agent.skills_dir.replace(/\/Users\/[^/]+/, "~").replace(/\/home\/[^/]+/, "~").replace(/^[A-Za-z]:\\Users\\[^\\]+/, "~")
                      : t("settings.notInstalled")}
                  </p>
                )}
              </div>
            ))}
          </div>
        </section>

        {/* Global config */}
        <section>
          <h2 className="app-section-title mb-3">
            {t("settings.globalConfig")}
          </h2>
          <div className="app-panel overflow-hidden divide-y divide-border-subtle">
            {/* Repo path */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.repoPath")}</h3>
                <p className="text-[13px] text-muted">{t("settings.repoPathDesc")}</p>
              </div>
              <div className="flex max-w-full flex-wrap items-center gap-2">
                <div className="flex min-w-0 items-center gap-1.5 rounded-[4px] border border-border-subtle bg-background px-2 py-1">
                  <Folder className="w-3 h-3 text-muted" />
                  <span className="truncate text-[13px] font-mono text-tertiary">{displayedRepoPath}</span>
                </div>
                <button
                  type="button"
                  onClick={handleOpenRepoInFinder}
                  disabled={openingRepo}
                  className={cn(
                    "inline-flex h-8 items-center gap-1 rounded-[4px] border px-2.5 text-[13px] font-medium transition-all outline-none",
                    "border-accent-border bg-accent-bg text-accent",
                    "hover:border-accent hover:bg-accent-bg",
                    openingRepo && "cursor-wait opacity-70"
                  )}
                >
                  {openingRepo ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <ExternalLink className="w-3 h-3" />
                  )}
                  {t("settings.openInFinder")}
                </button>
              </div>
            </div>

            {/* Sync mode */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.syncMode")}</h3>
                <p className="text-[13px] text-muted">{t("settings.syncModeDesc")}</p>
              </div>
              <div className="flex flex-wrap rounded-[4px] border border-border-subtle bg-background p-px">
                <button
                  onClick={() => handleSyncModeChange("symlink")}
                  className={cn(
                    segmentedButtonClass,
                    syncMode === "symlink" ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                  )}
                >
                  <LinkIcon className="w-3 h-3" /> {t("settings.symlink")}
                </button>
                <button
                  onClick={() => handleSyncModeChange("copy")}
                  className={cn(
                    segmentedButtonClass,
                    syncMode === "copy" ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                  )}
                >
                  <Copy className="w-3 h-3" /> {t("settings.copy")}
                </button>
              </div>
            </div>

            {/* Sync scope */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.syncScope")}</h3>
                <p className="text-[13px] text-muted">{t("settings.syncScopeDesc")}</p>
              </div>
              <div className="flex flex-wrap rounded-[4px] border border-border-subtle bg-background p-px">
                <button
                  onClick={async () => {
                    setSyncScope("scenario");
                    await api.setSkillSyncScope("scenario");
                  }}
                  className={cn(
                    segmentedButtonClass,
                    syncScope === "scenario" ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                  )}
                >
                  <Layers className="w-3 h-3" /> {t("settings.syncScope_scenario")}
                </button>
                <button
                  onClick={async () => {
                    setSyncScope("global");
                    await api.setSkillSyncScope("global");
                  }}
                  className={cn(
                    segmentedButtonClass,
                    syncScope === "global" ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                  )}
                >
                  <Globe className="w-3 h-3" /> {t("settings.syncScope_global")}
                </button>
              </div>
            </div>

            {/* Theme */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.theme")}</h3>
                <p className="text-[13px] text-muted">{t("settings.themeDesc")}</p>
              </div>
              <div className="flex flex-wrap rounded-[4px] border border-border-subtle bg-background p-px">
                {themeOptions.map((opt) => {
                  const Icon = opt.icon;
                  return (
                    <button
                      key={opt.value}
                      onClick={() => setTheme(opt.value)}
                      className={cn(
                        segmentedButtonClass,
                        theme === opt.value ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                      )}
                    >
                      <Icon className="w-3 h-3" /> {opt.label}
                    </button>
                  );
                })}
              </div>
            </div>

            {/* Text size */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.textSize")}</h3>
                <p className="text-[13px] text-muted">{t("settings.textSizeDesc")}</p>
              </div>
              <div className="flex flex-wrap rounded-[4px] border border-border-subtle bg-background p-px">
                {([
                  { value: "small", label: t("settings.textSizeSmall") },
                  { value: "default", label: t("settings.textSizeDefault") },
                  { value: "large", label: t("settings.textSizeLarge") },
                  { value: "xlarge", label: t("settings.textSizeXLarge") },
                ] as const).map((opt) => (
                  <button
                    key={opt.value}
                    onClick={() => handleTextSizeChange(opt.value)}
                    className={cn(
                      segmentedButtonClass,
                      textSize === opt.value ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                    )}
                  >
                    {opt.value === "small" && <Type className="w-2.5 h-2.5" />}
                    {opt.value === "default" && <Type className="w-3 h-3" />}
                    {opt.value === "large" && <Type className="w-3.5 h-3.5" />}
                    {opt.value === "xlarge" && <Type className="w-4 h-4" />}
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>

            {/* Default scenario */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.defaultScenario")}</h3>
                <p className="text-[13px] text-muted">{t("settings.defaultScenarioDesc")}</p>
              </div>
              <select
                value={defaultScenario}
                onChange={(e) => handleDefaultScenarioChange(e.target.value)}
                className={fieldClass}
              >
                <option value="">—</option>
                {scenarios.map((s) => (
                  <option key={s.id} value={s.id}>{s.name}</option>
                ))}
              </select>
            </div>

            {/* Language */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <h3 className="text-[13px] text-secondary font-medium">{t("settings.language")}</h3>
              </div>
              <div className="flex max-w-full flex-wrap items-center gap-2">
                <Globe className="w-3.5 h-3.5 text-muted" />
                <select
                  value={i18n.language}
                  onChange={(e) => handleLanguageChange(e.target.value)}
                  className={fieldClass}
                >
                  <option value="zh">简体中文 (zh-CN)</option>
                  <option value="zh-TW">繁體中文 (zh-TW)</option>
                  <option value="en">English (en-US)</option>
                </select>
              </div>
            </div>

            {/* Close action */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.closeAction")}</h3>
                <p className="text-[13px] text-muted">{t("settings.closeActionDesc")}</p>
                {!showTrayIcon && (
                  <p className="text-[12px] text-muted mt-1">{t("settings.trayIconOffHint")}</p>
                )}
              </div>
              <div className="flex flex-wrap rounded-[4px] border border-border-subtle bg-background p-px">
                {(["", "hide", "close"] as const).map((val) => (
                  <button
                    key={val}
                    onClick={() => handleCloseActionChange(val)}
                    disabled={val === "hide" && !showTrayIcon}
                    className={cn(
                      segmentedButtonClass,
                      closeAction === val ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary",
                      val === "hide" && !showTrayIcon && "opacity-50 cursor-not-allowed hover:text-muted"
                    )}
                  >
                    {t(`settings.closeAction_${val || "ask"}`)}
                  </button>
                ))}
              </div>
            </div>

            {/* Tray icon */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.trayIcon")}</h3>
                <p className="text-[13px] text-muted">{t("settings.trayIconDesc")}</p>
              </div>
              <div className="flex flex-wrap rounded-[4px] border border-border-subtle bg-background p-px">
                <button
                  onClick={() => handleShowTrayIconChange(true)}
                  className={cn(
                    segmentedButtonClass,
                    showTrayIcon ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                  )}
                >
                  {t("settings.trayIcon_on")}
                </button>
                <button
                  onClick={() => handleShowTrayIconChange(false)}
                  className={cn(
                    segmentedButtonClass,
                    !showTrayIcon ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                  )}
                >
                  {t("settings.trayIcon_off")}
                </button>
              </div>
            </div>

            {/* Developer mode */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.developerMode")}</h3>
                <p className="text-[13px] text-muted">{t("settings.developerModeDesc")}</p>
              </div>
              <div className="flex flex-wrap rounded-[4px] border border-border-subtle bg-background p-px">
                <button
                  onClick={() => handleDeveloperModeChange(true)}
                  className={cn(
                    segmentedButtonClass,
                    developerMode ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                  )}
                >
                  {t("settings.developerMode_on")}
                </button>
                <button
                  onClick={() => handleDeveloperModeChange(false)}
                  className={cn(
                    segmentedButtonClass,
                    !developerMode ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                  )}
                >
                  {t("settings.developerMode_off")}
                </button>
              </div>
            </div>
          </div>
        </section>

        {/* Proxy config */}
        <section>
          <h2 className="app-section-title mb-3">
            {t("settings.proxyConfig")}
          </h2>
          <div className="app-panel overflow-hidden divide-y divide-border-subtle">
            <div className="px-4 py-3">
              <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.proxyUrl")}</h3>
              <p className="text-[13px] text-muted mb-2">{t("settings.proxyUrlDesc")}</p>
              <div className="flex flex-wrap items-center gap-2">
                <input
                  type="text"
                  value={proxyInput}
                  onChange={(e) => setProxyInput(e.target.value)}
                  placeholder={t("settings.proxyUrlPlaceholder")}
                  className={`${fieldClass} min-w-0 flex-1 font-mono`}
                />
                <button
                  onClick={handleSaveProxy}
                  disabled={proxySaving}
                  className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
                >
                  {proxySaving ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <LinkIcon className="w-3 h-3" />
                  )}
                  {t("common.save")}
                </button>
              </div>
            </div>
          </div>
        </section>

        {/* SkillsMP API Key */}
        <section>
          <h2 className="app-section-title mb-3">
            {t("settings.skillsmpTitle", { defaultValue: "SkillsMP AI Search" })}
          </h2>
          <div className="app-panel overflow-hidden divide-y divide-border-subtle">
            <div className="px-4 py-3">
              <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.skillsmpApiKey", { defaultValue: "API Key" })}</h3>
              <p className="text-[13px] text-muted mb-2">
                {t("settings.skillsmpDesc", { defaultValue: "Enter your SkillsMP API key to enable AI-powered skill search." })}{" "}
                <button
                  type="button"
                  onClick={() => openUrl("https://skillsmp.com/docs/api")}
                  className="inline-flex items-center gap-0.5 text-accent-light hover:underline"
                >
                  {t("settings.skillsmpGetKey", { defaultValue: "Get your API key" })}
                  <ExternalLink className="h-3 w-3" />
                </button>
              </p>
              <div className="flex flex-wrap items-center gap-2">
                <input
                  type="password"
                  value={skillsmpApiKey}
                  onChange={(e) => setSkillsmpApiKey(e.target.value)}
                  placeholder="sk_live_..."
                  className={`${fieldClass} min-w-0 flex-1 font-mono`}
                />
                <button
                  onClick={handleSaveSkillsmpApiKey}
                  disabled={skillsmpSaving}
                  className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
                >
                  {skillsmpSaving ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <Key className="w-3 h-3" />
                  )}
                  {t("common.save")}
                </button>
              </div>
            </div>
          </div>
        </section>

        {/* Git sync config */}
        <section>
          <h2 className="app-section-title mb-3">
            {t("settings.gitSyncConfig")}
          </h2>
          <div className="app-panel overflow-hidden divide-y divide-border-subtle">
            <div className="px-4 py-3">
              <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.gitRemoteUrl")}</h3>
              <p className="text-[13px] text-muted mb-2">{t("settings.gitSyncConfigDesc")}</p>
              <div className="flex flex-wrap items-center gap-2">
                <input
                  type="text"
                  value={gitRemoteInput}
                  onChange={(e) => setGitRemoteInput(e.target.value)}
                  placeholder={t("settings.gitRemoteUrlPlaceholder")}
                  className={`${fieldClass} min-w-0 flex-1 font-mono`}
                />
                <button
                  onClick={handleSaveGitRemote}
                  disabled={gitRemoteSaving}
                  className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
                >
                  {gitRemoteSaving ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <LinkIcon className="w-3 h-3" />
                  )}
                  {t("common.save")}
                </button>
              </div>
            </div>
          </div>
        </section>

        {/* About */}
        <section>
          <div className="app-panel flex flex-wrap items-start justify-between gap-3 p-4">
            <div className="flex min-w-0 flex-1 items-center gap-3">
              <div className="w-8 h-8 rounded-lg bg-surface-hover border border-border flex items-center justify-center">
                <Settings2 className="w-4 h-4 text-accent" />
              </div>
              <div>
                <h3 className="text-[13px] font-semibold text-primary">{t("settings.version")}</h3>
                <p className="text-muted text-[13px]">
                  {t("settings.tagline")}
                  {updateInfo?.has_update && (
                    <span className="ml-2 text-amber-500 font-medium">
                      {t("settings.updateAvailable", { version: updateInfo.latest_version })}
                    </span>
                  )}
                </p>
              </div>
            </div>
            <div className="flex flex-wrap gap-2">
              {updateInfo?.has_update ? (
                IS_WINDOWS ? (
                  <>
                    <button
                      type="button"
                      onClick={handleAutoUpdate}
                      disabled={installing}
                      className={`${actionButtonClass} bg-accent text-white border-accent hover:opacity-90`}
                    >
                      {installing ? (
                        <Loader2 className="w-3 h-3 animate-spin" />
                      ) : (
                        <Download className="w-3 h-3" />
                      )}
                      {installing ? t("settings.installing") : t("settings.installUpdate")}
                    </button>
                    <button
                      type="button"
                      onClick={() => { openUrl(updateInfo.release_url).catch(() => {}); }}
                      className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
                    >
                      <ExternalLink className="w-3 h-3" /> {t("settings.download")}
                    </button>
                  </>
                ) : (
                  <button
                    type="button"
                    onClick={() => { openUrl(updateInfo.release_url).catch(() => {}); }}
                    className={`${actionButtonClass} bg-accent text-white border-accent hover:opacity-90`}
                  >
                    <Download className="w-3 h-3" /> {t("settings.download")}
                  </button>
                )
              ) : (
                <button
                  type="button"
                  onClick={handleCheckUpdate}
                  disabled={checkingUpdate}
                  className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
                >
                  {checkingUpdate ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <RefreshCw className="w-3 h-3" />
                  )}
                  {checkingUpdate ? t("settings.checking") : t("settings.checkUpdate")}
                </button>
              )}
              <button
                type="button"
                onClick={openHelp}
                className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
              >
                <BookOpen className="w-3 h-3" /> {t("settings.help")}
              </button>
              <button
                type="button"
                onClick={handleOpenGithub}
                disabled={openingGithub}
                className={`${actionButtonClass} bg-surface-hover hover:bg-surface-active text-tertiary border-border`}
              >
                <Github className="w-3 h-3" /> GitHub
              </button>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}
