import { useState, useEffect, useCallback } from "react";
import { useParams } from "react-router-dom";
import { Bot, ChevronDown, ChevronRight, Download, Eye, Layers, Loader2, Puzzle, Shield, ToggleLeft, ToggleRight, Undo2 } from "lucide-react";
import { toast } from "sonner";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import * as api from "../lib/tauri";
import type { AgentConfigDto, AgentSkillOwnership, PackRecord, PackSkillRecord } from "../lib/tauri";

// ─── Skill Tag Cloud ─────────────────────────────────────────────────────────

interface SkillTagCloudProps {
  skills: PackSkillRecord[];
  extraPackIds: Set<string>;
}

function SkillTagCloud({ skills, extraPackIds: _extraPackIds }: SkillTagCloudProps) {
  if (skills.length === 0) return null;
  return (
    <div className="flex flex-wrap gap-1.5 pt-2">
      {skills.map((skill) => (
        <span
          key={skill.id}
          className="inline-flex items-center rounded-full border border-border-subtle bg-surface-hover px-2 py-0.5 text-[11px] font-medium text-tertiary"
          title={skill.description || undefined}
        >
          {skill.name}
        </span>
      ))}
    </div>
  );
}

// ─── Progress Bar ─────────────────────────────────────────────────────────────

interface SkillProgressBarProps {
  scenarioCount: number;
  extraCount: number;
  total: number;
}

function SkillProgressBar({ scenarioCount, extraCount, total }: SkillProgressBarProps) {
  if (total === 0) return <div className="h-2 rounded-full bg-surface-hover" />;
  const scenarioPct = Math.round((scenarioCount / total) * 100);
  const extraPct = Math.round((extraCount / total) * 100);
  return (
    <div className="flex h-2 w-full overflow-hidden rounded-full bg-surface-hover">
      <div
        className="h-full bg-emerald-500 transition-all"
        style={{ width: `${scenarioPct}%` }}
        title={`Scenario: ${scenarioCount}`}
      />
      <div
        className="h-full bg-amber-400 transition-all"
        style={{ width: `${extraPct}%` }}
        title={`Extra packs: ${extraCount}`}
      />
    </div>
  );
}

// ─── Breakdown Row ──────────────────────────────────────────────────────────

interface BreakdownRowProps {
  icon: React.ReactNode;
  label: string;
  count: number;
  suffix?: string;
  expanded: boolean;
  onToggle: () => void;
  action?: React.ReactNode;
  children: React.ReactNode;
}

function BreakdownRow({ icon, label, count, suffix, expanded, onToggle, action, children }: BreakdownRowProps) {
  return (
    <div>
      <button
        onClick={onToggle}
        className="flex w-full items-center justify-between px-4 py-3 hover:bg-surface-hover transition-colors text-left"
      >
        <div className="flex items-center gap-2.5 min-w-0">
          {expanded ? <ChevronDown className="h-3.5 w-3.5 text-muted shrink-0" /> : <ChevronRight className="h-3.5 w-3.5 text-muted shrink-0" />}
          {icon}
          <span className="text-[13px] font-medium text-secondary">{label}</span>
          <span className="text-[12px] text-muted">
            {count}{suffix ? ` (${suffix})` : ""}
          </span>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          {action}
        </div>
      </button>
      {expanded && count > 0 && children}
    </div>
  );
}

// ─── Main View ───────────────────────────────────────────────────────────────

export function AgentDetail() {
  const { toolKey } = useParams<{ toolKey: string }>();
  const { scenarios } = useApp();

  const [agent, setAgent] = useState<AgentConfigDto | null>(null);
  const [effectiveSkills, setEffectiveSkills] = useState<PackSkillRecord[]>([]);
  const [extraPacks, setExtraPacks] = useState<PackRecord[]>([]);
  const [allPacks, setAllPacks] = useState<PackRecord[]>([]);
  const [scenarioPacks, setScenarioPacks] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(true);
  const [applyingScenario, setApplyingScenario] = useState(false);
  const [pendingScenarioId, setPendingScenarioId] = useState<string>("");
  const [togglingPack, setTogglingPack] = useState<string | null>(null);
  const [togglingManaged, setTogglingManaged] = useState(false);
  const [ownership, setOwnership] = useState<AgentSkillOwnership | null>(null);
  const [expandedSection, setExpandedSection] = useState<string | null>(null);
  const [deduping, setDeduping] = useState(false);
  const [importingId, setImportingId] = useState<string | null>(null);
  const [importingAll, setImportingAll] = useState(false);
  const [markingNativeId, setMarkingNativeId] = useState<string | null>(null);
  const [unmarkingNativeId, setUnmarkingNativeId] = useState<string | null>(null);

  const loadAgent = useCallback(async () => {
    if (!toolKey) return;
    try {
      const [cfg, skills, packs, all, own] = await Promise.all([
        api.getAgentConfig(toolKey),
        api.getEffectiveSkillsForAgent(toolKey),
        api.getAgentExtraPacks(toolKey),
        api.getAllPacks(),
        api.getAgentSkillOwnership(toolKey).catch(() => null),
      ]);
      setAgent(cfg);
      setEffectiveSkills(skills);
      setExtraPacks(packs);
      setAllPacks(all);
      setOwnership(own);
      setPendingScenarioId(cfg?.scenario_id ?? "");

      // Load the base scenario's packs so we can exclude them from extra picks
      if (cfg?.scenario_id) {
        try {
          const sp = await api.getPacksForScenario(cfg.scenario_id);
          setScenarioPacks(new Set(sp.map((p) => p.id)));
        } catch {
          setScenarioPacks(new Set());
        }
      } else {
        setScenarioPacks(new Set());
      }
    } catch (e) {
      console.error("Failed to load agent config", e);
      toast.error("Failed to load agent config");
    } finally {
      setLoading(false);
    }
  }, [toolKey]);

  useEffect(() => {
    setLoading(true);
    loadAgent();
  }, [loadAgent]);

  const handleApplyScenario = async () => {
    if (!toolKey || !pendingScenarioId) return;
    setApplyingScenario(true);
    try {
      await api.setAgentScenario(toolKey, pendingScenarioId);
      await loadAgent();
      toast.success("Scenario applied");
    } catch {
      toast.error("Failed to apply scenario");
    } finally {
      setApplyingScenario(false);
    }
  };

  const handleToggleManaged = async () => {
    if (!toolKey || !agent) return;
    setTogglingManaged(true);
    try {
      await api.setAgentManaged(toolKey, !agent.managed);
      await loadAgent();
      toast.success(agent.managed ? "Agent unmanaged" : "Agent managed");
    } catch {
      toast.error("Failed to update agent");
    } finally {
      setTogglingManaged(false);
    }
  };

  const handleTogglePack = async (pack: PackRecord, currentlyEnabled: boolean) => {
    if (!toolKey) return;
    setTogglingPack(pack.id);
    try {
      if (currentlyEnabled) {
        await api.removeAgentExtraPack(toolKey, pack.id);
        toast.success(`Removed "${pack.name}"`);
      } else {
        await api.addAgentExtraPack(toolKey, pack.id);
        toast.success(`Added "${pack.name}"`);
      }
      await loadAgent();
    } catch {
      toast.error("Failed to update pack");
    } finally {
      setTogglingPack(null);
    }
  };

  const handleDedup = async () => {
    if (!toolKey) return;
    setDeduping(true);
    try {
      const result = await api.dedupAgentSkills(toolKey);
      const parts: string[] = [];
      if (result.replaced_with_symlink.length > 0)
        parts.push(`${result.replaced_with_symlink.length} replaced with symlink`);
      if (result.marked_native.length > 0)
        parts.push(`${result.marked_native.length} marked native`);
      if (result.already_linked.length > 0)
        parts.push(`${result.already_linked.length} already linked`);
      if (result.errors.length > 0)
        parts.push(`${result.errors.length} error${result.errors.length !== 1 ? "s" : ""}`);
      toast.success(parts.length > 0 ? `Dedup: ${parts.join(", ")}` : "Dedup: nothing to do");
      await loadAgent();
    } catch {
      toast.error("Failed to dedup agent skills");
    } finally {
      setDeduping(false);
    }
  };

  const handleImportDiscovered = async (discoveredId: string, name: string) => {
    setImportingId(discoveredId);
    try {
      await api.importDiscoveredSkill(discoveredId);
      toast.success(`Imported "${name}"`);
      await loadAgent();
    } catch {
      toast.error(`Failed to import "${name}"`);
    } finally {
      setImportingId(null);
    }
  };

  const handleImportAllDiscovered = async () => {
    if (!ownership) return;
    const nonNative = ownership.discovered.filter((d) => !d.is_native);
    if (nonNative.length === 0) return;
    setImportingAll(true);
    try {
      for (const d of nonNative) {
        await api.importDiscoveredSkill(d.id);
      }
      toast.success(`Imported ${nonNative.length} discovered skill${nonNative.length !== 1 ? "s" : ""}`);
      await loadAgent();
    } catch {
      toast.error("Failed to import all discovered skills");
    } finally {
      setImportingAll(false);
    }
  };

  const handleMarkNative = async (discoveredId: string, name: string) => {
    setMarkingNativeId(discoveredId);
    try {
      await api.markSkillAsNative(discoveredId);
      toast.success(`Marked "${name}" as native`);
      await loadAgent();
    } catch {
      toast.error(`Failed to mark "${name}" as native`);
    } finally {
      setMarkingNativeId(null);
    }
  };

  const handleUnmarkNative = async (discoveredId: string, name: string) => {
    setUnmarkingNativeId(discoveredId);
    try {
      await api.unmarkSkillAsNative(discoveredId);
      toast.success(`Unmarked "${name}" as native`);
      await loadAgent();
    } catch {
      toast.error(`Failed to unmark "${name}"`);
    } finally {
      setUnmarkingNativeId(null);
    }
  };

  if (loading) {
    return (
      <div className="app-page app-page-narrow">
        <div className="flex items-center justify-center py-16 text-muted text-[13px]">
          Loading…
        </div>
      </div>
    );
  }

  if (!agent) {
    return (
      <div className="app-page app-page-narrow">
        <div className="flex items-center justify-center py-16 text-muted text-[13px]">
          Agent not found.
        </div>
      </div>
    );
  }

  const extraPackIds = new Set(extraPacks.map((p) => p.id));
  const availableExtraPacks = allPacks.filter((p) => !scenarioPacks.has(p.id));

  // Skill counts for progress bar
  // We approximate: total = effective, extra = extra_pack_count worth of skills
  // Actual split requires knowing which skills come from which source, but we use
  // agent's effective_skill_count and extra_pack_count as a proxy count ratio.
  const totalCount = effectiveSkills.length;
  const extraSkillCount = agent.extra_pack_count > 0
    ? Math.max(0, totalCount - (agent.effective_skill_count - agent.extra_pack_count))
    : 0;
  const scenarioSkillCount = totalCount - extraSkillCount;

  const fieldClass =
    "h-8 rounded-[4px] border border-border-subtle bg-background px-2.5 text-[13px] text-secondary outline-none transition-colors focus:border-border";

  const scenarioChanged = pendingScenarioId !== (agent.scenario_id ?? "");

  return (
    <div className="app-page app-page-narrow">
      {/* Header */}
      <div className="app-page-header">
        <div className="flex items-center justify-between gap-4">
          <div className="flex items-center gap-3 min-w-0">
            <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border border-border-subtle bg-surface">
              <Bot className="h-5 w-5 text-muted" />
            </div>
            <div className="min-w-0">
              <h1 className="app-page-title">{agent.display_name}</h1>
              <p className="app-page-subtitle">
                {agent.effective_skill_count} effective skill{agent.effective_skill_count !== 1 ? "s" : ""}
                {agent.extra_pack_count > 0 && ` · ${agent.extra_pack_count} extra pack${agent.extra_pack_count !== 1 ? "s" : ""}`}
              </p>
            </div>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            {/* Dedup button */}
            <button
              onClick={handleDedup}
              disabled={deduping}
              className="inline-flex items-center gap-1.5 rounded-lg border border-border-subtle bg-surface px-3 py-1.5 text-[12px] font-medium text-muted transition-colors outline-none hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
            >
              {deduping
                ? <Loader2 className="h-3.5 w-3.5 animate-spin" />
                : <Layers className="h-3.5 w-3.5" />}
              Dedup
            </button>
            {/* Managed toggle */}
            <button
              onClick={handleToggleManaged}
              disabled={togglingManaged}
              className={cn(
                "inline-flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-[12px] font-medium transition-colors outline-none",
                agent.managed
                  ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-400 hover:bg-emerald-500/20"
                  : "border-border-subtle bg-surface text-muted hover:bg-surface-hover"
              )}
            >
              {agent.managed
                ? <ToggleRight className="h-3.5 w-3.5" />
                : <ToggleLeft className="h-3.5 w-3.5" />}
              {agent.managed ? "Active" : "Unmanaged"}
            </button>
          </div>
        </div>
      </div>

      {/* Base Scenario */}
      <div>
        <h2 className="app-section-title mb-3">Base Scenario</h2>
        <div className="app-panel p-4">
          <div className="flex items-center gap-3">
            <select
              value={pendingScenarioId}
              onChange={(e) => setPendingScenarioId(e.target.value)}
              className={cn(fieldClass, "flex-1")}
            >
              <option value="">— No scenario —</option>
              {scenarios.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name} ({s.skill_count} skills)
                </option>
              ))}
            </select>
            <button
              onClick={handleApplyScenario}
              disabled={!scenarioChanged || applyingScenario || !pendingScenarioId}
              className="app-button-primary !py-1.5 !px-3 !text-[12px]"
            >
              {applyingScenario ? "Applying…" : "Apply"}
            </button>
          </div>
          {agent.scenario_name && (
            <p className="text-[12px] text-muted mt-2">
              Current: <span className="text-secondary font-medium">{agent.scenario_name}</span>
            </p>
          )}
        </div>
      </div>

      {/* Additional Packs */}
      {availableExtraPacks.length > 0 && (
        <div>
          <h2 className="app-section-title mb-3">Additional Packs</h2>
          <div className="app-panel overflow-hidden divide-y divide-border-subtle">
            {availableExtraPacks.map((pack) => {
              const isEnabled = extraPackIds.has(pack.id);
              const isLoading = togglingPack === pack.id;
              return (
                <div
                  key={pack.id}
                  className="flex items-center justify-between px-4 py-3 hover:bg-surface-hover transition-colors"
                >
                  <div className="min-w-0 flex-1">
                    <h4 className="text-[13px] font-medium text-secondary truncate">
                      {pack.name}
                    </h4>
                    {pack.description && (
                      <p className="text-[12px] text-muted truncate mt-0.5">{pack.description}</p>
                    )}
                  </div>
                  <button
                    onClick={() => handleTogglePack(pack, isEnabled)}
                    disabled={isLoading}
                    className={cn(
                      "flex h-5 w-5 shrink-0 items-center justify-center rounded border transition-colors outline-none ml-3",
                      isEnabled
                        ? "border-accent-border bg-accent-dark text-white"
                        : "border-border bg-surface hover:border-border-subtle"
                    )}
                  >
                    {isEnabled && (
                      <svg className="w-2.5 h-2.5" viewBox="0 0 12 12" fill="none">
                        <path
                          d="M2 6L5 9L10 3"
                          stroke="currentColor"
                          strokeWidth="2"
                          strokeLinecap="round"
                          strokeLinejoin="round"
                        />
                      </svg>
                    )}
                  </button>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Effective Skills */}
      <div>
        <h2 className="app-section-title mb-3">Effective Skills</h2>
        <div className="app-panel p-4 space-y-3">
          {/* Summary line */}
          <div className="text-[13px] text-secondary">
            {agent.scenario_name && (
              <span>
                <span className="text-emerald-400 font-medium">{agent.scenario_name}</span>
                {` (${scenarioSkillCount})`}
              </span>
            )}
            {agent.extra_pack_count > 0 && (
              <span className="text-muted">
                {agent.scenario_name && " + "}
                <span className="text-amber-400 font-medium">extra packs</span>
                {` (${extraSkillCount})`}
              </span>
            )}
            {totalCount > 0 && (
              <span className="text-muted"> = {totalCount} total</span>
            )}
            {totalCount === 0 && (
              <span className="text-muted">No skills assigned</span>
            )}
          </div>

          {/* Progress bar */}
          <SkillProgressBar
            scenarioCount={scenarioSkillCount}
            extraCount={extraSkillCount}
            total={totalCount}
          />

          {/* Tag cloud */}
          <SkillTagCloud skills={effectiveSkills} extraPackIds={extraPackIds} />
        </div>
      </div>

      {/* Skills Breakdown */}
      {ownership && (
        <div>
          <h2 className="app-section-title mb-3">Skills Breakdown</h2>
          <div className="app-panel overflow-hidden divide-y divide-border-subtle">
            {/* Managed */}
            <BreakdownRow
              icon={<Puzzle className="h-3.5 w-3.5 text-emerald-400" />}
              label="SM-Managed"
              count={ownership.managed.length}
              expanded={expandedSection === "managed"}
              onToggle={() => setExpandedSection(expandedSection === "managed" ? null : "managed")}
            >
              <div className="flex flex-wrap gap-1.5 px-4 pb-3">
                {ownership.managed.map((s) => (
                  <span
                    key={s.id}
                    className="inline-flex items-center rounded-full border border-emerald-500/20 bg-emerald-500/5 px-2 py-0.5 text-[11px] font-medium text-emerald-400"
                    title={s.description || undefined}
                  >
                    {s.name}
                  </span>
                ))}
              </div>
            </BreakdownRow>

            {/* Discovered (non-native only) */}
            {(() => {
              const nonNative = ownership.discovered.filter((d) => !d.is_native);
              const nativeFlagged = ownership.discovered.filter((d) => d.is_native);
              const totalNative = ownership.native.length + nativeFlagged.length;
              return (
                <>
                  <BreakdownRow
                    icon={<Eye className="h-3.5 w-3.5 text-amber-400" />}
                    label="Discovered"
                    count={nonNative.length}
                    suffix="not imported"
                    expanded={expandedSection === "discovered"}
                    onToggle={() => setExpandedSection(expandedSection === "discovered" ? null : "discovered")}
                    action={
                      nonNative.length > 0 ? (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            handleImportAllDiscovered();
                          }}
                          disabled={importingAll}
                          className="inline-flex items-center gap-1 rounded border border-amber-500/30 bg-amber-500/10 px-2 py-0.5 text-[11px] font-medium text-amber-400 hover:bg-amber-500/20 transition-colors disabled:opacity-50"
                        >
                          {importingAll
                            ? <Loader2 className="h-3 w-3 animate-spin" />
                            : <Download className="h-3 w-3" />}
                          Import All
                        </button>
                      ) : undefined
                    }
                  >
                    <div className="space-y-1 px-4 pb-3">
                      {nonNative.map((d) => (
                        <div
                          key={d.id}
                          className="flex items-center justify-between gap-2 rounded-lg px-2 py-1.5 hover:bg-surface-hover transition-colors"
                        >
                          <span
                            className="text-[12px] font-medium text-amber-400 truncate min-w-0"
                            title={d.found_path}
                          >
                            {d.name_guess || "(unnamed)"}
                          </span>
                          <div className="flex items-center gap-1.5 shrink-0">
                            <button
                              onClick={() => handleImportDiscovered(d.id, d.name_guess || "(unnamed)")}
                              disabled={importingId === d.id || importingAll}
                              className="inline-flex items-center gap-1 rounded border border-accent-border bg-accent-dark px-2 py-0.5 text-[11px] font-medium text-white hover:bg-accent transition-colors disabled:opacity-50"
                            >
                              {importingId === d.id
                                ? <Loader2 className="h-3 w-3 animate-spin" />
                                : <Download className="h-3 w-3" />}
                              Import
                            </button>
                            <button
                              onClick={() => handleMarkNative(d.id, d.name_guess || "(unnamed)")}
                              disabled={markingNativeId === d.id}
                              className="inline-flex items-center gap-1 rounded border border-border-subtle bg-surface px-2 py-0.5 text-[11px] font-medium text-muted hover:bg-surface-hover hover:text-secondary transition-colors disabled:opacity-50"
                            >
                              {markingNativeId === d.id
                                ? <Loader2 className="h-3 w-3 animate-spin" />
                                : <Shield className="h-3 w-3" />}
                              Mark Native
                            </button>
                          </div>
                        </div>
                      ))}
                    </div>
                  </BreakdownRow>

                  {/* Native */}
                  <BreakdownRow
                    icon={<Bot className="h-3.5 w-3.5 text-muted" />}
                    label="Native"
                    count={totalNative}
                    expanded={expandedSection === "native"}
                    onToggle={() => setExpandedSection(expandedSection === "native" ? null : "native")}
                  >
                    <div className="space-y-1 px-4 pb-3">
                      {/* Native-flagged discovered skills (have DB records, can un-mark) */}
                      {nativeFlagged.map((d) => (
                        <div
                          key={d.id}
                          className="flex items-center justify-between gap-2 rounded-lg px-2 py-1.5 hover:bg-surface-hover transition-colors"
                        >
                          <span
                            className="text-[12px] font-medium text-muted truncate min-w-0"
                            title={d.found_path}
                          >
                            {d.name_guess || "(unnamed)"}
                          </span>
                          <button
                            onClick={() => handleUnmarkNative(d.id, d.name_guess || "(unnamed)")}
                            disabled={unmarkingNativeId === d.id}
                            className="inline-flex items-center gap-1 rounded border border-border-subtle bg-surface px-2 py-0.5 text-[11px] font-medium text-muted hover:bg-surface-hover hover:text-secondary transition-colors disabled:opacity-50 shrink-0"
                          >
                            {unmarkingNativeId === d.id
                              ? <Loader2 className="h-3 w-3 animate-spin" />
                              : <Undo2 className="h-3 w-3" />}
                            Un-mark
                          </button>
                        </div>
                      ))}
                      {/* Filesystem-only native skills (no DB records, display only) */}
                      {ownership.native.map((name) => (
                        <div
                          key={name}
                          className="flex items-center rounded-lg px-2 py-1.5"
                        >
                          <span className="text-[12px] font-medium text-muted">
                            {name}
                          </span>
                        </div>
                      ))}
                    </div>
                  </BreakdownRow>
                </>
              );
            })()}
          </div>
        </div>
      )}
    </div>
  );
}
