import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useParams, useNavigate, Navigate } from "react-router-dom";
import {
  ChevronRight,
  Download,
  FileText,
  Globe,
  LayoutGrid,
  List,
  Loader2,
  Plus,
  RefreshCw,
  Search,
  CircleSlash,
  Trash2,
  Upload,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { PresetBar } from "../components/PresetBar";
import { AgentIcon } from "../components/AgentIcon";
import { DetailSheet } from "../components/DetailSheet";
import { SkillMarkdown } from "../components/SkillMarkdown";
import { DocumentDiffViewer } from "../components/DocumentDiffViewer";
import * as api from "../lib/tauri";
import type {
  ManagedSkill,
  Preset,
  PresetApplyMode,
  ProjectSkill,
} from "../lib/tauri";
import { getErrorMessage } from "../lib/error";
import { LatestRequestGate } from "../lib/latestRequestGate";
import { getTagActiveColor, getTagColor, UNTAGGED_FILTER } from "../lib/skillTags";
import { AddSkillsSheet } from "../components/AddSkillsSheet";
import type { WorkspaceConfig } from "./workspaceConfigs";

function compactHomePath(path: string) {
  return path.replace(/^\/Users\/[^/]+/, "~");
}

interface WorkspaceSkillCardTag {
  label: string;
  className: string;
}

interface WorkspaceSkillCardStatus {
  label: string;
  className: string;
}

function WorkspaceSkillCard({
  viewMode,
  title,
  description,
  tags = [],
  status,
  fileCount = 0,
  active = false,
  actions,
  actionsHover = false,
  onClick,
}: {
  viewMode: "grid" | "list";
  title: string;
  description?: string | null;
  tags?: WorkspaceSkillCardTag[];
  status: WorkspaceSkillCardStatus;
  fileCount?: number;
  active?: boolean;
  actions?: ReactNode;
  actionsHover?: boolean;
  onClick: () => void;
}) {
  if (viewMode === "list") {
    return (
      <div
        className={cn(
          "app-panel group relative flex cursor-pointer items-center gap-3.5 rounded-xl border-transparent px-3.5 py-3 transition-all hover:border-border hover:bg-surface-hover",
          active && "border-l-2 border-l-accent"
        )}
        onClick={onClick}
      >
        <h3
          className="w-[180px] shrink-0 truncate text-[14px] font-semibold text-secondary group-hover:text-primary"
          title={title}
        >
          {title}
        </h3>
        <p className="min-w-0 flex-1 truncate text-[13px] text-muted">
          {description || "-"}
        </p>
        {tags.length > 0 && (
          <div className="flex shrink-0 items-center gap-1.5">
            {tags.map((tag) => (
              <span
                key={tag.label}
                className={cn(
                  "inline-flex items-center rounded-full px-1.5 py-0.5 text-[11px] font-medium",
                  tag.className
                )}
              >
                {tag.label}
              </span>
            ))}
          </div>
        )}
        <div className="flex shrink-0 items-center gap-2.5">
          <span className={cn("rounded-full px-2 py-0.5 text-[12px] font-medium", status.className)}>
            {status.label}
          </span>
          {fileCount > 0 && (
            <span className="flex items-center gap-1 text-[12px] text-faint">
              <FileText className="h-3 w-3" />
              {fileCount}
            </span>
          )}
        </div>
        {actions && (
          <div
            className={cn(
              "flex shrink-0 items-center gap-1",
              actionsHover && "opacity-0 transition-opacity group-hover:opacity-100"
            )}
          >
            {actions}
          </div>
        )}
      </div>
    );
  }

  return (
    <div
      className={cn(
        "app-panel group relative flex h-full cursor-pointer flex-col overflow-hidden transition-all hover:border-border hover:bg-surface-hover",
        active && "border-l-2 border-l-accent"
      )}
      onClick={onClick}
    >
      <div className="flex items-center gap-2.5 px-3.5 pt-3 pb-1.5">
        <h3
          className="flex-1 truncate text-[14px] font-semibold text-primary group-hover:text-accent-light"
          title={title}
        >
          {title}
        </h3>
        {fileCount > 0 && (
          <span className="flex shrink-0 items-center gap-1 text-[12px] text-faint">
            <FileText className="h-3 w-3" />
            {fileCount}
          </span>
        )}
      </div>
      <div className="px-3.5 pb-3">
        <p className="truncate text-[13px] leading-[18px] text-muted">
          {description || "-"}
        </p>
        {tags.length > 0 && (
          <div className="mt-2 flex flex-wrap items-center gap-1">
            {tags.map((tag) => (
              <span
                key={tag.label}
                className={cn(
                  "inline-flex items-center rounded-full px-2 py-0.5 text-[11px] font-medium",
                  tag.className
                )}
              >
                {tag.label}
              </span>
            ))}
          </div>
        )}
      </div>
      <div className="mt-auto flex items-center justify-between gap-2 border-t border-border-subtle px-3.5 py-2.5">
        <span className={cn("rounded-full px-2 py-0.5 text-[12px] font-medium", status.className)}>
          {status.label}
        </span>
        {actions && <div className="flex shrink-0 items-center gap-1.5">{actions}</div>}
      </div>
    </div>
  );
}

function getLocalStatusMeta(t: (key: string) => string, status: ProjectSkill["sync_status"]) {
  switch (status) {
    case "in_sync":
      return {
        label: t("globalWorkspace.localSkills.status.inSync"),
        className: "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
      };
    case "project_newer":
      return {
        label: t("globalWorkspace.localSkills.status.localNewer"),
        className: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
      };
    case "center_newer":
      return {
        label: t("globalWorkspace.localSkills.status.centerNewer"),
        className: "bg-sky-500/10 text-sky-700 dark:text-sky-300",
      };
    case "diverged":
      return {
        label: t("globalWorkspace.localSkills.status.diverged"),
        className: "bg-violet-500/10 text-violet-700 dark:text-violet-300",
      };
    default:
      return {
        label: t("globalWorkspace.localSkills.status.localOnly"),
        className: "bg-surface-hover text-muted",
      };
  }
}

export function WorkspaceView({ config }: { config: WorkspaceConfig }) {
  const { agentKey } = useParams<{ agentKey?: string }>();
  const navigate = useNavigate();
  const { t } = useTranslation();
  const { tools, managedSkills, presets, refreshManagedSkills, refreshTools } = useApp();

  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [search, setSearch] = useState("");
  const [tagFilters, setTagFilters] = useState<Set<string>>(new Set());
  const [addDialogOpen, setAddDialogOpen] = useState(false);
  const [removingLocalSkillId, setRemovingLocalSkillId] = useState<string | null>(null);
  const [localSkills, setLocalSkills] = useState<ProjectSkill[]>([]);
  const [localSkillsLoading, setLocalSkillsLoading] = useState(false);
  const [localSkillsRefreshing, setLocalSkillsRefreshing] = useState(false);
  const [localActionKey, setLocalActionKey] = useState<string | null>(null);
  const [localDetailSkill, setLocalDetailSkill] = useState<ProjectSkill | null>(null);
  const [localDocContent, setLocalDocContent] = useState<string | null>(null);
  const [localCenterDocContent, setLocalCenterDocContent] = useState<string | null>(null);
  const [localDocLoading, setLocalDocLoading] = useState(false);
  const [localCenterDocLoading, setLocalCenterDocLoading] = useState(false);
  const [localContentTab, setLocalContentTab] = useState<"local" | "diff" | "center">("local");
  const [uploadConfirmSkill, setUploadConfirmSkill] = useState<ProjectSkill | null>(null);
  const [pullConfirmSkill, setPullConfirmSkill] = useState<ProjectSkill | null>(null);
  const [deleteLocalConfirmSkill, setDeleteLocalConfirmSkill] = useState<ProjectSkill | null>(null);
  const localDetailRequestRef = useRef(0);

  // Cross-category redirect: a deep link like /global-workspace/openclaw should
  // land on /lobster-workspace/openclaw. Compute it before any filtering so a
  // category mismatch doesn't briefly render "agent not found".
  const requestedTool = useMemo(
    () => (agentKey ? tools.find((t) => t.key === agentKey) ?? null : null),
    [agentKey, tools]
  );
  const needsRedirect =
    !!agentKey &&
    !!requestedTool &&
    requestedTool.category !== config.category;
  const redirectTarget = needsRedirect && requestedTool
    ? (requestedTool.category === "lobster"
        ? `/lobster-workspace/${requestedTool.key}`
        : `/global-workspace/${requestedTool.key}`)
    : null;

  const installedTools = useMemo(
    () => tools.filter((t) => t.installed && t.enabled && t.category === config.category),
    [tools, config.category]
  );

  const skillCountByAgent = useMemo(() => {
    const map: Record<string, number> = {};
    for (const tool of installedTools) {
      map[tool.key] = managedSkills.filter((s) =>
        s.targets.some((target) => target.tool === tool.key)
      ).length;
    }
    return map;
  }, [installedTools, managedSkills]);

  // Overview cards should reflect each agent's ACTUAL on-disk skill count —
  // including skills installed outside Skills Manager — to match the per-agent
  // detail badge. The managed-only count above reads 0 for an agent whose
  // skills all live on disk but were never imported (#287). We fill this from a
  // per-agent scan and fall back to the managed count until it resolves.
  const [localCountByAgent, setLocalCountByAgent] = useState<Record<string, number>>({});
  const overviewCountsRef = useRef(0);

  const currentTool = useMemo(
    () => (agentKey ? installedTools.find((t) => t.key === agentKey) ?? null : null),
    [agentKey, installedTools]
  );

  // Preset actions must target what is actually rendered: a single agent when
  // `currentTool` resolves, otherwise every installed agent in this category.
  // Falling back to the raw URL `agentKey` would let a stale deep link (a
  // bookmarked route for a since-disabled or uninstalled agent) mutate the
  // hidden agent while the overview is shown.
  const presetBarAgentKeys = useMemo(
    () => (currentTool ? [currentTool.key] : installedTools.map((t) => t.key)),
    [currentTool, installedTools]
  );
  const currentToolKey = currentTool?.key ?? null;
  const presetBarScopeKey = useMemo(
    () => currentToolKey
      ? `${config.basePath}:agent:${currentToolKey}`
      : `${config.basePath}:overview:${[...presetBarAgentKeys].sort().join("|")}`,
    [config.basePath, currentToolKey, presetBarAgentKeys]
  );

  const localSkillsRequestGateRef = useRef(new LatestRequestGate());
  const loadLocalSkills = useCallback(async (options?: { silent?: boolean }) => {
    if (!currentToolKey) {
      setLocalSkills([]);
      setLocalSkillsLoading(false);
      setLocalSkillsRefreshing(false);
      return;
    }
    const requestGate = localSkillsRequestGateRef.current;
    const requestToken = requestGate.begin(currentToolKey);
    // 已切换到其他 Agent 时，旧事件处理器可能仍在收尾；不再发起旧 Agent 的刷新。
    if (!requestGate.isCurrent(requestToken)) return;
    const silent = options?.silent ?? false;
    if (!silent) setLocalSkillsLoading(true);
    setLocalSkillsRefreshing(true);
    try {
      const skills = await api.getGlobalLocalSkills(currentToolKey);
      if (requestGate.isCurrent(requestToken)) setLocalSkills(skills);
    } catch (error: unknown) {
      if (requestGate.isCurrent(requestToken)) {
        toast.error(getErrorMessage(error, t("common.error")));
        // 后台刷新失败时保留当前卡片，避免一次 Preset 操作让整个页面闪空。
        if (!silent) setLocalSkills([]);
      }
    } finally {
      if (requestGate.isCurrent(requestToken)) {
        // 最新请求无论前台还是后台都要收起可能由上一请求打开的首屏 loading；
        // 否则初次扫描被一次 Preset 静默刷新取代时会永久停在“加载中”。
        setLocalSkillsLoading(false);
        setLocalSkillsRefreshing(false);
      }
    }
  }, [currentToolKey, t]);

  useLayoutEffect(() => {
    localSkillsRequestGateRef.current.setScope(currentToolKey);
    // Agent 路由变化后、浏览器绘制前清空旧卡片和旧确认框，避免“新 Agent +
    // 旧 relative_path”的短暂危险组合。
    setLocalSkills([]);
    setLocalSkillsLoading(Boolean(currentToolKey));
    setLocalSkillsRefreshing(false);
    localDetailRequestRef.current += 1;
    setLocalDetailSkill(null);
    setUploadConfirmSkill(null);
    setPullConfirmSkill(null);
    setDeleteLocalConfirmSkill(null);
    setTagFilters(new Set());
  }, [currentToolKey]);

  useEffect(() => {
    if (!currentToolKey) return;
    const requestGate = localSkillsRequestGateRef.current;
    void loadLocalSkills();
    return () => {
      requestGate.invalidate();
    };
  }, [currentToolKey, loadLocalSkills]);

  // Load real on-disk skill counts for every installed agent while the overview
  // is shown (#287). Scoped to the overview (currentToolKey === null); the
  // detail view derives its own count from `localSkills`.
  useEffect(() => {
    if (currentToolKey) return;
    if (installedTools.length === 0) {
      setLocalCountByAgent({});
      return;
    }
    const requestId = ++overviewCountsRef.current;
    void (async () => {
      const entries = await Promise.all(
        installedTools.map(async (tool) => {
          try {
            const skills = await api.getGlobalLocalSkills(tool.key);
            return [tool.key, skills.length] as const;
          } catch {
            // Keep the managed-count fallback for an agent that fails to scan.
            return [tool.key, null] as const;
          }
        })
      );
      if (overviewCountsRef.current !== requestId) return;
      // Rebuild the map from this scan's results (don't merge into the previous
      // map): agents whose scan failed are omitted so they fall back to the
      // managed count rather than showing a stale value, and counts for agents
      // no longer installed are dropped.
      const next: Record<string, number> = {};
      for (const [key, count] of entries) {
        if (count !== null) next[key] = count;
      }
      setLocalCountByAgent(next);
    })();
    // Depend on the managedSkills array reference (not just its length): a
    // target-only enable/disable or an externally added unmanaged skill changes
    // on-disk presence without changing the managed count, but still produces a
    // fresh array via refreshManagedSkills (the file watcher triggers it), so
    // the overview counts re-scan and stay accurate (#287).
  }, [currentToolKey, installedTools, managedSkills]);

  const agentSkills = useMemo(
    () =>
      agentKey
        ? managedSkills.filter((skill) =>
            skill.targets.some((target) => target.tool === agentKey)
          )
        : [],
    [agentKey, managedSkills]
  );

  const allLocalTags = useMemo(() => {
    const tags = new Set<string>();
    for (const skill of localSkills) {
      for (const tag of skill.tags) {
        if (tag.trim()) tags.add(tag);
      }
    }
    return Array.from(tags).sort((a, b) => a.localeCompare(b));
  }, [localSkills]);

  const visibleLocalSkills = useMemo(() => {
    const q = search.trim().toLowerCase();
    return localSkills
      .filter((skill) => {
        if (q) {
          const matchesQuery =
            skill.name.toLowerCase().includes(q) ||
            skill.dir_name.toLowerCase().includes(q) ||
            (skill.description || "").toLowerCase().includes(q);
          if (!matchesQuery) return false;
        }
        if (tagFilters.size > 0) {
          const wantUntagged = tagFilters.has(UNTAGGED_FILTER);
          const matchUntagged = wantUntagged && skill.tags.length === 0;
          const matchTag = skill.tags.some((tag) => tagFilters.has(tag));
          if (!matchUntagged && !matchTag) return false;
        }
        return true;
      })
      .sort((a, b) => {
        const priority: Record<ProjectSkill["sync_status"], number> = {
          project_only: 0,
          project_newer: 1,
          diverged: 2,
          center_newer: 3,
          in_sync: 4,
        };
        return (
          priority[a.sync_status] - priority[b.sync_status] ||
          a.name.localeCompare(b.name)
        );
      });
  }, [localSkills, search, tagFilters]);

  const inSyncLocalCount = useMemo(
    () => localSkills.filter((skill) => skill.sync_status === "in_sync").length,
    [localSkills]
  );

  const installedIds = useMemo(() => new Set(agentSkills.map((s) => s.id)), [agentSkills]);

  const managedLocalIds = useMemo(
    () =>
      new Set(
        localSkills
          .map((skill) => skill.center_skill_id)
          .filter((id): id is string => !!id && installedIds.has(id))
      ),
    [installedIds, localSkills]
  );

  const managedLocalCount = useMemo(
    () => localSkills.filter((skill) => !!skill.center_skill_id && managedLocalIds.has(skill.center_skill_id)).length,
    [localSkills, managedLocalIds]
  );

  const handleRemoveLocalManagedSkill = async (skill: ProjectSkill) => {
    if (!agentKey || !skill.center_skill_id || !managedLocalIds.has(skill.center_skill_id)) return;
    setRemovingLocalSkillId(skill.relative_path);
    try {
      await api.unsyncSkillFromTool(skill.center_skill_id, agentKey);
      await Promise.all([refreshManagedSkills(), refreshTools(), loadLocalSkills()]);
      toast.success(t("globalWorkspace.removedToast", { name: skill.name }));
    } catch (e) {
      toast.error(getErrorMessage(e, t("common.error")));
    } finally {
      setRemovingLocalSkillId(null);
    }
  };

  const handleSheetInstalled = useCallback(async () => {
    await Promise.all([refreshManagedSkills(), refreshTools(), loadLocalSkills()]);
  }, [loadLocalSkills, refreshManagedSkills, refreshTools]);

  const handleUploadLocalSkill = useCallback(
    async (skill: ProjectSkill) => {
      if (!currentTool) return;
      const key = `upload:${skill.relative_path}`;
      setLocalActionKey(key);
      try {
        await api.importGlobalLocalSkillToCenter(currentTool.key, skill.relative_path);
        toast.success(t("globalWorkspace.localSkills.uploadedToast", { name: skill.name, agent: currentTool.display_name }));
        await Promise.all([loadLocalSkills(), refreshManagedSkills()]);
      } catch (error: unknown) {
        toast.error(getErrorMessage(error, t("common.error")));
      } finally {
        setLocalActionKey(null);
        setUploadConfirmSkill(null);
      }
    },
    [currentTool, loadLocalSkills, refreshManagedSkills, t]
  );

  const handleDeleteLocalSkill = useCallback(
    async (skill: ProjectSkill) => {
      if (!currentTool) return;
      const key = `delete:${skill.relative_path}`;
      setLocalActionKey(key);
      try {
        await api.deleteGlobalLocalSkill(currentTool.key, skill.relative_path);
        toast.success(t("globalWorkspace.localSkills.deletedLocalToast", { name: skill.name, agent: currentTool.display_name }));
        await loadLocalSkills();
      } catch (error: unknown) {
        toast.error(getErrorMessage(error, t("common.error")));
      } finally {
        setLocalActionKey(null);
        setDeleteLocalConfirmSkill(null);
      }
    },
    [currentTool, loadLocalSkills, t]
  );

  const handlePullLocalSkill = useCallback(
    async (skill: ProjectSkill) => {
      if (!currentTool) return;
      const key = `pull:${skill.relative_path}`;
      setLocalActionKey(key);
      try {
        await api.updateGlobalLocalSkillFromCenter(currentTool.key, skill.relative_path);
        toast.success(t("globalWorkspace.localSkills.pulledToast", { name: skill.name, agent: currentTool.display_name }));
        await loadLocalSkills();
      } catch (error: unknown) {
        toast.error(getErrorMessage(error, t("common.error")));
      } finally {
        setLocalActionKey(null);
        setPullConfirmSkill(null);
      }
    },
    [currentTool, loadLocalSkills, t]
  );

  const openLocalDetail = useCallback(
    async (skill: ProjectSkill) => {
      if (!currentTool) return;
      const requestId = localDetailRequestRef.current + 1;
      localDetailRequestRef.current = requestId;
      setLocalDetailSkill(skill);
      setLocalContentTab("local");
      setLocalDocContent(null);
      setLocalCenterDocContent(null);
      setLocalDocLoading(true);
      setLocalCenterDocLoading(!!skill.center_skill_id);

      api
        .getGlobalLocalSkillDocument(currentTool.key, skill.relative_path)
        .then((doc) => {
          if (localDetailRequestRef.current === requestId) setLocalDocContent(doc.content);
        })
        .catch(() => {
          if (localDetailRequestRef.current === requestId) setLocalDocContent(null);
        })
        .finally(() => {
          if (localDetailRequestRef.current === requestId) setLocalDocLoading(false);
        });

      if (skill.center_skill_id) {
        api
          .getSkillDocument(skill.center_skill_id)
          .then((doc) => {
            if (localDetailRequestRef.current === requestId) setLocalCenterDocContent(doc.content);
          })
          .catch(() => {
            if (localDetailRequestRef.current === requestId) setLocalCenterDocContent(null);
          })
          .finally(() => {
            if (localDetailRequestRef.current === requestId) setLocalCenterDocLoading(false);
          });
      }
    },
    [currentTool]
  );

  const existsInGlobal = useCallback(
    (skill: ManagedSkill, agentK: string) =>
      skill.targets.some((target) => target.tool === agentK),
    []
  );

  const handleApplyPreset = useCallback(
    async (preset: Preset, mode: PresetApplyMode) =>
      api.applyPresetToTools(preset.id, presetBarAgentKeys, mode),
    [presetBarAgentKeys]
  );

  const handlePresetComplete = useCallback(async () => {
    await Promise.all([
      refreshManagedSkills(),
      refreshTools(),
      loadLocalSkills({ silent: true }),
    ]);
  }, [loadLocalSkills, refreshManagedSkills, refreshTools]);

  const renderLocalSkillActions = (skill: ProjectSkill, variant: "grid" | "list") => {
    const uploadKey = `upload:${skill.relative_path}`;
    const pullKey = `pull:${skill.relative_path}`;
    const deleteKey = `delete:${skill.relative_path}`;
    const canPull = skill.sync_status === "center_newer" || skill.sync_status === "diverged";
    const isInSync = skill.sync_status === "in_sync";
    const isManaged = !!skill.center_skill_id && managedLocalIds.has(skill.center_skill_id);
    const canDeleteLocal = !isManaged && skill.sync_status === "project_only";
    const removing = removingLocalSkillId === skill.relative_path;
    const buttonClassName = variant === "grid"
      ? "rounded px-2 py-1 text-[13px] font-medium text-muted transition-colors outline-none hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
      : "rounded p-0.5 text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50";

    if (isInSync && !isManaged) return null;

    return (
      <>
        {!isInSync && canPull && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              setPullConfirmSkill(skill);
            }}
            disabled={localActionKey === pullKey}
            className={buttonClassName}
            title={t("globalWorkspace.localSkills.pull")}
          >
            {localActionKey === pullKey ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Download className="h-3.5 w-3.5" />
            )}
          </button>
        )}

        {!isInSync && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              if (skill.sync_status === "project_only") {
                void handleUploadLocalSkill(skill);
              } else {
                setUploadConfirmSkill(skill);
              }
            }}
            disabled={localActionKey === uploadKey}
            className={buttonClassName}
            title={t("globalWorkspace.localSkills.upload")}
          >
            {localActionKey === uploadKey ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Upload className="h-3.5 w-3.5" />
            )}
          </button>
        )}

        {isManaged ? (
          <button
            onClick={(e) => {
              e.stopPropagation();
              void handleRemoveLocalManagedSkill(skill);
            }}
            disabled={removing}
            title={t("globalWorkspace.localSkills.removeManaged")}
            className={cn(buttonClassName, "hover:bg-red-500/10 hover:text-red-500")}
          >
            {removing ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Trash2 className="h-3.5 w-3.5" />
            )}
          </button>
        ) : canDeleteLocal ? (
          <button
            onClick={(e) => {
              e.stopPropagation();
              setDeleteLocalConfirmSkill(skill);
            }}
            disabled={localActionKey === deleteKey}
            title={t("globalWorkspace.localSkills.deleteLocal")}
            className={cn(buttonClassName, "hover:bg-red-500/10 hover:text-red-500")}
          >
            {localActionKey === deleteKey ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Trash2 className="h-3.5 w-3.5" />
            )}
          </button>
        ) : null}
      </>
    );
  };

  if (redirectTarget) {
    return <Navigate to={redirectTarget} replace />;
  }

  if (installedTools.length === 0) {
    return (
      <div className="app-page">
        <div className="app-panel flex flex-col items-center justify-center py-16 text-center">
          <div className="mb-3 flex h-10 w-10 items-center justify-center rounded-full bg-surface-hover">
            <Globe className="h-5 w-5 text-muted" />
          </div>
          <p className="text-[13px] font-medium text-secondary">{t(config.i18nKeys.noAgents)}</p>
          <p className="mt-1 max-w-[260px] text-[12px] leading-relaxed text-muted">
            {t(config.i18nKeys.noAgentsHint)}
          </p>
        </div>
      </div>
    );
  }

  if (!currentTool) {
    return (
      <div className="app-page">
        <div className="app-page-header flex flex-col gap-2.5 pb-3 pr-2">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="min-w-0 flex-1">
              <h1 className="app-page-title flex items-center gap-2.5">
                <Globe className="h-5 w-5 text-accent" />
                {t(config.i18nKeys.title)}
                <span className="app-badge">{installedTools.length}</span>
              </h1>
            </div>
          </div>

          {presets.length > 0 && (
            <PresetBar
              presets={presets}
              managedSkills={managedSkills}
              agentKeys={presetBarAgentKeys}
              scopeKey={presetBarScopeKey}
              existsInWorkspace={existsInGlobal}
              onApplyPreset={handleApplyPreset}
              onComplete={handlePresetComplete}
            />
          )}
        </div>

        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">
          {installedTools.map((tool) => {
            const count = localCountByAgent[tool.key] ?? skillCountByAgent[tool.key] ?? 0;
            return (
              <button
                key={tool.key}
                onClick={() => navigate(`${config.basePath}/${tool.key}`)}
                className="app-panel group flex items-center gap-3 p-3.5 text-left transition-all hover:border-border hover:bg-surface-hover"
              >
                <AgentIcon
                  agentKey={tool.key}
                  displayName={tool.display_name}
                  className="h-9 w-9 rounded-lg transition-colors group-hover:border-border"
                />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-[13px] font-semibold text-secondary">{tool.display_name}</p>
                  <p className="text-[12px] text-muted">{t("globalWorkspace.skillCount", { count })}</p>
                </div>
                <ChevronRight className="h-4 w-4 shrink-0 text-faint transition-transform group-hover:translate-x-0.5" />
              </button>
            );
          })}
        </div>
      </div>
    );
  }

  return (
    <div className="app-page">
      {/* Header */}
      <div className="app-page-header flex flex-col gap-2.5 pb-3 pr-2">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="min-w-0 flex-[1_1_360px]">
            <h1 className="app-page-title flex items-center gap-2.5">
              <AgentIcon
                agentKey={currentTool.key}
                displayName={currentTool.display_name}
                className="h-7 w-7 rounded-lg"
              />
              {currentTool.display_name}
              <span className="app-badge">{localSkills.length}</span>
            </h1>
            <p className="mt-1 truncate text-[13px] text-muted" title={currentTool.skills_dir}>
              {compactHomePath(currentTool.skills_dir)}
              <span className="px-1.5">·</span>
              {t("globalWorkspace.localSkills.summary", {
                total: localSkills.length,
                managed: managedLocalCount,
                synced: inSyncLocalCount,
              })}
            </p>
          </div>

          <div className="flex min-w-0 flex-[2_1_520px] flex-wrap items-center justify-end gap-2">
            <div className="relative w-full min-w-[220px] max-w-[320px]">
              <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
              <input
                type="text"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder={t("globalWorkspace.localSkills.searchPlaceholder")}
                className="app-input h-9 w-full rounded-md pl-8 font-medium"
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
              />
            </div>

            <div className="app-segmented shrink-0">
              <button
                onClick={() => void loadLocalSkills()}
                disabled={localSkillsRefreshing}
                className="rounded-md p-2 text-muted transition-colors outline-none hover:text-tertiary disabled:opacity-50"
                title={t("settings.refresh")}
              >
                <RefreshCw className={cn("h-4 w-4", localSkillsRefreshing && "animate-spin")} />
              </button>
              <button
                onClick={() => setViewMode("grid")}
                className={cn(
                  "rounded-md p-2 transition-colors outline-none",
                  viewMode === "grid" ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                )}
              >
                <LayoutGrid className="h-4 w-4" />
              </button>
              <button
                onClick={() => setViewMode("list")}
                className={cn(
                  "rounded-md p-2 transition-colors outline-none",
                  viewMode === "list" ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                )}
              >
                <List className="h-4 w-4" />
              </button>
            </div>

            <button
              onClick={() => setAddDialogOpen(true)}
              className="inline-flex h-9 shrink-0 items-center gap-1.5 rounded-md bg-accent px-3 text-[13px] font-medium text-white transition-colors hover:bg-accent-hover"
            >
              <Plus className="h-3.5 w-3.5" />
              {t("globalWorkspace.addSkill")}
            </button>
          </div>
        </div>

        {allLocalTags.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="text-[12px] text-muted">{t("mySkills.tags.filter")}</span>
            <button
              onClick={() => setTagFilters(new Set())}
              className={cn(
                "rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                tagFilters.size === 0
                  ? "bg-accent text-white dark:bg-accent dark:text-white"
                  : "bg-surface-hover text-muted hover:text-secondary"
              )}
            >
              {t("mySkills.tags.allTags")}
            </button>
            {localSkills.some((s) => s.tags.length === 0) && (() => {
              const isActive = tagFilters.has(UNTAGGED_FILTER);
              return (
                <button
                  onClick={() => {
                    setTagFilters((prev) => {
                      const next = new Set(prev);
                      if (next.has(UNTAGGED_FILTER)) next.delete(UNTAGGED_FILTER);
                      else next.add(UNTAGGED_FILTER);
                      return next;
                    });
                  }}
                  className={cn(
                    "inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                    isActive
                      ? "bg-surface-active text-primary"
                      : "border border-dashed border-border text-muted hover:text-secondary"
                  )}
                  title={t("mySkills.tags.untagged")}
                >
                  <CircleSlash className="h-3 w-3" />
                  {t("mySkills.tags.untagged")}
                </button>
              );
            })()}
            {allLocalTags.map((tag) => {
              const active = tagFilters.has(tag);
              return (
                <button
                  key={tag}
                  onClick={() => {
                    setTagFilters((prev) => {
                      const next = new Set(prev);
                      if (next.has(tag)) next.delete(tag);
                      else next.add(tag);
                      return next;
                    });
                  }}
                  className={cn(
                    "rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                    active ? getTagActiveColor(tag, allLocalTags) : getTagColor(tag, allLocalTags)
                  )}
                >
                  {tag}
                </button>
              );
            })}
          </div>
        )}

        {/* Preset bar */}
        {presets.length > 0 && (
          <PresetBar
            presets={presets}
            managedSkills={managedSkills}
            agentKeys={presetBarAgentKeys}
            scopeKey={presetBarScopeKey}
            existsInWorkspace={existsInGlobal}
            onApplyPreset={handleApplyPreset}
            onComplete={handlePresetComplete}
          />
        )}
      </div>

      {localSkillsLoading && localSkills.length === 0 ? (
        <div className="flex items-center gap-2 py-4 text-[13px] text-muted">
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
          {t("common.loading")}
        </div>
      ) : visibleLocalSkills.length === 0 ? (
        <div className="flex min-h-[260px] flex-col items-center justify-center px-4 text-center">
          <Globe className="mb-4 h-12 w-12 text-faint" />
          <h3 className="mb-1.5 text-[14px] font-semibold text-tertiary">
            {localSkills.length === 0
              ? t("globalWorkspace.localSkills.empty")
              : t("mySkills.noMatch")}
          </h3>
          {localSkills.length === 0 && (
            <button
              onClick={() => setAddDialogOpen(true)}
              className="mt-4 inline-flex items-center gap-1.5 rounded-md bg-accent px-4 py-2 text-[13px] font-medium text-white transition-colors hover:bg-accent-hover"
            >
              <Plus className="h-3.5 w-3.5" />
              {t("globalWorkspace.addSkill")}
            </button>
          )}
        </div>
      ) : (
        <div
          className={cn(
            "pb-8",
            viewMode === "grid"
              ? "grid grid-cols-2 gap-3 lg:grid-cols-3"
              : "flex flex-col gap-0.5"
          )}
        >
          {visibleLocalSkills.map((skill) => {
            const statusMeta = getLocalStatusMeta(t, skill.sync_status);
            const isManaged = !!skill.center_skill_id && managedLocalIds.has(skill.center_skill_id);

            return (
              <WorkspaceSkillCard
                key={`${skill.agent}:${skill.relative_path}`}
                viewMode={viewMode}
                title={skill.name}
                description={skill.description || skill.relative_path}
                tags={skill.tags.map((tag) => ({ label: tag, className: getTagColor(tag, allLocalTags) }))}
                status={statusMeta}
                fileCount={skill.files.length}
                active={isManaged}
                actions={renderLocalSkillActions(skill, viewMode)}
                actionsHover={viewMode === "list"}
                onClick={() => void openLocalDetail(skill)}
              />
            );
          })}
        </div>
      )}

      {currentTool && (
        <AddSkillsSheet
          open={addDialogOpen}
          onClose={() => setAddDialogOpen(false)}
          target={{
            kind: "global",
            agentKey: currentTool.key,
            agentDisplayName: currentTool.display_name,
            installedSkillIds: installedIds,
          }}
          managedSkills={managedSkills}
          onInstalled={handleSheetInstalled}
        />
      )}

      <DetailSheet
        open={!!localDetailSkill}
        title={localDetailSkill?.name ?? ""}
        description={localDetailSkill?.description}
        meta={
          localDetailSkill ? (
            <div className="flex flex-wrap items-center gap-2">
              <span className={cn("rounded-full px-2.5 py-1 text-[12px] font-medium", getLocalStatusMeta(t, localDetailSkill.sync_status).className)}>
                {getLocalStatusMeta(t, localDetailSkill.sync_status).label}
              </span>
              <span className="rounded-full bg-surface-hover px-2.5 py-1 text-[12px] text-muted">
                {localDetailSkill.relative_path}
              </span>
            </div>
          ) : null
        }
        onClose={() => setLocalDetailSkill(null)}
      >
        {localDetailSkill?.center_skill_id && (
          <div className="mb-4 flex flex-wrap items-center gap-2">
            {(["local", "diff", "center"] as const).map((tab) => (
              <button
                key={tab}
                type="button"
                onClick={() => setLocalContentTab(tab)}
                className={cn(
                  "rounded-full px-3 py-1.5 text-[12px] font-medium transition-colors",
                  localContentTab === tab
                    ? "bg-accent text-white"
                    : "bg-surface-hover text-muted hover:text-secondary"
                )}
                disabled={(tab === "diff" || tab === "center") && localCenterDocLoading}
              >
                {tab === "local"
                  ? t("mySkills.docTabs.local")
                  : tab === "diff"
                    ? t("mySkills.docTabs.diff")
                    : t("project.docTabs.center")}
              </button>
            ))}
          </div>
        )}

        {localDocLoading ? (
          <div className="mt-12 text-center text-[13px] text-muted">{t("common.loading")}</div>
        ) : localContentTab === "diff" ? (
          localDocContent && localCenterDocContent ? (
            <DocumentDiffViewer original={localDocContent} updated={localCenterDocContent} />
          ) : localCenterDocLoading ? (
            <div className="mt-12 text-center text-[13px] text-muted">{t("common.loading")}</div>
          ) : (
            <div className="mt-12 text-center text-[13px] text-muted">{t("mySkills.sourceDiffUnavailable")}</div>
          )
        ) : localContentTab === "center" ? (
          localCenterDocLoading ? (
            <div className="mt-12 text-center text-[13px] text-muted">{t("common.loading")}</div>
          ) : localCenterDocContent ? (
            <SkillMarkdown content={localCenterDocContent} />
          ) : (
            <div className="mt-12 text-center text-[13px] text-muted">{t("mySkills.sourceDiffUnavailable")}</div>
          )
        ) : localDocContent ? (
          <SkillMarkdown content={localDocContent} />
        ) : (
          <div className="mt-12 text-center text-[13px] text-muted">{t("common.documentMissing")}</div>
        )}
      </DetailSheet>

      <ConfirmDialog
        open={!!uploadConfirmSkill}
        title={t("globalWorkspace.localSkills.uploadConfirmTitle")}
        message={t("globalWorkspace.localSkills.uploadConfirmMessage", {
          name: uploadConfirmSkill?.name ?? "",
        })}
        tone="warning"
        confirmLabel={t("globalWorkspace.localSkills.upload")}
        onClose={() => setUploadConfirmSkill(null)}
        onConfirm={() => uploadConfirmSkill ? handleUploadLocalSkill(uploadConfirmSkill) : Promise.resolve()}
      />
      <ConfirmDialog
        open={!!pullConfirmSkill}
        title={t("globalWorkspace.localSkills.pullConfirmTitle")}
        message={t("globalWorkspace.localSkills.pullConfirmMessage", {
          name: pullConfirmSkill?.name ?? "",
          agent: currentTool?.display_name ?? "",
        })}
        tone="danger"
        confirmLabel={t("globalWorkspace.localSkills.pull")}
        onClose={() => setPullConfirmSkill(null)}
        onConfirm={() => pullConfirmSkill ? handlePullLocalSkill(pullConfirmSkill) : Promise.resolve()}
      />
      <ConfirmDialog
        open={!!deleteLocalConfirmSkill}
        title={t("globalWorkspace.localSkills.deleteLocalConfirmTitle")}
        message={t("globalWorkspace.localSkills.deleteLocalConfirmMessage", {
          name: deleteLocalConfirmSkill?.name ?? "",
          agent: currentTool?.display_name ?? "",
        })}
        tone="danger"
        confirmLabel={t("common.delete")}
        onClose={() => setDeleteLocalConfirmSkill(null)}
        onConfirm={() => deleteLocalConfirmSkill ? handleDeleteLocalSkill(deleteLocalConfirmSkill) : Promise.resolve()}
      />
    </div>
  );
}

