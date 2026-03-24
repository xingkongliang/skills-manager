import { useState, useEffect } from "react";
import {
  Folder,
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
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { openUrl } from "@tauri-apps/plugin-opener";
import { check } from "@tauri-apps/plugin-updater";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import { useThemeContext } from "../context/ThemeContext";
import * as api from "../lib/tauri";
import type { AppUpdateInfo } from "../lib/tauri";
import type { Theme } from "../hooks/useTheme";

const IS_WINDOWS = navigator.userAgent.includes("Windows");

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
  const GITHUB_URL = "https://github.com/xingkongliang/skills-manager";

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

  const handleLanguageChange = (lng: string) => {
    localStorage.setItem("language", lng);
    i18n.changeLanguage(lng);
    api.setSettings("language", lng);
  };

  const textSizeZoomMap: Record<string, string> = {
    small: "0.9",
    default: "1",
    large: "1.1",
    xlarge: "1.2",
  };

  const applyTextSize = (size: string) => {
    document.documentElement.style.zoom = textSizeZoomMap[size] || "1";
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
      const update = await check();
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
          <div className="flex items-center justify-between mb-3">
            <h2 className="app-section-title">
              {t("settings.supportedAgents")} ({tools.filter((t) => t.installed).length}/{tools.length})
            </h2>
            <div className="flex items-center gap-3">
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
          <div className="grid grid-cols-3 gap-2 md:grid-cols-4">
            {tools.map((agent, i) => (
              <div
                key={i}
                className={cn(
                  "flex items-center gap-2 p-2.5 rounded-[4px] border transition-colors",
                  agent.installed && agent.enabled
                    ? "bg-surface border-border-subtle hover:border-border"
                    : "bg-bg-secondary border-border-subtle opacity-50"
                )}
              >
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
                <div className="min-w-0">
                  <h3 className={cn("text-[13px] font-medium truncate", agent.installed && agent.enabled ? "text-secondary" : "text-muted")}>
                    {agent.display_name}
                  </h3>
                  <p className="text-[13px] text-muted truncate" title={agent.skills_dir}>
                    {agent.installed ? agent.skills_dir.replace(/\/Users\/[^/]+/, "~").replace(/\/home\/[^/]+/, "~").replace(/^[A-Za-z]:\\Users\\[^\\]+/, "~") : t("settings.notInstalled")}
                  </p>
                </div>
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
            <div className="px-4 py-3 flex items-center justify-between gap-4">
              <div className="min-w-0">
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.repoPath")}</h3>
                <p className="text-[13px] text-muted">{t("settings.repoPathDesc")}</p>
              </div>
              <div className="flex items-center gap-2 shrink-0">
                <div className="flex items-center gap-1.5 bg-background border border-border-subtle rounded-[4px] px-2 py-1">
                  <Folder className="w-3 h-3 text-muted" />
                  <span className="text-[13px] font-mono text-tertiary">{displayedRepoPath}</span>
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
            <div className="px-4 py-3 flex items-center justify-between gap-4">
              <div>
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.syncMode")}</h3>
                <p className="text-[13px] text-muted">{t("settings.syncModeDesc")}</p>
              </div>
              <div className="flex bg-background border border-border-subtle rounded-[4px] p-px shrink-0">
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
            <div className="px-4 py-3 flex items-center justify-between gap-4">
              <div>
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.theme")}</h3>
                <p className="text-[13px] text-muted">{t("settings.themeDesc")}</p>
              </div>
              <div className="flex bg-background border border-border-subtle rounded-[4px] p-px shrink-0">
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
            <div className="px-4 py-3 flex items-center justify-between gap-4">
              <div>
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.textSize")}</h3>
                <p className="text-[13px] text-muted">{t("settings.textSizeDesc")}</p>
              </div>
              <div className="flex bg-background border border-border-subtle rounded-[4px] p-px shrink-0">
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
            <div className="px-4 py-3 flex items-center justify-between gap-4">
              <div>
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
            <div className="px-4 py-3 flex items-center justify-between gap-4">
              <div>
                <h3 className="text-[13px] text-secondary font-medium">{t("settings.language")}</h3>
              </div>
              <div className="flex items-center gap-2">
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
            <div className="px-4 py-3 flex items-center justify-between gap-4">
              <div>
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.closeAction")}</h3>
                <p className="text-[13px] text-muted">{t("settings.closeActionDesc")}</p>
                {!showTrayIcon && (
                  <p className="text-[12px] text-muted mt-1">{t("settings.trayIconOffHint")}</p>
                )}
              </div>
              <div className="flex bg-background border border-border-subtle rounded-[4px] p-px shrink-0">
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
            <div className="px-4 py-3 flex items-center justify-between gap-4">
              <div>
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.trayIcon")}</h3>
                <p className="text-[13px] text-muted">{t("settings.trayIconDesc")}</p>
              </div>
              <div className="flex bg-background border border-border-subtle rounded-[4px] p-px shrink-0">
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
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  value={proxyInput}
                  onChange={(e) => setProxyInput(e.target.value)}
                  placeholder={t("settings.proxyUrlPlaceholder")}
                  className={`${fieldClass} flex-1 font-mono`}
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
              <div className="flex items-center gap-2">
                <input
                  type="password"
                  value={skillsmpApiKey}
                  onChange={(e) => setSkillsmpApiKey(e.target.value)}
                  placeholder="sk_live_..."
                  className={`${fieldClass} flex-1 font-mono`}
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
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  value={gitRemoteInput}
                  onChange={(e) => setGitRemoteInput(e.target.value)}
                  placeholder={t("settings.gitRemoteUrlPlaceholder")}
                  className={`${fieldClass} flex-1 font-mono`}
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
          <div className="app-panel p-4 flex items-center justify-between">
            <div className="flex items-center gap-3">
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
            <div className="flex gap-2">
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
