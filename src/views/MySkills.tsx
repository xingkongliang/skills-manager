import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Search,
  LayoutGrid,
  List,
  CheckCircle2,
  Circle,
  Github,
  HardDrive,
  Globe,
  Trash2,
  Layers,
  RefreshCw,
  RotateCcw,
  GitBranch,
  History,
  ArrowUpCircle,
  Loader2,
  X,
  Plus,
  SquareCheck,
  Square,
  GripVertical,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import { useMultiSelect } from "../hooks/useMultiSelect";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { SkillDetailPanel } from "../components/SkillDetailPanel";
import { MultiSelectToolbar } from "../components/MultiSelectToolbar";
import * as api from "../lib/tauri";
import type {
  ManagedSkill,
  ToolInfo,
  GitBackupStatus,
  GitBackupVersion,
  SkillToolToggle,
} from "../lib/tauri";
import { getErrorMessage, getErrorKind } from "../lib/error";
import {
  DndContext,
  closestCenter,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  rectSortingStrategy,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";

interface SortableSkillItemProps {
  id: string;
  disabled: boolean;
  children: (dragHandle: React.ReactNode) => React.ReactNode;
}

function SortableSkillItem({ id, disabled, children }: SortableSkillItemProps) {
  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id, disabled });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.5 : undefined,
  };

  const handle = !disabled ? (
    <div
      ref={setActivatorNodeRef}
      {...listeners}
      className="flex cursor-grab items-center justify-center rounded p-1 text-faint transition-colors hover:bg-surface-hover hover:text-muted active:cursor-grabbing"
    >
      <GripVertical className="h-4 w-4" />
    </div>
  ) : null;

  return (
    <div ref={setNodeRef} style={style} {...attributes}>
      {children(handle)}
    </div>
  );
}

function getToolDisplayName(toolKey: string, tools: ToolInfo[]) {
  return tools.find((tool) => tool.key === toolKey)?.display_name || toolKey;
}

function displaySnapshotLabel(tag: string) {
  const raw = tag.startsWith("sm-v-") ? tag.slice("sm-v-".length) : tag;
  const parts = raw.split("-");
  if (parts.length < 3) return raw;
  // Supported forms:
  // 1) YYYYMMDD-HHMMSS-<short_sha>
  // 2) YYYYMMDD-HHMMSS-<millis>-<short_sha>
  return `${parts[0]}-${parts[1]}`;
}

export function MySkills() {
  const { t } = useTranslation();
  const {
    activeScenario,
    tools,
    managedSkills: skills,
    refreshScenarios,
    refreshManagedSkills,
    detailSkillId,
    openSkillDetailById,
    closeSkillDetail,
  } = useApp();
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [filterMode, setFilterMode] = useState<"all" | "enabled" | "available">("all");
  const [sourceFilters, setSourceFilters] = useState<Set<string>>(new Set());
  const [tagFilters, setTagFilters] = useState<Set<string>>(new Set());
  const [allTags, setAllTags] = useState<string[]>([]);
  const [search, setSearch] = useState("");
  const [deleteTarget, setDeleteTarget] = useState<ManagedSkill | null>(null);
  const [batchDeleteConfirm, setBatchDeleteConfirm] = useState(false);
  const [checkingAll, setCheckingAll] = useState(false);
  const [checkingSkillId, setCheckingSkillId] = useState<string | null>(null);
  const [updatingSkillId, setUpdatingSkillId] = useState<string | null>(null);
  const [toolToggles, setToolToggles] = useState<SkillToolToggle[] | null>(null);
  const [togglingToolKey, setTogglingToolKey] = useState<string | null>(null);
  const [gitStatus, setGitStatus] = useState<GitBackupStatus | null>(null);
  const [gitLoading, setGitLoading] = useState<string | null>(null); // "start" | "sync"
  const [gitRemoteConfig, setGitRemoteConfig] = useState("");
  const [gitVersionsOpen, setGitVersionsOpen] = useState(false);
  const [gitVersionsLoading, setGitVersionsLoading] = useState(false);
  const [gitVersions, setGitVersions] = useState<GitBackupVersion[]>([]);
  const [restoreVersionTag, setRestoreVersionTag] = useState<string | null>(null);
  const [restoringVersionTag, setRestoringVersionTag] = useState<string | null>(null);
  const [tagEditSkillId, setTagEditSkillId] = useState<string | null>(null);
  const [tagInput, setTagInput] = useState("");
  const tagInputRef = useRef<HTMLInputElement>(null);

  const [scenarioSkillOrder, setScenarioSkillOrder] = useState<string[]>([]);

  const installedTools = tools.filter((tool) => tool.installed);
  const activeScenarioName = activeScenario?.name || t("mySkills.currentScenarioFallback");

  // Fetch sort order whenever active scenario changes
  useEffect(() => {
    if (!activeScenario) {
      setScenarioSkillOrder([]);
      return;
    }
    api.getScenarioSkillOrder(activeScenario.id).then(setScenarioSkillOrder).catch(() => {});
  }, [activeScenario, skills]);

  const refreshAllTags = async () => {
    try {
      const tags = await api.getAllTags();
      setAllTags(tags);
    } catch {
      // not critical
    }
  };

  useEffect(() => {
    refreshAllTags();
  }, [skills]);

  const toggleFilter = (set: Set<string>, value: string): Set<string> => {
    const next = new Set(set);
    if (next.has(value)) next.delete(value);
    else next.add(value);
    return next;
  };

  const filtered = useMemo(() => {
    const result = skills.filter((skill) => {
      const matchesSearch =
        skill.name.toLowerCase().includes(search.toLowerCase()) ||
        (skill.description || "").toLowerCase().includes(search.toLowerCase());
      if (!matchesSearch) return false;

      if (sourceFilters.size > 0 && !sourceFilters.has(skill.source_type)) return false;

      if (tagFilters.size > 0 && !skill.tags.some((t) => tagFilters.has(t))) return false;

      if (!activeScenario) return true;

      const enabledInScenario = skill.scenario_ids.includes(activeScenario.id);
      if (filterMode === "enabled") return enabledInScenario;
      if (filterMode === "available") return !enabledInScenario;
      return true;
    });

    // Always sort enabled skills first; within enabled group, use custom sort order
    if (activeScenario) {
      result.sort((a, b) => {
        const aEnabled = a.scenario_ids.includes(activeScenario.id) ? 0 : 1;
        const bEnabled = b.scenario_ids.includes(activeScenario.id) ? 0 : 1;
        if (aEnabled !== bEnabled) return aEnabled - bEnabled;
        // Within same group, use scenario sort order
        const aOrder = scenarioSkillOrder.indexOf(a.id);
        const bOrder = scenarioSkillOrder.indexOf(b.id);
        if (aOrder !== -1 && bOrder !== -1) return aOrder - bOrder;
        if (aOrder !== -1) return -1;
        if (bOrder !== -1) return 1;
        return a.name.localeCompare(b.name);
      });
    }

    return result;
  }, [skills, search, sourceFilters, tagFilters, filterMode, activeScenario, scenarioSkillOrder]);

  const {
    isMultiSelect, setIsMultiSelect,
    selectedIds,
    toggleSelect,
    isAllSelected,
    anyDisabled,
    handleSelectAll,
    exitMultiSelect,
  } = useMultiSelect({
    items: skills,
    filtered,
    getKey: (s) => s.id,
    isItemActive: (s) => activeScenario ? s.scenario_ids.includes(activeScenario.id) : true,
  });

  const selectedSkill = useMemo(
    () => skills.find((skill) => skill.id === detailSkillId) || null,
    [detailSkillId, skills]
  );

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const handleDragEnd = useCallback(
    async (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id || !activeScenario) return;

      // Only reorder enabled skills (they are always at the front)
      const enabledSkills = filtered.filter((s) => s.scenario_ids.includes(activeScenario.id));
      const oldIndex = enabledSkills.findIndex((s) => s.id === active.id);
      const newIndex = enabledSkills.findIndex((s) => s.id === over.id);
      if (oldIndex === -1 || newIndex === -1) return;

      const reordered = [...enabledSkills];
      const [moved] = reordered.splice(oldIndex, 1);
      reordered.splice(newIndex, 0, moved);

      // Optimistic update
      setScenarioSkillOrder(reordered.map((s) => s.id));

      try {
        await api.reorderScenarioSkills(activeScenario.id, reordered.map((s) => s.id));
      } catch {
        // Revert on failure
        await api.getScenarioSkillOrder(activeScenario.id).then(setScenarioSkillOrder).catch(() => {});
      }
    },
    [filtered, activeScenario]
  );

  const canDrag = !!activeScenario;

  const mapGitError = (error: unknown) => {
    const kind = getErrorKind(error);
    const message = getErrorMessage(error, "");

    // Use structured kind for high-level classification
    if (kind === "network") {
      return t("settings.gitErrorNetwork");
    }

    // Fall back to message-based matching for git-specific sub-categories
    if (
      message.includes("Authentication failed")
      || message.includes("Permission denied")
      || message.includes("could not read Username")
    ) {
      return t("settings.gitErrorAuth");
    }
    if (
      message.includes("Could not resolve host")
      || message.includes("Failed to connect")
      || message.includes("Connection timed out")
    ) {
      return t("settings.gitErrorNetwork");
    }
    if (message.includes("CONFLICT") || message.includes("conflict")) {
      return t("settings.gitErrorConflict");
    }
    if (message.includes("not a git repository")) {
      return t("settings.gitErrorNotRepo");
    }
    const fallback = t("settings.gitErrorGeneric");
    const detail = message.trim();
    if (detail && detail !== "Error") {
      return `${fallback} (${detail})`;
    }
    return fallback;
  };

  const refreshGitStatus = useCallback(async () => {
    try {
      const status = await api.gitBackupStatus();
      setGitStatus(status);
    } catch {
      // not critical
    }
  }, []);

  const refreshGitVersions = useCallback(async () => {
    if (!gitStatus?.is_repo) {
      setGitVersions([]);
      return;
    }
    setGitVersionsLoading(true);
    try {
      const versions = await api.gitBackupListVersions(30);
      setGitVersions(versions);
    } catch {
      setGitVersions([]);
    } finally {
      setGitVersionsLoading(false);
    }
  }, [gitStatus?.is_repo]);

  useEffect(() => {
    (async () => {
      const savedRemote = (await api.getSettings("git_backup_remote_url").catch(() => null))?.trim() || "";
      const status = await api.gitBackupStatus().catch(() => null);
      setGitStatus(status);

      if (savedRemote) {
        setGitRemoteConfig(savedRemote);
        return;
      }

      const detectedRemote = status?.remote_url?.trim() || "";
      if (detectedRemote) {
        setGitRemoteConfig(detectedRemote);
        api.setSettings("git_backup_remote_url", detectedRemote).catch(() => {});
      }
    })();
  }, []);

  useEffect(() => {
    const handleWindowFocus = () => {
      refreshGitStatus();
    };
    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        refreshGitStatus();
      }
    };

    window.addEventListener("focus", handleWindowFocus);
    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      window.removeEventListener("focus", handleWindowFocus);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [refreshGitStatus]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      refreshGitStatus();
    }, 400);
    return () => window.clearTimeout(timer);
  }, [skills, refreshGitStatus]);

  useEffect(() => {
    if (gitVersionsOpen && gitStatus?.is_repo) {
      refreshGitVersions();
    }
  }, [gitVersionsOpen, gitStatus?.is_repo, refreshGitVersions]);

  useEffect(() => {
    let cancelled = false;
    const loadToggles = async () => {
      if (!selectedSkill || !activeScenario) {
        setToolToggles(null);
        return;
      }
      if (!selectedSkill.scenario_ids.includes(activeScenario.id)) {
        setToolToggles(null);
        return;
      }
      try {
        const toggles = await api.getSkillToolToggles(selectedSkill.id, activeScenario.id);
        if (!cancelled) setToolToggles(toggles);
      } catch {
        if (!cancelled) setToolToggles(null);
      }
    };
    loadToggles();
    return () => {
      cancelled = true;
    };
  }, [selectedSkill?.id, activeScenario?.id, tools]);

  const handleToggleSkillTool = async (toolKey: string, enabled: boolean) => {
    if (!selectedSkill || !activeScenario) return;
    setTogglingToolKey(toolKey);
    try {
      await api.setSkillToolToggle(selectedSkill.id, activeScenario.id, toolKey, enabled);
      const displayName = getToolDisplayName(toolKey, tools);
      toast.success(
        enabled
          ? t("mySkills.agentToggleEnabled", { agent: displayName })
          : t("mySkills.agentToggleDisabled", { agent: displayName })
      );
      const [, toggles] = await Promise.all([
        refreshManagedSkills(),
        api.getSkillToolToggles(selectedSkill.id, activeScenario.id),
      ]);
      setToolToggles(toggles);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
      await refreshManagedSkills();
    } finally {
      setTogglingToolKey(null);
    }
  };

  const handleDeleteManagedSkill = async () => {
    if (!deleteTarget) return;
    await api.deleteManagedSkill(deleteTarget.id);
    if (selectedSkill?.id === deleteTarget.id) closeSkillDetail();
    toast.success(`${deleteTarget.name} ${t("mySkills.deleted")}`);
    setDeleteTarget(null);
    await Promise.all([refreshManagedSkills(), refreshScenarios()]);
  };

  const handleBatchDelete = async () => {
    const ids = Array.from(selectedIds);
    let deleted = 0;
    for (const id of ids) {
      try {
        await api.deleteManagedSkill(id);
        if (selectedSkill?.id === id) closeSkillDetail();
        deleted++;
      } catch {
        // continue deleting remaining
      }
    }
    if (deleted > 0) {
      toast.success(t("mySkills.batchDeleted", { count: deleted }));
    }
    if (deleted < ids.length) {
      toast.error(t("mySkills.batchDeleteFailed", { count: ids.length - deleted }));
    }
    exitMultiSelect();
    setBatchDeleteConfirm(false);
    await Promise.all([refreshManagedSkills(), refreshScenarios()]);
  };

  const handleBatchToggleScenario = async () => {
    if (!activeScenario) return;
    const selectedSkillsList = skills.filter((s) => selectedIds.has(s.id));
    const enabling = anyDisabled;
    let count = 0;
    let failed = 0;
    for (const skill of selectedSkillsList) {
      try {
        const enabledInScenario = skill.scenario_ids.includes(activeScenario.id);
        if (enabling && !enabledInScenario) {
          await api.addSkillToScenario(skill.id, activeScenario.id);
          count++;
        } else if (!enabling && enabledInScenario) {
          await api.removeSkillFromScenario(skill.id, activeScenario.id);
          count++;
        }
      } catch {
        failed++;
        // continue with remaining
      }
    }
    if (count > 0) {
      toast.success(enabling
        ? t("mySkills.batchEnabled", { count })
        : t("mySkills.batchDisabled", { count }));
    }
    if (failed > 0) {
      toast.error(t("mySkills.batchToggleFailed", { count: failed }));
    }
    await Promise.all([refreshManagedSkills(), refreshScenarios()]);
  };

  const handleToggleScenario = async (skill: ManagedSkill) => {
    if (!activeScenario) return;
    const enabledInScenario = skill.scenario_ids.includes(activeScenario.id);
    if (enabledInScenario) {
      await api.removeSkillFromScenario(skill.id, activeScenario.id);
      toast.success(`${skill.name} ${t("mySkills.disabledInScenario")}`);
    } else {
      await api.addSkillToScenario(skill.id, activeScenario.id);
      toast.success(`${skill.name} ${t("mySkills.enabledInScenario")}`);
    }
    await Promise.all([refreshManagedSkills(), refreshScenarios()]);
  };

  const handleCheckAllUpdates = async () => {
    setCheckingAll(true);
    try {
      await api.checkAllSkillUpdates(true);
      toast.success(t("mySkills.updateActions.checkedAll"));
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      await refreshManagedSkills();
      setCheckingAll(false);
    }
  };

  const handleCheckUpdate = async (skill: ManagedSkill) => {
    setCheckingSkillId(skill.id);
    try {
      await api.checkSkillUpdate(skill.id, true);
      await refreshManagedSkills();
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
      await refreshManagedSkills();
    } finally {
      setCheckingSkillId(null);
    }
  };

  const handleRefreshSkill = async (skill: ManagedSkill) => {
    setUpdatingSkillId(skill.id);
    try {
      if (skill.source_type === "local" || skill.source_type === "import") {
        await api.reimportLocalSkill(skill.id);
        toast.success(t("mySkills.updateActions.reimported"));
      } else {
        await api.updateSkill(skill.id);
        toast.success(t("mySkills.updateActions.updated"));
      }
      await refreshManagedSkills();
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
      await refreshManagedSkills();
    } finally {
      setUpdatingSkillId(null);
    }
  };

  const handleAddTag = async (skill: ManagedSkill, inputValue?: string) => {
    const trimmed = (inputValue ?? tagInput).trim();
    if (!trimmed || skill.tags.includes(trimmed)) {
      setTagInput("");
      return;
    }
    try {
      await api.setSkillTags(skill.id, [...skill.tags, trimmed]);
      toast.success(t("mySkills.tags.tagAdded"));
      setTagEditSkillId(null);
      setTagInput("");
      await refreshManagedSkills();
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    }
  };

  const handleRemoveTag = async (skill: ManagedSkill, tagToRemove: string) => {
    try {
      await api.setSkillTags(skill.id, skill.tags.filter((t) => t !== tagToRemove));
      toast.success(t("mySkills.tags.tagsUpdated"));
      await refreshManagedSkills();
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    }
  };

  const getTagOptions = (skill: ManagedSkill, keyword: string) => {
    const needle = keyword.trim().toLowerCase();
    return allTags.filter((tag) => {
      if (skill.tags.includes(tag)) return false;
      if (!needle) return true;
      return tag.toLowerCase().includes(needle);
    });
  };

  const handleGitStartBackup = async () => {
    setGitLoading("start");
    try {
      if (gitRemoteConfig) {
        await api.gitBackupClone(gitRemoteConfig);
        toast.success(t("settings.gitCloneSuccess"));
      } else {
        await api.gitBackupInit();
        toast.success(t("settings.gitInitSuccess"));
      }
      await refreshGitStatus();
    } catch (e) {
      toast.error(mapGitError(e));
    } finally {
      setGitLoading(null);
    }
  };

  const handleGitSync = async () => {
    setGitLoading("sync");
    try {
      let status = await api.gitBackupStatus();
      if (!status.is_repo) {
        toast.info(t("settings.gitNotInitialized"));
        return;
      }

      if (!status.remote_url && gitRemoteConfig) {
        await api.gitBackupSetRemote(gitRemoteConfig);
        status = await api.gitBackupStatus();
      }

      if (!status.remote_url) {
        toast.info(t("settings.gitNeedRemoteSetup"));
        return;
      }

      let committed = false;
      if (status.has_changes) {
        await api.gitBackupCommit(t("settings.gitCommitPlaceholder"));
        committed = true;
        status = await api.gitBackupStatus();
      }

      if (status.behind > 0) {
        await api.gitBackupPull();
        status = await api.gitBackupStatus();
        toast.success(t("settings.gitPullSuccess"));
      }

      if (committed || status.ahead > 0) {
        const snapshotTag = await api.gitBackupCreateSnapshot();
        await api.gitBackupPush();
        toast.success(t("mySkills.gitSyncSuccessWithVersion", { tag: displaySnapshotLabel(snapshotTag) }));
      } else {
        toast.success(t("settings.gitUpToDate"));
      }

      await refreshGitStatus();
      if (gitVersionsOpen) {
        await refreshGitVersions();
      }
    } catch (e) {
      toast.error(mapGitError(e));
    } finally {
      setGitLoading(null);
    }
  };

  const handleRestoreVersion = async () => {
    if (!restoreVersionTag) return;
    setRestoringVersionTag(restoreVersionTag);
    try {
      await api.gitBackupRestoreVersion(restoreVersionTag);
      toast.success(t("mySkills.gitVersionRestoreSuccess", { tag: displaySnapshotLabel(restoreVersionTag) }));
      toast.info(t("mySkills.gitVersionRestoreNeedSync"));
      await Promise.all([refreshGitStatus(), refreshGitVersions(), refreshManagedSkills()]);
      setRestoreVersionTag(null);
    } catch (error: unknown) {
      toast.error(mapGitError(error));
    } finally {
      setRestoringVersionTag(null);
    }
  };

  const getGitSyncButtonState = () => {
    if (!gitStatus) {
      return {
        label: t("mySkills.gitRepoSync"),
        disabled: false,
        toneClassName: "text-secondary",
      };
    }
    if (!gitStatus.remote_url && !gitRemoteConfig) {
      return {
        label: t("mySkills.gitRepoNeedRemote"),
        disabled: true,
        toneClassName: "text-red-500",
      };
    }
    if (gitStatus.has_changes || gitStatus.ahead > 0 || gitStatus.behind > 0) {
      return {
        label: t("mySkills.gitRepoSync"),
        disabled: false,
        toneClassName: "text-amber-500",
      };
    }
    if (!gitStatus.has_changes && gitStatus.ahead === 0 && gitStatus.behind === 0) {
      return {
        label: t("mySkills.gitRepoUpToDate"),
        disabled: true,
        toneClassName: "text-muted",
      };
    }
    return {
      label: t("mySkills.gitRepoSync"),
      disabled: false,
      toneClassName: "text-secondary",
    };
  };

  const sourceIcon = (type: string) => {
    switch (type) {
      case "git":
      case "skillssh":
        return <Github className="h-3 w-3" />;
      case "local":
      case "import":
        return <HardDrive className="h-3 w-3" />;
      default:
        return <Globe className="h-3 w-3" />;
    }
  };

  const canRefresh = (skill: ManagedSkill) =>
    skill.source_type === "git" ||
    skill.source_type === "skillssh" ||
    skill.source_type === "local" ||
    skill.source_type === "import";

  const sourceTypeLabel = (skill: ManagedSkill) =>
    skill.source_type === "skillssh" ? "skills.sh" : skill.source_type;

  const formatGitDateTime = (iso: string) => {
    if (!iso) return "—";
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    return d.toLocaleString();
  };

  const renderCurrentVersionText = () => {
    if (!gitStatus?.is_repo) return null;
    if (gitStatus.current_snapshot_tag) {
      return t("mySkills.gitCurrentVersionSnapshot", {
        tag: displaySnapshotLabel(gitStatus.current_snapshot_tag),
      });
    }
    if (gitStatus.restored_from_tag) {
      return t("mySkills.gitCurrentVersionRestored", {
        tag: displaySnapshotLabel(gitStatus.restored_from_tag),
      });
    }
    return t("mySkills.gitCurrentVersionUnknown");
  };

  const refreshLabel = (skill: ManagedSkill) =>
    skill.source_type === "local" || skill.source_type === "import"
      ? t("mySkills.updateActions.reimport")
      : t("mySkills.updateActions.update");

  const statusBadge = (skill: ManagedSkill) => {
    if (skill.update_status === "update_available") {
      return {
        label: "Update",
        className: "bg-amber-500/12 text-amber-600 dark:text-amber-400",
      };
    }
    if (skill.update_status === "source_missing") {
      return {
        label: t("mySkills.updateStatus.sourceMissing"),
        className: "bg-red-500/10 text-red-600 dark:text-red-300",
      };
    }
    if (skill.update_status === "error") {
      return {
        label: t("mySkills.updateStatus.error"),
        className: "bg-red-500/10 text-red-600 dark:text-red-300",
      };
    }
    return null;
  };

  return (
    <div className="app-page">
      <div className="app-page-header pr-2 pb-1">
        <h1 className="app-page-title flex items-center gap-2">
          {t("mySkills.title")}
          <span className="app-badge">
            {skills.length}
          </span>
        </h1>
      </div>

      <div className="app-toolbar">
        <div className="flex flex-1 gap-3">
          <div className="relative w-full max-w-[280px]">
            <Search className="absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("mySkills.searchPlaceholder")}
              className="app-input w-full pl-9 font-medium"
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
            />
          </div>

          <div className="app-segmented">
            {(["all", "enabled", "available"] as const).map((mode) => (
              <button
                key={mode}
                onClick={() => setFilterMode(mode)}
                className={cn(
                  "app-segmented-button",
                  filterMode === mode && "app-segmented-button-active"
                )}
              >
                {t(`mySkills.filters.${mode}`)}
              </button>
            ))}
          </div>

        </div>

        <div className="app-segmented">
          {!gitStatus?.is_repo ? (
            <button
              onClick={handleGitStartBackup}
              disabled={!!gitLoading}
              className="inline-flex items-center gap-1 rounded-md px-3 py-2 text-[13px] font-medium text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
            >
              {gitLoading === "start" ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <GitBranch className="h-3.5 w-3.5" />
              )}
              {gitLoading === "start" ? t("settings.gitInitializing") : t("settings.gitStartBackup")}
            </button>
          ) : (
            (() => {
              const gitSyncButton = getGitSyncButtonState();
              return (
                <>
                  <button
                    onClick={handleGitSync}
                    disabled={!!gitLoading || gitSyncButton.disabled}
                    className={cn(
                      "inline-flex items-center gap-1 rounded-md px-3 py-2 text-[13px] font-medium transition-colors hover:bg-surface-hover disabled:opacity-50",
                      gitSyncButton.toneClassName
                    )}
                  >
                    {gitLoading === "sync" ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <ArrowUpCircle className="h-3.5 w-3.5" />
                    )}
                    {gitLoading === "sync" ? t("mySkills.gitRepoSyncing") : gitSyncButton.label}
                  </button>
                  <button
                    onClick={() => setGitVersionsOpen((v) => !v)}
                    disabled={!!gitLoading}
                    className={cn(
                      "ml-1 inline-flex items-center gap-1 rounded-md px-3 py-2 text-[13px] font-medium transition-colors hover:bg-surface-hover disabled:opacity-50",
                      gitVersionsOpen ? "text-secondary" : "text-muted"
                    )}
                  >
                    <History className="h-3.5 w-3.5" />
                    {t("mySkills.gitVersionHistory")}
                  </button>
                </>
              );
            })()
          )}
          <button
            onClick={handleCheckAllUpdates}
            disabled={checkingAll}
            className="ml-2 mr-2 inline-flex items-center gap-1 rounded-md border-l border-border-subtle pl-4 pr-3 py-2 text-[13px] font-medium text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
          >
            <RefreshCw className={cn("h-3.5 w-3.5", checkingAll && "animate-spin")} />
            {t("mySkills.updateActions.checkAll")}
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
          <button
            onClick={() => isMultiSelect ? exitMultiSelect() : setIsMultiSelect(true)}
            className={cn(
              "rounded-md p-2 transition-colors outline-none",
              isMultiSelect ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
            )}
            title={isMultiSelect ? t("mySkills.cancelSelect") : t("mySkills.selectMode")}
          >
            <SquareCheck className="h-4 w-4" />
          </button>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-1 px-1 -mt-2 -mb-3">
        {(["local", "import", "git", "skillssh"] as const).map((src) => (
          <button
            key={src}
            onClick={() => setSourceFilters(toggleFilter(sourceFilters, src))}
            className={cn(
              "rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
              sourceFilters.has(src)
                ? "bg-accent text-white dark:bg-accent dark:text-white"
                : "bg-surface-hover text-muted hover:text-secondary"
            )}
          >
            {t(`mySkills.sourceFilter.${src}`)}
          </button>
        ))}
        {allTags.length > 0 && (
          <>
            <span className="mx-0.5 h-3 w-px bg-border-subtle" />
            {allTags.map((tag, i) => {
              const colors = [
                "bg-blue-500/15 text-blue-600 dark:text-blue-400",
                "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400",
                "bg-violet-500/15 text-violet-600 dark:text-violet-400",
                "bg-amber-500/15 text-amber-600 dark:text-amber-400",
                "bg-rose-500/15 text-rose-600 dark:text-rose-400",
                "bg-cyan-500/15 text-cyan-600 dark:text-cyan-400",
                "bg-orange-500/15 text-orange-600 dark:text-orange-400",
                "bg-pink-500/15 text-pink-600 dark:text-pink-400",
              ];
              const activeColors = [
                "bg-blue-500 text-white dark:bg-blue-500",
                "bg-emerald-500 text-white dark:bg-emerald-500",
                "bg-violet-500 text-white dark:bg-violet-500",
                "bg-amber-500 text-white dark:bg-amber-500",
                "bg-rose-500 text-white dark:bg-rose-500",
                "bg-cyan-500 text-white dark:bg-cyan-500",
                "bg-orange-500 text-white dark:bg-orange-500",
                "bg-pink-500 text-white dark:bg-pink-500",
              ];
              const colorIndex = i % colors.length;
              const isActive = tagFilters.has(tag);
              return (
                <button
                  key={tag}
                  onClick={() => setTagFilters(toggleFilter(tagFilters, tag))}
                  className={cn(
                    "rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                    isActive ? activeColors[colorIndex] : colors[colorIndex]
                  )}
                >
                  {tag}
                </button>
              );
            })}
          </>
        )}
      </div>

      {isMultiSelect && (
        <MultiSelectToolbar
          selectedCount={selectedIds.size}
          isAllSelected={isAllSelected}
          anyDisabled={activeScenario ? anyDisabled : false}
          showToggle={!!activeScenario}
          labels={{
            hint: t("mySkills.selectHint"),
            selected: t("mySkills.selectedCount", { count: selectedIds.size }),
            delete: t("mySkills.deleteSelected", { count: selectedIds.size }),
            enable: t("mySkills.batchEnable", { count: selectedIds.size }),
            disable: t("mySkills.batchDisable", { count: selectedIds.size }),
            selectAll: t("mySkills.selectAll"),
            deselectAll: t("mySkills.deselectAll"),
            cancel: t("common.cancel"),
          }}
          onDelete={() => setBatchDeleteConfirm(true)}
          onToggle={handleBatchToggleScenario}
          onSelectAll={handleSelectAll}
          onCancel={exitMultiSelect}
        />
      )}

      {gitVersionsOpen && gitStatus?.is_repo && (
        <div className="app-panel -mt-2 mb-2 p-3">
          <div className="mb-2 flex items-center justify-between">
            <div className="min-w-0">
              <h3 className="text-[13px] font-semibold text-secondary">{t("mySkills.gitVersionHistory")}</h3>
              <div className="truncate text-[11px] text-faint">{renderCurrentVersionText()}</div>
            </div>
            <button
              onClick={refreshGitVersions}
              disabled={gitVersionsLoading || !!gitLoading}
              className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-[13px] text-muted hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
            >
              <RefreshCw className={cn("h-3 w-3", gitVersionsLoading && "animate-spin")} />
              {t("settings.refresh")}
            </button>
          </div>
          {gitVersionsLoading ? (
            <div className="py-2 text-[13px] text-muted">{t("mySkills.gitVersionLoading")}</div>
          ) : gitVersions.length === 0 ? (
            <div className="py-2 text-[13px] text-muted">{t("mySkills.gitVersionEmpty")}</div>
          ) : (
            <div className="max-h-64 space-y-1 overflow-auto pr-1">
              {gitVersions.map((version) => (
                <div
                  key={version.tag}
                  className="flex items-center justify-between rounded-md border border-border-subtle bg-bg-secondary px-2.5 py-2"
                >
                  <div className="min-w-0 pr-3">
                    <div className="truncate text-[13px] font-medium text-secondary">{displaySnapshotLabel(version.tag)}</div>
                    <div className="truncate text-[12px] text-muted">
                      {version.message || version.commit}
                    </div>
                    <div className="text-[11px] text-faint">
                      {version.commit} · {formatGitDateTime(version.committed_at)}
                    </div>
                  </div>
                  <button
                    onClick={() => setRestoreVersionTag(version.tag)}
                    disabled={!!restoringVersionTag}
                    className="shrink-0 rounded-md border border-border-subtle px-2 py-1 text-[12px] font-medium text-secondary hover:bg-surface-hover disabled:opacity-50"
                  >
                    {restoringVersionTag === version.tag
                      ? t("mySkills.gitVersionRestoring")
                      : t("mySkills.gitVersionRestore")}
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {filtered.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center pb-20 text-center">
          <Layers className="mb-4 h-12 w-12 text-faint" />
          <h3 className="mb-1.5 text-[14px] font-semibold text-tertiary">{t("mySkills.noSkills")}</h3>
          <p className="text-[13px] text-muted">
            {skills.length === 0 ? t("mySkills.addFirst") : t("mySkills.noMatch")}
          </p>
        </div>
      ) : (
        <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
          <SortableContext
            items={filtered.map((s) => s.id)}
            strategy={viewMode === "grid" ? rectSortingStrategy : verticalListSortingStrategy}
          >
          <div
            className={cn(
              "pb-8",
              viewMode === "grid"
                ? "grid grid-cols-2 gap-3 lg:grid-cols-3"
                : "flex flex-col gap-0.5"
            )}
          >
          {filtered.map((skill) => {
            const isSynced = skill.targets.length > 0;
            const enabledInScenario = activeScenario
              ? skill.scenario_ids.includes(activeScenario.id)
              : false;
            const badge = statusBadge(skill);

            if (viewMode === "grid") {
              return (
                <SortableSkillItem key={skill.id} id={skill.id} disabled={!canDrag}>
                {(dragHandle) => (
                <div
                  className={cn(
                    "app-panel group relative flex flex-col overflow-hidden transition-all hover:border-border hover:bg-surface-hover",
                    enabledInScenario && "border-l-2 border-l-accent",
                    isMultiSelect && "cursor-pointer",
                    isMultiSelect && selectedIds.has(skill.id) && "ring-1 ring-accent border-accent/40"
                  )}
                  onClick={isMultiSelect ? () => toggleSelect(skill.id) : undefined}
                >
                  <div className={cn("absolute right-2 top-2 flex items-center gap-0.5 rounded-lg border border-border-subtle bg-surface px-1 py-0.5 opacity-0 shadow-sm transition-all", !isMultiSelect && "group-hover:opacity-100")}>
                    {dragHandle}
                    <button
                      onClick={() => handleCheckUpdate(skill)}
                      disabled={checkingSkillId === skill.id}
                      className="rounded p-1 text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
                      title={t("mySkills.updateActions.check")}
                    >
                      <RefreshCw className={cn("h-3.5 w-3.5", checkingSkillId === skill.id && "animate-spin")} />
                    </button>
                    {canRefresh(skill) ? (
                      <button
                        onClick={() => handleRefreshSkill(skill)}
                        disabled={updatingSkillId === skill.id}
                        className="rounded p-1 text-accent-light transition-colors hover:bg-accent-bg disabled:opacity-50"
                        title={refreshLabel(skill)}
                      >
                        <RotateCcw className={cn("h-3.5 w-3.5", updatingSkillId === skill.id && "animate-spin")} />
                      </button>
                    ) : null}
                    <button
                      onClick={() => setDeleteTarget(skill)}
                      className="rounded p-1 text-faint transition-colors hover:text-red-400"
                      title={t("mySkills.delete")}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </button>
                  </div>

                  <div className="flex items-center gap-2.5 px-3.5 pr-20 pt-3 pb-1.5">
                    {isMultiSelect ? (
                      selectedIds.has(skill.id)
                        ? <SquareCheck className="h-3.5 w-3.5 shrink-0 text-accent" />
                        : <Square className="h-3.5 w-3.5 shrink-0 text-faint" />
                    ) : isSynced ? (
                      <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-emerald-500" />
                    ) : (
                      <Circle className="h-3.5 w-3.5 shrink-0 text-faint" />
                    )}
                    <h3
                      className="flex-1 cursor-pointer truncate text-[14px] font-semibold text-primary hover:text-accent-light"
                      onClick={isMultiSelect ? undefined : () => openSkillDetailById(skill.id)}
                      title={skill.name}
                    >
                      {skill.name}
                    </h3>
                  </div>

                  <div className="px-3.5 pb-3">
                    <p className="text-[13px] leading-[18px] text-muted truncate">
                      {skill.description || "—"}
                    </p>
                    {badge && (
                      <div className="mt-2 flex flex-wrap items-center gap-1.5">
                        <span
                          className={cn(
                            "rounded-full px-2 py-0.5 text-[13px] font-medium",
                            badge.className
                          )}
                        >
                          {badge.label}
                        </span>
                      </div>
                    )}
                    <div className="mt-2 flex flex-wrap items-center gap-1">
                      {skill.tags.map((tag) => (
                        <span
                          key={tag}
                          className="group/tag inline-flex items-center gap-0.5 rounded-full bg-accent-bg px-2 py-0.5 text-[11px] font-medium text-accent-light"
                        >
                          {tag}
                          <button
                            onClick={(e) => { e.stopPropagation(); handleRemoveTag(skill, tag); }}
                            className="hidden group-hover/tag:inline-flex rounded-full p-0 text-accent-light/60 hover:text-accent-light"
                          >
                            <X className="h-2.5 w-2.5" />
                          </button>
                        </span>
                      ))}
                      {tagEditSkillId === skill.id ? (
                        <div className="relative">
                          <input
                            ref={tagInputRef}
                            type="text"
                            value={tagInput}
                            onChange={(e) => setTagInput(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") { handleAddTag(skill); }
                              if (e.key === "Escape") { setTagEditSkillId(null); setTagInput(""); }
                            }}
                            onBlur={() => {
                              if (tagInput.trim()) handleAddTag(skill);
                              else { setTagEditSkillId(null); setTagInput(""); }
                            }}
                            placeholder={t("mySkills.tags.addTag")}
                            className="h-5 w-28 rounded-full border border-border-subtle bg-transparent px-1.5 text-[11px] text-secondary outline-none focus:border-accent"
                            autoFocus
                          />
                          {getTagOptions(skill, tagInput).length > 0 && (
                            <div className="absolute left-0 top-6 z-10 min-w-[112px] max-w-[180px] rounded-md border border-border-subtle bg-surface p-1 shadow-lg">
                              {getTagOptions(skill, tagInput).slice(0, 6).map((tagOption) => (
                                <button
                                  key={tagOption}
                                  type="button"
                                  onMouseDown={(e) => e.preventDefault()}
                                  onClick={() => handleAddTag(skill, tagOption)}
                                  className="w-full truncate rounded px-1.5 py-1 text-left text-[11px] text-secondary hover:bg-surface-hover"
                                  title={tagOption}
                                >
                                  {tagOption}
                                </button>
                              ))}
                            </div>
                          )}
                        </div>
                      ) : (
                        <button
                          onClick={(e) => { e.stopPropagation(); setTagEditSkillId(skill.id); setTagInput(""); }}
                          className="inline-flex items-center rounded-full p-0.5 text-faint transition-colors hover:text-muted opacity-0 group-hover:opacity-100"
                          title={t("mySkills.tags.addTag")}
                        >
                          <Plus className="h-3 w-3" />
                        </button>
                      )}
                    </div>
                  </div>

                  <div className="mt-auto flex items-center justify-between gap-2 border-t border-border-subtle px-3.5 py-2.5">
                    <div className="flex min-w-0 items-center gap-1.5">
                      <span className="inline-flex shrink-0 items-center gap-1 text-[13px] text-muted">
                        {sourceIcon(skill.source_type)}
                        {sourceTypeLabel(skill)}
                      </span>
                      {enabledInScenario && (
                        <>
                          <span className="text-faint">·</span>
                          <span className="truncate text-[13px] font-medium text-amber-600 dark:text-amber-400/80">
                            {activeScenarioName}
                          </span>
                        </>
                      )}
                    </div>
                    <div className="flex items-center gap-1.5 shrink-0">
                      <button
                        onClick={() => handleToggleScenario(skill)}
                        disabled={!activeScenario}
                        className={cn(
                          "rounded px-2 py-1 text-[13px] font-medium transition-colors outline-none",
                          enabledInScenario
                            ? "text-emerald-600 dark:text-emerald-400 hover:bg-emerald-500/10"
                            : "text-muted hover:bg-surface-hover hover:text-secondary"
                        )}
                      >
                        {enabledInScenario ? t("mySkills.enabledButton") : t("mySkills.enable")}
                      </button>
                    </div>
                  </div>
                </div>
                )}
                </SortableSkillItem>
              );
            }

            return (
              <SortableSkillItem key={skill.id} id={skill.id} disabled={!canDrag}>
              {(dragHandle) => (
              <div
                className={cn(
                  "app-panel group flex items-center gap-3.5 rounded-xl border-transparent px-3.5 py-3 transition-all hover:border-border hover:bg-surface-hover",
                  enabledInScenario && "border-l-2 border-l-accent",
                  isMultiSelect && "cursor-pointer",
                  isMultiSelect && selectedIds.has(skill.id) && "ring-1 ring-accent border-accent/40"
                )}
                onClick={isMultiSelect ? () => toggleSelect(skill.id) : undefined}
              >
                {dragHandle}
                {isMultiSelect ? (
                  selectedIds.has(skill.id)
                    ? <SquareCheck className="h-3.5 w-3.5 shrink-0 text-accent" />
                    : <Square className="h-3.5 w-3.5 shrink-0 text-faint" />
                ) : isSynced ? (
                  <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-emerald-500" />
                ) : (
                  <Circle className="h-3.5 w-3.5 shrink-0 text-faint" />
                )}

                <h3
                  className="w-[180px] shrink-0 truncate cursor-pointer text-[14px] font-semibold text-secondary hover:text-primary"
                  onClick={isMultiSelect ? undefined : () => openSkillDetailById(skill.id)}
                  title={skill.name}
                >
                  {skill.name}
                </h3>

                <p className="min-w-0 flex-1 truncate text-[13px] text-muted">
                  {skill.description || "—"}
                </p>

                <div className="flex shrink-0 items-center gap-1.5">
                  {skill.tags.map((tag) => (
                    <span
                      key={tag}
                      className="inline-flex items-center rounded-full bg-accent-bg px-1.5 py-0.5 text-[11px] font-medium text-accent-light"
                    >
                      {tag}
                    </span>
                  ))}
                </div>

                <div className="flex shrink-0 items-center gap-2.5">
                  <span className="inline-flex items-center gap-1 text-[13px] text-muted">
                    {sourceIcon(skill.source_type)}
                    {sourceTypeLabel(skill)}
                  </span>
                  {enabledInScenario && (
                    <span className="text-[13px] font-medium text-amber-600 dark:text-amber-400/80">
                      {activeScenarioName}
                    </span>
                  )}
                </div>

                <div className={cn("flex shrink-0 items-center gap-1 opacity-0 transition-opacity", !isMultiSelect && "group-hover:opacity-100")}>
                  <button
                    onClick={() => handleToggleScenario(skill)}
                    disabled={!activeScenario}
                    className={cn(
                      "rounded px-2 py-0.5 text-[13px] font-medium transition-colors outline-none",
                      enabledInScenario
                        ? "text-emerald-600 dark:text-emerald-400 hover:bg-emerald-500/10"
                        : "text-muted hover:bg-surface-hover hover:text-secondary"
                    )}
                  >
                    {enabledInScenario ? t("mySkills.enabledButton") : t("mySkills.enable")}
                  </button>
                  <button
                    onClick={() => handleCheckUpdate(skill)}
                    disabled={checkingSkillId === skill.id}
                    className="rounded p-0.5 text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
                    title={t("mySkills.updateActions.check")}
                  >
                    <RefreshCw className={cn("h-3.5 w-3.5", checkingSkillId === skill.id && "animate-spin")} />
                  </button>
                  {canRefresh(skill) ? (
                    <button
                      onClick={() => handleRefreshSkill(skill)}
                      disabled={updatingSkillId === skill.id}
                      className="rounded p-0.5 text-accent-light transition-colors hover:bg-accent-bg disabled:opacity-50"
                      title={refreshLabel(skill)}
                    >
                      <RotateCcw className={cn("h-3.5 w-3.5", updatingSkillId === skill.id && "animate-spin")} />
                    </button>
                  ) : null}
                  <button
                    onClick={() => setDeleteTarget(skill)}
                    className="rounded p-0.5 text-faint transition-colors hover:text-red-400"
                    title={t("mySkills.delete")}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                </div>
              </div>
              )}
              </SortableSkillItem>
            );
          })}
        </div>
          </SortableContext>
        </DndContext>
      )}

      <SkillDetailPanel
        skill={selectedSkill}
        onClose={closeSkillDetail}
        toolToggles={toolToggles}
        togglingTool={togglingToolKey}
        onToggleTool={handleToggleSkillTool}
      />

      <ConfirmDialog
        open={deleteTarget !== null}
        message={t("mySkills.deleteConfirm", { name: deleteTarget?.name || "" })}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleDeleteManagedSkill}
      />
      <ConfirmDialog
        open={batchDeleteConfirm}
        message={t("mySkills.batchDeleteConfirm", { count: selectedIds.size })}
        onClose={() => setBatchDeleteConfirm(false)}
        onConfirm={handleBatchDelete}
      />
      <ConfirmDialog
        open={restoreVersionTag !== null}
        title={t("mySkills.gitVersionRestoreTitle")}
        message={t("mySkills.gitVersionRestoreConfirm", { tag: displaySnapshotLabel(restoreVersionTag || "") })}
        tone="warning"
        confirmLabel={t("mySkills.gitVersionRestore")}
        onClose={() => setRestoreVersionTag(null)}
        onConfirm={handleRestoreVersion}
      />
    </div>
  );
}
