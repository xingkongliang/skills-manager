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
  ChevronDown,
  Search,
  Trash2,
  MoreHorizontal,
  Pencil,
  Calendar,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import * as api from "../lib/tauri";
import type { ScanResult, SkillsShSkill, BatchImportResult, CustomRepo } from "../lib/tauri";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useSearchParams, useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { StatusBanner } from "../components/StatusBanner";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { getErrorMessage, getErrorKind } from "../lib/error";

const MARKET_PAGE_SIZE = 24;
const MARKET_SEARCH_STEP = 60;
const MARKET_SEARCH_DEBOUNCE_MS = 450;
const MARKET_SEARCH_CACHE_TTL_MS = 120_000;
const MARKET_SEARCH_CACHE_MAX_ENTRIES = 150;

/** One skill row inside an expanded custom source (preview row + selection state). */
interface SourceScanRow {
  rel_path: string;
  name: string;
  description: string | null;
  installed: boolean;
  selected: boolean;
}

/**
 * Scan state of one custom source (design.md §3.3). `tempDir` is a real
 * on-disk temp directory owned by this state — see the temp_dir lifecycle
 * rules in design.md §3.4 before changing how it is cleared.
 */
interface SourceScanState {
  loading: boolean;
  /**
   * True while a refresh-scan runs on top of a still-rendered previous scan
   * (design.md §3.3 non-destructive refresh): the old rows keep showing and
   * install/select interactions pause so the temp-dir swap cannot race an
   * install.
   */
  refreshing: boolean;
  /** Live temp dir from preview_git_install; null while loading or after an error with no rows kept (backend cleans up on failure). */
  tempDir: string | null;
  rows: SourceScanRow[];
  error: string | null;
}

/**
 * "3 小时前"-style age of a unix-ms timestamp, shown on the source cards next
 * to the fetch metadata (`last_fetch_at`). Only the number is computed here —
 * the unit strings are i18n keys so each locale keeps its own granularity
 * wording (min/hour/day).
 */
function relativeAge(ms: number, t: TFunction): string {
  const minutes = Math.max(1, Math.floor((Date.now() - ms) / 60_000));
  if (minutes < 60) return t("install.sources.ageMinutes", { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("install.sources.ageHours", { count: hours });
  return t("install.sources.ageDays", { count: Math.floor(hours / 24) });
}

export function InstallSkills() {
  const { t } = useTranslation();
  const { refreshPresets, refreshManagedSkills, managedSkills, openSkillDetailById } = useApp();
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
  // ── Custom sources (git tab) — view-local by design (design.md §3.3) ──
  const [sources, setSources] = useState<CustomRepo[]>([]);
  const [sourcesError, setSourcesError] = useState<string | null>(null);
  const [sourceUrlInput, setSourceUrlInput] = useState("");
  const [addingSource, setAddingSource] = useState(false);
  const [refreshingAllSources, setRefreshingAllSources] = useState(false);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [scans, setScans] = useState<Record<string, SourceScanState>>({});
  const [installingSourceId, setInstallingSourceId] = useState<string | null>(null);
  const [deleteSourceId, setDeleteSourceId] = useState<string | null>(null);
  const [resetSourcesOpen, setResetSourcesOpen] = useState(false);
  const [scanResult, setScanResult] = useState<ScanResult | null>(null);
  const [scanLoading, setScanLoading] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);
  const [importingPaths, setImportingPaths] = useState<Set<string>>(new Set());
  const [importingAll, setImportingAll] = useState(false);
  const [renameEditing, setRenameEditing] = useState<Record<string, string>>({});
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

  // Latest scan states for the unmount cleanup below (same ref pattern as
  // managedSkillsRef above).
  const scansRef = useRef(scans);
  scansRef.current = scans;
  // Per-source scan generation counter: lets a late-resolving preview detect
  // that it was superseded (a delete or a newer scan) and dispose its temp dir.
  const scanGenRef = useRef<Record<string, number>>({});

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

  // Silent variant used after install/import. Never surfaces a toast or
  // new error state — failure here must not mask the install success.
  // Clears any stale localError on success so successful operations don't
  // leave previous error banners behind.
  const runScanSilent = useCallback(async () => {
    try {
      const result = await api.scanLocalSkills();
      setScanResult(result);
      setLocalError(null);
    } catch (error: unknown) {
      console.warn("silent scan failed:", error);
    }
  }, []);

  const warnRejected = (results: PromiseSettledResult<unknown>[], label: string) => {
    for (const r of results) {
      if (r.status === "rejected") console.warn(`${label} failed:`, r.reason);
    }
  };

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

  const installLocalSource = async (sourcePath: string) => {
    const name = sourcePath.split("/").pop() || sourcePath;
    const toastId = toast.loading(t("install.toast.installing", { name }));
    try {
      await api.installLocal(sourcePath);
    } catch (e) {
      const message = getErrorMessage(e, t("common.error"));
      setLocalError(message);
      toast.error(message, { id: toastId });
      return;
    }
    // Install succeeded — post-install refresh is best-effort and must not
    // surface as an install failure.
    const results = await Promise.allSettled([
      refreshPresets(),
      refreshManagedSkills(),
      runScanSilent(),
    ]);
    warnRejected(results, "post-install refresh");
    toast.success(t("install.toast.success", { name }), {
      id: toastId,
      action: {
        label: t("install.toast.view"),
        onClick: () => goToSkill(name),
      },
    });
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

      await Promise.all([refreshPresets(), refreshManagedSkills()]);
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
      unlisten = await listen<{ skill_id: string; phase: string; detail?: string }>(
        "install-progress",
        (event) => {
          if (event.payload.skill_id !== cancelKey) return;
          if (event.payload.phase === "cloning") {
            const detail = event.payload.detail?.trim();
            const msg = detail
              ? `${t("install.toast.cloning")}\n${detail}`
              : t("install.toast.cloning");
            toast.loading(msg, { id: toastId });
          } else if (event.payload.phase === "installing") {
            toast.loading(t("install.toast.installing", { name: displayName }), { id: toastId });
          }
        }
      );
      await api.installFromSkillssh(skill.source, skill.skill_id);
      await Promise.all([refreshPresets(), refreshManagedSkills()]);
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

  // ── Custom sources: load, scan, expand/collapse, install, delete ──

  const loadSources = useCallback(async () => {
    try {
      const list = await api.listCustomRepos();
      setSources(list);
      setSourcesError(null);
    } catch (error: unknown) {
      setSourcesError(getErrorMessage(error, t("common.error")));
    }
  }, [t]);

  // Load persisted sources once on mount (design.md §3.3).
  useEffect(() => {
    loadSources();
  }, [loadSources]);

  // temp_dir lifecycle (design.md §3.4): every completed scan holds a real
  // on-disk temp directory. On unmount, cancel every one still outstanding —
  // fire-and-forget because the component is going away. Also bump each
  // source's scan generation so a preview that is still in flight resolves
  // as superseded and cancels its own fresh temp dir (its setScans would
  // land on a dead component otherwise, orphaning that dir).
  useEffect(() => () => {
    for (const [id, state] of Object.entries(scansRef.current)) {
      scanGenRef.current[id] = (scanGenRef.current[id] ?? 0) + 1;
      if (state.tempDir) {
        api.cancelGitPreview(state.tempDir).catch(() => {});
      }
    }
  }, []);

  /**
   * Drop a source's scan state. `cancelTemp` must be false after
   * confirm_git_install (the backend always cleans the temp dir itself, on
   * success and failure) and true when abandoning a live preview.
   */
  const clearScanState = useCallback((id: string, cancelTemp: boolean) => {
    scanGenRef.current[id] = (scanGenRef.current[id] ?? 0) + 1;
    setScans((prev) => {
      const state = prev[id];
      if (!state) return prev;
      if (cancelTemp && state.tempDir) {
        api.cancelGitPreview(state.tempDir).catch(() => {});
      }
      const next = { ...prev };
      delete next[id];
      return next;
    });
  }, []);

  // Collapsing keeps the scan state (rows + temp dir) so re-expanding is
  // instant with zero backend calls (D2 feedback). The temp dir is released
  // on delete / rescan / unmount instead — see design.md §3.4.
  const collapseSource = useCallback((id: string) => {
    setExpanded((prev) => ({ ...prev, [id]: false }));
  }, []);

  /**
   * Scan (or re-scan) one source. `opts.silent` suppresses the per-scan
   * toast — used by refresh-all and the post-install background re-scan,
   * which give their own aggregate feedback; first-load scans keep the
   * toast. `opts.refresh` forces the backend to fetch over the network
   * instead of serving its warm repository cache offline — true whenever the
   * user explicitly asks for current data (add, retry, refresh all); cold
   * expands and the post-install badge re-scan stay cache-first so they are
   * instant. A refresh-scan is also NON-DESTRUCTIVE: the previous rows and
   * their temp dir stay live and rendered until the new snapshot lands, so an
   * offline refresh never blanks a working list — the failure surfaces as an
   * inline warning above the kept rows instead. Cold scans keep the old
   * clear-then-scan behaviour. Returns whether the scan landed without an
   * error state (a superseded scan counts as fine — a newer scan owns the
   * state).
   */
  const scanSource = useCallback(
    async (source: CustomRepo, opts?: { silent?: boolean; refresh?: boolean }) => {
      const silent = opts?.silent ?? false;
      const refresh = opts?.refresh ?? false;

      if (refresh) {
        // Supersede any in-flight scan WITHOUT dropping the rendered state:
        // bump the generation so the loser cancels its own fresh temp dir,
        // never the live old one, and only flip `refreshing` on.
        scanGenRef.current[source.id] = (scanGenRef.current[source.id] ?? 0) + 1;
        setScans((prev) => {
          const existing = prev[source.id];
          if (!existing || existing.loading || existing.rows.length === 0) {
            // Nothing rendered to keep — behave like a cold scan (spinner),
            // but preserve any live temp dir an empty-repo snapshot holds.
            return {
              ...prev,
              [source.id]: {
                loading: true,
                refreshing: true,
                tempDir: existing?.tempDir ?? null,
                rows: [],
                error: null,
              },
            };
          }
          return {
            ...prev,
            [source.id]: { ...existing, loading: false, refreshing: true, error: null },
          };
        });
      } else {
        // Cold scan: cancel any previous temp dir before starting a new one,
        // so the overwritten state can never orphan a live temp directory.
        clearScanState(source.id, true);
        setScans((prev) => ({
          ...prev,
          [source.id]: {
            loading: true,
            refreshing: false,
            tempDir: null,
            rows: [],
            error: null,
          },
        }));
      }
      const gen = scanGenRef.current[source.id];

      let toastId: number | string | undefined;
      let unlisten: (() => void) | null = null;

      try {
        if (!silent) {
          toastId = toast.loading(t("install.toast.cloning"));
          unlisten = await listen<{ skill_id: string; phase: string; detail?: string }>(
            "install-progress",
            (event) => {
              if (event.payload.skill_id !== source.url) return;
              if (event.payload.phase === "cloning") {
                const detail = event.payload.detail?.trim();
                const msg = detail
                  ? `${t("install.toast.cloning")}\n${detail}`
                  : t("install.toast.cloning");
                toast.loading(msg, { id: toastId });
              }
            },
          );
        }
        const preview = await api.previewGitInstall(source.url, refresh);
        if (toastId !== undefined) toast.dismiss(toastId);
        if (scanGenRef.current[source.id] !== gen) {
          // Superseded by a delete or newer scan — nobody owns this temp dir.
          api.cancelGitPreview(preview.temp_dir).catch(() => {});
          return true;
        }
        setScans((prev) => {
          const previous = prev[source.id];
          // A refresh is replacing a live snapshot — release its temp dir only
          // now that the new one is landing; cancelling earlier would break
          // the still-rendered list mid-refresh. (Cold scans already cleared,
          // so `previous.tempDir` is null there and this no-ops.)
          if (previous?.tempDir && previous.tempDir !== preview.temp_dir) {
            api.cancelGitPreview(previous.tempDir).catch(() => {});
          }
          return {
            ...prev,
            [source.id]: {
              loading: false,
              refreshing: false,
              tempDir: preview.temp_dir,
              rows: preview.skills.map((s) => ({
                rel_path: s.rel_path,
                name: s.name,
                description: s.description,
                installed: s.installed,
                // Installed rows start deselected; re-checking one means update.
                selected: !s.installed,
              })),
              error: null,
            },
          };
        });
        return true;
      } catch (error: unknown) {
        if (toastId !== undefined) toast.dismiss(toastId);
        if (scanGenRef.current[source.id] !== gen) return true;
        const message = getErrorMessage(error, t("common.error"));
        setScans((prev) => {
          const previous = prev[source.id];
          if (refresh && previous && (previous.rows.length > 0 || previous.tempDir)) {
            // Refresh failure over a live snapshot: keep the rows and temp
            // dir — the list stays usable, installs keep working — and report
            // the failure through `error` (rendered as an inline warning
            // banner above the kept rows).
            return {
              ...prev,
              [source.id]: {
                loading: false,
                refreshing: false,
                tempDir: previous.tempDir,
                rows: previous.rows,
                error: message,
              },
            };
          }
          // Cold failure — the backend cleans the temp dir itself on error,
          // so keep tempDir null and land today's full-card error state.
          return {
            ...prev,
            [source.id]: {
              loading: false,
              refreshing: false,
              tempDir: null,
              rows: [],
              error: message,
            },
          };
        });
        return false;
      } finally {
        unlisten?.();
      }
    },
    [clearScanState, t],
  );

  const toggleSource = useCallback((source: CustomRepo) => {
    if (expanded[source.id]) {
      collapseSource(source.id);
      return;
    }
    setExpanded((prev) => ({ ...prev, [source.id]: true }));
    // Reuse a live scan result; rescan only when there is nothing to reuse
    // (first expand of a cold source after restart/add, or a retry after a
    // scan that produced no list at all). Collapsed sources keep their rows,
    // so re-expand is instant with zero backend calls — including a kept list
    // whose last refresh failed (that failure shows as an inline warning, not
    // as a dead card). A cold expand serves the warm repository cache
    // offline; expanding over an error is a retry and asks the network for
    // current data.
    const scan = scans[source.id];
    if (!scan || (scan.error && scan.rows.length === 0)) {
      scanSource(source, { refresh: Boolean(scan?.error) });
    }
  }, [collapseSource, expanded, scanSource, scans]);

  const handleAddSource = async () => {
    const url = sourceUrlInput.trim();
    if (!url || addingSource) return;
    setAddingSource(true);
    try {
      const added = await api.addCustomRepo(url);
      setSourceUrlInput("");
      // Duplicate adds resolve to the existing record — just expand it.
      setSources((prev) =>
        prev.some((s) => s.id === added.id) ? prev : [...prev, added],
      );
      setExpanded((prev) => ({ ...prev, [added.id]: true }));
      // Adding is an explicit ask for this repo's current state — fetch, not
      // a possibly warm cache from an earlier era of this URL. The scan also
      // records fetch metadata (count + timestamp) server-side; reload the
      // list once it lands so the card's badge/age reflect it.
      void scanSource(added, { refresh: true }).then(() => loadSources());
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setAddingSource(false);
    }
  };

  // Manual refresh (D2 feedback): re-scan every added source once, in
  // parallel. Scans run silent; per-source failures land in each source's
  // own error area and this handler gives the single aggregate toast.
  const handleRefreshAllSources = async () => {
    if (sources.length === 0 || refreshingAllSources) return;
    setRefreshingAllSources(true);
    try {
      const results = await Promise.allSettled(
        sources.map((source) => scanSource(source, { silent: true, refresh: true })),
      );
      const failed = results.filter((r) => r.status === "rejected" || !r.value).length;
      if (failed === 0) {
        toast.success(t("install.sources.refreshAllDone", { count: sources.length }));
      } else {
        toast.error(
          t("install.sources.refreshAllErrors", { failed, total: sources.length }),
        );
      }
      // Every successful scan bumped its fetch metadata server-side; reload
      // so the cards' "N skills · x 小时前更新" reads the fresh timestamps.
      await loadSources();
    } finally {
      setRefreshingAllSources(false);
    }
  };

  const updateScanRows = useCallback(
    (id: string, updater: (rows: SourceScanRow[]) => SourceScanRow[]) => {
      setScans((prev) => {
        const state = prev[id];
        if (!state) return prev;
        return { ...prev, [id]: { ...state, rows: updater(state.rows) } };
      });
    },
    [],
  );

  const installSelected = async (source: CustomRepo) => {
    const scan = scans[source.id];
    // A running refresh owns the temp-dir swap — installing mid-refresh could
    // confirm against a dir the refresh is about to cancel.
    if (!scan?.tempDir || installingSourceId || scan.refreshing) return;
    const selected = scan.rows.filter((r) => r.selected);
    if (selected.length === 0) return;
    setInstallingSourceId(source.id);
    let installed = false;
    try {
      await api.confirmGitInstall(
        source.url,
        scan.tempDir,
        selected.map((r) => ({ rel_path: r.rel_path, name: r.name })),
      );
      installed = true;
      const results = await Promise.allSettled([refreshPresets(), refreshManagedSkills()]);
      warnRejected(results, "post-install refresh");
      toast.success(
        t("install.toast.success", { name: selected.map((r) => r.name).join(", ") }),
      );
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      // confirm_git_install cleans the temp dir on success AND failure
      // ("Always clean up", skills.rs), so never cancel here — just drop the
      // scan state and collapse. After a successful install, run ONE silent
      // background re-scan so installed badges/counts refresh exactly once
      // per install action (not per expand). It starts only after confirm
      // finished; the stale tempDir in the dropped state is dead and the
      // fire-and-forget cancel inside scanSource is harmless.
      clearScanState(source.id, false);
      setExpanded((prev) => ({ ...prev, [source.id]: false }));
      setInstallingSourceId(null);
      if (installed) {
        scanSource(source, { silent: true });
      }
    }
  };

  const deleteSource = sources.find((s) => s.id === deleteSourceId) ?? null;

  const handleRemoveSource = async () => {
    if (!deleteSource) return;
    const { id } = deleteSource;
    try {
      // The source is going away — explicitly cancel its live preview temp
      // dir and drop the scan state (collapse no longer clears; design.md
      // §3.4 keeps the temp_dir ownership contract on delete).
      clearScanState(id, true);
      await api.removeCustomRepo(id);
      setSources((prev) => prev.filter((s) => s.id !== id));
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    }
  };

  // Escape hatch for a corrupted sources list (design.md §4): when the saved
  // JSON cannot be parsed, list/add/remove all fail and a retry can never
  // succeed — the only way out is overwriting the list with a fresh empty
  // one. Saved bookmarks are lost; installed skills are never touched.
  const handleResetSources = async () => {
    try {
      await api.resetCustomRepos();
      // The saved sources are gone — release their live preview temp dirs
      // before dropping the cards (same ownership contract as delete).
      for (const source of sources) {
        clearScanState(source.id, true);
      }
      await loadSources();
      toast.success(t("install.sources.resetDone"));
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    }
  };

  const handleImportDiscovered = async (sourcePath: string, name: string) => {
    setImportingPaths((prev) => new Set(prev).add(sourcePath));
    try {
      try {
        await api.importExistingSkill(sourcePath, name);
      } catch (error: unknown) {
        toast.error(getErrorMessage(error, t("common.error")));
        return;
      }
      toast.success(t("install.scan.importedOne", { name }));
      const results = await Promise.allSettled([
        refreshPresets(),
        refreshManagedSkills(),
        runScanSilent(),
      ]);
      warnRejected(results, "post-import refresh");
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
      try {
        await api.importAllDiscovered();
      } catch (error: unknown) {
        toast.error(getErrorMessage(error, t("common.error")));
        return;
      }
      toast.success(t("install.scan.importedAll"));
      const results = await Promise.allSettled([
        refreshPresets(),
        refreshManagedSkills(),
        runScanSilent(),
      ]);
      warnRejected(results, "post-import refresh");
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

  // Measure how many source pills can fit in one row; reserve room for All + More.
  const computeVisibleCount = useCallback(() => {
    const container = filterContainerRef.current;
    const allBtn = allBtnMeasureRef.current;
    const moreBtn = moreBtnMeasureRef.current;
    if (!container || !allBtn || !moreBtn) {
      setVisibleSourceCount(Infinity);
      return;
    }

    const containerWidth = container.clientWidth;
    if (containerWidth <= 0) {
      setVisibleSourceCount(Infinity);
      return;
    }

    const styles = window.getComputedStyle(container);
    const gap = parseFloat(styles.columnGap || styles.gap || "6") || 6;
    const available = containerWidth - allBtn.offsetWidth - gap - moreBtn.offsetWidth - gap;

    if (available <= 0) {
      setVisibleSourceCount(0);
      return;
    }

    let used = 0;
    let count = 0;
    for (let i = 0; i < sourceOptions.length; i += 1) {
      const el = sourceMeasureRefs.current[i];
      const w = el?.offsetWidth ?? 0;
      if (w <= 0) continue;
      const nextUsed = used + (count > 0 ? gap : 0) + w;
      if (nextUsed <= available) {
        used = nextUsed;
        count += 1;
      } else {
        break;
      }
    }
    setVisibleSourceCount(count);
  }, [sourceOptions]);

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
    <div className="app-page gap-4">
      <div className="app-page-header border-b-0 pb-0">
        <h1 className="app-page-title mb-4">{t("install.title")}</h1>
        <div className="flex gap-1 border-b border-border-subtle">
          {[
            { id: "market" as const, label: t("install.browseMarket"), icon: Box },
            { id: "local" as const, label: t("install.localInstall"), icon: UploadCloud },
            { id: "git" as const, label: t("install.sources.tab"), icon: Github },
          ].map((tab) => {
            const Icon = tab.icon;
            const isActive = activeTab === tab.id;
            return (
              <button
                key={tab.id}
                onClick={() => switchTab(tab.id)}
                className={cn(
                  "mr-4 flex items-center gap-1.5 border-b-2 px-1 pb-1.5 text-[13px] font-medium transition-colors outline-none",
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

              {sourceOptions.length > 0 && (
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
              )}
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
                          <button
                            type="button"
                            onClick={() => setMarketSourceFilter(skill.source)}
                            disabled={marketSourceFilter === skill.source}
                            title={t("install.onlyThisContributor")}
                            className={cn(
                              "rounded-[5px] bg-accent-bg px-1.5 py-0.5 text-[13px] leading-4 font-medium text-accent-light transition-colors",
                              marketSourceFilter === skill.source
                                ? "cursor-default opacity-90"
                                : "hover:bg-accent-bg/80"
                            )}
                          >
                            @{skill.source}
                          </button>
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
        <div className="space-y-4 pb-8 animate-in fade-in duration-300">
          <section className="app-panel overflow-hidden">
            <div className="px-4 py-3.5">
              <div className="flex items-start gap-3">
                <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg border border-border bg-surface-hover">
                  <Github className="h-5 w-5 text-tertiary" />
                </div>
                <div className="min-w-0 flex-1">
                  <h2 className="text-[14px] font-semibold text-primary">
                    {t("install.sources.title")}
                  </h2>
                  <p className="mt-1 text-[13px] leading-5 text-muted">
                    {t("install.sources.desc")}
                  </p>
                </div>
              </div>
              <div className="mt-3 flex gap-2">
                <input
                  type="text"
                  value={sourceUrlInput}
                  onChange={(e) => setSourceUrlInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handleAddSource();
                  }}
                  placeholder={t("install.sources.urlPlaceholder")}
                  disabled={addingSource}
                  className="app-input min-w-0 flex-1 bg-background"
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                />
                <button
                  type="button"
                  onClick={handleAddSource}
                  disabled={!sourceUrlInput.trim() || addingSource}
                  className="app-button-primary shrink-0"
                >
                  {addingSource ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Plus className="h-4 w-4" />
                  )}
                  {t("install.sources.add")}
                </button>
                <button
                  type="button"
                  onClick={handleRefreshAllSources}
                  // Installing holds the other half of the install/refresh
                  // exclusion: a refresh landing mid-install would cancel the
                  // temp dir the install backend is still reading from.
                  disabled={
                    sources.length === 0 ||
                    refreshingAllSources ||
                    installingSourceId !== null
                  }
                  className="app-button-secondary shrink-0 bg-background"
                  title={t("install.sources.refreshAll")}
                >
                  <RefreshCw className={cn("h-4 w-4", refreshingAllSources && "animate-spin")} />
                  {t("install.sources.refreshAll")}
                </button>
              </div>
            </div>
          </section>

          {sourcesError ? (
            <div className="space-y-2">
              <StatusBanner
                compact
                title={t("common.requestFailed")}
                description={sourcesError}
                actionLabel={t("common.retry")}
                onAction={loadSources}
                tone="danger"
              />
              {/* Corrupted-list escape hatch: retry alone can never fix an
                  unreadable settings value, so offer the one action that can. */}
              <div className="flex justify-end">
                <button
                  type="button"
                  onClick={() => setResetSourcesOpen(true)}
                  className="text-[13px] font-medium text-muted underline-offset-2 transition-colors hover:text-secondary hover:underline"
                >
                  {t("install.sources.resetList")}
                </button>
              </div>
            </div>
          ) : null}

          {sources.length === 0 && !sourcesError ? (
            <div className="app-panel flex flex-col items-center justify-center rounded-2xl px-6 py-14 text-center">
              <div className="flex h-12 w-12 items-center justify-center rounded-2xl border border-border bg-background text-muted">
                <Github className="h-5 w-5" />
              </div>
              <h3 className="mt-4 text-[14px] font-semibold text-secondary">
                {t("install.sources.emptyTitle")}
              </h3>
              <p className="mt-1 max-w-md text-[13px] text-muted">
                {t("install.sources.emptyHint")}
              </p>
            </div>
          ) : (
            <div className="space-y-2.5">
              {sources.map((source) => {
                const isOpen = !!expanded[source.id];
                const scan = scans[source.id] ?? null;
                const isInstalling = installingSourceId !== null;
                // Install/selection pauses while a refresh runs so the
                // temp-dir swap can never race an install.
                const isBusy = isInstalling || !!scan?.refreshing;
                const selectedCount = scan
                  ? scan.rows.filter((r) => r.selected).length
                  : 0;
                // Session-only badge (PRD §四): shown whenever rows are
                // rendered — including under a refresh that later failed,
                // because the kept list is still what the user is looking at.
                // Falls back to the persisted `last_fetch_count` so a
                // restarted app still shows a count on collapsed cards.
                const skillCount =
                  scan && !scan.loading && scan.rows.length > 0
                    ? scan.rows.length
                    : source.last_fetch_count;
                // Persisted "when was this content last fetched" — the age of
                // what a cache-first expand will show (null before the first
                // refresh scan ever succeeded).
                const updatedAgo =
                  source.last_fetch_at !== null
                    ? t("install.sources.updatedAgo", {
                        time: relativeAge(source.last_fetch_at, t),
                      })
                    : null;
                // Slim strip shown while a refresh runs over the
                // still-rendered previous rows (non-destructive refresh).
                const refreshingRow = scan?.refreshing ? (
                  <div className="flex items-center gap-2 border-b border-border-subtle px-4 py-2 text-[12px] text-muted">
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    {t("install.sources.refreshing")}
                  </div>
                ) : null;

                return (
                  <section key={source.id} className="app-panel overflow-hidden">
                    <div className="flex items-center gap-3 px-4 py-3">
                      <button
                        type="button"
                        onClick={() => toggleSource(source)}
                        className="flex min-w-0 flex-1 items-center gap-2 text-left"
                      >
                        {isOpen ? (
                          <ChevronDown className="h-4 w-4 shrink-0 text-muted" />
                        ) : (
                          <ChevronRight className="h-4 w-4 shrink-0 text-muted" />
                        )}
                        <span className="truncate text-[13px] font-semibold text-secondary">
                          {source.label}
                        </span>
                        {skillCount !== null ? (
                          <span className="shrink-0 rounded-full border border-border-subtle bg-surface px-2 py-0.5 text-[13px] text-muted">
                            {t("install.sources.skillCount", { count: skillCount })}
                          </span>
                        ) : null}
                        {updatedAgo !== null ? (
                          <span className="shrink-0 text-[12px] text-faint">
                            {updatedAgo}
                          </span>
                        ) : null}
                      </button>
                      <span
                        className="hidden min-w-0 max-w-[320px] truncate text-[13px] text-muted lg:block"
                        title={source.url}
                      >
                        {source.url}
                      </span>
                      <button
                        type="button"
                        onClick={() => setDeleteSourceId(source.id)}
                        className="shrink-0 rounded p-1 text-muted transition-colors hover:bg-surface-hover hover:text-red-400"
                        title={t("install.sources.delete")}
                      >
                        <Trash2 className="h-4 w-4" />
                      </button>
                    </div>

                    {isOpen ? (
                      <div className="border-t border-border-subtle">
                        {!scan || scan.loading ? (
                          <div className="flex items-center justify-center gap-2.5 py-10 text-muted">
                            <Loader2 className="h-4 w-4 animate-spin" />
                            <span className="text-[13px]">
                              {t("install.sources.scanning")}
                            </span>
                          </div>
                        ) : scan.error && scan.rows.length === 0 ? (
                          // Cold failure (nothing to keep): today's full-card
                          // error with retry.
                          <div className="p-4">
                            <StatusBanner
                              compact
                              title={t("common.requestFailed")}
                              description={scan.error}
                              actionLabel={t("common.retry")}
                              onAction={() => scanSource(source, { refresh: true })}
                              tone="danger"
                            />
                          </div>
                        ) : scan.rows.length === 0 ? (
                          <div>
                            {refreshingRow}
                            <p className="px-4 py-10 text-center text-[13px] text-muted">
                              {t("install.sources.emptyRepo")}
                            </p>
                          </div>
                        ) : (
                          <div>
                            {refreshingRow}
                            {/* The visible list is a snapshot cloned from the
                                local cache — mark it as one, with the age of
                                the content (null = a pre-metadata source that
                                was never refresh-scanned). */}
                            <div className="flex items-center gap-1.5 border-b border-border-subtle px-4 py-2 text-[12px] text-faint">
                              <Clock className="h-3 w-3" />
                              {source.last_fetch_at !== null
                                ? t("install.sources.snapshotChip", {
                                    time: relativeAge(source.last_fetch_at, t),
                                  })
                                : t("install.sources.snapshotChipNoAge")}
                            </div>
                            <div className="space-y-2 p-4">
                              {/* Refresh failure over a kept list — a
                                  non-blocking warning above the rows, not a
                                  replacement of the card. */}
                              {scan.error ? (
                                <StatusBanner
                                  compact
                                  title={t("install.sources.refreshFailed")}
                                  description={scan.error}
                                  actionLabel={t("common.retry")}
                                  onAction={() => {
                                    // Same install/refresh exclusion as the
                                    // refresh-all button: a refresh landing
                                    // mid-install would cancel the temp dir
                                    // the install backend is still reading.
                                    if (!isBusy) {
                                      scanSource(source, { refresh: true });
                                    }
                                  }}
                                  tone="warning"
                                />
                              ) : null}
                            <div className="flex flex-wrap items-center justify-between gap-2">
                              <div className="flex items-center gap-2 text-[13px]">
                                <button
                                  type="button"
                                  onClick={() =>
                                    updateScanRows(source.id, (rows) =>
                                      rows.map((r) => ({ ...r, selected: true })),
                                    )
                                  }
                                  disabled={isBusy}
                                  className="text-accent-light hover:underline"
                                >
                                  {t("install.sources.selectAll")}
                                </button>
                                <span className="text-faint">·</span>
                                <button
                                  type="button"
                                  onClick={() =>
                                    updateScanRows(source.id, (rows) =>
                                      rows.map((r) => ({ ...r, selected: false })),
                                    )
                                  }
                                  disabled={isBusy}
                                  className="text-muted hover:underline"
                                >
                                  {t("install.sources.deselectAll")}
                                </button>
                              </div>
                              <button
                                type="button"
                                onClick={() => installSelected(source)}
                                disabled={isBusy || selectedCount === 0}
                                className="inline-flex items-center gap-1.5 rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white transition-colors hover:bg-accent disabled:opacity-50"
                              >
                                {installingSourceId === source.id ? (
                                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                ) : (
                                  <DownloadCloud className="h-3.5 w-3.5" />
                                )}
                                {t("install.sources.installSelected", {
                                  count: selectedCount,
                                })}
                              </button>
                            </div>

                            <div className="space-y-2">
                              {scan.rows.map((row, idx) => (
                                <div
                                  key={row.rel_path}
                                  className={cn(
                                    "flex items-center gap-3 rounded-lg border px-3 py-2 transition-colors",
                                    row.selected
                                      ? "border-accent-border bg-accent-bg/40"
                                      : "border-border-subtle bg-background opacity-50",
                                  )}
                                >
                                  <input
                                    type="checkbox"
                                    checked={row.selected}
                                    disabled={isBusy}
                                    onChange={(e) =>
                                      updateScanRows(source.id, (rows) =>
                                        rows.map((r, i) =>
                                          i === idx
                                            ? { ...r, selected: e.target.checked }
                                            : r,
                                        ),
                                      )
                                    }
                                    className="h-4 w-4 shrink-0 accent-accent"
                                  />
                                  <div className="flex min-w-0 flex-1 flex-col">
                                    <div className="flex min-w-0 items-center gap-2">
                                      <input
                                        type="text"
                                        value={row.name}
                                        onChange={(e) =>
                                          updateScanRows(source.id, (rows) =>
                                            rows.map((r, i) =>
                                              i === idx
                                                ? { ...r, name: e.target.value }
                                                : r,
                                            ),
                                          )
                                        }
                                        disabled={!row.selected || isBusy}
                                        placeholder={t("install.sources.namePlaceholder")}
                                        className="app-input min-w-0 flex-1 bg-background py-1 text-[13px]"
                                      />
                                      {row.installed ? (
                                        <span className="inline-flex shrink-0 items-center gap-1 rounded-[5px] border border-emerald-500/20 bg-emerald-500/10 px-1.5 py-0.5 text-[13px] leading-4 font-medium text-emerald-400">
                                          <Check className="h-3 w-3" />
                                          {t("install.installed")}
                                        </span>
                                      ) : null}
                                    </div>
                                    {row.description ? (
                                      <p className="mt-1 truncate text-[12px] text-muted">
                                        {row.description}
                                      </p>
                                    ) : null}
                                  </div>
                                </div>
                              ))}
                            </div>
                          </div>
                          </div>
                        )}
                      </div>
                    ) : null}
                  </section>
                );
              })}
            </div>
          )}

          <ConfirmDialog
            open={deleteSourceId !== null}
            title={t("install.sources.deleteTitle")}
            message={t("install.sources.deleteMessage", {
              label: deleteSource?.label ?? "",
            })}
            confirmLabel={t("install.sources.deleteConfirm")}
            onClose={() => setDeleteSourceId(null)}
            onConfirm={() =>
              deleteSource ? handleRemoveSource() : Promise.resolve()
            }
          />

          <ConfirmDialog
            open={resetSourcesOpen}
            title={t("install.sources.resetTitle")}
            message={t("install.sources.resetMessage")}
            confirmLabel={t("install.sources.resetConfirm")}
            tone="warning"
            onClose={() => setResetSourcesOpen(false)}
            onConfirm={handleResetSources}
          />
        </div>
      )}
    </div>
  );
}
