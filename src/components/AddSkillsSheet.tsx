import { useCallback, useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  CircleSlash,
  Loader2,
  Minus,
  Search,
  X,
} from "lucide-react";
import { cn } from "../utils";
import * as api from "../lib/tauri";
import type { ManagedSkill, ProjectAgentTarget } from "../lib/tauri";
import { getErrorMessage } from "../lib/error";
import {
  classifySkill,
  targetsToInstall,
  type PickerContext,
  type ProjectPickerContext,
} from "../lib/skillPickerStatus";
import {
  getTagActiveColor,
  getTagColor,
  UNTAGGED_FILTER,
} from "../lib/skillTags";
import { AgentIcon } from "./AgentIcon";
import { SkillPickerRow } from "./SkillPickerRow";

const SOURCE_PRIORITY = ["local", "import", "git", "skillssh"];
const VISIBLE_TARGET_ICON_LIMIT = 5;

export interface GlobalSheetTarget {
  kind: "global";
  agentKey: string;
  agentDisplayName: string;
  installedSkillIds: Set<string>;
}

export interface ProjectSheetTarget {
  kind: "project";
  projectId: string;
  projectName: string;
  exportTargets: ProjectAgentTarget[];
  /** dir/relative_path names already used in the project, keyed by agent */
  projectSkillDirNamesByAgent: Record<string, string[]>;
  /** managed skill ids already installed in the project, keyed by agent */
  projectCenterSkillIdsByAgent: Record<string, string[]>;
  /** Initial target agent selection (precomputed by caller using last-used > default > empty). */
  initialSelectedAgents: string[];
  /** Persist this selection as the per-project last-used set. */
  onPersistLastUsed: (agents: string[]) => void;
}

interface Props {
  open: boolean;
  onClose: () => void;
  target: GlobalSheetTarget | ProjectSheetTarget;
  managedSkills: ManagedSkill[];
  /** Called after one or more skills successfully installed. */
  onInstalled: () => Promise<void> | void;
}

export function AddSkillsSheet(props: Props) {
  if (!props.open) return null;
  return createPortal(<AddSkillsSheetBody {...props} />, document.body);
}

function AddSkillsSheetBody({ onClose, target, managedSkills, onInstalled }: Props) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [tagFilters, setTagFilters] = useState<Set<string>>(new Set());
  const [sourceFilters, setSourceFilters] = useState<Set<string>>(new Set());
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [installing, setInstalling] = useState(false);

  const initialAgents = target.kind === "project" ? target.initialSelectedAgents : [];
  const [selectedAgents, setSelectedAgents] = useState<string[]>(initialAgents);
  const [showInactiveAgents, setShowInactiveAgents] = useState(false);

  const [dirNameMap, setDirNameMap] = useState<Record<string, string>>({});
  const [dirNameMapError, setDirNameMapError] = useState(false);
  const [dirNameMapLoading, setDirNameMapLoading] = useState(target.kind === "project");

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !installing) onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [installing, onClose]);

  // For project mode: precompute slugified dir names for managed skills
  useEffect(() => {
    if (target.kind !== "project") return;
    let cancelled = false;
    const load = async () => {
      const names = managedSkills.map((s) => s.name);
      if (names.length === 0) {
        if (!cancelled) {
          setDirNameMap({});
          setDirNameMapError(false);
          setDirNameMapLoading(false);
        }
        return;
      }
      setDirNameMapLoading(true);
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
      } finally {
        if (!cancelled) setDirNameMapLoading(false);
      }
    };
    load();
    return () => {
      cancelled = true;
    };
  }, [managedSkills, target.kind]);

  const ctx: PickerContext = useMemo(() => {
    if (target.kind === "global") {
      return {
        kind: "global",
        installedSkillIds: target.installedSkillIds,
      };
    }
    return {
      kind: "project",
      selectedAgents,
      projectSkillDirNamesByAgent: target.projectSkillDirNamesByAgent,
      projectCenterSkillIdsByAgent: target.projectCenterSkillIdsByAgent,
      dirNameMap,
      dirNameMapError,
    };
  }, [target, selectedAgents, dirNameMap, dirNameMapError]);

  const allTags = useMemo(() => {
    const tags = new Set<string>();
    for (const skill of managedSkills) {
      for (const tag of skill.tags) {
        if (tag.trim()) tags.add(tag);
      }
    }
    return Array.from(tags).sort((a, b) => a.localeCompare(b));
  }, [managedSkills]);

  const sourceTypes = useMemo(() => {
    const present = new Set(managedSkills.map((s) => s.source_type).filter(Boolean));
    return [
      ...SOURCE_PRIORITY.filter((s) => present.has(s)),
      ...Array.from(present).filter((s) => !SOURCE_PRIORITY.includes(s)).sort(),
    ];
  }, [managedSkills]);

  const sourceLabel = useCallback(
    (source: string) => {
      if (SOURCE_PRIORITY.includes(source)) {
        return t(`mySkills.sourceFilter.${source}`);
      }
      return source;
    },
    [t],
  );

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    const hasUntagged = tagFilters.has(UNTAGGED_FILTER);
    const tagSelected = tagFilters.size > 0;
    return managedSkills.filter((skill) => {
      if (q) {
        const matches =
          skill.name.toLowerCase().includes(q) ||
          (skill.description || "").toLowerCase().includes(q);
        if (!matches) return false;
      }
      if (sourceFilters.size > 0 && !sourceFilters.has(skill.source_type)) return false;
      if (tagSelected) {
        const matchUntagged = hasUntagged && skill.tags.length === 0;
        const matchTag = skill.tags.some((tag) => tagFilters.has(tag));
        if (!matchUntagged && !matchTag) return false;
      }
      return true;
    });
  }, [managedSkills, search, sourceFilters, tagFilters]);

  // Sort: available first, then installed/conflict/unavailable (greyed out at bottom)
  const ordered = useMemo(() => {
    const statusOrder = { available: 0, conflict: 1, installed: 2, unavailable: 3 } as const;
    return [...filtered].sort((a, b) => {
      const sa = classifySkill(a, ctx);
      const sb = classifySkill(b, ctx);
      if (sa !== sb) return statusOrder[sa] - statusOrder[sb];
      return a.name.localeCompare(b.name);
    });
  }, [filtered, ctx]);

  const visibleSelectableIds = useMemo(
    () =>
      ordered
        .filter((skill) => classifySkill(skill, ctx) === "available")
        .map((skill) => skill.id),
    [ordered, ctx],
  );

  const anySelected = selectedIds.size > 0;
  const allVisibleSelected =
    visibleSelectableIds.length > 0 &&
    visibleSelectableIds.every((id) => selectedIds.has(id));
  const indeterminate = anySelected && !allVisibleSelected;

  const toggleSelectAll = () => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (allVisibleSelected || (visibleSelectableIds.length === 0 && next.size > 0)) {
        if (visibleSelectableIds.length > 0) {
          for (const id of visibleSelectableIds) next.delete(id);
        } else {
          next.clear();
        }
      } else if (visibleSelectableIds.length > 0) {
        for (const id of visibleSelectableIds) next.add(id);
      }
      return next;
    });
  };

  const selectAllLabel =
    allVisibleSelected || (visibleSelectableIds.length === 0 && anySelected)
      ? t("addFromLibrary.deselectAllSkills")
      : t("addFromLibrary.selectAllSkills");

  const skillsHaveUntagged = useMemo(
    () => managedSkills.some((s) => s.tags.length === 0),
    [managedSkills],
  );

  const toggleSelect = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleSourceFilter = (source: string) => {
    setSourceFilters((prev) => {
      const next = new Set(prev);
      if (next.has(source)) next.delete(source);
      else next.add(source);
      return next;
    });
  };

  const toggleTagFilter = (tag: string) => {
    setTagFilters((prev) => {
      const next = new Set(prev);
      if (next.has(tag)) next.delete(tag);
      else next.add(tag);
      return next;
    });
  };

  const toggleAgent = (key: string) => {
    setSelectedAgents((prev) => {
      const next = prev.includes(key) ? prev.filter((k) => k !== key) : [...prev, key];
      if (target.kind === "project") {
        target.onPersistLastUsed(next);
      }
      return next;
    });
  };

  const setAllEnabledAgents = (next: string[]) => {
    setSelectedAgents(next);
    if (target.kind === "project") {
      target.onPersistLastUsed(next);
    }
  };

  const selectableSelected = useMemo(
    () => Array.from(selectedIds).filter((id) => {
      const skill = managedSkills.find((s) => s.id === id);
      if (!skill) return false;
      return classifySkill(skill, ctx) === "available";
    }),
    [selectedIds, managedSkills, ctx],
  );

  const projectCtx = ctx.kind === "project" ? (ctx as ProjectPickerContext) : null;
  const projectNamesReady = target.kind !== "project" || dirNameMapError || !dirNameMapLoading;
  const enabledTargets = target.kind === "project"
    ? target.exportTargets.filter((tt) => tt.installed && tt.enabled)
    : [];
  const inactiveTargets = target.kind === "project"
    ? target.exportTargets.filter((tt) => !tt.installed || !tt.enabled)
    : [];

  const renderAgentIcons = (
    agents: { key: string; display_name: string }[],
    options: { dim?: string; limit?: number } = {},
  ) => {
    const dim = options.dim ?? "h-6 w-6";
    const limit = options.limit ?? VISIBLE_TARGET_ICON_LIMIT;
    const visible = agents.slice(0, limit);
    const hiddenCount = agents.length - visible.length;

    return (
      <span className="flex shrink-0 items-center -space-x-1.5">
        {visible.map((agent) => (
          <AgentIcon
            key={agent.key}
            agentKey={agent.key}
            displayName={agent.display_name}
            className={cn(
              dim,
              "rounded-[4px] border border-bg-secondary bg-surface shadow-[0_0_0_1px_var(--color-border-subtle)]",
            )}
          />
        ))}
        {hiddenCount > 0 && (
          <span
            className={cn(
              dim,
              "inline-flex items-center justify-center rounded-[4px] border border-bg-secondary bg-surface text-[10px] font-semibold text-muted shadow-[0_0_0_1px_var(--color-border-subtle)]",
            )}
            title={`+${hiddenCount}`}
          >
            +{hiddenCount}
          </span>
        )}
      </span>
    );
  };

  const ctaLabel = (() => {
    const count = selectableSelected.length;
    if (target.kind === "global") {
      return count === 0
        ? t("addFromLibrary.ctaEmpty", { agent: target.agentDisplayName })
        : t("addFromLibrary.ctaGlobal", { count, agent: target.agentDisplayName });
    }
    if (selectedAgents.length === 0) {
      return t("addFromLibrary.ctaNoTarget");
    }
    return count === 0
      ? t("addFromLibrary.ctaEmptyProject", { count: selectedAgents.length })
      : t("addFromLibrary.ctaProject", { count, agentCount: selectedAgents.length });
  })();

  const handleInstall = async () => {
    if (selectableSelected.length === 0) return;
    setInstalling(true);
    let ok = 0;
    let failed = 0;
    const failures: string[] = [];
    try {
      if (target.kind === "global") {
        for (const id of selectableSelected) {
          try {
            await api.syncSkillToTool(id, target.agentKey);
            ok++;
          } catch (e) {
            failed++;
            failures.push(getErrorMessage(e, t("common.error")));
          }
        }
      } else {
        if (selectedAgents.length === 0) {
          toast.error(t("addFromLibrary.errors.noTarget"));
          setInstalling(false);
          return;
        }
        if (!projectCtx || !projectNamesReady) return;
        for (const id of selectableSelected) {
          try {
            const skill = managedSkills.find((s) => s.id === id);
            if (!skill) continue;
            const agents = targetsToInstall(skill, projectCtx);
            if (agents.length === 0) continue;
            await api.exportSkillToProject(id, target.projectId, agents);
            ok++;
          } catch (e) {
            failed++;
            failures.push(getErrorMessage(e, t("common.error")));
          }
        }
      }
      if (ok > 0) {
        toast.success(t("addFromLibrary.toastInstalled", { count: ok }));
        setSelectedIds(new Set());
      }
      if (failed > 0) {
        // Surface why. A refusal carries the path it protected and what to do
        // about it (#363); collapsing that to a bare count leaves the user with
        // no idea which skill failed or how to resolve it.
        const detail = failures[0];
        toast.error(
          failed === 1 && detail
            ? detail
            : [t("addFromLibrary.toastFailed", { count: failed }), detail]
                .filter(Boolean)
                .join(" — "),
        );
      }
      await onInstalled();
      if (failed === 0) onClose();
    } catch (e) {
      toast.error(getErrorMessage(e, t("common.error")));
    } finally {
      setInstalling(false);
    }
  };

  const targetSummary = (() => {
    if (target.kind === "global") {
      return (
        <div className="flex items-center gap-2 text-[12px] text-muted">
          <span className="shrink-0">{t("addFromLibrary.targetLabel")}</span>
          <span className="inline-flex min-w-0 items-center gap-2 rounded-full border border-border-subtle bg-surface px-2.5 py-1 text-[12px] font-medium text-secondary">
            {renderAgentIcons([{ key: target.agentKey, display_name: target.agentDisplayName }], {
              dim: "h-5 w-5",
              limit: 1,
            })}
            <span className="truncate">{target.agentDisplayName}</span>
          </span>
        </div>
      );
    }

    const allSelected =
      enabledTargets.length > 0 && enabledTargets.every((tt) => selectedAgents.includes(tt.key));
    const toggleAll = () => {
      if (allSelected) {
        setAllEnabledAgents([]);
      } else {
        setAllEnabledAgents(enabledTargets.map((tt) => tt.key));
      }
    };

    return (
      <div className="space-y-2">
        <div className="flex items-start gap-2 text-[12px]">
          <span className="shrink-0 pt-1 text-muted">{t("addFromLibrary.targetLabel")}</span>
          <div className="min-w-0 flex-1">
            {enabledTargets.length === 0 ? (
              <span className="inline-flex items-center rounded-full border border-dashed border-border px-2.5 py-1 italic text-muted">
                {t("addFromLibrary.noTargetSelected")}
              </span>
            ) : (
              <div className="flex flex-wrap items-center gap-1.5">
                {enabledTargets.map((tt) => {
                  const active = selectedAgents.includes(tt.key);
                  return (
                    <button
                      key={tt.key}
                      type="button"
                      onClick={() => toggleAgent(tt.key)}
                      aria-pressed={active}
                      title={tt.display_name}
                      className={cn(
                        "inline-flex items-center gap-1.5 rounded-full border py-1 pl-1 pr-2.5 text-[12px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent",
                        active
                          ? "border-accent-border bg-accent-bg text-accent-light"
                          : "border-border-subtle bg-surface text-muted hover:bg-surface-hover hover:text-secondary",
                      )}
                    >
                      <span className="relative inline-flex">
                        <AgentIcon
                          agentKey={tt.key}
                          displayName={tt.display_name}
                          className={cn(
                            "h-5 w-5 rounded-full border-0 bg-transparent",
                            !active && "opacity-60",
                          )}
                        />
                        {active && (
                          <span className="absolute -right-0.5 -top-0.5 inline-flex h-3 w-3 items-center justify-center rounded-full bg-accent text-white">
                            <CheckCircle2 className="h-2.5 w-2.5" strokeWidth={3} />
                          </span>
                        )}
                      </span>
                      <span>{tt.display_name}</span>
                    </button>
                  );
                })}
              </div>
            )}
          </div>
          {enabledTargets.length > 0 && (
            <button
              type="button"
              onClick={toggleAll}
              className="shrink-0 pt-1 text-[12px] text-accent-light transition-colors hover:underline"
            >
              {allSelected ? t("addFromLibrary.clearTargets") : t("addFromLibrary.selectAllTargets")}
            </button>
          )}
        </div>
        {inactiveTargets.length > 0 && (
          <div className="ml-12">
            <button
              type="button"
              onClick={() => setShowInactiveAgents((prev) => !prev)}
              className="inline-flex items-center gap-1 text-[12px] text-muted transition-colors hover:text-secondary"
            >
              {showInactiveAgents ? (
                <ChevronDown className="h-3 w-3" />
              ) : (
                <ChevronRight className="h-3 w-3" />
              )}
              <span>{t("project.moreAgents", { count: inactiveTargets.length })}</span>
            </button>
            {showInactiveAgents && (
              <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
                {inactiveTargets.map((tt) => (
                  <span
                    key={tt.key}
                    title={t("addFromLibrary.tooltip.unavailable")}
                    className="inline-flex cursor-not-allowed items-center gap-1.5 rounded-full border border-border-subtle bg-background py-1 pl-1 pr-2.5 text-[12px] font-medium text-muted opacity-55"
                  >
                    <AgentIcon
                      agentKey={tt.key}
                      displayName={tt.display_name}
                      className="h-5 w-5 rounded-full border-0 bg-transparent"
                    />
                    <span>{tt.display_name}</span>
                  </span>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    );
  })();

  return (
    <div className="fixed inset-0 z-50">
      <div
        className="absolute inset-0 bg-black/40 backdrop-blur-[1px]"
        onClick={() => !installing && onClose()}
      />
      <div className="absolute right-0 top-0 flex h-full w-full max-w-[480px] flex-col overflow-hidden border-l border-border-subtle bg-bg-secondary shadow-2xl">
        <div className="flex shrink-0 items-start justify-between gap-3 border-b border-border-subtle px-5 py-4">
          <div className="min-w-0 flex-1">
            <h2 className="text-[14px] font-semibold text-primary">
              {t("addFromLibrary.title")}
            </h2>
            <div className="mt-2">{targetSummary}</div>
          </div>
          <button
            onClick={onClose}
            disabled={installing}
            className="shrink-0 rounded-md p-1.5 text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="shrink-0 border-b border-border-subtle px-5 py-3">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("addFromLibrary.searchPlaceholder")}
              className="app-input w-full pl-9"
              autoFocus
            />
          </div>

          {(allTags.length > 0 || skillsHaveUntagged) && (
            <div className="mt-2 flex flex-wrap items-center gap-1.5">
              <span className="text-[12px] text-muted">{t("mySkills.tags.filter")}</span>
              <button
                onClick={() => setTagFilters(new Set())}
                className={cn(
                  "rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                  tagFilters.size === 0
                    ? "bg-accent text-white dark:bg-accent dark:text-white"
                    : "bg-surface-hover text-muted hover:text-secondary",
                )}
              >
                {t("mySkills.tags.allTags")}
              </button>
              {skillsHaveUntagged && (
                <button
                  onClick={() => toggleTagFilter(UNTAGGED_FILTER)}
                  className={cn(
                    "inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                    tagFilters.has(UNTAGGED_FILTER)
                      ? "bg-surface-active text-primary"
                      : "border border-dashed border-border text-muted hover:text-secondary",
                  )}
                  title={t("mySkills.tags.untagged")}
                >
                  <CircleSlash className="h-3 w-3" />
                  {t("mySkills.tags.untagged")}
                </button>
              )}
              {allTags.map((tag) => {
                const active = tagFilters.has(tag);
                return (
                  <button
                    key={tag}
                    onClick={() => toggleTagFilter(tag)}
                    className={cn(
                      "rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                      active ? getTagActiveColor(tag, allTags) : getTagColor(tag, allTags),
                    )}
                  >
                    {tag}
                  </button>
                );
              })}
            </div>
          )}

          {sourceTypes.length > 1 && (
            <div className="mt-2 flex flex-wrap items-center gap-1.5">
              <span className="text-[12px] text-muted">{t("mySkills.sourceType")}</span>
              {sourceTypes.map((source) => {
                const active = sourceFilters.has(source);
                return (
                  <button
                    key={source}
                    onClick={() => toggleSourceFilter(source)}
                    className={cn(
                      "rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                      active
                        ? "bg-accent text-white dark:bg-accent dark:text-white"
                        : "bg-surface-hover text-muted hover:text-secondary",
                    )}
                  >
                    {sourceLabel(source)}
                  </button>
                );
              })}
            </div>
          )}
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto scrollbar-hide">
          {(ordered.length > 0 || anySelected) && (
            <div className="sticky top-0 z-10 flex items-center border-b border-border-subtle bg-bg-secondary px-5 py-2">
              <button
                type="button"
                onClick={toggleSelectAll}
                disabled={visibleSelectableIds.length === 0 && !anySelected}
                aria-pressed={allVisibleSelected}
                className="inline-flex items-center gap-3 rounded-md text-[12px] font-medium text-accent-light transition-colors hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:cursor-not-allowed disabled:text-muted disabled:hover:no-underline"
              >
                <span
                  className={cn(
                    "flex h-4 w-4 shrink-0 items-center justify-center rounded border transition-colors",
                    indeterminate || allVisibleSelected
                      ? "border-accent bg-accent text-white"
                      : "border-border",
                  )}
                >
                  {allVisibleSelected ? (
                    <svg viewBox="0 0 16 16" fill="currentColor" className="h-3 w-3">
                      <path d="M13.854 3.646a.5.5 0 0 1 0 .708l-7 7a.5.5 0 0 1-.708 0l-3.5-3.5a.5.5 0 1 1 .708-.708L6.5 10.293l6.646-6.647a.5.5 0 0 1 .708 0z" />
                    </svg>
                  ) : indeterminate ? (
                    <Minus className="h-3 w-3" strokeWidth={3} />
                  ) : null}
                </span>
                {selectAllLabel}
              </button>
              {anySelected && (
                <span className="ml-auto text-[12px] text-muted">
                  {t("addFromLibrary.selectedCount", { count: selectedIds.size })}
                </span>
              )}
            </div>
          )}
          {ordered.length === 0 ? (
            <div className="px-5 py-12 text-center text-[13px] text-muted">
              {managedSkills.length === 0
                ? t("addFromLibrary.emptyLibrary")
                : t("addFromLibrary.emptyMatch")}
            </div>
          ) : (
            <div className="divide-y divide-border-subtle">
              {ordered.map((skill) => {
                const status = classifySkill(skill, ctx);
                return (
                  <SkillPickerRow
                    key={skill.id}
                    skill={skill}
                    status={status}
                    allTags={allTags}
                    sourceLabel={sourceLabel(skill.source_type)}
                    selected={selectedIds.has(skill.id)}
                    onToggle={() => toggleSelect(skill.id)}
                  />
                );
              })}
            </div>
          )}
        </div>

        <div className="shrink-0 border-t border-border-subtle bg-bg-secondary px-5 py-3">
          <button
            onClick={handleInstall}
            disabled={
              installing ||
              !projectNamesReady ||
              selectableSelected.length === 0 ||
              (target.kind === "project" && selectedAgents.length === 0)
            }
            className="inline-flex w-full items-center justify-center gap-1.5 rounded-md bg-accent px-3 py-2.5 text-[13px] font-medium text-white transition-colors hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-50"
          >
            {installing ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
            {ctaLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
