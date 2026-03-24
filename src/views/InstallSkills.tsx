import { useState, useEffect, useLayoutEffect, useCallback, useRef, useMemo, useDeferredValue } from "react";
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
  FolderInput,
  ExternalLink,
  Check,
  ChevronLeft,
  ChevronRight,
  Search,
  X,
  MoreHorizontal,
  Pencil,
  Calendar,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import * as api from "../lib/tauri";
import type { ScanResult, SkillsShSkill, BatchImportResult, GitPreviewResult } from "../lib/tauri";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useSearchParams, useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { StatusBanner } from "../components/StatusBanner";
import { getErrorMessage, getErrorKind } from "../lib/error";

const MARKET_PAGE_SIZE = 24;
const MARKET_SEARCH_STEP = 60;
const MARKET_SEARCH_DEBOUNCE_MS = 450;
const MARKET_SEARCH_CACHE_TTL_MS = 120_000;
const MARKET_SEARCH_CACHE_MAX_ENTRIES = 150;

export function InstallSkills() {
  const { t } = useTranslation();
  const { refreshScenarios, refreshManagedSkills, managedSkills, openSkillDetailById } = useApp();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const [activeTab, setActiveTab] = useState<"market" | "local" | "git">("market");
  const [marketTab, setMarketTab] = useState<"hot" | "trending" | "alltime">("alltime");
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
  const [gitLoading, setGitLoading] = useState(false);
  const [gitCancelKey, setGitCancelKey] = useState<string | null>(null);
  const [gitPreview, setGitPreview] = useState<GitPreviewResult | null>(null);
  const [gitPreviewRepoUrl, setGitPreviewRepoUrl] = useState<string | null>(null);
  const [gitSelections, setGitSelections] = useState<{ dir_name: string; name: string; description: string | null; selected: boolean }[]>([]);
  const [gitConfirmLoading, setGitConfirmLoading] = useState(false);
  const [scanResult, setScanResult] = useState<ScanResult | null>(null);
  const [scanLoading, setScanLoading] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const [importingPaths, setImportingPaths] = useState<Set<string>>(new Set());
  const [importingAll, setImportingAll] = useState(false);
  const [renameEditing, setRenameEditing] = useState<Record<string, string>>({});
  const [expandedSources, setExpandedSources] = useState<Set<string>>(new Set());
  const [aiSearch, setAiSearch] = useState(false);
  const [skillsmpApiKey, setSkillsmpApiKey] = useState<string | null>(null);
  const marketListRef = useRef<HTMLDivElement | null>(null);
  const [sourceOverflowOpen, setSourceOverflowOpen] = useState(false);
  const [sourceOverflowSide, setSourceOverflowSide] = useState<"left" | "right">("left");
  const [sourceSearch, setSourceSearch] = useState("");
  const [sourceFocusedIndex, setSourceFocusedIndex] = useState(-1);
  const sourceListRef = useRef<HTMLDivElement | null>(null);
  const [visibleSourceCount, setVisibleSourceCount] = useState<number>(Infinity);
  const sourceOverflowBtnRef = useRef<HTMLButtonElement | null>(null);
  const sourceOverflowPanelRef = useRef<HTMLDivElement | null>(null);
  const filterContainerRef = useRef<HTMLDivElement | null>(null);
  const allBtnMeasureRef = useRef<HTMLButtonElement | null>(null);
  const moreBtnMeasureRef = useRef<HTMLButtonElement | null>(null);
  const sourceMeasureRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const marketSearchCacheRef = useRef<Map<string, { timestamp: number; data: SkillsShSkill[] }>>(new Map());
  const marketSkillsLengthRef = useRef(0);
  const [debouncedMarketQuery, setDebouncedMarketQuery] = useState("");
  const deferredMarketQuery = useDeferredValue(marketQuery);
  const resetSourceOverflowState = useCallback(() => {
    setSourceOverflowOpen(false);
    setSourceSearch("");
    setSourceFocusedIndex(-1);
  }, []);

  const managedSkillsRef = useRef(managedSkills);
  managedSkillsRef.current = managedSkills;

  const goToSkill = useCallback((skillName: string) => {
    // Use ref to get the latest managedSkills after refresh
    const skills = managedSkillsRef.current;
    const skill = skills.find(
      (s) => s.name === skillName || s.source_ref === skillName
    );
    if (skill) {
      openSkillDetailById(skill.id);
    }
    navigate("/my-skills");
  }, [navigate, openSkillDetailById]);

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

  const findInstalledByGitUrl = useCallback((url: string) => {
    const trimmed = url.trim().replace(/\.git$/, "").toLowerCase();
    return managedSkills.find((s) => {
      if (!s.source_ref) return false;
      const ref = s.source_ref.replace(/\.git$/, "").toLowerCase();
      return ref === trimmed || ref.endsWith("/" + trimmed.split("/").slice(-2).join("/"));
    });
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
    if (!sourceOverflowOpen) return;
    const handleClickOutside = (e: MouseEvent) => {
      if (
        sourceOverflowBtnRef.current?.contains(e.target as Node) ||
        sourceOverflowPanelRef.current?.contains(e.target as Node)
      ) return;
      resetSourceOverflowState();
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [resetSourceOverflowState, sourceOverflowOpen]);

  useEffect(() => {
    api.getSettings("skillsmp_api_key").then((v) => setSkillsmpApiKey(v || null));
  }, []);

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
    } catch (error: unknown) {
      console.error(error);
      const message = getErrorMessage(error, t("common.error"));
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
      const cacheKey = `${query.toLowerCase()}|${aiSearch ? "ai" : "kw"}|${marketSearchLimit}`;
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
      ? (aiSearch
        ? api.searchSkillsmp(query, true, undefined, marketSearchLimit)
        : api.searchSkillssh(query, marketSearchLimit))
      : api.fetchLeaderboard(marketTab);

    request
      .then((result) => {
        if (stale) return;
        setMarketSkills(result);
        if (query.length > 0 && !loadingMore) {
          const cacheKey = `${query.toLowerCase()}|${aiSearch ? "ai" : "kw"}|${marketSearchLimit}`;
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
  }, [activeTab, aiSearch, debouncedMarketQuery, marketReloadKey, marketSearchLimit, marketTab, pruneMarketSearchCache, t]);

  useEffect(() => {
    if (activeTab === "local" && !scanResult && !scanLoading) {
      runScan();
    }
  }, [activeTab, scanLoading, scanResult, runScan]);

  const installLocalSource = async (sourcePath: string) => {
    const name = sourcePath.split("/").pop() || sourcePath;
    const toastId = toast.loading(t("install.toast.installing", { name }));
    try {
      await api.installLocal(sourcePath);
      await Promise.all([refreshScenarios(), refreshManagedSkills()]);
      await runScan();
      toast.success(t("install.toast.success", { name }), {
        id: toastId,
        action: {
          label: t("install.toast.view"),
          onClick: () => goToSkill(name),
        },
      });
    } catch (e) {
      const message = (e as Error)?.toString?.() || t("common.error");
      setLocalError(message);
      toast.error(message, { id: toastId });
    }
  };

  const handleLocalFolderInstall = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
      });
      if (!selected) return;
      installLocalSource(selected as string);
    } catch (error: unknown) {
      const message = getErrorMessage(error, t("common.error"));
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
    } catch (error: unknown) {
      const message = getErrorMessage(error, t("common.error"));
      setLocalError(message);
      toast.error(message);
    }
  };

  const handleBatchImportFolder = async () => {
    let unlisten: (() => void) | null = null;
    try {
      const selected = await open({
        directory: true,
        multiple: false,
      });
      if (!selected) return;

      const toastId = toast.loading(t("install.local.batchImporting"));

      unlisten = await listen<{ current: number; total: number; name: string }>(
        "batch-import-progress",
        (event) => {
          const { current, total, name } = event.payload;
          toast.loading(
            t("install.local.batchProgress", { current, total, name }),
            { id: toastId }
          );
        }
      );

      const result: BatchImportResult = await api.batchImportFolder(
        selected as string
      );

      if (result.errors.length > 0) {
        const previewErrors = result.errors.slice(0, 3).join("; ");
        const remaining = result.errors.length - 3;
        const detail = remaining > 0 ? `${previewErrors}; +${remaining} more` : previewErrors;
        toast.error(
          `${t("install.local.batchErrors", { count: result.errors.length })}: ${detail}`,
          { id: toastId }
        );
      } else if (result.imported === 0) {
        toast.info(
          t("install.local.batchAllSkipped", { skipped: result.skipped }),
          { id: toastId }
        );
      } else {
        toast.success(
          t("install.local.batchSuccess", {
            imported: result.imported,
            skipped: result.skipped,
          }),
          { id: toastId }
        );
      }

      await Promise.all([refreshScenarios(), refreshManagedSkills()]);
      runScan();
    } catch (error: unknown) {
      const message = getErrorMessage(error, t("common.error"));
      setLocalError(message);
      toast.error(message);
    } finally {
      unlisten?.();
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
      toast.success(t("install.toast.success", { name: displayName }), {
        id: toastId,
        action: {
          label: t("install.toast.view"),
          onClick: () => goToSkill(displayName),
        },
      });
    } catch (error: unknown) {
      if (getErrorKind(error) === "cancelled") {
        toast.info(t("install.toast.cancelled"), { id: toastId });
      } else {
        toast.error(getErrorMessage(error, t("common.error")), { id: toastId });
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

  const handleGitPreview = async () => {
    if (!gitUrl.trim()) return;
    setGitLoading(true);
    const url = gitUrl.trim();
    setGitCancelKey(url);

    const toastId = toast.loading(t("install.toast.cloning"));
    let unlisten: (() => void) | null = null;

    try {
      unlisten = await listen<{ skill_id: string; phase: string }>(
        "install-progress",
        (event) => {
          if (event.payload.skill_id !== url) return;
          if (event.payload.phase === "cloning") {
            toast.loading(t("install.toast.cloning"), { id: toastId });
          }
        }
      );
      const preview = await api.previewGitInstall(url);
      toast.dismiss(toastId);
      setGitPreview(preview);
      setGitPreviewRepoUrl(url);
      setGitSelections(preview.skills.map((s) => ({
        dir_name: s.dir_name,
        name: s.name,
        description: s.description,
        selected: true,
      })));
    } catch (error: unknown) {
      if (getErrorKind(error) === "cancelled") {
        toast.info(t("install.toast.cancelled"), { id: toastId });
      } else {
        toast.error(getErrorMessage(error, t("common.error")), { id: toastId });
      }
    } finally {
      setGitLoading(false);
      setGitCancelKey(null);
      unlisten?.();
    }
  };

  const handleGitPreviewClose = () => {
    if (gitConfirmLoading) return;
    if (gitPreview) {
      api.cancelGitPreview(gitPreview.temp_dir).catch(() => {});
    }
    setGitPreview(null);
    setGitPreviewRepoUrl(null);
    setGitSelections([]);
  };

  const handleGitConfirm = async () => {
    if (!gitPreview) return;
    const repoUrl = gitPreviewRepoUrl ?? gitUrl.trim();
    if (!repoUrl) return;
    const selected = gitSelections.filter((s) => s.selected);
    if (selected.length === 0) return;
    setGitConfirmLoading(true);
    try {
      await api.confirmGitInstall(
        repoUrl,
        gitPreview.temp_dir,
        selected.map((s) => ({ dir_name: s.dir_name, name: s.name }))
      );
      await Promise.all([refreshScenarios(), refreshManagedSkills()]);
      toast.success(t("install.toast.success", { name: selected.map((s) => s.name).join(", ") }));
      setGitUrl("");
      setGitPreview(null);
      setGitPreviewRepoUrl(null);
      setGitSelections([]);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setGitConfirmLoading(false);
    }
  };

  const handleImportDiscovered = async (sourcePath: string, name: string) => {
    setImportingPaths((prev) => new Set(prev).add(sourcePath));
    try {
      await api.importExistingSkill(sourcePath, name);
      toast.success(t("install.scan.importedOne", { name }));
      await Promise.all([refreshScenarios(), refreshManagedSkills()]);
      await runScan();
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
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
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
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
    () => Array.from(new Set(marketSkills.map((skill) => skill.source))),
    [marketSkills]
  );

  // Trim stale measurement refs when sourceOptions shrinks
  useEffect(() => {
    sourceMeasureRefs.current.length = sourceOptions.length;
  }, [sourceOptions.length]);

  const computeVisibleCount = useCallback(() => {
    const container = filterContainerRef.current;
    if (!container || sourceOptions.length === 0) {
      setVisibleSourceCount(sourceOptions.length);
      return;
    }
    const GAP = 6; // gap-1.5 = 6px
    const containerWidth = container.offsetWidth;
    const allBtnWidth = allBtnMeasureRef.current?.offsetWidth ?? 80;
    const moreBtnWidth = moreBtnMeasureRef.current?.offsetWidth ?? 28;
    const available = containerWidth - allBtnWidth - GAP;
    const widths = sourceOptions.map((_, i) => sourceMeasureRefs.current[i]?.offsetWidth ?? 0);
    const totalNeeded = widths.reduce((sum, w) => sum + w + GAP, 0);
    if (totalNeeded <= available) {
      setVisibleSourceCount(sourceOptions.length);
      resetSourceOverflowState();
      return;
    }
    const availableWithMore = available - moreBtnWidth - GAP;
    let used = 0;
    let count = 0;
    for (const w of widths) {
      if (used + w + GAP <= availableWithMore) {
        used += w + GAP;
        count++;
      } else {
        break;
      }
    }
    setVisibleSourceCount(count);
  }, [resetSourceOverflowState, sourceOptions]);

  useLayoutEffect(() => {
    computeVisibleCount();
  }, [computeVisibleCount]);

  useEffect(() => {
    const container = filterContainerRef.current;
    if (!container) return;
    const observer = new ResizeObserver(computeVisibleCount);
    observer.observe(container);
    return () => observer.disconnect();
  }, [computeVisibleCount]);

  const filteredMarketSkills = useMemo(() => {
    const filtered = marketSourceFilter === "all"
      ? marketSkills
      : marketSkills.filter((skill) => skill.source === marketSourceFilter);
    if (debouncedMarketQuery.trim().length > 0) {
      return [...filtered].sort((a, b) => b.installs - a.installs);
    }
    return filtered;
  }, [marketSkills, marketSourceFilter, debouncedMarketQuery]);

  // Group skills by source: show top skill + collapse rest behind "+N more"
  type MarketEntry =
    | { type: "skill"; skill: SkillsShSkill }
    | { type: "collapsed"; source: string; skills: SkillsShSkill[]; totalInstalls: number };
  const groupedMarketEntries = useMemo<MarketEntry[]>(() => {
    // When filtering a specific source or searching, show all skills flat
    if (marketSourceFilter !== "all" || debouncedMarketQuery.trim().length > 0) {
      return filteredMarketSkills.map((skill) => ({ type: "skill" as const, skill }));
    }
    const sourceMap = new Map<string, SkillsShSkill[]>();
    for (const skill of filteredMarketSkills) {
      const arr = sourceMap.get(skill.source);
      if (arr) arr.push(skill);
      else sourceMap.set(skill.source, [skill]);
    }
    const entries: MarketEntry[] = [];
    const seen = new Set<string>();
    for (const skill of filteredMarketSkills) {
      if (seen.has(skill.source)) continue;
      seen.add(skill.source);
      const group = sourceMap.get(skill.source)!;
      entries.push({ type: "skill", skill: group[0] });
      if (group.length > 1) {
        const rest = group.slice(1);
        const totalInstalls = group.reduce((sum, s) => sum + s.installs, 0);
        entries.push({ type: "collapsed", source: skill.source, skills: rest, totalInstalls });
      }
    }
    return entries;
  }, [filteredMarketSkills, marketSourceFilter, debouncedMarketQuery]);

  // Expand collapsed entries based on expandedSources
  const visibleMarketEntries = useMemo<MarketEntry[]>(() => {
    const result: MarketEntry[] = [];
    for (const entry of groupedMarketEntries) {
      if (entry.type === "skill") {
        result.push(entry);
      } else if (expandedSources.has(entry.source)) {
        for (const skill of entry.skills) {
          result.push({ type: "skill", skill });
        }
      } else {
        result.push(entry);
      }
    }
    return result;
  }, [groupedMarketEntries, expandedSources]);

  const totalMarketPages = Math.max(1, Math.ceil(visibleMarketEntries.length / MARKET_PAGE_SIZE));
  const currentMarketPage = Math.min(marketPage, totalMarketPages);
  const marketPageStart = (currentMarketPage - 1) * MARKET_PAGE_SIZE;
  const paginatedMarketEntries = visibleMarketEntries.slice(
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

  const overflowSources = sourceOptions.slice(visibleSourceCount);
  const filteredOverflowSources = sourceSearch
    ? overflowSources.filter((s) => s.toLowerCase().includes(sourceSearch.toLowerCase()))
    : overflowSources;

  useEffect(() => {
    if (sourceOverflowOpen && visibleSourceCount >= sourceOptions.length) {
      resetSourceOverflowState();
    }
  }, [resetSourceOverflowState, sourceOptions.length, sourceOverflowOpen, visibleSourceCount]);

  useEffect(() => {
    setSourceFocusedIndex((idx) => {
      if (filteredOverflowSources.length === 0) return -1;
      if (idx < 0) return idx;
      return Math.min(idx, filteredOverflowSources.length - 1);
    });
  }, [filteredOverflowSources.length]);

  // Scroll the focused overflow item into view whenever the index changes
  useEffect(() => {
    if (sourceFocusedIndex < 0) return;
    sourceListRef.current
      ?.children[sourceFocusedIndex]
      ?.scrollIntoView({ block: "nearest" });
  }, [sourceFocusedIndex]);

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
                        { id: "alltime" as const, label: t("install.all"), icon: Clock },
                        { id: "trending" as const, label: t("install.trending"), icon: TrendingUp },
                        { id: "hot" as const, label: t("install.hot"), icon: Star },
                      ].map((tab) => {
                        const Icon = tab.icon;
                        const isActive = marketTab === tab.id;
                        return (
                          <button
                            key={tab.id}
                            onClick={() => { setMarketTab(tab.id); setExpandedSources(new Set()); }}
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
                      placeholder={aiSearch ? t("install.aiSearchPlaceholder", { defaultValue: "AI search — describe what you need..." }) : t("install.searchMarket")}
                      className="app-input w-full bg-background pl-9"
                      autoCapitalize="none"
                      autoCorrect="off"
                      spellCheck={false}
                    />
                  </div>
                  <button
                    onClick={() => {
                      if (skillsmpApiKey) {
                        setAiSearch((v) => !v);
                      } else {
                        toast.info(
                          t("install.aiSearchNoKey", { defaultValue: "Set your SkillsMP API key in Settings to enable AI search" }),
                          {
                            action: {
                              label: t("common.goToSettings", { defaultValue: "Settings" }),
                              onClick: () => navigate("/settings"),
                            },
                          }
                        );
                      }
                    }}
                    className={cn(
                      "shrink-0 rounded-[6px] border px-2.5 py-1.5 text-[13px] font-medium transition-colors",
                      aiSearch && skillsmpApiKey
                        ? "border-accent-border bg-accent-dark text-white"
                        : "border-border-subtle bg-surface text-muted hover:bg-surface-hover"
                    )}
                    title={t("install.aiSearchToggle", { defaultValue: "AI-powered search (SkillsMP)" })}
                  >
                    AI
                  </button>
                </div>
              </div>

              <div className="border-t border-border-subtle pt-2">
                <div className="flex items-center gap-3">
                  <span className="shrink-0 text-[13px] font-medium text-tertiary">
                    {t("install.filters.source")}
                  </span>
                  <div ref={filterContainerRef} className="relative min-w-0 flex-1">
                    {/* Hidden measurement layer — never visible, keeps all pills in DOM for width queries */}
                    <div className="pointer-events-none invisible absolute left-0 top-0 flex h-0 items-center gap-1.5 overflow-hidden" aria-hidden="true">
                      <button
                        ref={allBtnMeasureRef}
                        tabIndex={-1}
                        className="rounded-full border px-2.5 py-1 text-[13px] font-medium whitespace-nowrap"
                      >
                        {t("install.filters.allSources")}
                      </button>
                      {sourceOptions.map((source, i) => (
                        <button
                          key={source}
                          ref={(el) => { sourceMeasureRefs.current[i] = el; }}
                          tabIndex={-1}
                          className="rounded-full border px-2.5 py-1 text-[13px] font-medium whitespace-nowrap"
                        >
                          @{source}
                        </button>
                      ))}
                      <button
                        ref={moreBtnMeasureRef}
                        tabIndex={-1}
                        className="flex items-center rounded-full border px-2 py-1"
                      >
                        <MoreHorizontal className="h-3.5 w-3.5" />
                      </button>
                    </div>
                    {/* Visible row */}
                    <div className="flex items-center gap-1.5">
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
                      {sourceOptions.slice(0, visibleSourceCount).map((source) => (
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
                      {visibleSourceCount < sourceOptions.length && (
                        <div className="relative">
                          <button
                            ref={sourceOverflowBtnRef}
                            type="button"
                            onClick={() => {
                              if (sourceOverflowBtnRef.current) {
                                const rect = sourceOverflowBtnRef.current.getBoundingClientRect();
                                setSourceOverflowSide(rect.left + 192 > window.innerWidth ? "right" : "left");
                              }
                              setSourceOverflowOpen((v) => {
                                if (v) {
                                  setSourceSearch("");
                                  setSourceFocusedIndex(-1);
                                }
                                return !v;
                              });
                            }}
                            className={cn(
                              "flex items-center rounded-full border px-2 py-1 text-[13px] font-medium transition-colors",
                              sourceOverflowOpen
                                ? "border-accent-border bg-accent-bg text-accent-light"
                                : "border-border-subtle bg-background text-muted hover:text-secondary"
                            )}
                            title={`${sourceOptions.length - visibleSourceCount} more`}
                            aria-expanded={sourceOverflowOpen}
                            aria-haspopup="listbox"
                          >
                            <MoreHorizontal className="h-3.5 w-3.5" />
                          </button>
                          {sourceOverflowOpen && (
                            <div
                              ref={sourceOverflowPanelRef}
                              role="listbox"
                              className={cn(
                                "absolute top-full z-50 mt-1.5 w-48 overflow-hidden rounded-xl border border-border bg-surface shadow-lg",
                                sourceOverflowSide === "left" ? "left-0" : "right-0"
                              )}
                            >
                              <div className="border-b border-border-subtle px-2 py-1.5">
                                <div className="relative">
                                  <Search className="pointer-events-none absolute left-2 top-1/2 h-3 w-3 -translate-y-1/2 text-muted" />
                                  <input
                                    type="text"
                                    value={sourceSearch}
                                    onChange={(e) => {
                                      setSourceSearch(e.target.value);
                                      setSourceFocusedIndex(-1);
                                    }}
                                    onKeyDown={(e) => {
                                      if (e.key === "ArrowDown") {
                                        e.preventDefault();
                                        if (filteredOverflowSources.length === 0) return;
                                        setSourceFocusedIndex((i) =>
                                          Math.min(i + 1, filteredOverflowSources.length - 1)
                                        );
                                      } else if (e.key === "ArrowUp") {
                                        e.preventDefault();
                                        if (filteredOverflowSources.length === 0) return;
                                        setSourceFocusedIndex((i) =>
                                          i <= 0 ? 0 : i - 1
                                        );
                                      } else if (e.key === "Enter" && sourceFocusedIndex >= 0) {
                                        const target = filteredOverflowSources[sourceFocusedIndex];
                                        if (target) {
                                          setMarketSourceFilter(target);
                                          resetSourceOverflowState();
                                        }
                                      } else if (e.key === "Escape") {
                                        resetSourceOverflowState();
                                      }
                                    }}
                                    placeholder={t("common.search")}
                                    className="app-input w-full bg-background py-1 pl-6 pr-2 text-[12px]"
                                    autoFocus
                                    autoCapitalize="none"
                                    autoCorrect="off"
                                    spellCheck={false}
                                  />
                                </div>
                              </div>
                              <div ref={sourceListRef} className="max-h-48 overflow-y-auto scrollbar-hide py-1">
                                {filteredOverflowSources.map((source, idx) => (
                                  <button
                                    key={source}
                                    type="button"
                                    role="option"
                                    aria-selected={marketSourceFilter === source}
                                    onClick={() => {
                                      setMarketSourceFilter(source);
                                      resetSourceOverflowState();
                                    }}
                                    className={cn(
                                      "flex w-full items-center px-3 py-1.5 text-left text-[13px] transition-colors",
                                      idx === sourceFocusedIndex
                                        ? "bg-surface-hover text-primary"
                                        : marketSourceFilter === source
                                          ? "bg-accent-bg text-accent-light"
                                          : "text-secondary hover:bg-surface-hover"
                                    )}
                                  >
                                    @{source}
                                  </button>
                                ))}
                              </div>
                            </div>
                          )}
                        </div>
                      )}
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
                    {paginatedMarketEntries.map((entry) => {
                      if (entry.type === "collapsed") {
                        const owner = entry.source.split("/")[0];
                        const avatarUrl = `https://github.com/${owner}.png?size=32`;
                        const formatCount = (n: number) =>
                          n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M`
                            : n >= 1_000 ? `${(n / 1_000).toFixed(1)}K`
                            : String(n);
                        return (
                          <button
                            key={`collapsed-${entry.source}`}
                            onClick={() =>
                              setExpandedSources((prev) => {
                                const next = new Set(prev);
                                next.add(entry.source);
                                return next;
                              })
                            }
                            className="app-panel col-span-2 flex items-center gap-2 p-3 text-left text-[13px] text-muted transition-colors hover:border-border hover:text-secondary lg:col-span-3"
                          >
                            <img
                              src={avatarUrl}
                              alt={owner}
                              className="h-5 w-5 shrink-0 rounded-full border border-border-subtle"
                              loading="lazy"
                            />
                            <span>
                              +{entry.skills.length} more from{" "}
                              <span className="font-medium text-accent-light">{entry.source}</span>
                              {marketTab === "alltime" && entry.totalInstalls > 0 && (
                                <span className="ml-1 text-faint">
                                  ({formatCount(entry.totalInstalls)} total)
                                </span>
                              )}
                            </span>
                          </button>
                        );
                      }

                      const skill = entry.skill;
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
                                className="inline-flex items-center gap-1 rounded-[5px] border border-red-500/30 bg-red-500/10 px-1.5 py-1 text-red-400 transition-colors hover:bg-red-500/20"
                                title={t("install.cancel")}
                                aria-label={t("install.cancel")}
                              >
                                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                <span className="text-[11px] leading-none font-medium">
                                  {t("install.cancel")}
                                </span>
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
                          {marketTab === "alltime" && skill.installs > 0 && (
                            <span className="inline-flex items-center gap-1 rounded-[5px] border border-border-subtle bg-background px-1.5 py-0.5 text-[13px] leading-4 text-muted">
                              <DownloadCloud className="h-3 w-3" />
                              {skill.installs >= 1_000_000
                                ? `${(skill.installs / 1_000_000).toFixed(1)}M`
                                : skill.installs >= 1_000
                                  ? `${(skill.installs / 1_000).toFixed(1)}K`
                                  : skill.installs}
                            </span>
                          )}
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
                  <button
                    type="button"
                    onClick={handleBatchImportFolder}
                    className="app-button-secondary bg-background"
                  >
                    <FolderInput className="h-4 w-4" />
                    {t("install.local.batchImport")}
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
                      const isRenaming = group.name in renameEditing;
                      const importName = renameEditing[group.name] ?? group.name;
                      const foundDate = new Date(group.found_at).toLocaleDateString(undefined, {
                        year: "numeric",
                        month: "short",
                        day: "numeric",
                      });

                      return (
                        <article key={group.name} className="border-b border-border-subtle last:border-b-0">
                          <div className="flex items-start justify-between gap-3 px-3 py-2">
                            <div className="min-w-0 flex-1 space-y-1.5">
                              <div className="flex min-w-0 items-center gap-2">
                                {isRenaming ? (
                                  <input
                                    autoFocus
                                    value={renameEditing[group.name]}
                                    onChange={(e) =>
                                      setRenameEditing((prev) => ({ ...prev, [group.name]: e.target.value }))
                                    }
                                    onBlur={() => {
                                      if (!renameEditing[group.name]?.trim()) {
                                        setRenameEditing((prev) => {
                                          const next = { ...prev };
                                          delete next[group.name];
                                          return next;
                                        });
                                      }
                                    }}
                                    onKeyDown={(e) => {
                                      if (e.key === "Escape") {
                                        setRenameEditing((prev) => {
                                          const next = { ...prev };
                                          delete next[group.name];
                                          return next;
                                        });
                                      } else if (e.key === "Enter") {
                                        (e.target as HTMLInputElement).blur();
                                      }
                                    }}
                                    className="min-w-0 max-w-[220px] rounded border border-accent-border bg-surface px-1.5 py-0.5 text-[13px] font-semibold text-secondary outline-none focus:ring-1 focus:ring-accent"
                                  />
                                ) : (
                                  <h3 className="truncate text-[13px] font-semibold text-secondary">
                                    {group.name}
                                  </h3>
                                )}
                                {!group.imported && !isRenaming ? (
                                  <button
                                    onClick={() =>
                                      setRenameEditing((prev) => ({ ...prev, [group.name]: group.name }))
                                    }
                                    className="shrink-0 rounded p-0.5 text-muted transition-colors hover:bg-surface-hover hover:text-secondary"
                                    title={t("install.scan.rename")}
                                  >
                                    <Pencil className="h-3 w-3" />
                                  </button>
                                ) : null}
                                {group.imported ? (
                                  <span className="inline-flex shrink-0 items-center gap-1 rounded-full border border-emerald-500/20 bg-emerald-500/10 px-2 py-0.5 text-[13px] font-semibold text-emerald-400">
                                    <Check className="h-3 w-3" />
                                    {t("install.scan.imported")}
                                  </span>
                                ) : null}
                                <span className="shrink-0 rounded-full border border-border-subtle bg-surface px-2 py-0.5 text-[13px] text-muted">
                                  {t("install.scan.locations", { count: group.locations.length })}
                                </span>
                                <span className="inline-flex shrink-0 items-center gap-1 text-[11px] text-muted">
                                  <Calendar className="h-3 w-3" />
                                  {foundDate}
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
                                  onClick={() => primaryPath && handleImportDiscovered(primaryPath, importName)}
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
                  onKeyDown={(e) => { if (e.key === "Enter" && !gitLoading && gitUrl.trim()) handleGitPreview(); }}
                  placeholder={t("install.repoUrlPlaceholder")}
                  disabled={gitLoading}
                  className="app-input w-full bg-background"
                />
              </div>
              {gitUrl.trim() && findInstalledByGitUrl(gitUrl) && (
                <div className="flex items-center gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-[13px] text-amber-400">
                  <Check className="h-3.5 w-3.5 shrink-0" />
                  <span>
                    {t("install.gitAlreadyInstalled", { name: findInstalledByGitUrl(gitUrl)!.name })}
                  </span>
                </div>
              )}
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
                    onClick={handleGitPreview}
                    disabled={!gitUrl.trim()}
                    className={cn(
                      "flex w-full",
                      gitUrl.trim() && findInstalledByGitUrl(gitUrl)
                        ? "app-button-secondary bg-background"
                        : "app-button-primary"
                    )}
                  >
                    <DownloadCloud className="h-3.5 w-3.5" />
                    {gitUrl.trim() && findInstalledByGitUrl(gitUrl)
                      ? t("install.gitReinstall")
                      : t("install.installClone")}
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Git preview / selection dialog */}
      {gitPreview && (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
          <div
            className="absolute inset-0 bg-black/70 backdrop-blur-sm"
            onClick={handleGitPreviewClose}
          />
          <div className="relative w-full max-w-md rounded-xl border border-border bg-surface p-5 shadow-2xl">
            <div className="mb-4 flex items-center justify-between">
              <h2 className="text-[14px] font-semibold text-primary">{t("install.gitPreview.title")}</h2>
              <button
                onClick={handleGitPreviewClose}
                disabled={gitConfirmLoading}
                className="rounded p-1 text-muted transition-colors hover:text-secondary"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            <p className="mb-3 text-[13px] text-muted">{t("install.gitPreview.description")}</p>

            {/* Select all / deselect all */}
            <div className="mb-2 flex gap-2">
              <button
                type="button"
                onClick={() => setGitSelections((prev) => prev.map((s) => ({ ...s, selected: true })))}
                disabled={gitConfirmLoading}
                className="text-[13px] text-accent-light hover:underline"
              >
                {t("install.gitPreview.selectAll")}
              </button>
              <span className="text-faint">·</span>
              <button
                type="button"
                onClick={() => setGitSelections((prev) => prev.map((s) => ({ ...s, selected: false })))}
                disabled={gitConfirmLoading}
                className="text-[13px] text-muted hover:underline"
              >
                {t("install.gitPreview.deselectAll")}
              </button>
            </div>

            {gitSelections.length === 0 ? (
              <p className="py-6 text-center text-[13px] text-muted">{t("install.gitPreview.empty")}</p>
            ) : (
              <div className="max-h-64 space-y-2 overflow-y-auto scrollbar-hide pr-1">
                {gitSelections.map((item, idx) => (
                  <div
                    key={item.dir_name}
                    className={cn(
                      "flex items-center gap-3 rounded-lg border px-3 py-2 transition-colors",
                      item.selected
                        ? "border-accent-border bg-accent-bg/40"
                        : "border-border-subtle bg-background opacity-50"
                    )}
                  >
                    <input
                      type="checkbox"
                      checked={item.selected}
                      disabled={gitConfirmLoading}
                      onChange={(e) =>
                        setGitSelections((prev) =>
                          prev.map((s, i) => i === idx ? { ...s, selected: e.target.checked } : s)
                        )
                      }
                      className="h-4 w-4 shrink-0 accent-accent"
                    />
                    <div className="min-w-0 flex-1">
                      <input
                        type="text"
                        value={item.name}
                        onChange={(e) =>
                          setGitSelections((prev) =>
                            prev.map((s, i) => i === idx ? { ...s, name: e.target.value } : s)
                          )
                        }
                        disabled={!item.selected || gitConfirmLoading}
                        placeholder={t("install.gitPreview.namePlaceholder")}
                        className="app-input w-full bg-background py-1 text-[13px]"
                      />
                      {item.description ? (
                        <p className="mt-1 truncate text-[12px] text-muted">{item.description}</p>
                      ) : null}
                    </div>
                  </div>
                ))}
              </div>
            )}

            <div className="mt-4 flex justify-end gap-2">
              <button
                type="button"
                onClick={handleGitPreviewClose}
                disabled={gitConfirmLoading}
                className="px-3 py-1.5 text-[13px] font-medium text-muted hover:text-secondary transition-colors"
              >
                {t("common.cancel")}
              </button>
              <button
                type="button"
                onClick={handleGitConfirm}
                disabled={gitConfirmLoading || gitSelections.every((s) => !s.selected)}
                className="app-button-primary"
              >
                {gitConfirmLoading ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <DownloadCloud className="h-3.5 w-3.5" />
                )}
                {t("install.gitPreview.confirm")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
