import { useState, useEffect, useRef } from "react";
import { DragDropContext, Droppable, Draggable, type DropResult } from "@hello-pangea/dnd";
import { Link, useLocation, useNavigate } from "react-router-dom";
import {
  LayoutDashboard,
  Layers,
  Package,
  Download,
  Grid3X3,
  Plug,
  Settings,
  Plus,
  Pencil,
  Trash2,
  FolderOpen,
  GripVertical,
  Link2,
  ChevronRight,
  Bot,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import { CreateScenarioDialog } from "./CreateScenarioDialog";
import { RenameScenarioDialog } from "./RenameScenarioDialog";
import { AddProjectDialog } from "./AddProjectDialog";
import { ConfirmDialog } from "./ConfirmDialog";
import * as api from "../lib/tauri";
import type { SyncHealth, AgentConfigDto } from "../lib/tauri";
import { getScenarioIconOption } from "../lib/scenarioIcons";

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
  const { scenarios, activeScenario, switchScenario, refreshScenarios, refreshManagedSkills, projects, refreshProjects } = useApp();
  const [showCreate, setShowCreate] = useState(false);
  const [showAddProject, setShowAddProject] = useState(false);
  const [renameTarget, setRenameTarget] = useState<{ id: string; name: string; icon?: string | null } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string } | null>(null);
  const [deleteProjectTarget, setDeleteProjectTarget] = useState<{ id: string; name: string } | null>(null);
  const [orderedScenarios, setOrderedScenarios] = useState(scenarios);
  const [orderedProjects, setOrderedProjects] = useState(projects);
  const [agentConfigs, setAgentConfigs] = useState<AgentConfigDto[]>([]);
  const [scenariosCollapsed, setScenariosCollapsed] = useState(false);
  const scenarioReorderQueueRef = useRef<Promise<void>>(Promise.resolve());
  const projectReorderQueueRef = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => { setOrderedScenarios(scenarios); }, [scenarios]);
  useEffect(() => { setOrderedProjects(projects); }, [projects]);

  useEffect(() => {
    api.getAllAgentConfigs().then(setAgentConfigs).catch(() => {});
  }, [activeScenario]);

  const handleDragEnd = (result: DropResult) => {
    if (!result.destination || result.destination.index === result.source.index) return;
    const reordered = [...orderedScenarios];
    const [moved] = reordered.splice(result.source.index, 1);
    reordered.splice(result.destination.index, 0, moved);
    setOrderedScenarios(reordered);

    scenarioReorderQueueRef.current = scenarioReorderQueueRef.current
      .catch(() => undefined)
      .then(async () => {
        try {
          await api.reorderScenarios(reordered.map((s) => s.id));
        } catch {
          await refreshScenarios();
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

  const NAV_ITEMS = [
    { name: t("sidebar.dashboard"), path: "/", icon: LayoutDashboard },
    { name: t("sidebar.mySkills"), path: "/my-skills", icon: Layers },
    { name: "Packs", path: "/packs", icon: Package },
    { name: t("sidebar.installSkills"), path: "/install", icon: Download },
    { name: t("sidebar.matrix"), path: "/matrix", icon: Grid3X3 },
    { name: t("sidebar.plugins"), path: "/plugins", icon: Plug },
  ];

  const handleSwitchScenario = async (id: string) => {
    await switchScenario(id);
    const s = scenarios.find((s) => s.id === id);
    if (location.pathname === "/settings") {
      navigate("/my-skills");
    }
    if (s) toast.success(t("scenario.switched", { name: s.name }));
  };

  const handleCreateScenario = async (name: string, description?: string, icon?: string) => {
    await api.createScenario(name, description, icon);
    await Promise.all([refreshScenarios(), refreshManagedSkills()]);
    if (location.pathname === "/settings") {
      navigate("/my-skills");
    }
    toast.success(t("scenario.created"));
  };

  const handleRenameScenario = async (newName: string, icon?: string) => {
    if (!renameTarget) return;
    const scenario = scenarios.find((s) => s.id === renameTarget.id);
    if (!scenario) return;
    await api.updateScenario(
      renameTarget.id,
      newName,
      scenario.description || undefined,
      icon || scenario.icon || undefined
    );
    await refreshScenarios();
    toast.success(t("scenario.renamed"));
  };

  const handleDeleteScenario = async () => {
    if (!deleteTarget) return;
    await api.deleteScenario(deleteTarget.id);
    await Promise.all([refreshScenarios(), refreshManagedSkills()]);
    if (location.pathname === "/settings") {
      navigate("/my-skills");
    }
    toast.success(t("scenario.deleted"));
  };

  const handleRenameClick = (
    event: React.MouseEvent,
    scenario: { id: string; name: string; icon?: string | null }
  ) => {
    event.preventDefault();
    event.stopPropagation();
    setRenameTarget(scenario);
  };

  const handleDeleteClick = (event: React.MouseEvent, scenario: { id: string; name: string }) => {
    event.preventDefault();
    event.stopPropagation();
    setDeleteTarget(scenario);
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
                  "flex items-center gap-2.5 px-2.5 py-[7px] rounded-[5px] text-sm font-medium transition-colors outline-none",
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

        <div className="px-2.5 flex-1 overflow-y-auto scrollbar-hide min-h-0">
          {/* Agents */}
          {agentConfigs.filter((a) => a.managed).length > 0 && (
            <>
              <div className="mb-1.5 px-2.5">
                <span className="block truncate text-[12px] font-semibold tracking-[0.01em] text-muted whitespace-nowrap">
                  AGENTS
                </span>
              </div>
              <div className="space-y-0.5 mb-2">
                {agentConfigs.filter((a) => a.managed).map((agent) => (
                  <Link
                    key={agent.tool_key}
                    to={`/agent/${agent.tool_key}`}
                    className={cn(
                      "flex items-center gap-2 px-2.5 py-[7px] rounded-[5px] text-[13px] transition-colors outline-none",
                      location.pathname === `/agent/${agent.tool_key}`
                        ? "bg-surface-active text-primary font-medium"
                        : "text-tertiary hover:text-secondary hover:bg-surface-hover"
                    )}
                  >
                    <Bot
                      className={cn(
                        "w-4 h-4 shrink-0",
                        location.pathname === `/agent/${agent.tool_key}` ? "text-accent" : "text-muted"
                      )}
                    />
                    <span
                      className={cn(
                        "w-1.5 h-1.5 rounded-full shrink-0",
                        agent.managed ? "bg-emerald-400" : "bg-gray-500"
                      )}
                    />
                    <span className="flex-1 truncate">{agent.display_name}</span>
                    <span className="text-[10px] text-muted shrink-0 tabular-nums">
                      {agent.scenario_name || "—"}
                      {agent.extra_pack_count > 0 && ` +${agent.extra_pack_count}`}
                    </span>
                  </Link>
                ))}
              </div>
              <div className="mx-0.5 mb-2.5 border-t border-border-subtle" />
            </>
          )}

          {/* Scenarios */}
          <div className="mb-1.5 px-2.5 flex items-center justify-between">
            <span className="block truncate text-[12px] font-semibold tracking-[0.01em] text-muted whitespace-nowrap">
              {t("sidebar.scenarios")}
            </span>
            <button
              onClick={() => setScenariosCollapsed((c) => !c)}
              className="text-faint hover:text-muted transition-colors outline-none"
              title={scenariosCollapsed ? "Expand" : "Collapse"}
            >
              <ChevronRight
                className={cn("w-3 h-3 transition-transform", !scenariosCollapsed && "rotate-90")}
              />
            </button>
          </div>
          {!scenariosCollapsed && (
          <>
          <DragDropContext onDragEnd={handleDragEnd}>
            <Droppable droppableId="scenarios">
              {(droppableProvided) => (
                <div
                  className="space-y-0.5"
                  ref={droppableProvided.innerRef}
                  {...droppableProvided.droppableProps}
                >
                  {orderedScenarios.map((scenario, index) => {
                    const isActive = activeScenario?.id === scenario.id;
                    const scenarioIcon = getScenarioIconOption(scenario);
                    const ScenarioIcon = scenarioIcon.icon;
                    return (
                      <Draggable key={scenario.id} draggableId={scenario.id} index={index}>
                        {(provided) => (
                          <div
                            ref={provided.innerRef}
                            {...provided.draggableProps}
                            className={cn(
                              "group relative flex items-center rounded-[5px] transition-colors",
                              isActive ? "bg-surface-active" : "hover:bg-surface-hover"
                            )}
                          >
                            <button
                              onClick={() => handleSwitchScenario(scenario.id)}
                              className={cn(
                                "flex min-w-0 flex-1 items-center gap-2 px-2.5 py-[7px] text-left text-[15px] leading-5 outline-none",
                                isActive ? "font-medium text-primary" : "text-tertiary group-hover:text-secondary"
                              )}
                            >
                              <span
                                className={cn(
                                  "flex h-[20px] w-[20px] shrink-0 items-center justify-center rounded border",
                                  isActive
                                    ? `${scenarioIcon.activeClass} ${scenarioIcon.colorClass}`
                                    : "border-border bg-surface text-muted group-hover:border-border group-hover:text-tertiary"
                                )}
                              >
                                <ScenarioIcon className="h-3 w-3" />
                              </span>
                              <span className="flex-1 truncate">{scenario.name}</span>
                              <span className="ml-auto flex h-[18px] w-[32px] shrink-0 items-center justify-end group-hover:hidden">
                                {scenario.skill_count > 0 && (
                                  <span
                                    className={cn(
                                      "min-w-[18px] rounded-full px-1.5 text-center text-[12px] font-medium leading-[18px] tabular-nums",
                                      isActive
                                        ? "bg-accent-bg text-accent-light"
                                        : "bg-surface-hover text-muted"
                                    )}
                                  >
                                    {scenario.skill_count}
                                  </span>
                                )}
                              </span>
                            </button>

                            <div className={cn(
                              "absolute right-1 flex items-center rounded-[3px] invisible opacity-0 transition-opacity group-hover:visible group-hover:opacity-100",
                              isActive ? "bg-surface-active" : "bg-surface-hover"
                            )}>
                              <div
                                {...provided.dragHandleProps}
                                className="rounded p-1 text-faint cursor-grab active:cursor-grabbing"
                              >
                                <GripVertical className="h-3 w-3" />
                              </div>
                              <button
                                onClick={(event) => handleRenameClick(event, scenario)}
                                className="rounded p-1 text-faint transition hover:text-secondary"
                                title={t("common.rename")}
                              >
                                <Pencil className="h-3 w-3" />
                              </button>
                              <button
                                onClick={(event) => handleDeleteClick(event, scenario)}
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
            className="flex items-center gap-2 px-2.5 py-[7px] mt-1 rounded-[5px] text-[13px] text-muted hover:text-secondary hover:bg-surface-hover transition-colors w-full outline-none"
          >
            <Plus className="w-3.5 h-3.5" />
            {t("sidebar.newScenario")}
          </button>
          </>
          )}

          {/* Divider */}
          <div className="mx-0.5 mt-3.5 mb-2.5 border-t border-border-subtle" />

          {/* Projects */}
          <div className="mb-1.5 px-2.5">
            <span className="block truncate text-[12px] font-semibold tracking-[0.01em] text-muted whitespace-nowrap">
              {t("sidebar.projects")}
            </span>
          </div>
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
                              "group relative flex items-center rounded-[5px] transition-colors",
                              isActive ? "bg-surface-active" : "hover:bg-surface-hover"
                            )}
                          >
                            <button
                              onClick={() => navigate(`/project/${project.id}`)}
                              className={cn(
                                "flex min-w-0 flex-1 items-center gap-2 px-2.5 py-[7px] text-left text-[15px] leading-5 outline-none",
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
                              "absolute right-1 flex items-center rounded-[3px] invisible opacity-0 transition-opacity group-hover:visible group-hover:opacity-100",
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
            className="flex items-center gap-2 px-2.5 py-[7px] mt-1 rounded-[5px] text-[13px] text-muted hover:text-secondary hover:bg-surface-hover transition-colors w-full outline-none"
          >
            <Plus className="w-3.5 h-3.5" />
            {t("sidebar.addProject")}
          </button>
        </div>

        {/* Settings */}
        <div className="p-2.5 border-t border-border-subtle shrink-0">
          <Link
            to="/settings"
            className={cn(
              "flex items-center gap-2.5 px-2.5 py-[7px] rounded-[5px] text-sm font-medium transition-colors outline-none",
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
          </Link>
        </div>
      </div>

      <CreateScenarioDialog
        open={showCreate}
        onClose={() => setShowCreate(false)}
        onCreate={handleCreateScenario}
      />

      <RenameScenarioDialog
        open={renameTarget !== null}
        currentName={renameTarget?.name || ""}
        currentIcon={renameTarget?.icon}
        onClose={() => setRenameTarget(null)}
        onRename={handleRenameScenario}
      />

      <ConfirmDialog
        open={deleteTarget !== null}
        message={t("scenario.deleteConfirm", { name: deleteTarget?.name || "" })}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleDeleteScenario}
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
