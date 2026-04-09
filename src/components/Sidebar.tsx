import { useState, useEffect, useRef } from "react";
import { DragDropContext, Droppable, Draggable, type DropResult } from "@hello-pangea/dnd";
import { Link, useLocation, useNavigate } from "react-router-dom";
import {
  LayoutDashboard,
  Layers,
  Download,
  Settings,
  Plus,
  Pencil,
  Trash2,
  FolderOpen,
  GripVertical,
  Sparkles,
  Loader2,
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
import { getScenarioIconOption } from "../lib/scenarioIcons";

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
  const [aiCreating, setAiCreating] = useState(false);
  const [untaggedWarning, setUntaggedWarning] = useState(false);
  const [orderedScenarios, setOrderedScenarios] = useState(scenarios);
  const [orderedProjects, setOrderedProjects] = useState(projects);
  const scenarioReorderQueueRef = useRef<Promise<void>>(Promise.resolve());
  const projectReorderQueueRef = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => { setOrderedScenarios(scenarios); }, [scenarios]);
  useEffect(() => { setOrderedProjects(projects); }, [projects]);

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
    { name: t("sidebar.installSkills"), path: "/install", icon: Download },
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

  const handleAiCreateScenario = async () => {
    const apiKeyCheck = await api.getSettings("codebuddy_api_key");
    if (!apiKeyCheck) {
      toast.error(t("mySkills.aiTaggingNoApiKey"));
      return;
    }

    // Check untagged ratio — warn if > 50%
    const allSkills = await api.getManagedSkills();
    const untaggedCount = allSkills.filter((s) => !s.tags || s.tags.length === 0).length;
    if (allSkills.length > 0 && untaggedCount / allSkills.length > 0.5) {
      setUntaggedWarning(true);
      return;
    }

    await doAiCreateScenario();
  };

  const doAiCreateScenario = async () => {
    setAiCreating(true);
    try {
      const skills = await api.getManagedSkills();
      const skillList = skills.map((s) => ({
        name: s.name,
        description: s.description || "",
        tags: s.tags || [],
      }));
      const result = await api.invokeCodebuddyAgent("create_scenario", {
        skills: skillList,
        existingScenarios: scenarios.map((s) => s.name),
      });
      if (result.scenarios && result.scenarios.length > 0) {
        for (const suggestion of result.scenarios) {
          const created = await api.createScenario(
            suggestion.name,
            suggestion.description,
            suggestion.icon
          );
          for (const skillName of suggestion.skillNames) {
            const matchedSkill = skills.find((s) => s.name === skillName);
            if (matchedSkill) {
              await api.addSkillToScenario(matchedSkill.id, created.id);
            }
          }
        }
        await Promise.all([refreshScenarios(), refreshManagedSkills()]);
        toast.success(t("scenario.aiCreateScenarioSuccess"));
      } else {
        toast.info(t("scenario.aiCreateScenarioEmpty"));
      }
    } catch (error: unknown) {
      const msg = error instanceof Error ? error.message : t("scenario.aiCreateScenarioError");
      toast.error(msg);
    } finally {
      setAiCreating(false);
    }
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

        {/* Scenarios */}
        <div className="px-2.5 flex-1 overflow-y-auto scrollbar-hide min-h-0">
          <div className="text-[13px] font-semibold text-muted mb-1.5 px-2.5 tracking-[0.1em] uppercase">
            {t("sidebar.scenarios")}
          </div>
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
                                "flex min-w-0 flex-1 items-center gap-2 px-2.5 py-[7px] text-left text-sm outline-none",
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
                              {scenario.skill_count > 0 && (
                                <span
                                  className={cn(
                                    "shrink-0 rounded-full px-1.5 text-[13px] font-medium leading-[18px] group-hover:hidden",
                                    isActive
                                      ? "bg-accent-bg text-accent-light"
                                      : "bg-surface-hover text-muted"
                                  )}
                                >
                                  {scenario.skill_count}
                                </span>
                              )}
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

          <div className="flex items-center gap-0.5 mt-0.5">
            <button
              onClick={() => setShowCreate(true)}
              className="flex items-center gap-2 px-2.5 py-[7px] rounded-[5px] text-[13px] text-muted hover:text-secondary hover:bg-surface-hover transition-colors flex-1 outline-none"
            >
              <Plus className="w-3.5 h-3.5" />
              {t("sidebar.newScenario")}
            </button>
            <button
              onClick={handleAiCreateScenario}
              disabled={aiCreating}
              className="rounded p-1 text-accent-light/70 hover:text-accent-light hover:bg-accent-bg/50 transition-colors disabled:opacity-50"
              title={t("scenario.aiCreateScenario")}
            >
              {aiCreating ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Sparkles className="h-3.5 w-3.5" />
              )}
            </button>
          </div>

          {/* Divider */}
          <div className="mx-0.5 mt-3.5 mb-2.5 border-t border-border-subtle" />

          {/* Projects */}
          <div className="text-[13px] font-semibold text-muted mb-1.5 px-2.5 tracking-[0.1em] uppercase">
            {t("sidebar.projects")}
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
                                "flex min-w-0 flex-1 items-center gap-2 px-2.5 py-[7px] text-left text-sm outline-none",
                                isActive ? "font-medium text-primary" : "text-tertiary group-hover:text-secondary"
                              )}
                            >
                              <span
                                className={cn(
                                  "flex h-[20px] w-[20px] shrink-0 items-center justify-center rounded border",
                                  isActive
                                    ? "border-blue-500/30 bg-blue-500/10 text-blue-500"
                                    : "border-border bg-surface text-muted group-hover:border-border group-hover:text-tertiary"
                                )}
                              >
                                <FolderOpen className="h-3 w-3" />
                              </span>
                              <span className="flex-1 truncate">{project.name}</span>
                              {project.skill_count > 0 && (
                                <span
                                  className={cn(
                                    "shrink-0 rounded-full px-1.5 text-[13px] font-medium leading-[18px] group-hover:hidden",
                                    isActive
                                      ? "bg-accent-bg text-accent-light"
                                      : "bg-surface-hover text-muted"
                                  )}
                                >
                                  {project.skill_count}
                                </span>
                              )}
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
            className="flex items-center gap-2 px-2.5 py-[7px] mt-0.5 rounded-[5px] text-[13px] text-muted hover:text-secondary hover:bg-surface-hover transition-colors w-full outline-none"
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
          toast.success(t("project.added"));
        }}
      />

      <ConfirmDialog
        open={deleteProjectTarget !== null}
        message={t("project.removeConfirm", { name: deleteProjectTarget?.name || "" })}
        onClose={() => setDeleteProjectTarget(null)}
        onConfirm={handleDeleteProject}
      />

      <ConfirmDialog
        open={untaggedWarning}
        tone="warning"
        title={t("scenario.untaggedWarningTitle")}
        message={t("scenario.untaggedWarningMessage")}
        cancelLabel={t("scenario.goTagFirst")}
        confirmLabel={t("scenario.continueAnyway")}
        onClose={() => {
          setUntaggedWarning(false);
          navigate("/my-skills");
        }}
        onConfirm={async () => {
          setUntaggedWarning(false);
          await doAiCreateScenario();
        }}
      />
    </>
  );
}
