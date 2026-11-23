import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { FolderOpen, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import * as api from "../lib/tauri";
import type { ManagedSkill, Project, ProjectAgentTarget } from "../lib/tauri";
import { cn } from "../utils";
import { getErrorMessage } from "../lib/error";
import { AgentIcon } from "./AgentIcon";
import { ConfirmDialog } from "./ConfirmDialog";

interface Props {
  skill: ManagedSkill;
  projects: Project[];
  onChanged?: () => void;
}

type RowState = "loading" | "installed" | "available" | "error";

interface RowData {
  state: RowState;
  installedAgents: string[];
  installedPathByAgent: Record<string, string>;
  dirNamesByAgent: Record<string, string[]>;
  targets: ProjectAgentTarget[];
  dirName?: string;
  error?: string;
}

export function SkillProjectsSection({ skill, projects, onChanged }: Props) {
  const { t } = useTranslation();
  const [rows, setRows] = useState<Record<string, RowData>>({});
  const [pendingKey, setPendingKey] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  // The "Added" chip reads like a status, but clicking it deletes the
  // workspace folder outright (#542), so ask first.
  const [removeTarget, setRemoveTarget] = useState<{
    project: Project;
    target: ProjectAgentTarget;
  } | null>(null);

  useEffect(() => {
    let cancelled = false;
    setRows((prev) => {
      const next: Record<string, RowData> = {};
      for (const p of projects) {
        next[p.id] = prev[p.id] ?? {
          state: "loading",
          installedAgents: [],
          installedPathByAgent: {},
          dirNamesByAgent: {},
          targets: [],
        };
      }
      return next;
    });

    const loadAll = async () => {
      const results = await Promise.all(
        projects.map(async (p) => {
          try {
            const [projectSkills, targets, dirNames] = await Promise.all([
              api.getProjectSkills(p.id),
              api.getProjectAgentTargets(p.id),
              api.slugifySkillNames([skill.name]),
            ]);
            const installedPathByAgent: Record<string, string> = {};
            for (const projectSkill of projectSkills) {
              if (projectSkill.center_skill_id === skill.id) {
                installedPathByAgent[projectSkill.agent] = projectSkill.relative_path;
              }
            }
            const installedAgents = Object.keys(installedPathByAgent);
            const dirNamesByAgent: Record<string, string[]> = {};
            for (const projectSkill of projectSkills) {
              if (!dirNamesByAgent[projectSkill.agent]) {
                dirNamesByAgent[projectSkill.agent] = [];
              }
              dirNamesByAgent[projectSkill.agent].push(projectSkill.relative_path.toLowerCase());
            }
            return [p.id, {
              state: installedAgents.length > 0 ? "installed" : "available",
              installedAgents: Array.from(new Set(installedAgents)),
              installedPathByAgent,
              dirNamesByAgent,
              targets,
              dirName: dirNames[0]?.toLowerCase(),
            }] as const;
          } catch (e) {
            return [p.id, {
              state: "error" as const,
              installedAgents: [],
              installedPathByAgent: {},
              dirNamesByAgent: {},
              targets: [],
              error: getErrorMessage(e, ""),
            }] as const;
          }
        }),
      );
      if (cancelled) return;
      setRows(Object.fromEntries(results));
    };
    void loadAll();
    return () => {
      cancelled = true;
    };
  }, [projects, skill.id, skill.name]);

  const installedCount = useMemo(
    () => Object.values(rows).filter((r) => r.state === "installed").length,
    [rows],
  );

  const getAgentState = (row: RowData | undefined, target: ProjectAgentTarget) => {
    if (!row || row.state === "loading") return "loading";
    if (row.state === "error") return "error";
    if (!target.installed || !target.enabled) return "unavailable";
    if (row.installedAgents.includes(target.key)) return "installed";
    if (row.dirName && (row.dirNamesByAgent[target.key] ?? []).includes(row.dirName)) {
      return "conflict";
    }
    return "available";
  };

  const handleAdd = async (project: Project, target: ProjectAgentTarget) => {
    const row = rows[project.id];
    if (!row || getAgentState(row, target) !== "available") return;
    if (!target.installed || !target.enabled) {
      toast.error(t("addFromLibrary.errors.noTarget"));
      return;
    }
    const key = `${project.id}:${target.key}`;
    setPendingKey(key);
    try {
      await api.exportSkillToProject(skill.id, project.id, [target.key]);
      toast.success(
        t("addFromLibrary.toastAddedToProject", {
          skill: skill.name,
          project: project.name,
        }),
      );
      setRows((prev) => ({
        ...prev,
        [project.id]: {
          ...row,
          state: "installed",
          installedAgents: Array.from(new Set([...row.installedAgents, target.key])),
          installedPathByAgent: {
            ...row.installedPathByAgent,
            [target.key]: row.dirName ?? skill.name,
          },
        },
      }));
      onChanged?.();
    } catch (e) {
      toast.error(getErrorMessage(e, t("common.error")));
    } finally {
      setPendingKey(null);
    }
  };

  const handleRemove = async (project: Project, target: ProjectAgentTarget) => {
    const row = rows[project.id];
    const relativePath = row?.installedPathByAgent[target.key];
    if (!row || !relativePath) return;
    const key = `${project.id}:${target.key}`;
    setPendingKey(key);
    try {
      await api.deleteProjectSkill(project.id, relativePath, target.key);
      toast.success(
        t("addFromLibrary.toastRemovedFromProject", {
          skill: skill.name,
          project: project.name,
        }),
      );
      const nextInstalledAgents = row.installedAgents.filter((agent) => agent !== target.key);
      const nextPathByAgent = { ...row.installedPathByAgent };
      delete nextPathByAgent[target.key];
      const removedDirName = relativePath.toLowerCase();
      const nextDirNamesByAgent = { ...row.dirNamesByAgent };
      nextDirNamesByAgent[target.key] = (nextDirNamesByAgent[target.key] ?? [])
        .filter((dirName) => dirName !== removedDirName);
      setRows((prev) => ({
        ...prev,
        [project.id]: {
          ...row,
          state: nextInstalledAgents.length > 0 ? "installed" : "available",
          installedAgents: nextInstalledAgents,
          installedPathByAgent: nextPathByAgent,
          dirNamesByAgent: nextDirNamesByAgent,
        },
      }));
      onChanged?.();
    } catch (e) {
      toast.error(getErrorMessage(e, t("common.error")));
    } finally {
      setPendingKey(null);
    }
  };

  if (projects.length === 0) return null;

  // Collapsed, the list holds only the projects that already have the skill;
  // "Add to project…" opens every project for adding.
  const anyLoading = projects.some((p) => !rows[p.id] || rows[p.id].state === "loading");
  const visibleProjects = expanded
    ? projects
    : projects.filter((p) => rows[p.id]?.state === "installed");

  return (
    <div>
      <h3 className="app-section-title truncate">{t("addFromLibrary.projectsTitle")}</h3>
      <div className="mt-1 text-[12px] text-muted tabular-nums">
        {t("addFromLibrary.projectsSummary", {
          installed: installedCount,
          total: projects.length,
        })}
      </div>
      {visibleProjects.length > 0 ? (
        <div className="mt-2">
          {visibleProjects.map((project) => {
            const row = rows[project.id];
            const activeTargets = row?.targets.filter((target) => target.installed && target.enabled) ?? [];
            return (
              <div key={project.id} className="flex h-8 min-w-0 items-center gap-2 text-[13px]">
                <FolderOpen className="h-3.5 w-3.5 shrink-0 text-muted" />
                <span className="min-w-0 flex-1 truncate text-secondary" title={project.name}>
                  {project.name}
                </span>
                {!row || row.state === "loading" ? (
                  <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-faint" />
                ) : row.state === "error" ? (
                  <span className="shrink-0 text-[12px] text-danger" title={row.error || t("common.error")}>
                    {t("common.error")}
                  </span>
                ) : activeTargets.length === 0 ? (
                  <span className="shrink-0 text-[12px] text-muted">{t("addFromLibrary.status.unavailable")}</span>
                ) : (
                  // Same language as the card's agent dots: opaque = added,
                  // faded = not added, click to change.
                  <div className="flex shrink-0 items-center">
                    {activeTargets.map((target) => {
                      const agentState = getAgentState(row, target);
                      const agentPending = pendingKey === `${project.id}:${target.key}`;
                      const title =
                        agentState === "conflict"
                          ? `${target.display_name} · ${t("addFromLibrary.tooltip.conflict")}`
                          : agentState === "installed"
                            ? `${target.display_name} · ${t("addFromLibrary.tooltip.remove")}`
                            : t("addFromLibrary.addToAgent", { agent: target.display_name });
                      const clickable = agentState === "available" || agentState === "installed";
                      return (
                        <button
                          key={target.key}
                          type="button"
                          title={title}
                          aria-label={title}
                          onClick={() => {
                            if (agentState === "installed") {
                              setRemoveTarget({ project, target });
                            } else {
                              void handleAdd(project, target);
                            }
                          }}
                          disabled={!clickable || agentPending}
                          className="flex h-6 w-6 items-center justify-center rounded-md transition-colors disabled:cursor-default enabled:hover:bg-surface-hover"
                        >
                          {agentPending ? (
                            <Loader2 className="h-3.5 w-3.5 animate-spin text-muted" />
                          ) : (
                            <AgentIcon
                              agentKey={target.key}
                              displayName={target.display_name}
                              className={cn(
                                "h-[18px] w-[18px] rounded-[4px]",
                                agentState === "available" && "opacity-45",
                                agentState === "conflict" && "ring-1 ring-danger",
                              )}
                            />
                          )}
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      ) : anyLoading ? (
        // Nothing installed is already said by the count above.
        <p className="mt-2 text-[12px] text-muted">{t("common.loading")}</p>
      ) : null}
      <button
        type="button"
        onClick={() => setExpanded((prev) => !prev)}
        aria-expanded={expanded}
        className="app-button-secondary mt-3 h-7 w-full px-2.5 py-0 text-[12px]"
      >
        {expanded ? t("common.collapse") : t("addFromLibrary.addToProject")}
      </button>
      {/* Portaled out of the DetailSheet so the sheet's Escape handler sees
          a covering dialog and leaves the key to it. */}
      {createPortal(
        <ConfirmDialog
          open={!!removeTarget}
          title={t("addFromLibrary.removeConfirmTitle")}
          message={t("addFromLibrary.removeConfirm", {
            skill: skill.name,
            project: removeTarget?.project.name,
            agent: removeTarget?.target.display_name,
          })}
          confirmLabel={t("common.delete")}
          tone="danger"
          onClose={() => setRemoveTarget(null)}
          onConfirm={async () => {
            if (removeTarget) await handleRemove(removeTarget.project, removeTarget.target);
          }}
        />,
        document.body,
      )}
    </div>
  );
}
