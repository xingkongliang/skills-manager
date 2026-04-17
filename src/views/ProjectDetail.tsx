import { useState, useEffect, useCallback, useMemo } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
  FolderOpen,
  Search,
  LayoutGrid,
  List,
  RefreshCw,
  FileText,
  Download,
  Upload,
  RotateCcw,
  Layers,
  X,
  Loader2,
  ChevronDown,
  ChevronRight,
  Trash2,
  SquareCheck,
  Square,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { createPortal } from "react-dom";
import { toast } from "sonner";
import { useApp } from "../context/AppContext";
import { useMultiSelect } from "../hooks/useMultiSelect";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { MultiSelectToolbar } from "../components/MultiSelectToolbar";
import { SkillMarkdown } from "../components/SkillMarkdown";
import { cn } from "../utils";
import * as api from "../lib/tauri";
import type { ProjectSkill, ManagedSkill, ProjectAgentTarget } from "../lib/tauri";
import { getErrorMessage } from "../lib/error";

const PROJECT_DEFAULT_EXPORT_AGENTS_KEY = "project_default_export_agents";
const PROJECT_EXPORT_AGENT_PRIORITY = ["claude_code", "codex", "cursor", "gemini_cli", "github_copilot"];

interface ProjectSkillGroup {
  id: string;
  name: string;
  dir_name: string;
  relative_path: string;
  description: string | null;
  files: string[];
  variants: ProjectSkill[];
  enabledCount: number;
  totalCount: number;
  primaryVariant: ProjectSkill;
  status: ProjectSkill["sync_status"];
}

function getDefaultExportAgents(targets: ProjectAgentTarget[], savedValue?: string | null) {
  const availableKeys = new Set(targets.map((target) => target.key));
  if (savedValue) {
    try {
      const parsed = JSON.parse(savedValue);
      if (Array.isArray(parsed)) {
        const filtered = parsed.filter((item): item is string => typeof item === "string" && availableKeys.has(item));
        if (filtered.length > 0) {
          return Array.from(new Set(filtered));
        }
      }
    } catch {
      // Ignore invalid persisted settings and fall back to built-in defaults.
    }
  }

  const prioritized = PROJECT_EXPORT_AGENT_PRIORITY.filter((key) => availableKeys.has(key));
  const fallback = targets.map((target) => target.key);
  return Array.from(new Set((prioritized.length > 0 ? prioritized : fallback).slice(0, 3)));
}

function getSyncStatusMeta(t: (key: string) => string, status: ProjectSkill["sync_status"]) {
  switch (status) {
    case "in_sync":
      return {
        label: t("project.syncStatus.inSync"),
        className: "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
      };
    case "project_newer":
      return {
        label: t("project.syncStatus.projectNewer"),
        className: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
      };
    case "center_newer":
      return {
        label: t("project.syncStatus.centerNewer"),
        className: "bg-sky-500/10 text-sky-700 dark:text-sky-300",
      };
    case "diverged":
      return {
        label: t("project.syncStatus.diverged"),
        className: "bg-violet-500/10 text-violet-700 dark:text-violet-300",
      };
    default:
      return {
        label: t("project.syncStatus.projectOnly"),
        className: "bg-surface-hover text-muted",
      };
  }
}

function getGroupStatus(variants: ProjectSkill[]): ProjectSkill["sync_status"] {
  const priority: ProjectSkill["sync_status"][] = [
    "diverged",
    "project_newer",
    "center_newer",
    "project_only",
    "in_sync",
  ];
  for (const status of priority) {
    if (variants.some((variant) => variant.sync_status === status)) {
      return status;
    }
  }
  return "project_only";
}

function areAgentSetsEqual(left: string[], right: string[]) {
  if (left.length !== right.length) return false;
  const rightSet = new Set(right);
  return left.every((value) => rightSet.has(value));
}

function getAssignedAgents(variants: ProjectSkill[]) {
  return Array.from(new Set(variants.map((variant) => variant.agent))).sort();
}

function isGroupEnabled(skill: Pick<ProjectSkillGroup, "enabledCount">) {
  return skill.enabledCount > 0;
}

export function ProjectDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { t } = useTranslation();
  const { projects, managedSkills, refreshManagedSkills, refreshScenarios, refreshProjects } = useApp();
  const [skills, setSkills] = useState<ProjectSkill[]>([]);
  const [projectAgentTargets, setProjectAgentTargets] = useState<ProjectAgentTarget[]>([]);
  const [selectedExportAgents, setSelectedExportAgents] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [filterMode, setFilterMode] = useState<"all" | "enabled" | "disabled">("all");
  const [search, setSearch] = useState("");
  const [detailSkill, setDetailSkill] = useState<ProjectSkillGroup | null>(null);
  const [docContent, setDocContent] = useState<string | null>(null);
  const [docLoading, setDocLoading] = useState(false);
  const [updatingCenterSkill, setUpdatingCenterSkill] = useState<string | null>(null);
  const [updatingProjectSkill, setUpdatingProjectSkill] = useState<string | null>(null);
  const [togglingSkill, setTogglingSkill] = useState<string | null>(null);
  const [showExportDialog, setShowExportDialog] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<ProjectSkillGroup | null>(null);
  const [batchDeleteConfirm, setBatchDeleteConfirm] = useState(false);

  const project = projects.find((p) => p.id === id);
  const getSkillKey = useCallback((skill: Pick<ProjectSkillGroup, "id">) => {
    return skill.id;
  }, []);

  const loadSkills = useCallback(async () => {
    if (!id) return;
    setLoading(true);
    try {
      const result = await api.getProjectSkills(id);
      setSkills(result);
    } catch (e) {
      console.error("Failed to load project skills:", e);
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    loadSkills();
  }, [loadSkills]);

  useEffect(() => {
    let cancelled = false;
    const loadProjectAgentTargets = async () => {
      if (!id) return;
      try {
        const result = await api.getProjectAgentTargets(id);
        if (!cancelled) {
          setProjectAgentTargets(result);
        }
      } catch (e) {
        console.error("Failed to load project agent targets:", e);
      }
    };
    loadProjectAgentTargets();
    return () => {
      cancelled = true;
    };
  }, [id]);

  useEffect(() => {
    if (!project && !loading) {
      navigate("/");
    }
  }, [project, loading, navigate]);

  const groupedSkills = useMemo<ProjectSkillGroup[]>(() => {
    const groups = new Map<string, ProjectSkillGroup>();
    for (const skill of skills) {
      const key = skill.relative_path.toLowerCase();
      const existing = groups.get(key);
      if (existing) {
        existing.variants.push(skill);
        existing.enabledCount += skill.enabled ? 1 : 0;
        existing.totalCount += 1;
        existing.files = Array.from(new Set([...existing.files, ...skill.files])).sort();
        if (!existing.description && skill.description) {
          existing.description = skill.description;
        }
        continue;
      }
      groups.set(key, {
        id: key,
        name: skill.name,
        dir_name: skill.dir_name,
        relative_path: skill.relative_path,
        description: skill.description,
        files: [...skill.files],
        variants: [skill],
        enabledCount: skill.enabled ? 1 : 0,
        totalCount: 1,
        primaryVariant: skill,
        status: skill.sync_status,
      });
    }
    return Array.from(groups.values())
      .map((group) => ({
        ...group,
        variants: [...group.variants].sort((a, b) => a.agent_display_name.localeCompare(b.agent_display_name)),
        primaryVariant: [...group.variants].sort((a, b) => a.agent_display_name.localeCompare(b.agent_display_name))[0],
        status: getGroupStatus(group.variants),
      }))
      .sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  }, [skills]);

  const filtered = useMemo(() => {
    return groupedSkills.filter((skill) => {
      const matchesSearch =
        skill.name.toLowerCase().includes(search.toLowerCase()) ||
        (skill.description || "").toLowerCase().includes(search.toLowerCase());
      if (!matchesSearch) return false;
      if (filterMode === "enabled") return skill.enabledCount > 0;
      if (filterMode === "disabled") return skill.enabledCount === 0;
      return true;
    });
  }, [groupedSkills, search, filterMode]);

  const {
    isMultiSelect, setIsMultiSelect,
    selectedIds,
    toggleSelect,
    isAllSelected,
    anyDisabled,
    handleSelectAll,
    exitMultiSelect,
  } = useMultiSelect({
    items: groupedSkills,
    filtered,
    getKey: getSkillKey,
    isItemActive: isGroupEnabled,
  });

  const exportTargets = useMemo(() => {
    if (projectAgentTargets.length > 0) return projectAgentTargets;
    return [{ key: "claude_code", display_name: "Claude Code", enabled: true, installed: true, is_custom: false }];
  }, [projectAgentTargets]);

  const projectSkillDirNamesByAgent = useMemo(() => {
    const map: Record<string, string[]> = {};
    for (const skill of skills) {
      if (!map[skill.agent]) {
        map[skill.agent] = [];
      }
      map[skill.agent].push(skill.relative_path.toLowerCase());
    }
    return map;
  }, [skills]);

  useEffect(() => {
    let cancelled = false;
    const loadDefaultExportAgents = async () => {
      const savedValue = await api.getSettings(PROJECT_DEFAULT_EXPORT_AGENTS_KEY).catch(() => null);
      if (cancelled) return;
      setSelectedExportAgents(getDefaultExportAgents(exportTargets, savedValue));
    };
    loadDefaultExportAgents();
    return () => {
      cancelled = true;
    };
  }, [exportTargets]);

  const enabledCount = groupedSkills.filter((s) => s.enabledCount > 0).length;
  const defaultAgentKeys = useMemo(
    () => [...selectedExportAgents].sort(),
    [selectedExportAgents]
  );

  const handleOpenDetail = async (skill: ProjectSkillGroup) => {
    setDetailSkill(skill);
    setDocContent(null);
    setDocLoading(true);
    if (!project || !id) return;
    try {
      const doc = await api.getProjectSkillDocument(
        id,
        skill.primaryVariant.relative_path,
        skill.primaryVariant.agent
      );
      setDocContent(doc.content);
    } catch {
      setDocContent(null);
    } finally {
      setDocLoading(false);
    }
  };

  const handleUpdateCenter = async (skill: ProjectSkillGroup) => {
    if (!id) return;
    setUpdatingCenterSkill(getSkillKey(skill));
    try {
      await api.updateProjectSkillToCenter(id, skill.primaryVariant.relative_path, skill.primaryVariant.agent);
      toast.success(t("project.updateCenterSuccess", { name: skill.name }));
      await Promise.all([refreshManagedSkills(), refreshScenarios(), loadSkills()]);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setUpdatingCenterSkill(null);
    }
  };

  const handleUpdateProject = async (skill: ProjectSkillGroup) => {
    if (!id) return;
    setUpdatingProjectSkill(getSkillKey(skill));
    try {
      await Promise.all(
        skill.variants.map((variant) =>
          api.updateProjectSkillFromCenter(id, variant.relative_path, variant.agent)
        )
      );
      if (skill.status === "project_newer") {
        toast.success(t("project.resetFromCenterSuccess", { name: skill.name }));
      } else {
        toast.success(t("project.updateProjectSuccess", { name: skill.name }));
      }
      await Promise.all([loadSkills(), refreshProjects()]);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setUpdatingProjectSkill(null);
    }
  };

  const handleToggleSkill = async (skill: ProjectSkillGroup) => {
    if (!id) return;
    setTogglingSkill(getSkillKey(skill));
    try {
      const nextEnabled = !isGroupEnabled(skill);
      await Promise.all(
        skill.variants.map((variant) =>
          api.toggleProjectSkill(id, variant.relative_path, variant.agent, nextEnabled)
        )
      );
      if (nextEnabled) {
        toast.success(t("project.skillEnabled", { name: skill.name }));
      } else {
        toast.success(t("project.skillDisabled", { name: skill.name }));
      }
      await loadSkills();
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setTogglingSkill(null);
    }
  };

  const handleExportFromCenter = async (managedSkill: ManagedSkill) => {
    if (!id) return;
    if (selectedExportAgents.length === 0) {
      toast.error(t("project.selectTargetAgents"));
      return;
    }
    try {
      await api.exportSkillToProject(managedSkill.id, id, selectedExportAgents);
      toast.success(t("project.importFromCenterSuccess", {
        name: managedSkill.name,
        count: selectedExportAgents.length,
      }));
      setShowExportDialog(false);
      await Promise.all([loadSkills(), refreshProjects()]);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    }
  };

  const handleBatchExportFromCenter = async (skills: ManagedSkill[]) => {
    if (!id) return;
    if (selectedExportAgents.length === 0) {
      toast.error(t("project.selectTargetAgents"));
      return;
    }
    let imported = 0;
    let failed = 0;
    for (const skill of skills) {
      try {
        await api.exportSkillToProject(skill.id, id, selectedExportAgents);
        imported++;
      } catch {
        failed++;
        // continue with remaining
      }
    }
    if (imported > 0) {
      toast.success(t("project.batchImported", { count: imported }));
    }
    if (failed > 0) {
      toast.error(t("project.batchImportFailed", { count: failed }));
    }
    if (imported > 0) {
      setShowExportDialog(false);
    }
    await Promise.all([loadSkills(), refreshProjects()]);
  };

  const handleDeleteSkill = async () => {
    if (!id || !deleteTarget) return;
    try {
      await Promise.all(
        deleteTarget.variants.map((variant) =>
          api.deleteProjectSkill(id, variant.relative_path, variant.agent)
        )
      );
      toast.success(t("project.skillDeleted", { name: deleteTarget.name }));
      await Promise.all([loadSkills(), refreshProjects()]);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    }
  };

  const handleBatchDeleteProject = async () => {
    if (!id) return;
    const selectedSkills = groupedSkills.filter((s) => selectedIds.has(getSkillKey(s)));
    let deleted = 0;
    let failed = 0;
    for (const skill of selectedSkills) {
      try {
        await Promise.all(
          skill.variants.map((variant) =>
            api.deleteProjectSkill(id, variant.relative_path, variant.agent)
          )
        );
        deleted++;
      } catch {
        failed++;
        // continue deleting remaining
      }
    }
    if (deleted > 0) {
      toast.success(t("project.batchDeleted", { count: deleted }));
    }
    if (failed > 0) {
      toast.error(t("project.batchDeleteFailed", { count: failed }));
    }
    exitMultiSelect();
    setBatchDeleteConfirm(false);
    await Promise.all([loadSkills(), refreshProjects()]);
  };

  const handleBatchToggleProject = async () => {
    if (!id) return;
    const selectedSkillsList = groupedSkills.filter((s) => selectedIds.has(getSkillKey(s)));
    const enabling = anyDisabled;
    let count = 0;
    let failed = 0;
    for (const skill of selectedSkillsList) {
      try {
        if (enabling && !isGroupEnabled(skill)) {
          await Promise.all(
            skill.variants.map((variant) =>
              api.toggleProjectSkill(id, variant.relative_path, variant.agent, true)
            )
          );
          count++;
        } else if (!enabling && isGroupEnabled(skill)) {
          await Promise.all(
            skill.variants.map((variant) =>
              api.toggleProjectSkill(id, variant.relative_path, variant.agent, false)
            )
          );
          count++;
        }
      } catch {
        failed++;
        // continue with remaining
      }
    }
    if (count > 0) {
      toast.success(enabling
        ? t("project.batchEnabled", { count })
        : t("project.batchDisabled", { count }));
    }
    if (failed > 0) {
      toast.error(t("project.batchToggleFailed", { count: failed }));
    }
    await loadSkills();
  };

  if (!project) return null;

  return (
    <div className="app-page">
      <div className="app-page-header pr-2">
        <h1 className="app-page-title flex items-center gap-2.5">
          <FolderOpen className="w-5 h-5 text-accent" />
          {project.name}
          <span className="app-badge">{groupedSkills.length}</span>
        </h1>
        <p className="app-page-subtitle">
          {project.path}
          {groupedSkills.length > 0 && ` \u00B7 ${enabledCount} / ${groupedSkills.length} ${t("project.enabled")}`}
        </p>
        <p className="mt-1 text-[13px] text-muted">
          {project.workspace_type === "linked" ? t("project.linkedWorkspaceHint") : t("project.workspaceHint")}
        </p>
      </div>

      <div className="app-toolbar">
        <div className="flex flex-1 gap-3">
          <div className="relative w-full max-w-[280px]">
            <Search className="absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("project.searchPlaceholder")}
              className="app-input w-full pl-9 font-medium"
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
            />
          </div>
          <div className="app-segmented">
            {(["all", "enabled", "disabled"] as const).map((mode) => (
              <button
                key={mode}
                onClick={() => setFilterMode(mode)}
                className={cn(
                  "app-segmented-button",
                  filterMode === mode && "app-segmented-button-active"
                )}
              >
                {t(`project.filters.${mode}`)}
              </button>
            ))}
          </div>
        </div>

        <div className="app-segmented">
          <button
            onClick={() => setShowExportDialog(true)}
            className="inline-flex items-center gap-1 rounded-md px-3 py-2 text-[13px] font-medium text-muted transition-colors hover:bg-surface-hover hover:text-secondary"
          >
            <Download className="h-3.5 w-3.5" />
            {t("project.addSkill")}
          </button>
          <button
            onClick={loadSkills}
            className="ml-2 mr-2 inline-flex items-center gap-1 rounded-md border-l border-border-subtle pl-4 pr-3 py-2 text-[13px] font-medium text-muted transition-colors hover:bg-surface-hover hover:text-secondary"
          >
            <RefreshCw className={cn("h-3.5 w-3.5", loading && "animate-spin")} />
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
            title={isMultiSelect ? t("project.cancelSelect") : t("project.selectMode")}
          >
            <SquareCheck className="h-4 w-4" />
          </button>
        </div>
      </div>

      {isMultiSelect && (
        <MultiSelectToolbar
          selectedCount={selectedIds.size}
          isAllSelected={isAllSelected}
          anyDisabled={anyDisabled}
          showToggle={project.supports_skill_toggle}
          labels={{
            hint: t("project.selectHint"),
            selected: t("project.selectedCount", { count: selectedIds.size }),
            delete: t("project.deleteSelected", { count: selectedIds.size }),
            enable: t("project.batchEnable", { count: selectedIds.size }),
            disable: t("project.batchDisable", { count: selectedIds.size }),
            selectAll: t("project.selectAll"),
            deselectAll: t("project.deselectAll"),
            cancel: t("common.cancel"),
          }}
          onDelete={() => setBatchDeleteConfirm(true)}
          onToggle={handleBatchToggleProject}
          onSelectAll={handleSelectAll}
          onCancel={exitMultiSelect}
        />
      )}

      {loading ? (
        <div className="flex flex-1 flex-col items-center justify-center pb-20 text-center">
          <div className="text-[13px] text-muted">{t("common.loading")}</div>
        </div>
      ) : filtered.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center pb-20 text-center">
          <Layers className="mb-4 h-12 w-12 text-faint" />
          <h3 className="mb-1.5 text-[14px] font-semibold text-tertiary">
            {groupedSkills.length === 0 ? t("project.noSkills") : t("mySkills.noMatch")}
          </h3>
          <p className="text-[13px] text-muted">
            {groupedSkills.length === 0 ? t("project.noSkillsHint") : ""}
          </p>
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
          {filtered.map((skill) => {
            const skillKey = getSkillKey(skill);
            const isSelected = selectedIds.has(skillKey);
            const isUpdatingCenter = updatingCenterSkill === skillKey;
            const isUpdatingProject = updatingProjectSkill === skillKey;
            const isToggling = togglingSkill === skillKey;
            const canUpdateCenter =
              skill.status === "project_only" ||
              skill.status === "project_newer" ||
              skill.status === "diverged";
            const canUpdateProject =
              skill.status === "project_newer" ||
              skill.status === "center_newer" ||
              skill.status === "diverged";
            const statusMeta = getSyncStatusMeta(t, skill.status);
            const assignedAgents = getAssignedAgents(skill.variants);
            const hasCustomAssignment =
              defaultAgentKeys.length > 0 && !areAgentSetsEqual(assignedAgents, defaultAgentKeys);

            if (viewMode === "grid") {
              return (
                <div
                  key={skillKey}
                  className={cn(
                    "app-panel group relative flex h-full flex-col overflow-hidden transition-all hover:border-border hover:bg-surface-hover",
                    skill.enabledCount > 0 && "border-l-2 border-l-accent",
                    skill.enabledCount === 0 && "opacity-60",
                    isMultiSelect && "cursor-pointer",
                    isMultiSelect && isSelected && "ring-1 ring-accent border-accent/40"
                  )}
                  onClick={isMultiSelect ? () => toggleSelect(skillKey) : undefined}
                >
                  <div className="flex items-center gap-2.5 px-3.5 pt-3 pb-1.5">
                    {isMultiSelect && (
                      isSelected
                        ? <SquareCheck className="h-3.5 w-3.5 shrink-0 text-accent" />
                        : <Square className="h-3.5 w-3.5 shrink-0 text-faint" />
                    )}
                    <h3
                      className="flex-1 truncate text-[14px] font-semibold text-primary"
                      onClick={!isMultiSelect ? () => handleOpenDetail(skill) : undefined}
                      style={!isMultiSelect ? { cursor: "pointer" } : undefined}
                      title={skill.name}
                    >
                      {skill.name}
                    </h3>
                    {skill.files.length > 0 && (
                      <span className="flex items-center gap-1 text-[12px] text-faint shrink-0">
                        <FileText className="w-3 h-3" />
                        {skill.files.length}
                      </span>
                    )}
                  </div>

                  <div className="px-3.5 pb-3">
                    <p className="text-[13px] leading-[18px] text-muted truncate">
                      {skill.description || "\u2014"}
                    </p>
                  </div>

                  <div className="mt-auto flex items-center justify-between gap-2 border-t border-border-subtle px-3.5 py-2.5">
                    <div className="flex items-center gap-1.5">
                      <span className={cn("rounded-full px-2 py-0.5 text-[12px] font-medium", statusMeta.className)}>
                        {statusMeta.label}
                      </span>
                      {hasCustomAssignment && (
                        <span className="rounded-full bg-surface-hover px-2 py-0.5 text-[12px] font-medium text-muted">
                          {t("project.customAssignment", { assigned: assignedAgents.length, total: defaultAgentKeys.length })}
                        </span>
                      )}
                      {skill.enabledCount === 0 && (
                        <span className="rounded-full bg-red-500/10 px-2 py-0.5 text-[12px] font-medium text-red-600 dark:text-red-300">
                          {t("project.disabled")}
                        </span>
                      )}
                    </div>
                    {!isMultiSelect && (
                      <div className="flex items-center gap-1.5 shrink-0">
                        {canUpdateCenter && (
                          <button
                            onClick={() => handleUpdateCenter(skill)}
                            disabled={isUpdatingCenter || isUpdatingProject}
                            className="rounded px-2 py-1 text-[13px] font-medium text-muted transition-colors outline-none hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
                            title={t("project.updateCenter")}
                          >
                            {isUpdatingCenter ? (
                              <Loader2 className="h-3.5 w-3.5 animate-spin" />
                            ) : (
                              <Upload className="h-3.5 w-3.5" />
                            )}
                          </button>
                        )}
                        {canUpdateProject && (
                          <button
                            onClick={() => handleUpdateProject(skill)}
                            disabled={isUpdatingCenter || isUpdatingProject}
                            className="rounded px-2 py-1 text-[13px] font-medium text-muted transition-colors outline-none hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
                            title={
                              skill.status === "project_newer"
                                ? t("project.resetFromCenter")
                                : t("project.updateProject")
                            }
                          >
                            {isUpdatingProject ? (
                              <Loader2 className="h-3.5 w-3.5 animate-spin" />
                            ) : skill.status === "project_newer" ? (
                              <RotateCcw className="h-3.5 w-3.5" />
                            ) : (
                              <Download className="h-3.5 w-3.5" />
                            )}
                          </button>
                        )}
                        {project.supports_skill_toggle ? (
                          <button
                            onClick={() => handleToggleSkill(skill)}
                            disabled={isToggling}
                            className={cn(
                              "rounded px-2 py-1 text-[13px] font-medium transition-colors outline-none",
                              skill.enabledCount > 0
                                ? "text-emerald-600 dark:text-emerald-400 hover:bg-emerald-500/10"
                                : "text-muted hover:bg-surface-hover hover:text-secondary"
                            )}
                          >
                            {isToggling ? (
                              <Loader2 className="h-3.5 w-3.5 animate-spin" />
                            ) : isGroupEnabled(skill) ? (
                              t("project.enabled")
                            ) : (
                              t("project.enableSkill")
                            )}
                          </button>
                        ) : null}
                        <button
                          onClick={() => setDeleteTarget(skill)}
                          className="rounded px-2 py-1 text-muted transition-colors outline-none opacity-0 group-hover:opacity-100 hover:bg-red-500/10 hover:text-red-500"
                          title={t("project.deleteSkill")}
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </button>
                      </div>
                    )}
                  </div>
                </div>
              );
            }

            // List view
            return (
              <div
                key={skillKey}
                className={cn(
                  "app-panel group flex items-center gap-3.5 rounded-xl border-transparent px-3.5 py-3 transition-all hover:border-border hover:bg-surface-hover",
                  skill.enabledCount > 0 && "border-l-2 border-l-accent",
                  skill.enabledCount === 0 && "opacity-60",
                  isMultiSelect && "cursor-pointer",
                  isMultiSelect && isSelected && "ring-1 ring-accent border-accent/40"
                )}
                onClick={isMultiSelect ? () => toggleSelect(skillKey) : undefined}
              >
                {isMultiSelect && (
                  isSelected
                    ? <SquareCheck className="h-3.5 w-3.5 shrink-0 text-accent" />
                    : <Square className="h-3.5 w-3.5 shrink-0 text-faint" />
                )}
                <h3
                  className="w-[180px] shrink-0 truncate text-[14px] font-semibold text-secondary"
                  onClick={!isMultiSelect ? () => handleOpenDetail(skill) : undefined}
                  style={!isMultiSelect ? { cursor: "pointer" } : undefined}
                  title={skill.name}
                >
                  {skill.name}
                </h3>

                <p className="min-w-0 flex-1 truncate text-[13px] text-muted">
                  {skill.description || "\u2014"}
                </p>

                <div className="flex shrink-0 items-center gap-2.5">
                  <span className={cn("rounded-full px-2 py-0.5 text-[12px] font-medium", statusMeta.className)}>
                    {statusMeta.label}
                  </span>
                  {hasCustomAssignment && (
                    <span className="rounded-full bg-surface-hover px-2 py-0.5 text-[12px] font-medium text-muted">
                      {t("project.customAssignment", { assigned: assignedAgents.length, total: defaultAgentKeys.length })}
                    </span>
                  )}
                  {skill.enabledCount === 0 && (
                    <span className="rounded-full bg-red-500/10 px-2 py-0.5 text-[12px] font-medium text-red-600 dark:text-red-300">
                      {t("project.disabled")}
                    </span>
                  )}
                  {skill.files.length > 0 && (
                    <span className="flex items-center gap-1 text-[12px] text-faint">
                      <FileText className="w-3 h-3" />
                      {skill.files.length}
                    </span>
                  )}
                </div>

                {!isMultiSelect && (
                  <div className="flex shrink-0 items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100">
                    {canUpdateCenter && (
                      <button
                        onClick={() => handleUpdateCenter(skill)}
                        disabled={isUpdatingCenter || isUpdatingProject}
                        className="rounded p-0.5 text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
                        title={t("project.updateCenter")}
                      >
                        {isUpdatingCenter ? (
                          <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        ) : (
                          <Upload className="h-3.5 w-3.5" />
                        )}
                      </button>
                    )}
                    {canUpdateProject && (
                      <button
                        onClick={() => handleUpdateProject(skill)}
                        disabled={isUpdatingCenter || isUpdatingProject}
                        className="rounded p-0.5 text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
                        title={
                          skill.status === "project_newer"
                            ? t("project.resetFromCenter")
                            : t("project.updateProject")
                        }
                      >
                        {isUpdatingProject ? (
                          <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        ) : skill.status === "project_newer" ? (
                          <RotateCcw className="h-3.5 w-3.5" />
                        ) : (
                          <Download className="h-3.5 w-3.5" />
                        )}
                      </button>
                    )}
                    {project.supports_skill_toggle ? (
                      <button
                        onClick={() => handleToggleSkill(skill)}
                        disabled={isToggling}
                        className={cn(
                          "rounded px-2 py-0.5 text-[13px] font-medium transition-colors outline-none",
                          skill.enabledCount > 0
                            ? "text-emerald-600 dark:text-emerald-400 hover:bg-emerald-500/10"
                            : "text-muted hover:bg-surface-hover hover:text-secondary"
                        )}
                      >
                        {isToggling ? (
                          <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        ) : isGroupEnabled(skill) ? (
                          t("project.enabled")
                        ) : (
                          t("project.enableSkill")
                        )}
                      </button>
                    ) : null}
                    <button
                      onClick={() => setDeleteTarget(skill)}
                      className="rounded p-0.5 text-muted transition-colors hover:bg-red-500/10 hover:text-red-500"
                      title={t("project.deleteSkill")}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </button>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      {/* Skill Document Detail Panel */}
      {detailSkill && project && (
        <ProjectSkillDetailPanel
          skill={detailSkill}
          docContent={docContent}
          docLoading={docLoading}
          onClose={() => setDetailSkill(null)}
        />
      )}

      {/* Delete Confirm Dialog */}
      <ConfirmDialog
        open={!!deleteTarget}
        title={t("project.deleteSkill")}
        message={t("project.deleteSkillConfirm", { name: deleteTarget?.name })}
        tone="danger"
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleDeleteSkill}
      />

      {/* Batch Delete Confirm Dialog */}
      <ConfirmDialog
        open={batchDeleteConfirm}
        title={t("project.deleteSkill")}
        message={t("project.batchDeleteConfirm", { count: selectedIds.size })}
        tone="danger"
        onClose={() => setBatchDeleteConfirm(false)}
        onConfirm={handleBatchDeleteProject}
      />

      {/* Export from Center Dialog */}
      {showExportDialog && id && (
        <ExportFromCenterDialog
          exportTargets={exportTargets}
          managedSkills={managedSkills}
          selectedAgents={selectedExportAgents}
          onSelectedAgentsChange={setSelectedExportAgents}
          projectSkillDirNamesByAgent={projectSkillDirNamesByAgent}
          onExport={handleExportFromCenter}
          onBatchExport={handleBatchExportFromCenter}
          onClose={() => setShowExportDialog(false)}
        />
      )}
    </div>
  );
}

function ProjectSkillDetailPanel({
  skill,
  docContent,
  docLoading,
  onClose,
}: {
  skill: ProjectSkillGroup;
  docContent: string | null;
  docLoading: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation();

  return createPortal(
    <div className="fixed inset-y-0 right-0 left-[220px] z-50 flex">
      <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" onClick={onClose} />
      <div className="relative flex h-full min-h-0 w-full flex-col border-l border-border-subtle bg-bg-secondary shadow-2xl animate-in slide-in-from-right duration-200">
        <div className="border-b border-border-subtle px-6 pt-5 pb-4">
          <div className="flex items-start justify-between mb-3">
            <h2 className="text-lg font-semibold text-primary truncate mr-3">{skill.name}</h2>
            <button
              onClick={onClose}
              className="text-muted hover:text-secondary p-1.5 rounded-[4px] hover:bg-surface-hover transition-colors outline-none shrink-0"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
          {skill.description && (
            <p className="text-[13.5px] leading-relaxed text-secondary line-clamp-3">{skill.description}</p>
          )}
          <div className="mt-3 flex flex-wrap items-center gap-2 text-[12.5px] text-muted">
            {skill.variants.map((variant) => (
              <span
                key={variant.agent}
                className="rounded-full bg-surface-hover px-2 py-0.5 text-[12px] font-medium text-muted shrink-0"
              >
                {variant.agent_display_name}
              </span>
            ))}
          </div>
          <div className="flex items-center gap-4 mt-3 text-[12.5px] text-muted">
            <div className="flex items-center gap-1.5 min-w-0">
              <FolderOpen className="w-3.5 h-3.5 shrink-0" />
              <span className="font-mono truncate">{skill.primaryVariant.path}</span>
            </div>
            {skill.files.length > 0 && (
              <div className="flex items-center gap-1.5 shrink-0">
                <FileText className="w-3.5 h-3.5" />
                {skill.files.join(", ")}
              </div>
            )}
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 scrollbar-hide">
          {docLoading ? (
            <div className="text-[13px] text-muted text-center mt-12">{t("common.loading")}</div>
          ) : docContent ? (
            <SkillMarkdown content={docContent} />
          ) : (
            <div className="text-[13px] text-muted text-center mt-12">{t("common.documentMissing")}</div>
          )}
        </div>
      </div>
    </div>,
    document.body
  );
}

function ExportFromCenterDialog({
  exportTargets,
  managedSkills,
  selectedAgents,
  onSelectedAgentsChange,
  projectSkillDirNamesByAgent,
  onExport,
  onBatchExport,
  onClose,
}: {
  exportTargets: ProjectAgentTarget[];
  managedSkills: ManagedSkill[];
  selectedAgents: string[];
  onSelectedAgentsChange: (agents: string[]) => void;
  projectSkillDirNamesByAgent: Record<string, string[]>;
  onExport: (skill: ManagedSkill) => Promise<void>;
  onBatchExport: (skills: ManagedSkill[]) => Promise<void>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [tagFilters, setTagFilters] = useState<Set<string>>(new Set());
  const [exporting, setExporting] = useState<string | null>(null);
  const [batchExporting, setBatchExporting] = useState(false);
  const [dirNameMap, setDirNameMap] = useState<Record<string, string>>({});
  const [dirNameMapError, setDirNameMapError] = useState(false);
  const [agentPickerOpen, setAgentPickerOpen] = useState(false);
  const [showInactiveAgents, setShowInactiveAgents] = useState(false);

  const toggleAgent = useCallback((agentKey: string) => {
    onSelectedAgentsChange(
      selectedAgents.includes(agentKey)
        ? selectedAgents.filter((key) => key !== agentKey)
        : [...selectedAgents, agentKey]
    );
  }, [onSelectedAgentsChange, selectedAgents]);

  const handleSaveDefaults = useCallback(async () => {
    await api.setSettings(PROJECT_DEFAULT_EXPORT_AGENTS_KEY, JSON.stringify(selectedAgents));
    toast.success(t("project.defaultAgentsSaved"));
  }, [selectedAgents, t]);

  useEffect(() => {
    let cancelled = false;
    const loadDirNames = async () => {
      const names = managedSkills.map((s) => s.name);
      if (names.length === 0) {
        if (!cancelled) {
          setDirNameMap({});
          setDirNameMapError(false);
        }
        return;
      }
      try {
        const slugified = await api.slugifySkillNames(names);
        if (cancelled) return;
        const map: Record<string, string> = {};
        managedSkills.forEach((s, i) => {
          map[s.id] = slugified[i];
        });
        setDirNameMap(map);
        setDirNameMapError(false);
      } catch {
        if (cancelled) return;
        setDirNameMap({});
        setDirNameMapError(true);
      }
    };
    loadDirNames();
    return () => {
      cancelled = true;
    };
  }, [managedSkills]);

  const allTags = useMemo(() => {
    const tags = new Set<string>();
    for (const skill of managedSkills) {
      for (const tag of skill.tags) {
        if (tag.trim()) tags.add(tag);
      }
    }
    return Array.from(tags).sort((a, b) => a.localeCompare(b));
  }, [managedSkills]);

  const activeTargets = useMemo(
    () => exportTargets.filter((target) => target.installed && target.enabled),
    [exportTargets]
  );

  const inactiveTargets = useMemo(
    () => exportTargets.filter((target) => !target.installed || !target.enabled),
    [exportTargets]
  );

  const selectedTargetLabels = useMemo(
    () => exportTargets
      .filter((target) => selectedAgents.includes(target.key))
      .map((target) => target.display_name),
    [exportTargets, selectedAgents]
  );

  const filtered = useMemo(() => managedSkills.filter((skill) => {
    const matchesSearch =
      skill.name.toLowerCase().includes(search.toLowerCase()) ||
      (skill.description || "").toLowerCase().includes(search.toLowerCase());
    if (!matchesSearch) return false;
    if (tagFilters.size === 0) return true;
    return skill.tags.some((tag) => tagFilters.has(tag));
  }), [managedSkills, search, tagFilters]);

  const isAlreadyExists = useCallback((skill: ManagedSkill) => {
    const exportDirName = dirNameMap[skill.id];
    if (dirNameMapError || selectedAgents.length === 0) return true;
    if (!exportDirName) return false;
    return selectedAgents.some((agent) =>
      (projectSkillDirNamesByAgent[agent] ?? []).includes(exportDirName)
    );
  }, [dirNameMap, dirNameMapError, projectSkillDirNamesByAgent, selectedAgents]);

  const selectableFiltered = useMemo(
    () => filtered.filter((s) => !isAlreadyExists(s)),
    [filtered, isAlreadyExists]
  );

  const {
    isMultiSelect, setIsMultiSelect,
    selectedIds,
    toggleSelect,
    isAllSelected,
    handleSelectAll,
    exitMultiSelect,
  } = useMultiSelect({
    items: managedSkills,
    filtered: selectableFiltered,
    getKey: (s) => s.id,
    isItemActive: () => true,
  });

  const selectedSelectable = useMemo(
    () => selectableFiltered.filter((s) => selectedIds.has(s.id)),
    [selectableFiltered, selectedIds]
  );

  const handleExport = async (skill: ManagedSkill) => {
    setExporting(skill.id);
    try {
      await onExport(skill);
    } finally {
      setExporting(null);
    }
  };

  const handleBatchExport = async () => {
    if (selectedSelectable.length === 0) return;
    setBatchExporting(true);
    try {
      await onBatchExport(selectedSelectable);
    } finally {
      setBatchExporting(false);
    }
  };

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" onClick={onClose} />
      <div className="relative w-full max-w-lg rounded-xl border border-border-subtle bg-bg-secondary shadow-2xl">
        <div className="flex items-center justify-between border-b border-border-subtle px-5 py-4">
          <h2 className="text-[14px] font-semibold text-primary">
            {t("project.selectSkillToExport")}
          </h2>
          <button
            onClick={onClose}
            className="text-muted hover:text-secondary p-1.5 rounded-[4px] hover:bg-surface-hover transition-colors outline-none"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="px-5 py-3 border-b border-border-subtle">
          <div className="mb-3 flex items-center gap-2">
            <label className="shrink-0 text-[12px] font-medium text-muted">
              {t("project.targetAgents")}
            </label>
            <button
              onClick={handleSaveDefaults}
              disabled={selectedAgents.length === 0}
              className="ml-auto rounded-md border border-border-subtle px-2.5 py-1 text-[12px] font-medium text-muted transition-colors hover:border-border hover:text-secondary disabled:cursor-not-allowed disabled:opacity-50"
            >
              {t("project.saveDefaultAgents")}
            </button>
          </div>
          <div className="mb-3">
            <button
              onClick={() => setAgentPickerOpen((prev) => !prev)}
              className="flex w-full items-center gap-3 rounded-lg border border-border-subtle bg-background px-3 py-2.5 text-left transition-colors hover:border-border"
            >
              <div className="min-w-0 flex-1">
                <div className="text-[13px] font-medium text-secondary">
                  {selectedAgents.length > 0
                    ? t("project.selectedAgentCount", { count: selectedAgents.length })
                    : t("project.selectTargetAgents")}
                </div>
                <div className="truncate text-[12px] text-muted">
                  {selectedTargetLabels.length > 0
                    ? selectedTargetLabels.join(", ")
                    : t("project.agentPickerHint")}
                </div>
              </div>
              {agentPickerOpen ? (
                <ChevronDown className="h-4 w-4 shrink-0 text-muted" />
              ) : (
                <ChevronRight className="h-4 w-4 shrink-0 text-muted" />
              )}
            </button>

            {agentPickerOpen && (
              <div className="mt-2 rounded-lg border border-border-subtle bg-background">
                <div className="max-h-[220px] overflow-y-auto px-3 py-3 scrollbar-hide">
                  <div className="mb-2 text-[11px] font-medium uppercase tracking-[0.08em] text-muted">
                    {t("project.enabledAgents")}
                  </div>
                  <div className="flex flex-wrap gap-2">
                    {activeTargets.map((target) => {
                      const active = selectedAgents.includes(target.key);
                      return (
                        <button
                          key={target.key}
                          onClick={() => toggleAgent(target.key)}
                          className={cn(
                            "inline-flex items-center gap-1.5 rounded-full border px-3 py-1.5 text-[12px] font-medium transition-colors",
                            active
                              ? "border-accent-border bg-accent-bg text-accent-light"
                              : "border-border-subtle text-muted hover:border-border hover:text-secondary"
                          )}
                        >
                          {active ? <SquareCheck className="h-3.5 w-3.5" /> : <Square className="h-3.5 w-3.5" />}
                          {target.display_name}
                        </button>
                      );
                    })}
                  </div>

                  {inactiveTargets.length > 0 && (
                    <div className="mt-3 border-t border-border-subtle pt-3">
                      <button
                        onClick={() => setShowInactiveAgents((prev) => !prev)}
                        className="flex w-full items-center justify-between text-left text-[12px] font-medium text-muted transition-colors hover:text-secondary"
                      >
                        <span>{t("project.moreAgents", { count: inactiveTargets.length })}</span>
                        {showInactiveAgents ? (
                          <ChevronDown className="h-4 w-4" />
                        ) : (
                          <ChevronRight className="h-4 w-4" />
                        )}
                      </button>
                      {showInactiveAgents && (
                        <div className="mt-2 flex flex-wrap gap-2">
                          {inactiveTargets.map((target) => {
                            const active = selectedAgents.includes(target.key);
                            return (
                              <button
                                key={target.key}
                                onClick={() => toggleAgent(target.key)}
                                className={cn(
                                  "inline-flex items-center gap-1.5 rounded-full border px-3 py-1.5 text-[12px] font-medium transition-colors",
                                  active
                                    ? "border-accent-border bg-accent-bg text-accent-light"
                                    : "border-border-subtle text-muted hover:border-border hover:text-secondary"
                                )}
                              >
                                {active ? <SquareCheck className="h-3.5 w-3.5" /> : <Square className="h-3.5 w-3.5" />}
                                {target.display_name}
                              </button>
                            );
                          })}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>

          <div className="flex items-center gap-2">
            <div className="relative flex-1">
              <Search className="absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
              <input
                type="text"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder={t("project.searchCenterSkills")}
                className="app-input w-full pl-9 font-medium"
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
                autoFocus
              />
            </div>
            {selectedSelectable.length > 0 && isMultiSelect && (
              <button
                onClick={handleBatchExport}
                disabled={batchExporting}
                className="shrink-0 inline-flex items-center gap-1.5 rounded-md bg-accent px-3 py-1.5 text-[13px] font-medium text-white hover:bg-accent/90 disabled:opacity-50 transition-colors"
              >
                {batchExporting
                  ? <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  : t("project.updateSelected", { count: selectedSelectable.length })}
              </button>
            )}
            <button
              onClick={() => isMultiSelect ? exitMultiSelect() : setIsMultiSelect(true)}
              className={cn(
                "shrink-0 rounded-md p-2 transition-colors outline-none",
                isMultiSelect ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary hover:bg-surface-hover"
              )}
              title={isMultiSelect ? t("project.cancelSelect") : t("project.selectMode")}
            >
              <SquareCheck className="h-4 w-4" />
            </button>
          </div>
          {allTags.length > 0 && (
            <div className="mt-2 flex flex-wrap items-center gap-1.5">
              <span className="text-[12px] text-muted">{t("mySkills.tags.filter")}</span>
              <button
                onClick={() => setTagFilters(new Set())}
                className={cn(
                  "rounded-full border px-2 py-0.5 text-[12px] transition-colors",
                  tagFilters.size === 0
                    ? "border-accent-border bg-accent-bg text-accent-light"
                    : "border-border-subtle text-muted hover:border-border hover:text-secondary"
                )}
              >
                {t("mySkills.tags.allTags")}
              </button>
              {allTags.map((tag) => {
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
                      "rounded-full border px-2 py-0.5 text-[12px] transition-colors",
                      active
                        ? "border-accent-border bg-accent-bg text-accent-light"
                        : "border-border-subtle text-muted hover:border-border hover:text-secondary"
                    )}
                  >
                    {tag}
                  </button>
                );
              })}
              {isMultiSelect && selectableFiltered.length > 0 && (
                <button
                  onClick={handleSelectAll}
                  className="ml-auto text-[12px] text-accent hover:underline"
                >
                  {isAllSelected ? t("project.deselectAll") : t("project.selectAll")}
                </button>
              )}
            </div>
          )}
        </div>

        <div className="max-h-[400px] overflow-y-auto scrollbar-hide">
          {filtered.length === 0 ? (
            <div className="py-12 text-center text-[13px] text-muted">
              {t("project.noSkillsToExport")}
            </div>
          ) : (
            <div className="divide-y divide-border-subtle">
              {filtered.map((skill) => {
                const alreadyExists = isAlreadyExists(skill);
                const isSelected = selectedIds.has(skill.id);
                const selectable = isMultiSelect && !alreadyExists;
                return (
                  <div
                    key={skill.id}
                    className={cn(
                      "flex items-center gap-3 px-5 py-3 transition-colors",
                      selectable ? "cursor-pointer hover:bg-surface-hover" : "hover:bg-surface-hover",
                      selectable && isSelected && "bg-accent/5"
                    )}
                    onClick={selectable ? () => toggleSelect(skill.id) : undefined}
                  >
                    {isMultiSelect && !alreadyExists && (
                      isSelected
                        ? <SquareCheck className="h-3.5 w-3.5 shrink-0 text-accent" />
                        : <Square className="h-3.5 w-3.5 shrink-0 text-faint" />
                    )}
                    <div className="flex-1 min-w-0">
                      <div className="text-[13px] font-medium text-primary truncate">
                        {skill.name}
                      </div>
                      {skill.description && (
                        <div className="text-[12px] text-muted truncate mt-0.5">
                          {skill.description}
                        </div>
                      )}
                    </div>
                    {alreadyExists ? (
                      <span className="rounded-full bg-surface-hover px-2 py-0.5 text-[12px] font-medium text-muted shrink-0">
                        {t("project.alreadyExists")}
                      </span>
                    ) : !isMultiSelect && (
                      <button
                        onClick={() => handleExport(skill)}
                        disabled={exporting === skill.id}
                        className="shrink-0 rounded px-3 py-1 text-[13px] font-medium text-accent-light transition-colors hover:bg-accent-bg disabled:opacity-50 outline-none"
                      >
                        {exporting === skill.id ? (
                          <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        ) : (
                          t("project.import")
                        )}
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>,
    document.body
  );
}
