import { useCallback, useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import {
  ChevronRight,
  ChevronDown,
  Grid3X3,
  Loader2,
  Package,
  Check,
  X,
  ToggleLeft,
  ToggleRight,
  FileText,
  Info,
  ExternalLink,
} from "lucide-react";
import { toast } from "sonner";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import { getErrorMessage } from "../lib/error";
import * as api from "../lib/tauri";
import type {
  PackSkillRecord,
  ToolInfo,
  SkillToolToggle,
  AgentConfigDto,
} from "../lib/tauri";

// ── Types ──

interface SkillGroup {
  id: string; // pack.id or "__ungrouped__"
  name: string;
  icon: "pack" | "ungrouped";
  skills: PackSkillRecord[];
}

const UNGROUPED_ID = "__ungrouped__";

// ── Component ──

export function MatrixView() {
  const { activeScenario, tools, refreshManagedSkills } = useApp();

  const [groups, setGroups] = useState<SkillGroup[]>([]);
  const [loading, setLoading] = useState(true);
  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(new Set());
  const [togglingCell, setTogglingCell] = useState<string | null>(null);
  const [agentConfigs, setAgentConfigs] = useState<AgentConfigDto[]>([]);

  // Toggles loaded per skill (skill_id -> SkillToolToggle[])
  const [skillToggles, setSkillToggles] = useState<
    Record<string, SkillToolToggle[]>
  >({});

  // Load agent configs whenever the active scenario changes
  useEffect(() => {
    api.getAllAgentConfigs().then(setAgentConfigs).catch(() => {});
  }, [activeScenario]);

  // Agents with a custom scenario (different from the global active scenario)
  const agentsWithCustomConfig = useMemo(() => {
    if (!activeScenario) return [];
    return agentConfigs.filter(
      (a) => a.managed && a.scenario_id !== null && a.scenario_id !== activeScenario.id,
    );
  }, [agentConfigs, activeScenario]);

  // Only show installed + enabled tools as columns
  const columns = useMemo(
    () => tools.filter((t) => t.installed && t.enabled),
    [tools],
  );

  // All effective skill IDs (for toggle-all operations)
  const allSkillIds = useMemo(
    () => groups.flatMap((g) => g.skills.map((s) => s.id)),
    [groups],
  );

  // ── Data loading ──

  const loadData = useCallback(async () => {
    if (!activeScenario) {
      setGroups([]);
      setLoading(false);
      return;
    }

    setLoading(true);
    try {
      // Load effective skills and all packs in parallel
      const [effectiveSkills, allPacks] = await Promise.all([
        api.getEffectiveSkillsForScenario(activeScenario.id),
        api.getAllPacks(),
      ]);

      // Load skills for each pack
      const packSkillsMap = new Map<string, PackSkillRecord[]>();
      await Promise.all(
        allPacks.map(async (pack) => {
          const skills = await api.getSkillsForPack(pack.id);
          packSkillsMap.set(pack.id, skills);
        }),
      );

      // Build a set of effective skill IDs for quick lookup
      const effectiveIds = new Set(effectiveSkills.map((s) => s.id));

      // Track which effective skills are claimed by a pack
      const claimedIds = new Set<string>();

      // Build groups from packs (only include skills that are effective)
      const packGroups: SkillGroup[] = [];
      for (const pack of allPacks) {
        const packSkills = packSkillsMap.get(pack.id) ?? [];
        const effectivePackSkills = packSkills.filter((s) =>
          effectiveIds.has(s.id),
        );
        if (effectivePackSkills.length === 0) continue;

        for (const s of effectivePackSkills) {
          claimedIds.add(s.id);
        }

        packGroups.push({
          id: pack.id,
          name: pack.name,
          icon: "pack",
          skills: effectivePackSkills,
        });
      }

      // Build ungrouped group from effective skills not in any pack
      const ungroupedSkills = effectiveSkills.filter(
        (s) => !claimedIds.has(s.id),
      );

      const result: SkillGroup[] = [...packGroups];
      if (ungroupedSkills.length > 0) {
        result.push({
          id: UNGROUPED_ID,
          name: "Ungrouped",
          icon: "ungrouped",
          skills: ungroupedSkills,
        });
      }

      setGroups(result);
    } catch (error) {
      toast.error(getErrorMessage(error, "Failed to load matrix data"));
    } finally {
      setLoading(false);
    }
  }, [activeScenario]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  // Load toggles for ALL effective skills upfront when groups change
  const loadAllToggles = useCallback(async () => {
    if (!activeScenario || groups.length === 0) return;

    const allIds = groups.flatMap((g) => g.skills.map((s) => s.id));
    if (allIds.length === 0) return;

    const results: Record<string, SkillToolToggle[]> = {};
    await Promise.all(
      allIds.map(async (skillId) => {
        try {
          const toggles = await api.getSkillToolToggles(
            skillId,
            activeScenario.id,
          );
          results[skillId] = toggles;
        } catch {
          // skill may have been removed between load and toggle fetch
        }
      }),
    );
    setSkillToggles(results);
  }, [activeScenario, groups]);

  useEffect(() => {
    loadAllToggles();
  }, [loadAllToggles]);

  // Reload toggles for specific skills (used after mutations)
  const reloadSkillToggles = useCallback(
    async (skillIds: string[]) => {
      if (!activeScenario) return;
      const results: Record<string, SkillToolToggle[]> = {};
      await Promise.all(
        skillIds.map(async (skillId) => {
          try {
            const toggles = await api.getSkillToolToggles(
              skillId,
              activeScenario.id,
            );
            results[skillId] = toggles;
          } catch {
            // ignore
          }
        }),
      );
      setSkillToggles((prev) => ({ ...prev, ...results }));
    },
    [activeScenario],
  );

  // ── Helpers ──

  const toggleGroupExpand = (groupId: string) => {
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(groupId)) next.delete(groupId);
      else next.add(groupId);
      return next;
    });
  };

  /** Calculate group-level status for a tool column: all/some/none enabled */
  const getGroupToolStatus = (
    groupSkills: PackSkillRecord[],
    toolKey: string,
  ): "all" | "some" | "none" => {
    let enabled = 0;
    let total = 0;
    for (const skill of groupSkills) {
      const toggles = skillToggles[skill.id];
      if (!toggles) continue;
      const toggle = toggles.find((t) => t.tool === toolKey);
      if (toggle) {
        total++;
        if (toggle.enabled) enabled++;
      }
    }
    if (total === 0) return "none";
    if (enabled === total) return "all";
    if (enabled > 0) return "some";
    return "none";
  };

  const handleToggleSkillTool = async (
    skillId: string,
    toolKey: string,
    enabled: boolean,
  ) => {
    if (!activeScenario) return;
    const cellKey = `${skillId}-${toolKey}`;
    setTogglingCell(cellKey);
    try {
      await api.setSkillToolToggle(
        skillId,
        activeScenario.id,
        toolKey,
        enabled,
      );
      // Reload toggles for this skill
      const toggles = await api.getSkillToolToggles(
        skillId,
        activeScenario.id,
      );
      setSkillToggles((prev) => ({ ...prev, [skillId]: toggles }));
      await refreshManagedSkills();
    } catch (error) {
      toast.error(getErrorMessage(error, "Failed to toggle"));
    } finally {
      setTogglingCell(null);
    }
  };

  const handleToggleGroupTool = async (
    group: SkillGroup,
    toolKey: string,
  ) => {
    if (!activeScenario) return;
    const status = getGroupToolStatus(group.skills, toolKey);
    const newEnabled = status !== "all";

    setTogglingCell(`group-${group.id}-${toolKey}`);
    try {
      for (const skill of group.skills) {
        const toggles = skillToggles[skill.id];
        if (!toggles) continue;
        const toggle = toggles.find((t) => t.tool === toolKey);
        if (toggle && toggle.enabled !== newEnabled) {
          await api.setSkillToolToggle(
            skill.id,
            activeScenario.id,
            toolKey,
            newEnabled,
          );
        }
      }
      // Reload all toggles for this group's skills
      await reloadSkillToggles(group.skills.map((s) => s.id));
      await refreshManagedSkills();
      toast.success(
        newEnabled
          ? `${group.name} enabled for ${toolKey}`
          : `${group.name} disabled for ${toolKey}`,
      );
    } catch (error) {
      toast.error(getErrorMessage(error, "Failed to toggle group"));
    } finally {
      setTogglingCell(null);
    }
  };

  const handleToggleColumnAll = async (toolKey: string) => {
    if (!activeScenario) return;
    // Determine if we should enable or disable all
    let allEnabled = true;
    for (const { skills } of groups) {
      for (const skill of skills) {
        const toggles = skillToggles[skill.id];
        if (!toggles) continue;
        const toggle = toggles.find((t) => t.tool === toolKey);
        if (toggle && !toggle.enabled) {
          allEnabled = false;
          break;
        }
      }
      if (!allEnabled) break;
    }
    const newEnabled = !allEnabled;
    const toolName =
      columns.find((c) => c.key === toolKey)?.display_name || toolKey;

    setTogglingCell(`col-${toolKey}`);
    try {
      for (const { skills } of groups) {
        for (const skill of skills) {
          const toggles = skillToggles[skill.id];
          if (!toggles) continue;
          const toggle = toggles.find((t) => t.tool === toolKey);
          if (toggle && toggle.enabled !== newEnabled) {
            await api.setSkillToolToggle(
              skill.id,
              activeScenario.id,
              toolKey,
              newEnabled,
            );
          }
        }
      }
      // Reload all skill toggles
      await reloadSkillToggles(allSkillIds);
      await refreshManagedSkills();
      toast.success(
        newEnabled
          ? `All skills enabled for ${toolName}`
          : `All skills disabled for ${toolName}`,
      );
    } catch (error) {
      toast.error(getErrorMessage(error, "Failed to toggle column"));
    } finally {
      setTogglingCell(null);
    }
  };

  // ── Render helpers ──

  const renderGroupToolCell = (group: SkillGroup, tool: ToolInfo) => {
    const status = getGroupToolStatus(group.skills, tool.key);
    const isToggling =
      togglingCell === `group-${group.id}-${tool.key}`;

    return (
      <td key={tool.key} className="px-2 py-2 text-center">
        <button
          onClick={() => handleToggleGroupTool(group, tool.key)}
          disabled={isToggling || !activeScenario}
          className={cn(
            "inline-flex h-7 w-7 items-center justify-center rounded-md transition-colors",
            status === "all" &&
              "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 hover:bg-emerald-500/25",
            status === "some" &&
              "bg-amber-500/15 text-amber-600 dark:text-amber-400 hover:bg-amber-500/25",
            status === "none" &&
              "bg-surface-hover text-faint hover:bg-surface-active hover:text-muted",
            isToggling && "opacity-50",
          )}
          title={
            status === "all"
              ? `All skills in ${group.name} enabled for ${tool.display_name}`
              : status === "some"
                ? `Some skills in ${group.name} enabled for ${tool.display_name}`
                : `No skills in ${group.name} enabled for ${tool.display_name}`
          }
        >
          {isToggling ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : status === "all" ? (
            <Check className="h-3.5 w-3.5" />
          ) : status === "some" ? (
            <ToggleLeft className="h-3.5 w-3.5" />
          ) : (
            <X className="h-3.5 w-3.5" />
          )}
        </button>
      </td>
    );
  };

  const renderSkillToolCell = (skill: PackSkillRecord, tool: ToolInfo) => {
    const toggles = skillToggles[skill.id];
    const toggle = toggles?.find((t) => t.tool === tool.key);
    const enabled = toggle?.enabled ?? false;
    const cellKey = `${skill.id}-${tool.key}`;
    const isToggling = togglingCell === cellKey;

    if (!toggle) {
      return (
        <td key={tool.key} className="px-2 py-1.5 text-center">
          <span className="inline-flex h-6 w-6 items-center justify-center text-faint">
            <span className="h-1.5 w-1.5 rounded-full bg-border-subtle" />
          </span>
        </td>
      );
    }

    return (
      <td key={tool.key} className="px-2 py-1.5 text-center">
        <button
          onClick={() => handleToggleSkillTool(skill.id, tool.key, !enabled)}
          disabled={isToggling || !activeScenario}
          className={cn(
            "inline-flex h-6 w-6 items-center justify-center rounded transition-colors",
            enabled
              ? "text-emerald-500 hover:bg-emerald-500/10"
              : "text-faint hover:bg-surface-hover hover:text-muted",
            isToggling && "opacity-50",
          )}
          title={
            enabled
              ? `${skill.name} enabled for ${tool.display_name}`
              : `${skill.name} disabled for ${tool.display_name}`
          }
        >
          {isToggling ? (
            <Loader2 className="h-3 w-3 animate-spin" />
          ) : enabled ? (
            <ToggleRight className="h-3.5 w-3.5" />
          ) : (
            <ToggleLeft className="h-3.5 w-3.5" />
          )}
        </button>
      </td>
    );
  };

  // ── Main render ──

  if (loading) {
    return (
      <div className="app-page">
        <div className="app-page-header">
          <h1 className="app-page-title flex items-center gap-2">
            <Grid3X3 className="h-5 w-5 text-accent" />
            Agent Matrix
          </h1>
        </div>
        <div className="flex flex-1 items-center justify-center">
          <Loader2 className="h-8 w-8 animate-spin text-muted" />
        </div>
      </div>
    );
  }

  if (groups.length === 0) {
    return (
      <div className="app-page">
        <div className="app-page-header">
          <h1 className="app-page-title flex items-center gap-2">
            <Grid3X3 className="h-5 w-5 text-accent" />
            Agent Matrix
          </h1>
        </div>
        <div className="flex flex-1 flex-col items-center justify-center pb-20 text-center">
          <Package className="mb-4 h-12 w-12 text-faint" />
          <h3 className="mb-1.5 text-[14px] font-semibold text-tertiary">
            No skills in this scenario
          </h3>
          <p className="text-[13px] text-muted">
            Add skills or packs to the active scenario to see the agent matrix
            view.
          </p>
        </div>
      </div>
    );
  }

  if (columns.length === 0) {
    return (
      <div className="app-page">
        <div className="app-page-header">
          <h1 className="app-page-title flex items-center gap-2">
            <Grid3X3 className="h-5 w-5 text-accent" />
            Agent Matrix
          </h1>
        </div>
        <div className="flex flex-1 flex-col items-center justify-center pb-20 text-center">
          <Grid3X3 className="mb-4 h-12 w-12 text-faint" />
          <h3 className="mb-1.5 text-[14px] font-semibold text-tertiary">
            No agents available
          </h3>
          <p className="text-[13px] text-muted">
            Enable at least one agent in Settings to use the matrix view.
          </p>
        </div>
      </div>
    );
  }

  const totalSkills = groups.reduce((sum, g) => sum + g.skills.length, 0);

  return (
    <div className="app-page">
      <div className="app-page-header pr-2 pb-1">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h1 className="app-page-title flex items-center gap-2">
              <Grid3X3 className="h-5 w-5 text-accent" />
              Agent Matrix
              <span className="app-badge">{totalSkills} skills</span>
            </h1>
            {activeScenario && (
              <p className="app-page-subtitle text-tertiary">
                Showing toggles for scenario:{" "}
                <span className="font-medium text-secondary">
                  {activeScenario.name}
                </span>
              </p>
            )}
            {!activeScenario && (
              <p className="app-page-subtitle text-amber-600 dark:text-amber-400">
                No active scenario. Select a scenario to manage toggles.
              </p>
            )}
          </div>
          <Link
            to="/packs"
            className="inline-flex items-center gap-1.5 rounded-lg border border-border-subtle bg-surface px-3 py-1.5 text-[13px] font-medium text-secondary transition-colors hover:bg-surface-hover hover:text-primary shrink-0"
          >
            <Package className="h-3.5 w-3.5" />
            Manage Packs
            <ExternalLink className="h-3 w-3 text-faint" />
          </Link>
        </div>
      </div>

      {agentsWithCustomConfig.length > 0 && (
        <div className="mb-3 flex items-start gap-2.5 rounded-lg border border-blue-200 bg-blue-50 px-4 py-3 text-[13px] dark:border-blue-800 dark:bg-blue-950/40">
          <Info className="mt-0.5 h-4 w-4 shrink-0 text-blue-500 dark:text-blue-400" />
          <div className="min-w-0">
            <p className="font-medium text-blue-800 dark:text-blue-300">
              Some agents have custom configurations:
            </p>
            <ul className="mt-1 space-y-0.5">
              {agentsWithCustomConfig.map((a) => (
                <li key={a.tool_key} className="text-blue-700 dark:text-blue-400">
                  <Link
                    to={`/agent/${a.tool_key}`}
                    className="font-medium underline-offset-2 hover:underline"
                  >
                    {a.display_name}
                  </Link>
                  {": "}
                  {a.scenario_name ?? "custom"} ({a.effective_skill_count} skills)
                </li>
              ))}
            </ul>
            <p className="mt-1.5 text-blue-600 dark:text-blue-500">
              Click an agent above for details. The matrix below shows the global scenario.
            </p>
          </div>
        </div>
      )}

      <div className="overflow-x-auto rounded-xl border border-border-subtle bg-surface">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-border-subtle">
              <th className="sticky left-0 z-10 bg-surface px-4 py-3 text-left text-[13px] font-semibold text-secondary">
                Pack / Skill
              </th>
              {columns.map((tool) => (
                <th
                  key={tool.key}
                  className="px-2 py-3 text-center text-[12px] font-semibold text-muted"
                >
                  <div className="flex flex-col items-center gap-1">
                    <span className="truncate max-w-[80px]" title={tool.display_name}>
                      {tool.display_name}
                    </span>
                    {activeScenario && (
                      <button
                        onClick={() => handleToggleColumnAll(tool.key)}
                        disabled={!!togglingCell}
                        className="rounded px-1.5 py-0.5 text-[10px] font-medium text-faint transition-colors hover:bg-surface-hover hover:text-muted"
                      >
                        toggle all
                      </button>
                    )}
                  </div>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {groups.map((group) => {
              const isExpanded = expandedGroups.has(group.id);

              return (
                <GroupRows
                  key={group.id}
                  group={group}
                  isExpanded={isExpanded}
                  columns={columns}
                  onToggleExpand={() => toggleGroupExpand(group.id)}
                  renderGroupToolCell={renderGroupToolCell}
                  renderSkillToolCell={renderSkillToolCell}
                />
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

// ── Sub-components ──

interface GroupRowsProps {
  group: SkillGroup;
  isExpanded: boolean;
  columns: ToolInfo[];
  onToggleExpand: () => void;
  renderGroupToolCell: (group: SkillGroup, tool: ToolInfo) => React.ReactNode;
  renderSkillToolCell: (
    skill: PackSkillRecord,
    tool: ToolInfo,
  ) => React.ReactNode;
}

function GroupRows({
  group,
  isExpanded,
  columns,
  onToggleExpand,
  renderGroupToolCell,
  renderSkillToolCell,
}: GroupRowsProps) {
  const isUngrouped = group.id === UNGROUPED_ID;

  return (
    <>
      {/* Group header row */}
      <tr
        className={cn(
          "group cursor-pointer border-b border-border-subtle transition-colors hover:bg-surface-hover",
          isExpanded && "bg-surface-hover/50",
        )}
        onClick={onToggleExpand}
      >
        <td className="sticky left-0 z-10 bg-inherit px-4 py-2.5">
          <div className="flex items-center gap-2">
            {isExpanded ? (
              <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted" />
            ) : (
              <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted" />
            )}
            {isUngrouped ? (
              <FileText className="h-3.5 w-3.5 shrink-0 text-muted" />
            ) : (
              <Package className="h-3.5 w-3.5 shrink-0 text-accent" />
            )}
            <span
              className={cn(
                "text-[13px] font-semibold",
                isUngrouped ? "text-secondary italic" : "text-primary",
              )}
            >
              {group.name}
            </span>
            <span className="text-[12px] text-faint">
              {group.skills.length}{" "}
              {group.skills.length === 1 ? "skill" : "skills"}
            </span>
          </div>
        </td>
        {columns.map((tool) => renderGroupToolCell(group, tool))}
      </tr>

      {/* Expanded skill rows */}
      {isExpanded &&
        group.skills.map((skill) => (
          <tr
            key={skill.id}
            className="border-b border-border-subtle/50 bg-bg-secondary/30"
          >
            <td className="sticky left-0 z-10 bg-inherit py-1.5 pl-12 pr-4">
              <span
                className="text-[13px] text-secondary"
                title={skill.description || undefined}
              >
                {skill.name}
              </span>
            </td>
            {columns.map((tool) => renderSkillToolCell(skill, tool))}
          </tr>
        ))}
    </>
  );
}
