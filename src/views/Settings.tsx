import { useState, useEffect, useCallback, useMemo } from "react";
import {
  Folder,
  FolderOpen,
  RefreshCw,
  CheckCircle2,
  Circle,
  Globe,
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
  ChevronDown,
  ChevronRight,
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
import { applyTextSize } from "../lib/textScale";
import type { AppUpdateInfo } from "../lib/tauri";
import type { Theme } from "../hooks/useTheme";

const IS_WINDOWS = navigator.userAgent.includes("Windows");

const MAINSTREAM_AGENT_KEYS = new Set([
  "claude_code",
  "cursor",
  "codex",
  "gemini_cli",
  "github_copilot",
  "opencode",
  "windsurf",
  "kiro",
  "antigravity",
  "amp",
]);

function compactHomePath(path: string) {
  return path
    .replace(/\/Users\/[^/]+/, "~")
    .replace(/\/home\/[^/]+/, "~")
    .replace(/^[A-Za-z]:\\Users\\[^\\]+/, "~");
}

export function Settings() {
  const { t, i18n } = useTranslation();
  const { tools, scenarios, refreshTools, openHelp } = useApp();
  const [togglingTools, setTogglingTools] = useState<Set<string>>(new Set());
  const { theme, setTheme } = useThemeContext();
  const [syncMode, setSyncMode] = useState("symlink");
  const [defaultScenario, setDefaultScenario] = useState("");
  const [closeAction, setCloseAction] = useState("");
  const [showTrayIcon, setShowTrayIcon] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [openingRepo, setOpeningRepo] = useState(false);
  const [openingGithub, setOpeningGithub] = useState(false);
  const [centralRepoPath, setCentralRepoPath] = useState("");
  const [centralRepoPathOverride, setCentralRepoPathOverride] = useState<string | null>(null);
  const [editingCentralRepoPath, setEditingCentralRepoPath] = useState(false);
  const [centralRepoPathInput, setCentralRepoPathInput] = useState("");
  const [savingCentralRepoPath, setSavingCentralRepoPath] = useState(false);
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
  const [customProjectPath, setCustomProjectPath] = useState("");
  const [addingCustom, setAddingCustom] = useState(false);
  const [showMoreAgents, setShowMoreAgents] = useState(false);

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
    const trimProjectPath = customProjectPath.trim();
    if (!trimName || !trimPath) return;
    const trimKey = generateCustomAgentKey(trimName);
    setAddingCustom(true);
    try {
      await api.addCustomTool(trimKey, trimName, trimPath, trimProjectPath || undefined);
      await refreshTools();
      toast.success(t("settings.customAgentAdded"));
      setShowAddCustom(false);
      setCustomName("");
      setCustomPath("");
      setCustomProjectPath("");
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
    api.getSettings("default_scenario").then((v) => { if (v) setDefaultScenario(v); });
    api.getSettings("proxy_url").then((v) => { setProxyInput(v ?? ""); });
    api.getSettings("close_action").then((v) => { setCloseAction(v ?? ""); });
    api.getSettings("show_tray_icon").then((v) => {
      const normalized = (v ?? "true").trim().toLowerCase();
      setShowTrayIcon(!(normalized === "false" || normalized === "0" || normalized === "no" || normalized === "off"));
    });
    api.getSettings("text_size").then((v) => { if (v) { setTextSize(v); applyTextSize(v); } });
    api.getSettings("skillsmp_api_key").then((v) => { if (v) setSkillsmpApiKey(v); });
    api.getCentralRepoPath().then((path) => {
      setCentralRepoPath(path);
      setCentralRepoPathInput(path);
    }).catch(() => {});
    api.getCentralRepoPathOverride().then(setCentralRepoPathOverride).catch(() => {});

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

  const handleStartEditCentralRepoPath = () => {
    setCentralRepoPathInput(centralRepoPathOverride ?? centralRepoPath);
    setEditingCentralRepoPath(true);
  };

  const handleSaveCentralRepoPath = async () => {
    const trimmed = centralRepoPathInput.trim();
    if (!trimmed) {
      toast.error(t("settings.repoPathEmpty"));
      return;
    }
    setSavingCentralRepoPath(true);
    try {
      const nextPath = await api.setCentralRepoPath(trimmed);
      setCentralRepoPath(nextPath);
      setCentralRepoPathOverride(nextPath);
      setEditingCentralRepoPath(false);
      toast.success(t("settings.repoPathSaved"));
      toast.info(t("settings.repoPathRestartNotice"));
    } catch (error) {
      toast.error(String(error));
    } finally {
      setSavingCentralRepoPath(false);
    }
  };

  const handleResetCentralRepoPath = async () => {
    setSavingCentralRepoPath(true);
    try {
      const nextPath = await api.setCentralRepoPath(null);
      setCentralRepoPath(nextPath);
      setCentralRepoPathOverride(null);
      setCentralRepoPathInput(nextPath);
      setEditingCentralRepoPath(false);
      toast.success(t("settings.repoPathReset"));
      toast.info(t("settings.repoPathRestartNotice"));
    } catch (error) {
      toast.error(String(error));
    } finally {
      setSavingCentralRepoPath(false);
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
  const selectClass = `${fieldClass} min-w-[180px] appearance-none pr-8`;
  const actionButtonClass =
    "inline-flex h-8 items-center gap-1.5 rounded-[4px] border px-2.5 text-[13px] font-medium transition-colors outline-none disabled:opacity-60";
  const segmentedButtonClass =
    "flex h-8 items-center gap-1.5 px-2.5 rounded-[3px] text-[13px] font-medium transition-colors outline-none";

  const themeOptions: Array<{ value: Theme; label: string; icon: typeof Sun }> = [
    { value: "light", label: t("settings.themeLight"), icon: Sun },
    { value: "dark", label: t("settings.themeDark"), icon: Moon },
    { value: "system", label: t("settings.themeSystem"), icon: Monitor },
  ];
  const installedTools = useMemo(() => tools.filter((tool) => tool.installed), [tools]);
  const enabledTools = useMemo(
    () => installedTools.filter((tool) => tool.enabled),
    [installedTools]
  );
  const customTools = useMemo(() => tools.filter((tool) => tool.is_custom), [tools]);
  const builtInTools = useMemo(() => tools.filter((tool) => !tool.is_custom), [tools]);
  const sortTools = useCallback((items: typeof tools) => {
    return [...items].sort((a, b) => {
      const installRank = Number(b.installed) - Number(a.installed);
      if (installRank !== 0) return installRank;
      const enabledRank = Number(b.enabled) - Number(a.enabled);
      if (enabledRank !== 0) return enabledRank;
      return a.display_name.localeCompare(b.display_name);
    });
  }, []);
  const displayedBuiltInTools = useMemo(() => sortTools(builtInTools), [builtInTools, sortTools]);
  const displayedCustomTools = useMemo(() => sortTools(customTools), [customTools, sortTools]);
  const mainstreamTools = useMemo(
    () => displayedBuiltInTools.filter((tool) => MAINSTREAM_AGENT_KEYS.has(tool.key)),
    [displayedBuiltInTools]
  );
  const secondaryTools = useMemo(
    () => displayedBuiltInTools.filter((tool) => !MAINSTREAM_AGENT_KEYS.has(tool.key)),
    [displayedBuiltInTools]
  );
  const displayedRepoPath = centralRepoPath
    ? compactHomePath(centralRepoPath)
    : t("common.loading");

  const renderAgentCard = (agent: typeof tools[number]) => (
    <div
      key={agent.key}
      className={cn(
        "group relative flex flex-col gap-1.5 rounded-[6px] border px-3 py-2.5 transition-colors",
        agent.installed && agent.enabled
          ? "border-border bg-surface"
          : agent.installed
            ? "border-border-subtle bg-surface"
            : "border-border-subtle bg-bg-secondary"
      )}
    >
      <div className="flex items-start gap-2">
        <div className="mt-0.5 shrink-0">
          {agent.installed ? (
            <button
              onClick={() => handleToggleTool(agent.key, !agent.enabled)}
              disabled={togglingTools.has(agent.key)}
              className="shrink-0 outline-none"
              title={agent.enabled ? t("settings.disableAgent") : t("settings.enableAgent")}
            >
              {togglingTools.has(agent.key) ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin text-muted" />
              ) : agent.enabled ? (
                <CheckCircle2 className="h-3.5 w-3.5 text-emerald-500" />
              ) : (
                <Circle className="h-3.5 w-3.5 text-amber-500" />
              )}
            </button>
          ) : (
            <Circle className="h-3.5 w-3.5 text-faint" />
          )}
        </div>

        <div className="min-w-0 flex-1">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <h3 className={cn("truncate text-[13px] font-medium", agent.installed ? "text-secondary" : "text-muted")}>
                  {agent.display_name}
                </h3>
                <span
                  className={cn(
                    "shrink-0 rounded-full px-2 py-0.5 text-[10px] font-medium",
                    agent.installed
                      ? agent.enabled
                        ? "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
                        : "bg-amber-500/10 text-amber-700 dark:text-amber-300"
                      : "bg-surface-hover text-muted"
                  )}
                >
                  {agent.installed
                    ? agent.enabled
                      ? t("settings.enabledState")
                      : t("settings.disabledState")
                    : t("settings.notInstalled")}
                </span>
              </div>
            </div>
            <div className="hidden shrink-0 items-center gap-0.5 group-hover:flex">
              {agent.has_path_override && !agent.is_custom && (
                <button
                  onClick={() => handleResetPath(agent.key)}
                  className="p-0.5 text-muted hover:text-amber-500 outline-none"
                  title={t("settings.resetPath")}
                >
                  <RotateCcw className="h-3 w-3" />
                </button>
              )}
              <button
                onClick={() => startEditPath(agent.key, agent.skills_dir)}
                className="p-0.5 text-muted hover:text-accent outline-none"
                title={t("settings.editPath")}
              >
                <Pencil className="h-3 w-3" />
              </button>
              {agent.is_custom && (
                <button
                  onClick={() => handleRemoveCustomAgent(agent.key, agent.display_name)}
                  className="p-0.5 text-muted hover:text-red-500 outline-none"
                  title={t("settings.removeCustomAgent")}
                >
                  <Trash2 className="h-3 w-3" />
                </button>
              )}
            </div>
          </div>

          <div className="mt-0.5 flex flex-wrap items-center gap-1">
            {agent.is_custom && (
              <span className="rounded-full bg-sky-500/10 px-2 py-0.5 text-[10px] font-medium text-sky-700 dark:text-sky-300">
                {t("settings.customAgent")}
              </span>
            )}
            {agent.is_custom && agent.project_relative_skills_dir && (
              <span className="rounded-full bg-emerald-500/10 px-2 py-0.5 text-[10px] font-medium text-emerald-700 dark:text-emerald-300">
                {t("settings.projectAgentSupported")}
              </span>
            )}
            {agent.has_path_override && !agent.is_custom && (
              <span className="rounded-full bg-amber-500/10 px-2 py-0.5 text-[10px] font-medium text-amber-700 dark:text-amber-300">
                {t("settings.pathOverridden")}
              </span>
            )}
          </div>
        </div>
      </div>

      {editingPathKey === agent.key ? (
        <div className="flex items-center gap-1">
          <input
            type="text"
            value={editingPathValue}
            onChange={(e) => setEditingPathValue(e.target.value)}
            className="h-7 min-w-0 flex-1 rounded border border-border-subtle bg-background px-1.5 text-[12px] font-mono text-secondary outline-none focus:border-accent"
            autoFocus
            onKeyDown={(e) => {
              if (e.key === "Enter") handleSavePath();
              if (e.key === "Escape") setEditingPathKey(null);
            }}
          />
          <button
            onClick={() => handleBrowsePath(setEditingPathValue)}
            className="shrink-0 p-1 text-muted hover:text-accent outline-none"
            title={t("settings.selectFolder")}
          >
            <FolderOpen className="h-3 w-3" />
          </button>
          <button
            onClick={handleSavePath}
            className="shrink-0 p-1 text-emerald-500 hover:text-emerald-400 outline-none"
          >
            <Check className="h-3 w-3" />
          </button>
          <button
            onClick={() => setEditingPathKey(null)}
            className="shrink-0 p-1 text-muted hover:text-secondary outline-none"
          >
            <X className="h-3 w-3" />
          </button>
        </div>
      ) : (
        <div className="space-y-1">
          <p className="truncate text-[12px] font-mono leading-tight text-muted" title={agent.skills_dir}>
            {agent.installed ? compactHomePath(agent.skills_dir) : t("settings.notInstalled")}
          </p>
          {agent.is_custom && agent.project_relative_skills_dir && (
            <p
              className="truncate text-[12px] font-mono leading-tight text-muted"
              title={agent.project_relative_skills_dir}
            >
              {t("settings.projectSkillsPathValue", { path: agent.project_relative_skills_dir })}
            </p>
          )}
        </div>
      )}
    </div>
  );

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
            <div>
              <h2 className="app-section-title">
                {t("settings.supportedAgents")} ({installedTools.length}/{tools.length})
              </h2>
            </div>
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

          <div className="mb-3 flex flex-wrap items-center gap-3 text-[13px] text-muted">
            <span>{t("settings.detectedAgents")} <span className="font-medium text-secondary">{installedTools.length}</span></span>
            <span>{t("settings.enabledAgents")} <span className="font-medium text-secondary">{enabledTools.length}</span></span>
            <span>{t("settings.customAgents")} <span className="font-medium text-secondary">{customTools.length}</span></span>
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
                </div>
              </div>
              <div>
                <label className="text-[12px] text-muted mb-1 block">
                  {t("settings.projectSkillsPath")}
                </label>
                <input
                  type="text"
                  value={customProjectPath}
                  onChange={(e) => setCustomProjectPath(e.target.value)}
                  placeholder={t("settings.projectSkillsPathPlaceholder")}
                  className={`${fieldClass} w-full font-mono`}
                />
                <p className="mt-1 text-[12px] text-muted">
                  {t("settings.projectSkillsPathDesc")}
                </p>
              </div>
              <div className="flex justify-end">
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
          )}

          <div className="space-y-4">
            <div>
              <div className="mb-2 flex items-center justify-between gap-2">
                <h3 className="text-[13px] font-medium text-secondary">{t("settings.builtInAgents")}</h3>
                <span className="text-[12px] text-muted">{mainstreamTools.length}</span>
              </div>
              <div className="grid grid-cols-1 gap-1.5 md:grid-cols-2 xl:grid-cols-3">
                {mainstreamTools.map(renderAgentCard)}
              </div>
            </div>

            {secondaryTools.length > 0 && (
              <div>
                <button
                  type="button"
                  onClick={() => setShowMoreAgents((value) => !value)}
                  className="mb-2 inline-flex items-center gap-1.5 text-[13px] font-medium text-muted transition-colors hover:text-secondary outline-none"
                >
                  {showMoreAgents ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
                  {t("settings.moreAgentsSection", { count: secondaryTools.length })}
                </button>
                {showMoreAgents && (
                  <div className="grid grid-cols-1 gap-1.5 md:grid-cols-2 xl:grid-cols-3">
                    {secondaryTools.map(renderAgentCard)}
                  </div>
                )}
              </div>
            )}

            {displayedCustomTools.length > 0 && (
              <div>
                <div className="mb-2 flex items-center justify-between gap-2">
                  <h3 className="text-[13px] font-medium text-secondary">{t("settings.customAgentsSection")}</h3>
                  <span className="text-[12px] text-muted">{displayedCustomTools.length}</span>
                </div>
                <div className="grid grid-cols-1 gap-1.5 md:grid-cols-2 xl:grid-cols-3">
                  {displayedCustomTools.map(renderAgentCard)}
                </div>
              </div>
            )}
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
                {editingCentralRepoPath ? (
                  <div className="flex min-w-[320px] max-w-full items-center gap-1">
                    <input
                      type="text"
                      value={centralRepoPathInput}
                      onChange={(e) => setCentralRepoPathInput(e.target.value)}
                      className="h-8 min-w-0 flex-1 rounded-[4px] border border-border-subtle bg-background px-2.5 text-[13px] font-mono text-secondary outline-none transition-colors focus:border-border"
                      autoFocus
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void handleSaveCentralRepoPath();
                        if (e.key === "Escape") {
                          setCentralRepoPathInput(centralRepoPathOverride ?? centralRepoPath);
                          setEditingCentralRepoPath(false);
                        }
                      }}
                    />
                    <button
                      type="button"
                      onClick={() => handleBrowsePath(setCentralRepoPathInput)}
                      disabled={savingCentralRepoPath}
                      className="inline-flex h-8 items-center gap-1 rounded-[4px] border border-border-subtle px-2.5 text-[13px] font-medium text-muted transition-colors outline-none hover:text-secondary disabled:opacity-60"
                    >
                      <FolderOpen className="w-3 h-3" />
                      {t("settings.selectFolder")}
                    </button>
                    <button
                      type="button"
                      onClick={() => void handleSaveCentralRepoPath()}
                      disabled={savingCentralRepoPath}
                      className="inline-flex h-8 items-center gap-1 rounded-[4px] border border-emerald-500/30 px-2.5 text-[13px] font-medium text-emerald-600 transition-colors outline-none hover:bg-emerald-500/5 disabled:opacity-60"
                    >
                      {savingCentralRepoPath ? (
                        <Loader2 className="w-3 h-3 animate-spin" />
                      ) : (
                        <Check className="w-3 h-3" />
                      )}
                      {t("common.save")}
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setCentralRepoPathInput(centralRepoPathOverride ?? centralRepoPath);
                        setEditingCentralRepoPath(false);
                      }}
                      disabled={savingCentralRepoPath}
                      className="inline-flex h-8 items-center gap-1 rounded-[4px] border border-border-subtle px-2.5 text-[13px] font-medium text-muted transition-colors outline-none hover:text-secondary disabled:opacity-60"
                    >
                      <X className="w-3 h-3" />
                    </button>
                  </div>
                ) : (
                  <div className="flex min-w-0 items-center gap-1.5 rounded-[4px] border border-border-subtle bg-background px-2 py-1">
                    <Folder className="w-3 h-3 text-muted" />
                    <span className="truncate text-[13px] font-mono text-tertiary">{displayedRepoPath}</span>
                  </div>
                )}
                {!editingCentralRepoPath && (
                  <button
                    type="button"
                    onClick={handleStartEditCentralRepoPath}
                    className="inline-flex h-8 items-center gap-1 rounded-[4px] border border-border-subtle px-2.5 text-[13px] font-medium text-muted transition-colors outline-none hover:text-secondary"
                  >
                    <Pencil className="w-3 h-3" />
                    {t("settings.changeDir")}
                  </button>
                )}
                {!editingCentralRepoPath && centralRepoPathOverride && (
                  <button
                    type="button"
                    onClick={() => void handleResetCentralRepoPath()}
                    disabled={savingCentralRepoPath}
                    className="inline-flex h-8 items-center gap-1 rounded-[4px] border border-border-subtle px-2.5 text-[13px] font-medium text-muted transition-colors outline-none hover:text-secondary disabled:opacity-60"
                  >
                    {savingCentralRepoPath ? (
                      <Loader2 className="w-3 h-3 animate-spin" />
                    ) : (
                      <RotateCcw className="w-3 h-3" />
                    )}
                    {t("settings.resetPath")}
                  </button>
                )}
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
              <div className="w-full text-[12px] text-muted">
                {centralRepoPathOverride
                  ? t("settings.repoPathCustomHint")
                  : t("settings.repoPathDefaultHint")}
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
              <div className="relative shrink-0">
                <select
                  value={defaultScenario}
                  onChange={(e) => handleDefaultScenarioChange(e.target.value)}
                  className={selectClass}
                >
                  <option value="">—</option>
                  {scenarios.map((s) => (
                    <option key={s.id} value={s.id}>{s.name}</option>
                  ))}
                </select>
                <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
              </div>
            </div>

            {/* Language */}
            <div className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <h3 className="text-[13px] text-secondary font-medium">{t("settings.language")}</h3>
              </div>
              <div className="flex max-w-full flex-wrap items-center gap-2">
                <Globe className="w-3.5 h-3.5 text-muted" />
                <div className="relative">
                  <select
                    value={i18n.language}
                    onChange={(e) => handleLanguageChange(e.target.value)}
                    className={selectClass}
                  >
                    <option value="zh">简体中文 (zh-CN)</option>
                    <option value="zh-TW">繁體中文 (zh-TW)</option>
                    <option value="en">English (en-US)</option>
                  </select>
                  <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
                </div>
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
