import { useState, useEffect, useCallback, useRef, useMemo, useDeferredValue } from "react";
import {
  DownloadCloud,
  UploadCloud,
  Github,
  Box,
  Star,
  TrendingUp,
  Clock,
  Plus,
  FolderUp,
  Loader2,
  RefreshCw,
  FolderSearch,
  ExternalLink,
  Check,
  ChevronLeft,
  ChevronRight,
  Search,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import * as api from "../lib/tauri";
import type { ScanResult, SkillsShSkill } from "../lib/tauri";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useSearchParams } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { StatusBanner } from "../components/StatusBanner";

const MARKET_PAGE_SIZE = 24;
const MARKET_SEARCH_STEP = 60;
const MARKET_SEARCH_DEBOUNCE_MS = 450;
const MARKET_SEARCH_CACHE_TTL_MS = 120_000;
const MARKET_SEARCH_CACHE_MAX_ENTRIES = 150;

export function InstallSkills() {
  const { t } = useTranslation();
  const { refreshScenarios, refreshManagedSkills, managedSkills } = useApp();
  const [searchParams, setSearchParams] = useSearchParams();
  const [activeTab, setActiveTab] = useState<"market" | "local" | "git">("market");
  const [marketTab, setMarketTab] = useState<"hot" | "trending" | "alltime">("hot");
  const [marketQuery, setMarketQuery] = useState("");
  const [marketSourceFilter, setMarketSourceFilter] = useState("all");
  const [marketSkills, setMarketSkills] = useState<SkillsShSkill[]>([]);
  const [marketPage, setMarketPage] = useState(1);
  const [marketSearchLimit, setMarketSearchLimit] = useState(MARKET_SEARCH_STEP);
  const [marketLoading, setMarketLoading] = useState(false);
  const [marketLoadingMore, setMarketLoadingMore] = useState(false);
  const [marketError, setMarketError] = useState<string | null>(null);
  const [marketReloadKey, setMarketReloadKey] = useState(0);
  const [installing, setInstalling] = useState<string | null>(null);
  const [gitUrl, setGitUrl] = useState("");
  const [gitName, setGitName] = useState("");
  const [gitLoading, setGitLoading] = useState(false);
  const [gitCancelKey, setGitCancelKey] = useState<string | null>(null);
  const [scanResult, setScanResult] = useState<ScanResult | null>(null);
  const [scanLoading, setScanLoading] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const [importingPaths, setImportingPaths] = useState<Set<string>>(new Set());
  const [importingAll, setImportingAll] = useState(false);
  const marketListRef = useRef<HTMLDivElement | null>(null);
  const marketSearchCacheRef = useRef<Map<string, { timestamp: number; data: SkillsShSkill[] }>>(new Map());
  const marketSkillsLengthRef = useRef(0);
  const [debouncedMarketQuery, setDebouncedMarketQuery] = useState("");
  const deferredMarketQuery = useDeferredValue(marketQuery);

  const pruneMarketSearchCache = useCallback(() => {
    const now = Date.now();
    const entries = Array.from(marketSearchCacheRef.current.entries());

    for (const [key, value] of entries) {
      if (now - value.timestamp >= MARKET_SEARCH_CACHE_TTL_MS) {
        marketSearchCacheRef.current.delete(key);
      }
    }

    if (marketSearchCacheRef.current.size <= MARKET_SEARCH_CACHE_MAX_ENTRIES) {
      return;
    }

    const sorted = Array.from(marketSearchCacheRef.current.entries()).sort(
      (a, b) => a[1].timestamp - b[1].timestamp
    );
    const removeCount = marketSearchCacheRef.current.size - MARKET_SEARCH_CACHE_MAX_ENTRIES;
    for (const [key] of sorted.slice(0, removeCount)) {
      marketSearchCacheRef.current.delete(key);
    }
  }, []);

  const installedSourceRefs = useMemo(() => {
    const set = new Set<string>();
    for (const skill of managedSkills) {
      if (skill.source_type === "skillssh" && skill.source_ref) {
        set.add(skill.source_ref);
      }
    }
    return set;
  }, [managedSkills]);

  useEffect(() => {
    const timer = setTimeout(() => {
      setDebouncedMarketQuery(deferredMarketQuery);
    }, MARKET_SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [deferredMarketQuery]);

  useEffect(() => {
    marketSkillsLengthRef.current = marketSkills.length;
  }, [marketSkills.length]);

  useEffect(() => {
    const tab = searchParams.get("tab");
    if (tab === "market" || tab === "local" || tab === "git") {
      setActiveTab(tab);
    }
  }, [searchParams]);

  const switchTab = (tab: "market" | "local" | "git") => {
    setActiveTab(tab);
    setSearchParams({ tab });
  };

  const runScan = useCallback(async () => {
    setScanLoading(true);
    setLocalError(null);
    try {
      const result = await api.scanLocalSkills();
      setScanResult(result);
    } catch (e: any) {
      console.error(e);
      const message = e?.toString?.() || t("common.error");
      setLocalError(message);
      toast.error(message);
    } finally {
      setScanLoading(false);
    }
  }, [t]);

  useEffect(() => {
    if (activeTab !== "market") return;

    const query = debouncedMarketQuery.trim();
    const loadingMore =
      query.length > 0 &&
      marketSkillsLengthRef.current > 0 &&
      marketSearchLimit > marketSkillsLengthRef.current;

    if (query.length > 0 && !loadingMore) {
      const cacheKey = `${query.toLowerCase()}|${marketSearchLimit}`;
      const cached = marketSearchCacheRef.current.get(cacheKey);
      if (cached && Date.now() - cached.timestamp < MARKET_SEARCH_CACHE_TTL_MS) {
        setMarketSkills(cached.data);
        setMarketLoading(false);
        setMarketLoadingMore(false);
        setMarketPage(1);
        setMarketError(null);
        return;
      }
    }

    setMarketLoadingMore(loadingMore);
    setMarketLoading(true);
    if (!loadingMore) {
      setMarketPage(1);
    }
    setMarketError(null);

    let stale = false;
    const request = query
      ? api.searchSkillssh(query, marketSearchLimit)
      : api.fetchLeaderboard(marketTab);

    request
      .then((result) => {
        if (stale) return;
        setMarketSkills(result);
        if (query.length > 0 && !loadingMore) {
          const cacheKey = `${query.toLowerCase()}|${marketSearchLimit}`;
          marketSearchCacheRef.current.set(cacheKey, { timestamp: Date.now(), data: result });
          pruneMarketSearchCache();
        }
        if (!loadingMore) {
          setMarketSourceFilter("all");
        }
      })
      .catch((e) => {
        if (stale) return;
        console.error(e);
        const message = e?.toString?.() || t("common.error");
        setMarketError(message);
        toast.error(message);
      })
      .finally(() => {
        if (stale) return;
        setMarketLoading(false);
        setMarketLoadingMore(false);
      });

    return () => { stale = true; };
  }, [activeTab, debouncedMarketQuery, marketReloadKey, marketSearchLimit, marketTab, pruneMarketSearchCache, t]);

  useEffect(() => {
    if (activeTab === "local" && !scanResult && !scanLoading) {
      runScan();
    }
  }, [activeTab, scanLoading, scanResult, runScan]);

  const installLocalSource = (sourcePath: string) => {
    const name = sourcePath.split("/").pop() || sourcePath;
    toast.promise(
      (async () => {
        await api.installLocal(sourcePath);
        await Promise.all([refreshScenarios(), refreshManagedSkills()]);
        await runScan();
      })(),
      {
        loading: t("install.toast.installing", { name }),
        success: t("install.toast.success", { name }),
        error: (e) => {
          const message = e?.toString?.() || t("common.error");
          setLocalError(message);
          return message;
        },
      }
    );
  };

  const handleLocalFolderInstall = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
      });
      if (!selected) return;
      installLocalSource(selected as string);
    } catch (e: any) {
      const message = e?.toString?.() || t("common.error");
      setLocalError(message);
      toast.error(message);
    }
  };

  const handleLocalFileInstall = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "Skills", extensions: ["zip", "skill"] }],
      });
      if (!selected) return;
      installLocalSource(selected as string);
    } catch (e: any) {
      const message = e?.toString?.() || t("common.error");
      setLocalError(message);
      toast.error(message);
    }
  };

  const handleInstallSkillssh = async (skill: SkillsShSkill) => {
    const displayName = skill.name || skill.skill_id;
    const cancelKey = `${skill.source}/${skill.skill_id}`;
    setInstalling(skill.id);

    const toastId = toast.loading(t("install.toast.cloning"));
    let unlisten: (() => void) | null = null;

    try {
      unlisten = await listen<{ skill_id: string; phase: string }>(
        "install-progress",
        (event) => {
          if (event.payload.skill_id !== cancelKey) return;
          if (event.payload.phase === "cloning") {
            toast.loading(t("install.toast.cloning"), { id: toastId });
          } else if (event.payload.phase === "installing") {
            toast.loading(t("install.toast.installing", { name: displayName }), { id: toastId });
          }
        }
      );
      await api.installFromSkillssh(skill.source, skill.skill_id);
      await Promise.all([refreshScenarios(), refreshManagedSkills()]);
      toast.success(t("install.toast.success", { name: displayName }), { id: toastId });
    } catch (e: any) {
      const msg = e?.toString() || t("common.error");
      if (msg.includes("cancelled")) {
        toast.info(t("install.toast.cancelled"), { id: toastId });
      } else {
        toast.error(msg, { id: toastId });
      }
    } finally {
      setInstalling(null);
      unlisten?.();
    }
  };

  const handleCancelInstall = (cancelKey: string) => {
    api.cancelInstall(cancelKey).catch(() => {
      // Ignore race: install may have completed before cancel request arrives.
    });
  };

  const handleGitInstall = async () => {
    if (!gitUrl.trim()) return;
    setGitLoading(true);
    const url = gitUrl.trim();
    const name = gitName.trim() || undefined;
    const cancelKey = url;
    setGitCancelKey(cancelKey);

    const toastId = toast.loading(t("install.toast.cloning"));
    let unlisten: (() => void) | null = null;

    try {
      unlisten = await listen<{ skill_id: string; phase: string }>(
        "install-progress",
        (event) => {
          if (event.payload.skill_id !== cancelKey) return;
          if (event.payload.phase === "cloning") {
            toast.loading(t("install.toast.cloning"), { id: toastId });
          } else if (event.payload.phase === "installing") {
            toast.loading(t("install.toast.installing", { name: name || url }), { id: toastId });
          }
        }
      );
      await api.installGit(url, name);
      setGitUrl("");
      setGitName("");
      await Promise.all([refreshScenarios(), refreshManagedSkills()]);
      toast.success(t("install.toast.success", { name: name || url }), { id: toastId });
    } catch (e: any) {
      const msg = e?.toString() || t("common.error");
      if (msg.includes("cancelled")) {
        toast.info(t("install.toast.cancelled"), { id: toastId });
      } else {
        toast.error(msg, { id: toastId });
      }
    } finally {
      setGitLoading(false);
      setGitCancelKey(null);
      unlisten?.();
    }
  };

  const handleImportDiscovered = async (sourcePath: string, name: string) => {
    setImportingPaths((prev) => new Set(prev).add(sourcePath));
    try {
      await api.importExistingSkill(sourcePath, name);
      toast.success(t("install.scan.importedOne", { name }));
      await Promise.all([refreshScenarios(), refreshManagedSkills()]);
      await runScan();
    } catch (e: any) {
      toast.error(e.toString());
    } finally {
      setImportingPaths((prev) => {
        const next = new Set(prev);
        next.delete(sourcePath);
        return next;
      });
    }
  };

  const handleImportAllDiscovered = async () => {
    setImportingAll(true);
    try {
      await api.importAllDiscovered();
      toast.success(t("install.scan.importedAll"));
      await Promise.all([refreshScenarios(), refreshManagedSkills()]);
      await runScan();
    } catch (e: any) {
      toast.error(e.toString());
    } finally {
      setImportingAll(false);
    }
  };

  const scrollMarketListToTop = () => {
    marketListRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  const changeMarketPage = (page: number) => {
    setMarketPage(page);
    scrollMarketListToTop();
  };

  const scanGroups = scanResult?.groups ?? [];
  const pendingGroups = scanGroups.filter((group) => !group.imported);
  const sourceOptions = useMemo(
    () => Array.from(new Set(marketSkills.map((skill) => skill.source))).slice(0, 8),
    [marketSkills]
  );
  const filteredMarketSkills = useMemo(() => {
    const filtered = marketSourceFilter === "all"
      ? marketSkills
      : marketSkills.filter((skill) => skill.source === marketSourceFilter);
    if (debouncedMarketQuery.trim().length > 0) {
      return [...filtered].sort((a, b) => b.installs - a.installs);
    }
    return filtered;
  }, [marketSkills, marketSourceFilter, debouncedMarketQuery]);
  const totalMarketPages = Math.max(1, Math.ceil(filteredMarketSkills.length / MARKET_PAGE_SIZE));
  const currentMarketPage = Math.min(marketPage, totalMarketPages);
  const marketPageStart = (currentMarketPage - 1) * MARKET_PAGE_SIZE;
  const paginatedMarketSkills = filteredMarketSkills.slice(
    marketPageStart,
    marketPageStart + MARKET_PAGE_SIZE
  );
  const visibleMarketPages = Array.from(
    { length: totalMarketPages },
    (_, index) => index + 1
  ).filter((page) => {
    if (totalMarketPages <= 7) return true;
    if (page === 1 || page === totalMarketPages) return true;
    return Math.abs(page - currentMarketPage) <= 1;
  });
  const hasMarketQuery = debouncedMarketQuery.trim().length > 0;
  const canLoadMoreSearch = hasMarketQuery && marketSkills.length >= marketSearchLimit;
  const isLoadingMoreSearch = hasMarketQuery && marketLoadingMore;

  return (
    <div className="app-page">
      <div className="app-page-header">
        <h1 className="app-page-title mb-4">{t("install.title")}</h1>
        <div className="flex gap-1 border-b border-border-subtle">
          {[
            { id: "market" as const, label: t("install.browseMarket"), icon: Box },
            { id: "local" as const, label: t("install.localInstall"), icon: UploadCloud },
            { id: "git" as const, label: t("install.gitInstall"), icon: Github },
          ].map((tab) => {
            const Icon = tab.icon;
            const isActive = activeTab === tab.id;
            return (
              <button
                key={tab.id}
                onClick={() => switchTab(tab.id)}
                className={cn(
                  "mr-4 flex items-center gap-1.5 border-b-2 px-1 pb-2.5 text-[13px] font-medium transition-colors outline-none",
                  isActive
                    ? "border-accent text-accent"
                    : "border-transparent text-muted hover:text-tertiary"
                )}
              >
                <Icon className="h-3.5 w-3.5" />
                {tab.label}
              </button>
            );
          })}
        </div>
      </div>

      {activeTab === "market" && (
        <div className="animate-in fade-in duration-300">
          <div className="app-panel mb-3 p-3.5">
            <div className="flex flex-col gap-3">
              <div className="flex flex-col gap-2">
                <div className="min-w-0">
                  <div className="mb-1.5 flex flex-wrap items-center gap-2 text-[13px] text-muted">
                    <span className="inline-flex items-center gap-1.5 rounded-[5px] border border-border-subtle bg-background px-2 py-1 font-medium text-tertiary">
                      <Box className="h-3 w-3" />
                      {t("install.browseMarket")}
                    </span>
                    <span className="text-faint">·</span>
                    <span>
                      {hasMarketQuery
                        ? t("install.marketMode.search", { query: debouncedMarketQuery.trim() })
                        : t(`install.marketMode.${marketTab}`)}
                    </span>
                    <span className="text-faint">·</span>
                    <span>{t("install.filters.filteredCount", { count: filteredMarketSkills.length })}</span>
                  </div>
                </div>

                <div className="flex flex-col gap-1.5 lg:flex-row lg:items-center">
                  {!hasMarketQuery ? (
                    <div className="app-segmented shrink-0 bg-background">
                      {[
                        { id: "hot" as const, label: t("install.hot"), icon: Star },
                        { id: "trending" as const, label: t("install.trending"), icon: TrendingUp },
                        { id: "alltime" as const, label: t("install.all"), icon: Clock },
                      ].map((tab) => {
                        const Icon = tab.icon;
                        const isActive = marketTab === tab.id;
                        return (
                          <button
                            key={tab.id}
                            onClick={() => setMarketTab(tab.id)}
                            className={cn(
                              "app-segmented-button flex items-center gap-1.5",
                              isActive && "app-segmented-button-active"
                            )}
                          >
                            <Icon className="h-3 w-3" />
                            {tab.label}
                          </button>
                        );
                      })}
                    </div>
                  ) : null}

                  <div className="relative flex-1 lg:max-w-[640px]">
                    <Search className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
                    <input
                      type="text"
                      value={marketQuery}
                      onChange={(event) => {
                        setMarketQuery(event.target.value);
                        setMarketSearchLimit(MARKET_SEARCH_STEP);
                      }}
                      placeholder={t("install.searchMarket")}
                      className="app-input w-full bg-background pl-9"
                      autoCapitalize="none"
                      autoCorrect="off"
                      spellCheck={false}
                    />
                  </div>
                </div>
              </div>

              <div className="border-t border-border-subtle pt-2">
                <div className="flex items-center gap-3">
                  <span className="shrink-0 text-[13px] font-medium text-tertiary">
                    {t("install.filters.source")}
                  </span>
                  <div className="min-w-0 flex-1 overflow-x-auto scrollbar-hide">
                  <div className="flex min-w-max justify-end gap-1.5 pr-1">
                    <button
                      type="button"
                      onClick={() => setMarketSourceFilter("all")}
                      className={cn(
                        "rounded-full border px-2.5 py-1 text-[13px] font-medium whitespace-nowrap transition-colors",
                        marketSourceFilter === "all"
                          ? "border-accent-border bg-accent-bg text-accent-light"
                          : "border-border-subtle bg-background text-muted hover:text-secondary"
                      )}
                    >
                      {t("install.filters.allSources")}
                    </button>
                    {sourceOptions.map((source) => (
                      <button
                        key={source}
                        type="button"
                        onClick={() => setMarketSourceFilter(source)}
                        className={cn(
                          "rounded-full border px-2.5 py-1 text-[13px] font-medium whitespace-nowrap transition-colors",
                          marketSourceFilter === source
                            ? "border-accent-border bg-accent-bg text-accent-light"
                            : "border-border-subtle bg-background text-muted hover:text-secondary"
                        )}
                      >
                        @{source}
                      </button>
                    ))}
                  </div>
                  </div>
                </div>
              </div>
            </div>
          </div>

          {marketError ? (
            <div className="mb-4">
              <StatusBanner
                compact
                title={t("common.requestFailed")}
                description={marketError}
                actionLabel={t("common.retry")}
                onAction={() => setMarketReloadKey((value) => value + 1)}
                tone="danger"
              />
            </div>
          ) : null}

          {marketLoading && !marketLoadingMore ? (
            <div className="flex items-center justify-center py-16">
              <Loader2 className="h-5 w-5 animate-spin text-muted" />
            </div>
          ) : (
            <div className="pb-8">
              <div ref={marketListRef} className="scroll-mt-4" />

              {filteredMarketSkills.length === 0 ? (
                <div className="app-panel flex flex-col items-center justify-center rounded-2xl px-6 py-14 text-center">
                  <div className="flex h-12 w-12 items-center justify-center rounded-2xl border border-border bg-background text-muted">
                    <Search className="h-5 w-5" />
                  </div>
                  <h3 className="mt-4 text-[14px] font-semibold text-secondary">
                    {t("install.noResults.title")}
                  </h3>
                  <p className="mt-1 max-w-md text-[13px] text-muted">
                    {t("install.noResults.description")}
                  </p>
                </div>
              ) : (
                <>
                  <div className="grid grid-cols-2 gap-2.5 lg:grid-cols-3">
                    {paginatedMarketSkills.map((skill) => {
                      const displayName = skill.name || skill.skill_id;
                      const showSkillId = skill.skill_id.trim() !== displayName.trim();
                      const owner = skill.source.split("/")[0];
                      const avatarUrl = `https://github.com/${owner}.png?size=32`;
                      const sourceRef = `${skill.source}/${skill.skill_id}`;
                      const isInstalled = installedSourceRefs.has(sourceRef);

                      return (
                      <div
                        key={skill.id}
                        className="app-panel flex flex-col gap-2 p-3 transition-colors hover:border-border"
                      >
                        <div className="flex items-start justify-between gap-2">
                          <div className="flex min-w-0 flex-1 items-center gap-2">
                            <img
                              src={avatarUrl}
                              alt={owner}
                              className="h-6 w-6 shrink-0 rounded-full border border-border-subtle"
                              loading="lazy"
                            />
                            <div className="min-w-0">
                              <h3 className="truncate text-[13px] font-semibold text-secondary">
                                {displayName}
                              </h3>
                              {showSkillId ? (
                                <p className="truncate text-[13px] leading-4 text-muted">{skill.skill_id}</p>
                              ) : null}
                            </div>
                          </div>

                          <div className="flex shrink-0 items-center gap-1">
                            <button
                              onClick={() => openUrl(`https://skills.sh/${skill.source}/${skill.skill_id}`)}
                              className="rounded-[5px] p-1 text-muted transition-colors hover:bg-surface-hover hover:text-secondary"
                              title={t("install.viewOnWeb")}
                            >
                              <ExternalLink className="h-3.5 w-3.5" />
                            </button>
                            {isInstalled ? (
                              <span
                                className="rounded-[5px] border border-emerald-500/20 bg-emerald-500/10 p-1 text-emerald-400"
                                title={t("install.installed")}
                              >
                                <Check className="h-3.5 w-3.5" />
                              </span>
                            ) : installing === skill.id ? (
                              <button
                                onClick={() => handleCancelInstall(`${skill.source}/${skill.skill_id}`)}
                                className="rounded-[5px] border border-red-500/30 bg-red-500/10 p-1 text-red-400 transition-colors hover:bg-red-500/20"
                                title={t("install.cancel")}
                              >
                                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                              </button>
                            ) : (
                              <button
                                onClick={() => handleInstallSkillssh(skill)}
                                disabled={installing !== null}
                                className="rounded-[5px] border border-accent-border bg-accent-dark p-1 text-white transition-colors hover:bg-accent disabled:opacity-50"
                                title={t("install.oneClickInstall")}
                              >
                                <Plus className="h-3.5 w-3.5" />
                              </button>
                            )}
                          </div>
                        </div>

                        <div className="flex flex-wrap items-center gap-1">
                          <span className="rounded-[5px] bg-accent-bg px-1.5 py-0.5 text-[13px] leading-4 font-medium text-accent-light">
                            @{skill.source}
                          </span>
                          <span className="inline-flex items-center gap-1 rounded-[5px] border border-border-subtle bg-background px-1.5 py-0.5 text-[13px] leading-4 text-muted">
                            <DownloadCloud className="h-3 w-3" />
                            {skill.installs > 1000
                              ? `${(skill.installs / 1000).toFixed(0)}k`
                              : skill.installs}
                          </span>
                          {isInstalled ? (
                            <span className="inline-flex items-center gap-1 rounded-[5px] border border-emerald-500/20 bg-emerald-500/10 px-1.5 py-0.5 text-[13px] leading-4 font-medium text-emerald-400">
                              <Check className="h-3 w-3" />
                              {t("install.installed")}
                            </span>
                          ) : null}
                        </div>
                      </div>
                      );
                    })}
                  </div>

                  {totalMarketPages > 1 ? (
                    <div className="mt-5 flex flex-wrap items-center justify-center gap-1.5">
                      <button
                        onClick={() => changeMarketPage(Math.max(1, currentMarketPage - 1))}
                        disabled={currentMarketPage === 1}
                        className="inline-flex items-center gap-1 rounded-[6px] border border-border-subtle bg-surface px-3 py-1.5 text-[13px] font-medium text-secondary transition-colors hover:bg-surface-hover disabled:opacity-50"
                      >
                        <ChevronLeft className="h-3.5 w-3.5" />
                        {t("install.pagination.previous")}
                      </button>

                      {visibleMarketPages.map((page, index) => {
                        const previousPage = visibleMarketPages[index - 1];
                        const showGap = previousPage && page - previousPage > 1;

                        return (
                          <div key={page} className="flex items-center gap-1.5">
                            {showGap ? <span className="px-1 text-[13px] text-faint">...</span> : null}
                            <button
                              onClick={() => changeMarketPage(page)}
                              className={cn(
                                "min-w-8 rounded-[6px] border px-2.5 py-1.5 text-[13px] font-semibold transition-colors",
                                page === currentMarketPage
                                  ? "border-accent-border bg-accent-dark text-white"
                                  : "border-border-subtle bg-surface text-secondary hover:bg-surface-hover"
                              )}
                            >
                              {page}
                            </button>
                          </div>
                        );
                      })}

                      <button
                        onClick={() => changeMarketPage(Math.min(totalMarketPages, currentMarketPage + 1))}
                        disabled={currentMarketPage === totalMarketPages}
                        className="inline-flex items-center gap-1 rounded-[6px] border border-border-subtle bg-surface px-3 py-1.5 text-[13px] font-medium text-secondary transition-colors hover:bg-surface-hover disabled:opacity-50"
                      >
                        {t("install.pagination.next")}
                        <ChevronRight className="h-3.5 w-3.5" />
                      </button>
                    </div>
                  ) : null}

                  {hasMarketQuery ? (
                    <div className="mt-4 flex justify-center">
                      <button
                        type="button"
                        onClick={() => setMarketSearchLimit((value) => value + MARKET_SEARCH_STEP)}
                        disabled={!canLoadMoreSearch || marketLoading}
                        className="inline-flex items-center gap-2 rounded-[6px] border border-border-subtle bg-surface px-3.5 py-2 text-[13px] font-medium text-secondary transition-colors hover:bg-surface-hover disabled:cursor-not-allowed disabled:opacity-50"
                      >
                        {marketLoading ? (
                          <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        ) : (
                          <Search className="h-3.5 w-3.5" />
                        )}
                        {isLoadingMoreSearch
                          ? t("install.loadingMore")
                          : t("install.loadMoreSearch")}
                      </button>
                    </div>
                  ) : null}
                </>
              )}
            </div>
          )}
        </div>
      )}

      {activeTab === "local" && (
        <div className="space-y-4 pb-8 animate-in fade-in duration-300">
          <section className="app-panel overflow-hidden">
            <div className="border-b border-border-subtle px-4 py-3.5">
              <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
                <div className="max-w-xl">
                  <div className="mb-2 flex flex-wrap items-center gap-2 text-[13px] text-muted">
                    <span className="inline-flex items-center gap-1.5 rounded-[5px] border border-accent-border bg-accent-bg px-2 py-1 font-medium text-accent-light">
                      <FolderUp className="h-3.5 w-3.5" />
                      {t("install.local.title")}
                    </span>
                  </div>

                  <h2 className="text-[14px] font-semibold text-secondary">
                    {t("install.local.title")}
                  </h2>
                  <p className="mt-1 text-[13px] leading-5 text-muted">
                    {t("install.local.description")}
                  </p>
                </div>

                <div className="flex flex-wrap gap-2">
                  <button
                    type="button"
                    onClick={handleLocalFolderInstall}
                    className="app-button-primary"
                  >
                    <FolderUp className="h-4 w-4" />
                    {t("install.local.selectFolder")}
                  </button>
                  <button
                    type="button"
                    onClick={handleLocalFileInstall}
                    className="app-button-secondary bg-background"
                  >
                    <UploadCloud className="h-4 w-4" />
                    {t("install.local.selectArchive")}
                  </button>
                </div>
              </div>
            </div>

          </section>

          {localError ? (
            <StatusBanner
              compact
              title={t("common.requestFailed")}
              description={localError}
              actionLabel={t("common.retry")}
              onAction={runScan}
              tone="danger"
            />
          ) : null}

          <section className="app-panel overflow-hidden">
            <div className="flex items-center justify-between gap-4 border-b border-border-subtle px-4 py-3.5">
              <div>
                <h2 className="text-[13px] font-semibold text-secondary">{t("install.scan.title")}</h2>
                <p className="mt-0.5 text-[13px] text-muted">
                  {scanResult
                    ? t("install.scan.summary", {
                        tools: scanResult.tools_scanned,
                        skills: scanResult.skills_found,
                      })
                    : t("install.scan.initial")}
                </p>
              </div>

              <div className="flex items-center gap-2">
                <button
                  onClick={runScan}
                  disabled={scanLoading}
                  className="inline-flex items-center gap-1.5 rounded-lg border border-border bg-surface-hover px-3 py-2 text-[13px] font-medium text-secondary transition-colors hover:bg-surface-active disabled:opacity-50"
                >
                  <RefreshCw className={cn("h-3.5 w-3.5", scanLoading && "animate-spin")} />
                  {t("install.scan.rescan")}
                </button>
                <button
                  onClick={handleImportAllDiscovered}
                  disabled={scanLoading || importingAll || pendingGroups.length === 0}
                  className="inline-flex items-center gap-1.5 rounded-lg border border-accent-border bg-accent-dark px-3 py-2 text-[13px] font-medium text-white transition-colors hover:bg-accent disabled:opacity-50"
                >
                  {importingAll ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <DownloadCloud className="h-3.5 w-3.5" />
                  )}
                  {t("install.scan.importAll")}
                </button>
              </div>
            </div>

            <div className="space-y-4 p-4">
              {scanLoading ? (
                <div className="flex items-center justify-center gap-2.5 py-12 text-muted">
                  <Loader2 className="h-4 w-4 animate-spin" />
                  <span className="text-[13px]">{t("install.scan.scanning")}</span>
                </div>
              ) : scanResult && scanGroups.length === 0 ? (
                <div className="flex flex-col items-center justify-center py-12 text-center">
                  <div className="mb-3 flex h-10 w-10 items-center justify-center rounded-lg border border-border bg-surface-hover">
                    <FolderSearch className="h-5 w-5 text-muted" />
                  </div>
                  <h3 className="mb-1 text-[13px] font-semibold text-tertiary">
                    {t("install.scan.noResults")}
                  </h3>
                  <p className="text-[13px] text-muted">{t("install.scan.noResultsHint")}</p>
                </div>
              ) : (
                <>
                  <div className="app-panel-muted overflow-hidden">
                    {scanGroups.map((group) => {
                      const [primaryLocation, ...otherLocations] = group.locations;
                      const primaryPath = primaryLocation?.found_path;
                      const isImporting = !!primaryPath && importingPaths.has(primaryPath);

                      return (
                        <article key={group.name} className="border-b border-border-subtle last:border-b-0">
                          <div className="flex items-start justify-between gap-3 px-3 py-2">
                            <div className="min-w-0 flex-1 space-y-1.5">
                              <div className="flex min-w-0 items-center gap-2">
                              <h3 className="truncate text-[13px] font-semibold text-secondary">
                                {group.name}
                              </h3>
                              {group.imported ? (
                                <span className="inline-flex shrink-0 items-center gap-1 rounded-full border border-emerald-500/20 bg-emerald-500/10 px-2 py-0.5 text-[13px] font-semibold text-emerald-400">
                                  <Check className="h-3 w-3" />
                                  {t("install.scan.imported")}
                                </span>
                              ) : null}
                              <span className="shrink-0 rounded-full border border-border-subtle bg-surface px-2 py-0.5 text-[13px] text-muted">
                                {t("install.scan.locations", { count: group.locations.length })}
                              </span>
                              </div>

                              {primaryLocation ? (
                                <div className="flex min-w-0 items-center gap-2">
                                  <span className="inline-flex shrink-0 rounded-[4px] border border-border-subtle bg-surface px-1.5 py-px text-[13px] font-medium text-tertiary">
                                    {primaryLocation.tool}
                                  </span>
                                  <code className="block min-w-0 truncate text-[13px] text-tertiary">
                                    {primaryLocation.found_path}
                                  </code>
                                </div>
                              ) : null}
                            </div>

                            <div className="flex shrink-0 items-start justify-end">
                              {group.imported ? null : (
                                <button
                                  onClick={() => primaryPath && handleImportDiscovered(primaryPath, group.name)}
                                  disabled={!primaryPath || isImporting}
                                  className="inline-flex items-center justify-center gap-1.5 rounded-[6px] border border-accent-border bg-accent-dark px-2.5 py-1.5 text-[13px] font-medium text-white transition-colors hover:bg-accent disabled:opacity-50"
                                >
                                  {isImporting ? (
                                    <Loader2 className="h-3 w-3 animate-spin" />
                                  ) : (
                                    <DownloadCloud className="h-3 w-3" />
                                  )}
                                  {t("install.scan.importOne")}
                                </button>
                              )}
                            </div>
                          </div>

                          {otherLocations.length > 0 ? (
                            <div className="border-t border-border-subtle bg-surface/40 px-3 py-1.5">
                              <div className="space-y-1">
                                {otherLocations.map((location) => (
                                  <div key={location.id} className="flex min-w-0 items-center gap-2">
                                    <span className="inline-flex shrink-0 rounded-[4px] border border-border-subtle bg-surface px-1.5 py-px text-[13px] font-medium text-tertiary">
                                      {location.tool}
                                    </span>
                                    <code className="block min-w-0 truncate text-[13px] text-muted">
                                      {location.found_path}
                                    </code>
                                  </div>
                                ))}
                              </div>
                            </div>
                          ) : null}
                        </article>
                      );
                    })}
                  </div>
                </>
              )}
            </div>
          </section>
        </div>
      )}

      {activeTab === "git" && (
        <div className="animate-in fade-in duration-300">
          <div className="app-panel max-w-lg p-5">
            <div className="mb-4 flex h-10 w-10 items-center justify-center rounded-lg border border-border bg-surface-hover">
              <Github className="h-5 w-5 text-tertiary" />
            </div>
            <h2 className="mb-1 text-[14px] font-semibold text-primary">{t("install.gitTitle")}</h2>
            <p className="mb-4 text-[13px] text-muted">{t("install.gitDesc")}</p>

            <div className="space-y-3">
              <div>
                <label className="mb-1 block text-[13px] font-medium text-tertiary">
                  {t("install.repoUrl")}
                </label>
                <input
                  type="text"
                  value={gitUrl}
                  onChange={(e) => setGitUrl(e.target.value)}
                  placeholder={t("install.repoUrlPlaceholder")}
                  disabled={gitLoading}
                  className="app-input w-full bg-background"
                />
              </div>
              <div>
                <label className="mb-1 flex items-center gap-2 text-[13px] font-medium text-tertiary">
                  {t("install.customName")}
                  <span className="text-[13px] font-normal text-muted">
                    {t("install.customNameOptional")}
                  </span>
                </label>
                <input
                  type="text"
                  value={gitName}
                  onChange={(e) => setGitName(e.target.value)}
                  placeholder={t("install.customNamePlaceholder")}
                  disabled={gitLoading}
                  className="app-input w-full bg-background"
                />
              </div>
              <div className="flex gap-2 pt-2">
                {gitLoading ? (
                  <button
                    onClick={() => gitCancelKey && handleCancelInstall(gitCancelKey)}
                    className="inline-flex w-full items-center justify-center gap-2 rounded-lg border border-red-500/30 bg-red-500/10 px-4 py-2.5 text-[13px] font-medium text-red-400 transition-colors hover:bg-red-500/20"
                    disabled={!gitCancelKey}
                  >
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    {t("install.cancel")}
                  </button>
                ) : (
                  <button
                    onClick={handleGitInstall}
                    disabled={!gitUrl.trim()}
                    className="app-button-primary flex w-full"
                  >
                    <DownloadCloud className="h-3.5 w-3.5" />
                    {t("install.installClone")}
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
