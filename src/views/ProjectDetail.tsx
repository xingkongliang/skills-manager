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
import type { ProjectSkill, ManagedSkill } from "../lib/tauri";
import { getErrorMessage } from "../lib/error";

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

export function ProjectDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { t } = useTranslation();
  const { projects, managedSkills, refreshManagedSkills, refreshScenarios, refreshProjects } = useApp();
  const [skills, setSkills] = useState<ProjectSkill[]>([]);
  const [loading, setLoading] = useState(true);
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [filterMode, setFilterMode] = useState<"all" | "enabled" | "disabled">("all");
  const [search, setSearch] = useState("");
  const [detailSkill, setDetailSkill] = useState<ProjectSkill | null>(null);
  const [docContent, setDocContent] = useState<string | null>(null);
  const [docLoading, setDocLoading] = useState(false);
  const [updatingCenterSkill, setUpdatingCenterSkill] = useState<string | null>(null);
  const [updatingProjectSkill, setUpdatingProjectSkill] = useState<string | null>(null);
  const [togglingSkill, setTogglingSkill] = useState<string | null>(null);
  const [showExportDialog, setShowExportDialog] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<ProjectSkill | null>(null);
  const [batchDeleteConfirm, setBatchDeleteConfirm] = useState(false);

  const project = projects.find((p) => p.id === id);

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
    if (!project && !loading) {
      navigate("/");
    }
  }, [project, loading, navigate]);

  const filtered = useMemo(() => {
    return skills.filter((skill) => {
      const matchesSearch =
        skill.name.toLowerCase().includes(search.toLowerCase()) ||
        (skill.description || "").toLowerCase().includes(search.toLowerCase());
      if (!matchesSearch) return false;
      if (filterMode === "enabled") return skill.enabled;
      if (filterMode === "disabled") return !skill.enabled;
      return true;
    });
  }, [skills, search, filterMode]);

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
    getKey: (s) => s.dir_name,
    isItemActive: (s) => s.enabled,
  });

  const enabledCount = skills.filter((s) => s.enabled).length;

  const handleOpenDetail = async (skill: ProjectSkill) => {
    setDetailSkill(skill);
    setDocContent(null);
    setDocLoading(true);
    if (!project) return;
    try {
      const doc = await api.getProjectSkillDocument(project.path, skill.dir_name);
      setDocContent(doc.content);
    } catch {
      setDocContent(null);
    } finally {
      setDocLoading(false);
    }
  };

  const handleUpdateCenter = async (skill: ProjectSkill) => {
    if (!id) return;
    setUpdatingCenterSkill(skill.dir_name);
    try {
      await api.updateProjectSkillToCenter(id, skill.dir_name);
      toast.success(t("project.updateCenterSuccess", { name: skill.name }));
      await Promise.all([refreshManagedSkills(), refreshScenarios(), loadSkills()]);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setUpdatingCenterSkill(null);
    }
  };

  const handleUpdateProject = async (skill: ProjectSkill) => {
    if (!id) return;
    setUpdatingProjectSkill(skill.dir_name);
    try {
      await api.updateProjectSkillFromCenter(id, skill.dir_name);
      if (skill.sync_status === "project_newer") {
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

  const handleToggleSkill = async (skill: ProjectSkill) => {
    if (!id) return;
    setTogglingSkill(skill.dir_name);
    try {
      await api.toggleProjectSkill(id, skill.dir_name, !skill.enabled);
      if (skill.enabled) {
        toast.success(t("project.skillDisabled", { name: skill.name }));
      } else {
        toast.success(t("project.skillEnabled", { name: skill.name }));
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
    try {
      await api.exportSkillToProject(managedSkill.id, id);
      toast.success(t("project.importFromCenterSuccess", { name: managedSkill.name }));
      setShowExportDialog(false);
      await Promise.all([loadSkills(), refreshProjects()]);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    }
  };

  const handleBatchExportFromCenter = async (skills: ManagedSkill[]) => {
    if (!id) return;
    let imported = 0;
    let failed = 0;
    for (const skill of skills) {
      try {
        await api.exportSkillToProject(skill.id, id);
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
      await api.deleteProjectSkill(id, deleteTarget.dir_name);
      toast.success(t("project.skillDeleted", { name: deleteTarget.name }));
      await Promise.all([loadSkills(), refreshProjects()]);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    }
  };

  const handleBatchDeleteProject = async () => {
    if (!id) return;
    const dirNames = Array.from(selectedIds);
    let deleted = 0;
    let failed = 0;
    for (const dirName of dirNames) {
      try {
        await api.deleteProjectSkill(id, dirName);
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
    const selectedSkillsList = skills.filter((s) => selectedIds.has(s.dir_name));
    const enabling = anyDisabled;
    let count = 0;
    let failed = 0;
    for (const skill of selectedSkillsList) {
      try {
        if (enabling && !skill.enabled) {
          await api.toggleProjectSkill(id, skill.dir_name, true);
          count++;
        } else if (!enabling && skill.enabled) {
          await api.toggleProjectSkill(id, skill.dir_name, false);
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
          <span className="app-badge">{skills.length}</span>
        </h1>
        <p className="app-page-subtitle">
          {project.path}
          {skills.length > 0 && ` \u00B7 ${enabledCount} / ${skills.length} ${t("project.enabled")}`}
        </p>
        <p className="mt-1 text-[13px] text-muted">
          {t("project.workspaceHint")}
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
            {t("project.updateProject")}
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
          showToggle={true}
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
            {skills.length === 0 ? t("project.noSkills") : t("mySkills.noMatch")}
          </h3>
          <p className="text-[13px] text-muted">
            {skills.length === 0 ? t("project.noSkillsHint") : ""}
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
            const isSelected = selectedIds.has(skill.dir_name);
            const isUpdatingCenter = updatingCenterSkill === skill.dir_name;
            const isUpdatingProject = updatingProjectSkill === skill.dir_name;
            const isToggling = togglingSkill === skill.dir_name;
            const canUpdateCenter =
              skill.sync_status === "project_only" ||
              skill.sync_status === "project_newer" ||
              skill.sync_status === "diverged";
            const canUpdateProject =
              skill.sync_status === "project_newer" ||
              skill.sync_status === "center_newer" ||
              skill.sync_status === "diverged";
            const statusMeta = getSyncStatusMeta(t, skill.sync_status);

            if (viewMode === "grid") {
              return (
                <div
                  key={skill.dir_name}
                  className={cn(
                    "app-panel group relative flex flex-col overflow-hidden transition-all hover:border-border hover:bg-surface-hover",
                    skill.enabled && "border-l-2 border-l-accent",
                    !skill.enabled && "opacity-60",
                    isMultiSelect && "cursor-pointer",
                    isMultiSelect && isSelected && "ring-1 ring-accent border-accent/40"
                  )}
                  onClick={isMultiSelect ? () => toggleSelect(skill.dir_name) : undefined}
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
                      {!skill.enabled && (
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
                              skill.sync_status === "project_newer"
                                ? t("project.resetFromCenter")
                                : t("project.updateProject")
                            }
                          >
                            {isUpdatingProject ? (
                              <Loader2 className="h-3.5 w-3.5 animate-spin" />
                            ) : skill.sync_status === "project_newer" ? (
                              <RotateCcw className="h-3.5 w-3.5" />
                            ) : (
                              <Download className="h-3.5 w-3.5" />
                            )}
                          </button>
                        )}
                        <button
                          onClick={() => handleToggleSkill(skill)}
                          disabled={isToggling}
                          className={cn(
                            "rounded px-2 py-1 text-[13px] font-medium transition-colors outline-none",
                            skill.enabled
                              ? "text-emerald-600 dark:text-emerald-400 hover:bg-emerald-500/10"
                              : "text-muted hover:bg-surface-hover hover:text-secondary"
                          )}
                        >
                          {isToggling ? (
                            <Loader2 className="h-3.5 w-3.5 animate-spin" />
                          ) : skill.enabled ? (
                            t("project.enabled")
                          ) : (
                            t("project.enableSkill")
                          )}
                        </button>
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
                key={skill.dir_name}
                className={cn(
                  "app-panel group flex items-center gap-3.5 rounded-xl border-transparent px-3.5 py-3 transition-all hover:border-border hover:bg-surface-hover",
                  skill.enabled && "border-l-2 border-l-accent",
                  !skill.enabled && "opacity-60",
                  isMultiSelect && "cursor-pointer",
                  isMultiSelect && isSelected && "ring-1 ring-accent border-accent/40"
                )}
                onClick={isMultiSelect ? () => toggleSelect(skill.dir_name) : undefined}
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
                  {!skill.enabled && (
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
                          skill.sync_status === "project_newer"
                            ? t("project.resetFromCenter")
                            : t("project.updateProject")
                        }
                      >
                        {isUpdatingProject ? (
                          <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        ) : skill.sync_status === "project_newer" ? (
                          <RotateCcw className="h-3.5 w-3.5" />
                        ) : (
                          <Download className="h-3.5 w-3.5" />
                        )}
                      </button>
                    )}
                    <button
                      onClick={() => handleToggleSkill(skill)}
                      disabled={isToggling}
                      className={cn(
                        "rounded px-2 py-0.5 text-[13px] font-medium transition-colors outline-none",
                        skill.enabled
                          ? "text-emerald-600 dark:text-emerald-400 hover:bg-emerald-500/10"
                          : "text-muted hover:bg-surface-hover hover:text-secondary"
                      )}
                    >
                      {isToggling ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : skill.enabled ? (
                        t("project.enabled")
                      ) : (
                        t("project.enableSkill")
                      )}
                    </button>
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
          managedSkills={managedSkills}
          projectSkillDirNames={skills.map((s) => s.dir_name.toLowerCase())}
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
  skill: ProjectSkill;
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
          <div className="flex items-center gap-4 mt-3 text-[12.5px] text-muted">
            <div className="flex items-center gap-1.5 min-w-0">
              <FolderOpen className="w-3.5 h-3.5 shrink-0" />
              <span className="font-mono truncate">{skill.path}</span>
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
  managedSkills,
  projectSkillDirNames,
  onExport,
  onBatchExport,
  onClose,
}: {
  managedSkills: ManagedSkill[];
  projectSkillDirNames: string[];
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
    return dirNameMapError ? true : (exportDirName ? projectSkillDirNames.includes(exportDirName) : false);
  }, [dirNameMap, dirNameMapError, projectSkillDirNames]);

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
