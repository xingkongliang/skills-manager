import { useState, useEffect, useRef, useMemo } from "react";
import { DragDropContext, Droppable, Draggable, type DropResult } from "@hello-pangea/dnd";
import { Link, useLocation, useNavigate } from "react-router-dom";
import {
  LayoutDashboard,
  Layers,
  Globe,
  Download,
  CloudUpload,
  Settings,
  Plus,
  Pencil,
  Trash2,
  FolderOpen,
  GripVertical,
  Link2,
  ChevronDown,
  ChevronRight,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import { CreatePresetDialog } from "./CreatePresetDialog";
import { RenamePresetDialog } from "./RenamePresetDialog";
import { AddProjectDialog } from "./AddProjectDialog";
import { ConfirmDialog } from "./ConfirmDialog";
import { AgentIcon } from "./AgentIcon";
import * as api from "../lib/tauri";
import type { SyncHealth, ToolCategory, ToolInfo } from "../lib/tauri";
import { getPresetIconOption } from "../lib/presetIcons";

function getSyncHealthIndicator(health: SyncHealth, skillCount: number): { color: string; title: string } | null {
  if (skillCount === 0) return null;
  if (health.diverged > 0) return { color: "bg-red-400", title: `${health.diverged} diverged` };
  if (health.project_newer > 0 || health.center_newer > 0) {
    const parts: string[] = [];
    if (health.project_newer > 0) parts.push(`${health.project_newer} project newer`);
    if (health.center_newer > 0) parts.push(`${health.center_newer} center newer`);
    return { color: "bg-amber-400", title: parts.join(", ") };
  }
  if (health.project_only > 0) return { color: "bg-blue-400", title: `${health.project_only} project only` };
  if (health.in_sync === skillCount) return { color: "bg-emerald-400", title: "All in sync" };
  return null;
}

export function Sidebar() {
  const { t } = useTranslation();
  const location = useLocation();
  const navigate = useNavigate();
  const { presets, viewedPreset, setViewedPresetId, refreshPresets, refreshManagedSkills, projects, refreshProjects, tools, managedSkills, appUpdate } = useApp();
  const [showCreate, setShowCreate] = useState(false);
  const [showAddProject, setShowAddProject] = useState(false);
  const [renameTarget, setRenameTarget] = useState<{ id: string; name: string; icon?: string | null } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string } | null>(null);
  const [deleteProjectTarget, setDeleteProjectTarget] = useState<{ id: string; name: string } | null>(null);
  const installedTools = useMemo(() => tools.filter((t) => t.installed && t.enabled), [tools]);
  const installedCodingTools = useMemo(
    () => installedTools.filter((t) => t.category === "coding"),
    [installedTools]
  );
  const installedLobsterTools = useMemo(
    () => installedTools.filter((t) => t.category === "lobster"),
    [installedTools]
  );
  const [orderedPresets, setOrderedPresets] = useState(presets);
  const [orderedProjects, setOrderedProjects] = useState(projects);
  const [orderedCodingTools, setOrderedCodingTools] = useState(installedCodingTools);
  const [orderedLobsterTools, setOrderedLobsterTools] = useState(installedLobsterTools);
  const presetReorderQueueRef = useRef<Promise<void>>(Promise.resolve());
  const projectReorderQueueRef = useRef<Promise<void>>(Promise.resolve());
  const [presetsOpen, setPresetsOpen] = useState(true);
  const [projectsOpen, setProjectsOpen] = useState(true);
  const [globalWorkspaceOpen, setGlobalWorkspaceOpen] = useState(true);
  const [lobsterWorkspaceOpen, setLobsterWorkspaceOpen] = useState(true);

  const globalSkillsByAgent = useMemo(() => {
    const map: Record<string, number> = {};
    for (const tool of installedTools) {
      map[tool.key] = managedSkills.filter((skill) =>
        skill.targets.some((target) => target.tool === tool.key)
      ).length;
    }
    return map;
  }, [installedTools, managedSkills]);

  useEffect(() => { setOrderedPresets(presets); }, [presets]);
  useEffect(() => { setOrderedProjects(projects); }, [projects]);
  useEffect(() => {
    const stored = localStorage.getItem("skills-manager:tool-order");
    const storedOrder: string[] = stored ? JSON.parse(stored) : [];
    const sorted = [
      ...storedOrder.flatMap((key) => {
        const t = installedCodingTools.find((t) => t.key === key);
        return t ? [t] : [];
      }),
      ...installedCodingTools.filter((t) => !storedOrder.includes(t.key)),
    ];
    setOrderedCodingTools(sorted);
  }, [installedCodingTools]);
  useEffect(() => {
    const stored = localStorage.getItem("skills-manager:lobster-tool-order");
    const storedOrder: string[] = stored ? JSON.parse(stored) : [];
    const sorted = [
      ...storedOrder.flatMap((key) => {
        const t = installedLobsterTools.find((t) => t.key === key);
        return t ? [t] : [];
      }),
      ...installedLobsterTools.filter((t) => !storedOrder.includes(t.key)),
    ];
    setOrderedLobsterTools(sorted);
  }, [installedLobsterTools]);

  const handleDragEnd = (result: DropResult) => {
    if (!result.destination || result.destination.index === result.source.index) return;
    const reordered = [...orderedPresets];
    const [moved] = reordered.splice(result.source.index, 1);
    reordered.splice(result.destination.index, 0, moved);
    setOrderedPresets(reordered);

    presetReorderQueueRef.current = presetReorderQueueRef.current
      .catch(() => undefined)
      .then(async () => {
        try {
          await api.reorderPresets(reordered.map((s) => s.id));
        } catch {
          await refreshPresets();
          toast.error(t("common.error"));
        }
      });
  };

  const handleProjectDragEnd = (result: DropResult) => {
    if (!result.destination || result.destination.index === result.source.index) return;
    const reordered = [...orderedProjects];
    const [moved] = reordered.splice(result.source.index, 1);
    reordered.splice(result.destination.index, 0, moved);
    setOrderedProjects(reordered);

    projectReorderQueueRef.current = projectReorderQueueRef.current
      .catch(() => undefined)
      .then(async () => {
        try {
          await api.reorderProjects(reordered.map((p) => p.id));
        } catch {
          await refreshProjects();
          toast.error(t("common.error"));
        }
      });
  };

  const handleToolDragEnd = (category: ToolCategory) => (result: DropResult) => {
    if (!result.destination || result.destination.index === result.source.index) return;
    const current = category === "lobster" ? orderedLobsterTools : orderedCodingTools;
    const reordered = [...current];
    const [moved] = reordered.splice(result.source.index, 1);
    reordered.splice(result.destination.index, 0, moved);
    if (category === "lobster") {
      setOrderedLobsterTools(reordered);
      localStorage.setItem("skills-manager:lobster-tool-order", JSON.stringify(reordered.map((t) => t.key)));
    } else {
      setOrderedCodingTools(reordered);
      localStorage.setItem("skills-manager:tool-order", JSON.stringify(reordered.map((t) => t.key)));
    }
  };

  const NAV_ITEMS = [
    { name: t("sidebar.dashboard"), path: "/", icon: LayoutDashboard },
    { name: t("sidebar.mySkills"), path: "/my-skills", icon: Layers },
    { name: t("sidebar.installSkills"), path: "/install", icon: Download },
    { name: t("sidebar.backup"), path: "/backup", icon: CloudUpload },
  ];

  const handleSwitchPreset = (id: string) => {
    setViewedPresetId(id);
    if (location.pathname !== "/my-skills") {
      navigate("/my-skills");
    }
  };

  const handleCreatePreset = async (name: string, description?: string, icon?: string) => {
    await api.createPreset(name, description, icon);
    await Promise.all([refreshPresets(), refreshManagedSkills()]);
    if (location.pathname === "/settings") {
      navigate("/my-skills");
    }
    toast.success(t("preset.created"));
  };

  const handleRenamePreset = async (newName: string, icon?: string) => {
    if (!renameTarget) return;
    const preset = presets.find((s) => s.id === renameTarget.id);
    if (!preset) return;
    await api.updatePreset(
      renameTarget.id,
      newName,
      preset.description || undefined,
      icon || preset.icon || undefined
    );
    await refreshPresets();
    toast.success(t("preset.renamed"));
  };

  const handleDeletePreset = async () => {
    if (!deleteTarget) return;
    await api.deletePreset(deleteTarget.id);
    await Promise.all([refreshPresets(), refreshManagedSkills()]);
    if (location.pathname === "/settings") {
      navigate("/my-skills");
    }
    toast.success(t("preset.deleted"));
  };

  const handleRenameClick = (
    event: React.MouseEvent,
    preset: { id: string; name: string; icon?: string | null }
  ) => {
    event.preventDefault();
    event.stopPropagation();
    setRenameTarget(preset);
  };

  const handleDeleteClick = (event: React.MouseEvent, preset: { id: string; name: string }) => {
    event.preventDefault();
    event.stopPropagation();
    setDeleteTarget(preset);
  };

  const handleDeleteProject = async () => {
    if (!deleteProjectTarget) return;
    await api.removeProject(deleteProjectTarget.id);
    await refreshProjects();
    if (location.pathname.startsWith("/project/")) {
      navigate("/");
    }
    toast.success(t("project.removed"));
  };

  // Renders one workspace category section (Global Workspace for coding agents,
  // Lobster Agents for lobster agents). Both sections share identical UX —
  // collapsible heading, "All Agents" overview entry, and a drag-orderable list.
  const renderToolGroup = (group: {
    category: ToolCategory;
    headingLabel: string;
    allAgentsLabel: string;
    emptyLabel: string;
    basePath: string;
    droppableId: string;
    tools: ToolInfo[];
    isOpen: boolean;
    onToggle: () => void;
    hideWhenEmpty: boolean;
  }) => {
    if (group.hideWhenEmpty && group.tools.length === 0) return null;
    return (
      <>
        <div className="mb-1.5 px-2.5 flex items-center gap-1">
          <button
            onClick={group.onToggle}
            className="flex min-w-0 flex-1 items-center gap-1 text-left outline-none"
          >
            {group.isOpen
              ? <ChevronDown className="h-3 w-3 shrink-0 text-faint" />
              : <ChevronRight className="h-3 w-3 shrink-0 text-faint" />}
            <span className="truncate text-[12px] font-semibold tracking-[0.01em] text-muted whitespace-nowrap">
              {group.headingLabel}
            </span>
          </button>
        </div>
        {group.isOpen && (
          <>
            {/* Pinned overview item */}
            {(() => {
              const isActive = location.pathname === group.basePath;
              return (
                <Link
                  to={group.basePath}
                  className={cn(
                    "mb-0.5 flex items-center gap-2 px-2.5 py-[7px] rounded-md text-sm transition-colors outline-none",
                    isActive
                      ? "bg-surface-active font-medium text-primary"
                      : "text-tertiary hover:text-secondary hover:bg-surface-hover"
                  )}
                >
                  <span className={cn(
                    "flex h-[20px] w-[20px] shrink-0 items-center justify-center rounded border",
                    isActive
                      ? "border-accent/30 bg-accent/10 text-accent"
                      : "border-border bg-surface text-muted"
                  )}>
                    <Globe className="h-3 w-3" />
                  </span>
                  <span className="flex-1 truncate">{group.allAgentsLabel}</span>
                </Link>
              );
            })()}
            {group.tools.length === 0 ? (
              <p className="px-5 py-1.5 text-[12px] text-faint">{group.emptyLabel}</p>
            ) : (
              <DragDropContext onDragEnd={handleToolDragEnd(group.category)}>
                <Droppable droppableId={group.droppableId}>
                  {(droppableProvided) => (
                    <div
                      className="space-y-0.5"
                      ref={droppableProvided.innerRef}
                      {...droppableProvided.droppableProps}
                    >
                      {group.tools.map((tool, index) => {
                        const skillCount = globalSkillsByAgent[tool.key] ?? 0;
                        const isActive = location.pathname === `${group.basePath}/${tool.key}`;
                        return (
                          <Draggable key={tool.key} draggableId={tool.key} index={index}>
                            {(provided) => (
                              <div
                                ref={provided.innerRef}
                                {...provided.draggableProps}
                                className={cn(
                                  "group relative flex items-center rounded-md transition-colors",
                                  isActive ? "bg-surface-active" : "hover:bg-surface-hover"
                                )}
                              >
                                <button
                                  onClick={() => navigate(`${group.basePath}/${tool.key}`)}
                                  className={cn(
                                    "flex min-w-0 flex-1 items-center gap-2 px-2.5 py-[7px] text-left text-sm leading-5 outline-none",
                                    isActive ? "font-medium text-primary" : "text-tertiary group-hover:text-secondary"
                                  )}
                                >
                                  <AgentIcon
                                    agentKey={tool.key}
                                    displayName={tool.display_name}
                                    className={cn(
                                      "h-[20px] w-[20px] rounded border transition-colors",
                                      isActive ? "border-accent/30 bg-accent/10" : "group-hover:border-border"
                                    )}
                                  />
                                  <span className="flex-1 truncate">{tool.display_name}</span>
                                  <span className="ml-auto flex h-[18px] w-[32px] shrink-0 items-center justify-end group-hover:hidden">
                                    {skillCount > 0 && (
                                      <span className={cn(
                                        "min-w-[18px] rounded-full px-1.5 text-center text-[12px] font-medium leading-[18px] tabular-nums",
                                        isActive ? "bg-accent-bg text-accent-light" : "bg-surface-hover text-muted"
                                      )}>
                                        {skillCount}
                                      </span>
                                    )}
                                  </span>
                                </button>
                                <div className={cn(
                                  "absolute right-1 flex items-center rounded-md invisible opacity-0 transition-opacity group-hover:visible group-hover:opacity-100",
                                  isActive ? "bg-surface-active" : "bg-surface-hover"
                                )}>
                                  <div
                                    {...provided.dragHandleProps}
                                    className="rounded p-1 text-faint cursor-grab active:cursor-grabbing"
                                  >
                                    <GripVertical className="h-3 w-3" />
                                  </div>
                                </div>
                              </div>
                            )}
                          </Draggable>
                        );
                      })}
                      {droppableProvided.placeholder}
                    </div>
                  )}
                </Droppable>
              </DragDropContext>
            )}
          </>
        )}
      </>
    );
  };

  return (
    <>
      <div className="w-[220px] flex-shrink-0 bg-bg-secondary border-r border-border-subtle h-full flex flex-col select-none relative z-10">
        {/* Traffic-light safe zone */}
        <div className="h-[38px] shrink-0" />
        {/* App logo — sits below macOS window controls */}
        <div className="flex items-center px-3 gap-3 pb-2.5 shrink-0">
          <img
            src="/icons/32x32.png"
            alt="logo"
            className="w-[24px] h-[24px] shrink-0"
          />
          <span className="text-[16px] font-semibold text-secondary tracking-tight truncate leading-[22px]">
            {t("app.name")}
          </span>
        </div>

        {/* Nav */}
        <div className="px-2.5 space-y-0.5 shrink-0">
          {NAV_ITEMS.map((item) => {
            const Icon = item.icon;
            const isActive = location.pathname === item.path;
            return (
              <Link
                key={item.path}
                to={item.path}
                className={cn(
                  "flex items-center gap-2.5 px-2.5 py-[7px] rounded-md text-sm font-medium transition-colors outline-none",
                  isActive
                    ? "bg-surface-active text-primary"
                    : "text-tertiary hover:text-secondary hover:bg-surface-hover"
                )}
              >
                <Icon className={cn("w-4 h-4 shrink-0", isActive ? "text-accent" : "text-muted")} />
                {item.name}
              </Link>
            );
          })}
        </div>

        {/* Divider */}
        <div className="mx-3 mt-3.5 mb-2.5 border-t border-border-subtle" />

        {/* Scrollable section */}
        <div className="px-2.5 flex-1 overflow-y-auto scrollbar-hide min-h-0">

          {/* ── Presets ── */}
          <div className="mb-1.5 px-2.5 flex items-center gap-1">
            <button
              onClick={() => setPresetsOpen((v) => !v)}
              className="flex min-w-0 flex-1 items-center gap-1 text-left outline-none"
            >
              {presetsOpen
                ? <ChevronDown className="h-3 w-3 shrink-0 text-faint" />
                : <ChevronRight className="h-3 w-3 shrink-0 text-faint" />}
              <span className="truncate text-[12px] font-semibold tracking-[0.01em] text-muted whitespace-nowrap">
                {t("sidebar.presets")}
              </span>
            </button>
          </div>
          {presetsOpen && (
            <>
              <DragDropContext onDragEnd={handleDragEnd}>
                <Droppable droppableId="presets">
                  {(droppableProvided) => (
                    <div
                      className="space-y-0.5"
                      ref={droppableProvided.innerRef}
                      {...droppableProvided.droppableProps}
                    >
                      {orderedPresets.map((preset, index) => {
                        const isActive = viewedPreset?.id === preset.id;
                        const presetIcon = getPresetIconOption(preset);
                        const PresetIcon = presetIcon.icon;
                        return (
                          <Draggable key={preset.id} draggableId={preset.id} index={index}>
                            {(provided) => (
                              <div
                                ref={provided.innerRef}
                                {...provided.draggableProps}
                                className={cn(
                                  "group relative flex items-center rounded-md transition-colors",
                                  isActive ? "bg-surface-active" : "hover:bg-surface-hover"
                                )}
                              >
                                <button
                                  onClick={() => handleSwitchPreset(preset.id)}
                                  className={cn(
                                    "flex min-w-0 flex-1 items-center gap-2 px-2.5 py-[7px] text-left text-sm leading-5 outline-none",
                                    isActive ? "font-medium text-primary" : "text-tertiary group-hover:text-secondary"
                                  )}
                                >
                                  <span
                                    className={cn(
                                      "flex h-[20px] w-[20px] shrink-0 items-center justify-center rounded border",
                                      isActive
                                        ? `${presetIcon.activeClass} ${presetIcon.colorClass}`
                                        : "border-border bg-surface text-muted group-hover:border-border group-hover:text-tertiary"
                                    )}
                                  >
                                    <PresetIcon className="h-3 w-3" />
                                  </span>
                                  <span className="flex-1 truncate">{preset.name}</span>
                                  <span className="ml-auto flex h-[18px] w-[32px] shrink-0 items-center justify-end group-hover:hidden">
                                    {preset.skill_count > 0 && (
                                      <span
                                        className={cn(
                                          "min-w-[18px] rounded-full px-1.5 text-center text-[12px] font-medium leading-[18px] tabular-nums",
                                          isActive
                                            ? "bg-accent-bg text-accent-light"
                                            : "bg-surface-hover text-muted"
                                        )}
                                      >
                                        {preset.skill_count}
                                      </span>
                                    )}
                                  </span>
                                </button>
                                <div className={cn(
                                  "absolute right-1 flex items-center rounded-md invisible opacity-0 transition-opacity group-hover:visible group-hover:opacity-100",
                                  isActive ? "bg-surface-active" : "bg-surface-hover"
                                )}>
                                  <div
                                    {...provided.dragHandleProps}
                                    className="rounded p-1 text-faint cursor-grab active:cursor-grabbing"
                                  >
                                    <GripVertical className="h-3 w-3" />
                                  </div>
                                  <button
                                    onClick={(event) => handleRenameClick(event, preset)}
                                    className="rounded p-1 text-faint transition hover:text-secondary"
                                    title={t("common.rename")}
                                  >
                                    <Pencil className="h-3 w-3" />
                                  </button>
                                  <button
                                    onClick={(event) => handleDeleteClick(event, preset)}
                                    className="rounded p-1 text-faint transition hover:text-red-400"
                                    title={t("common.delete")}
                                  >
                                    <Trash2 className="h-3 w-3" />
                                  </button>
                                </div>
                              </div>
                            )}
                          </Draggable>
                        );
                      })}
                      {droppableProvided.placeholder}
                    </div>
                  )}
                </Droppable>
              </DragDropContext>
              <button
                onClick={() => setShowCreate(true)}
                className="flex items-center gap-2 px-2.5 py-[7px] mt-1 rounded-md text-sm text-muted hover:text-secondary hover:bg-surface-hover transition-colors w-full outline-none"
              >
                <Plus className="w-3.5 h-3.5" />
                {t("sidebar.newPreset")}
              </button>
            </>
          )}

          {/* Divider */}
          <div className="mx-0.5 mt-3.5 mb-2.5 border-t border-border-subtle" />

          {renderToolGroup({
            category: "coding",
            headingLabel: t("sidebar.globalWorkspace"),
            allAgentsLabel: t("globalWorkspace.allAgents"),
            emptyLabel: t("globalWorkspace.noAgents"),
            basePath: "/global-workspace",
            droppableId: "global-workspace-tools",
            tools: orderedCodingTools,
            isOpen: globalWorkspaceOpen,
            onToggle: () => setGlobalWorkspaceOpen((v) => !v),
            // Always show the Global Workspace section (even when empty) so users
            // with no detected coding agents still see the "All Agents" entry.
            hideWhenEmpty: false,
          })}

          {installedLobsterTools.length > 0 && (
            <>
              {/* Divider */}
              <div className="mx-0.5 mt-3.5 mb-2.5 border-t border-border-subtle" />

              {renderToolGroup({
                category: "lobster",
                headingLabel: t("sidebar.lobsterAgents"),
                allAgentsLabel: t("lobsterWorkspace.allAgents"),
                emptyLabel: t("lobsterWorkspace.noAgents"),
                basePath: "/lobster-workspace",
                droppableId: "lobster-workspace-tools",
                tools: orderedLobsterTools,
                isOpen: lobsterWorkspaceOpen,
                onToggle: () => setLobsterWorkspaceOpen((v) => !v),
                hideWhenEmpty: true,
              })}
            </>
          )}

          {/* Divider */}
          <div className="mx-0.5 mt-3.5 mb-2.5 border-t border-border-subtle" />

          {/* ── Projects ── */}
          <div className="mb-1.5 px-2.5 flex items-center gap-1">
            <button
              onClick={() => setProjectsOpen((v) => !v)}
              className="flex min-w-0 flex-1 items-center gap-1 text-left outline-none"
            >
              {projectsOpen
                ? <ChevronDown className="h-3 w-3 shrink-0 text-faint" />
                : <ChevronRight className="h-3 w-3 shrink-0 text-faint" />}
              <span className="truncate text-[12px] font-semibold tracking-[0.01em] text-muted whitespace-nowrap">
                {t("sidebar.projects")}
              </span>
            </button>
          </div>
          {projectsOpen && (
            <>
              <DragDropContext onDragEnd={handleProjectDragEnd}>
                <Droppable droppableId="projects">
                  {(droppableProvided) => (
                    <div
                      className="space-y-0.5"
                      ref={droppableProvided.innerRef}
                      {...droppableProvided.droppableProps}
                    >
                      {orderedProjects.map((project, index) => {
                        const isActive = location.pathname === `/project/${project.id}`;
                        const healthIndicator = getSyncHealthIndicator(project.sync_health, project.skill_count);
                        return (
                          <Draggable key={project.id} draggableId={project.id} index={index}>
                            {(provided) => (
                              <div
                                ref={provided.innerRef}
                                {...provided.draggableProps}
                                className={cn(
                                  "group relative flex items-center rounded-md transition-colors",
                                  isActive ? "bg-surface-active" : "hover:bg-surface-hover"
                                )}
                              >
                                <button
                                  onClick={() => navigate(`/project/${project.id}`)}
                                  className={cn(
                                    "flex min-w-0 flex-1 items-center gap-2 px-2.5 py-[7px] text-left text-sm leading-5 outline-none",
                                    isActive ? "font-medium text-primary" : "text-tertiary group-hover:text-secondary"
                                  )}
                                >
                                  <span
                                    className={cn(
                                      "flex h-[20px] w-[20px] shrink-0 items-center justify-center rounded border",
                                      isActive
                                        ? project.workspace_type === "linked"
                                          ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-500"
                                          : "border-blue-500/30 bg-blue-500/10 text-blue-500"
                                        : "border-border bg-surface text-muted group-hover:border-border group-hover:text-tertiary"
                                    )}
                                  >
                                    {project.workspace_type === "linked"
                                      ? <Link2 className="h-3 w-3" />
                                      : <FolderOpen className="h-3 w-3" />}
                                  </span>
                                  <span className="flex-1 truncate">{project.name}</span>
                                  <span className="ml-auto flex h-[18px] w-[52px] shrink-0 items-center justify-end gap-2 group-hover:hidden">
                                    {healthIndicator && (
                                      <span
                                        className={cn("h-1.5 w-1.5 shrink-0 rounded-full", healthIndicator.color)}
                                        title={healthIndicator.title}
                                      />
                                    )}
                                    {project.skill_count > 0 && (
                                      <span
                                        className={cn(
                                          "min-w-[24px] rounded-full px-1.5 text-center text-[12px] font-medium leading-[18px] tabular-nums",
                                          isActive
                                            ? "bg-accent-bg text-accent-light"
                                            : "bg-surface-hover text-muted"
                                        )}
                                      >
                                        {project.skill_count}
                                      </span>
                                    )}
                                  </span>
                                </button>
                                <div className={cn(
                                  "absolute right-1 flex items-center rounded-md invisible opacity-0 transition-opacity group-hover:visible group-hover:opacity-100",
                                  isActive ? "bg-surface-active" : "bg-surface-hover"
                                )}>
                                  <div
                                    {...provided.dragHandleProps}
                                    className="rounded p-1 text-faint cursor-grab active:cursor-grabbing"
                                  >
                                    <GripVertical className="h-3 w-3" />
                                  </div>
                                  <button
                                    onClick={(e) => {
                                      e.preventDefault();
                                      e.stopPropagation();
                                      setDeleteProjectTarget(project);
                                    }}
                                    className="rounded p-1 text-faint transition hover:text-red-400"
                                    title={t("common.delete")}
                                  >
                                    <Trash2 className="h-3 w-3" />
                                  </button>
                                </div>
                              </div>
                            )}
                          </Draggable>
                        );
                      })}
                      {droppableProvided.placeholder}
                    </div>
                  )}
                </Droppable>
              </DragDropContext>
              <button
                onClick={() => setShowAddProject(true)}
                className="flex items-center gap-2 px-2.5 py-[7px] mt-1 rounded-md text-sm text-muted hover:text-secondary hover:bg-surface-hover transition-colors w-full outline-none"
              >
                <Plus className="w-3.5 h-3.5" />
                {t("sidebar.addProject")}
              </button>
            </>
          )}

        </div>

        {/* Settings */}
        <div className="p-2.5 border-t border-border-subtle shrink-0">
          <Link
            to="/settings"
            className={cn(
              "flex items-center gap-2.5 px-2.5 py-[7px] rounded-md text-sm font-medium transition-colors outline-none",
              location.pathname === "/settings"
                ? "bg-surface-active text-primary"
                : "text-tertiary hover:text-secondary hover:bg-surface-hover"
            )}
          >
            <Settings
              className={cn(
                "w-4 h-4 shrink-0",
                location.pathname === "/settings" ? "text-accent" : "text-muted"
              )}
            />
            {t("sidebar.settings")}
            {/* A newer app version exists. Amber = "有更新" per the UI spec;
                the dot only points at Settings, where the user decides. */}
            {appUpdate?.has_update && (
              <span
                className="ml-auto h-1.5 w-1.5 shrink-0 rounded-full bg-amber-400"
                title={t("settings.updateAvailable", { version: appUpdate.latest_version })}
              />
            )}
          </Link>
        </div>
      </div>

      <CreatePresetDialog
        open={showCreate}
        onClose={() => setShowCreate(false)}
        onCreate={handleCreatePreset}
      />

      <RenamePresetDialog
        open={renameTarget !== null}
        currentName={renameTarget?.name || ""}
        currentIcon={renameTarget?.icon}
        onClose={() => setRenameTarget(null)}
        onRename={handleRenamePreset}
      />

      <ConfirmDialog
        open={deleteTarget !== null}
        message={t("preset.deleteConfirm", { name: deleteTarget?.name || "" })}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleDeletePreset}
      />

      <AddProjectDialog
        open={showAddProject}
        onClose={() => setShowAddProject(false)}
        onAdded={async () => {
          await refreshProjects();
          toast.success(t("project.workspaceAdded"));
        }}
      />

      <ConfirmDialog
        open={deleteProjectTarget !== null}
        message={t("project.removeConfirm", { name: deleteProjectTarget?.name || "" })}
        onClose={() => setDeleteProjectTarget(null)}
        onConfirm={handleDeleteProject}
      />
    </>
  );
}
