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
  const { tools, scenarios, activeScenario, refreshTools, switchScenario, openHelp } = useApp();
  const { theme, setTheme } = useThemeContext();
  const [syncMode, setSyncMode] = useState("symlink");
  const [defaultScenario, setDefaultScenario] = useState("");
  const [refreshing, setRefreshing] = useState(false);
  const [openingRepo, setOpeningRepo] = useState(false);
  const [openingGithub, setOpeningGithub] = useState(false);
  const [centralRepoPath, setCentralRepoPath] = useState("");
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<AppUpdateInfo | null>(null);
  const [installing, setInstalling] = useState(false);
  const GITHUB_URL = "https://github.com/xingkongliang/skills-manager";

  useEffect(() => {
    api.getSettings("sync_mode").then((v) => { if (v) setSyncMode(v); });
    api.getSettings("default_scenario").then((v) => { if (v) setDefaultScenario(v); });
    api.getCentralRepoPath().then(setCentralRepoPath).catch(() => {});
  }, []);

  const handleRefresh = async () => {
    setRefreshing(true);
    await refreshTools();
    setRefreshing(false);
    toast.success(t("common.success"));
  };

  const handleSyncModeChange = async (mode: string) => {
    setSyncMode(mode);
    await api.setSettings("sync_mode", mode);
  };

  const handleDefaultScenarioChange = async (id: string) => {
    setDefaultScenario(id);
    await api.setSettings("default_scenario", id);
  };

  const handleActiveScenarioChange = async (id: string) => {
    if (!id) return;
    await switchScenario(id);
    toast.success(t("scenario.switched", { name: scenarios.find((s) => s.id === id)?.name || "" }));
  };

  const handleLanguageChange = (lng: string) => {
    localStorage.setItem("language", lng);
    i18n.changeLanguage(lng);
    api.setSettings("language", lng);
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
    } finally {
      setInstalling(false);
    }
  };

  const selectClass =
    "h-10 rounded-lg border border-border-subtle bg-background px-3 text-[13px] text-secondary outline-none transition-colors focus:border-border";

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
          <div className="grid grid-cols-3 gap-2 md:grid-cols-4">
            {tools.map((agent, i) => (
              <div
                key={i}
                className={cn(
                  "flex items-center gap-2 p-2.5 rounded-[4px] border transition-colors",
                  agent.installed
                    ? "bg-surface border-border-subtle hover:border-border"
                    : "bg-bg-secondary border-border-subtle opacity-50"
                )}
              >
                {agent.installed ? (
                  <CheckCircle2 className="w-3.5 h-3.5 text-emerald-500 shrink-0" />
                ) : (
                  <Circle className="w-3.5 h-3.5 text-faint shrink-0" />
                )}
                <div className="min-w-0">
                  <h3 className={cn("text-[13px] font-medium truncate", agent.installed ? "text-secondary" : "text-muted")}>
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
                    "inline-flex items-center gap-1 rounded-[4px] border px-2.5 py-1 text-[13px] font-medium transition-all outline-none",
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
                    "flex items-center gap-1.5 px-2.5 py-1 rounded-[3px] text-[13px] font-medium transition-colors outline-none",
                    syncMode === "symlink" ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                  )}
                >
                  <LinkIcon className="w-3 h-3" /> {t("settings.symlink")}
                </button>
                <button
                  onClick={() => handleSyncModeChange("copy")}
                  className={cn(
                    "flex items-center gap-1.5 px-2.5 py-1 rounded-[3px] text-[13px] font-medium transition-colors outline-none",
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
                        "flex items-center gap-1.5 px-2.5 py-1 rounded-[3px] text-[13px] font-medium transition-colors outline-none",
                        theme === opt.value ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                      )}
                    >
                      <Icon className="w-3 h-3" /> {opt.label}
                    </button>
                  );
                })}
              </div>
            </div>

            {/* Current scenario */}
            <div className="px-4 py-3 flex items-center justify-between gap-4">
              <div>
                <h3 className="text-[13px] text-secondary font-medium mb-0.5">{t("settings.currentScenario")}</h3>
                <p className="text-[13px] text-muted">{t("settings.currentScenarioDesc")}</p>
              </div>
              <select
                value={activeScenario?.id || ""}
                onChange={(e) => handleActiveScenarioChange(e.target.value)}
                className={selectClass}
              >
                <option value="" disabled>—</option>
                {scenarios.map((s) => (
                  <option key={s.id} value={s.id}>{s.name}</option>
                ))}
              </select>
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
                className={selectClass}
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
                  className={selectClass}
                >
                  <option value="zh">简体中文 (zh-CN)</option>
                  <option value="en">English (en-US)</option>
                </select>
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
                  <button
                    type="button"
                    onClick={handleAutoUpdate}
                    disabled={installing}
                    className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-[4px] bg-accent text-white text-[13px] font-medium transition-colors border border-accent hover:opacity-90 outline-none disabled:opacity-60"
                  >
                    {installing ? (
                      <Loader2 className="w-3 h-3 animate-spin" />
                    ) : (
                      <Download className="w-3 h-3" />
                    )}
                    {installing ? t("settings.installing") : t("settings.installUpdate")}
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={() => openUrl(updateInfo.release_url)}
                    className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-[4px] bg-accent text-white text-[13px] font-medium transition-colors border border-accent hover:opacity-90 outline-none"
                  >
                    <Download className="w-3 h-3" /> {t("settings.download")}
                  </button>
                )
              ) : (
                <button
                  type="button"
                  onClick={handleCheckUpdate}
                  disabled={checkingUpdate}
                  className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-[4px] bg-surface-hover hover:bg-surface-active text-tertiary text-[13px] font-medium transition-colors border border-border outline-none disabled:opacity-60"
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
                className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-[4px] bg-surface-hover hover:bg-surface-active text-tertiary text-[13px] font-medium transition-colors border border-border outline-none"
              >
                <BookOpen className="w-3 h-3" /> {t("settings.help")}
              </button>
              <button
                type="button"
                onClick={handleOpenGithub}
                disabled={openingGithub}
                className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-[4px] bg-surface-hover hover:bg-surface-active text-tertiary text-[13px] font-medium transition-colors border border-border outline-none disabled:opacity-60"
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
