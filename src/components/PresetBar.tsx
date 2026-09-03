import { useCallback, useMemo, useState } from "react";
import { Check, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import { computePresetStatus, type PresetStatusMode } from "../lib/presetStatus";
import { getPresetIconOption } from "../lib/presetIcons";
import type { ManagedSkill, Preset } from "../lib/tauri";
import { getErrorMessage } from "../lib/error";

export interface PresetBarProps {
  presets: Preset[];
  managedSkills: ManagedSkill[];
  agentKeys: string[];
  existsInWorkspace: (skill: ManagedSkill, agentKey: string) => boolean;
  onAddSkill: (skill: ManagedSkill, agentKey: string) => Promise<void>;
  onRemoveSkill: (skill: ManagedSkill, agentKey: string) => Promise<void>;
  onComplete: () => Promise<void>;
  /** 控制状态角标按 agent 副本还是逻辑 skill 统计。 */
  statusMode?: PresetStatusMode;
}

export function PresetBar({
  presets,
  managedSkills,
  agentKeys,
  existsInWorkspace,
  onAddSkill,
  onRemoveSkill,
  onComplete,
  statusMode = "agent-pair",
}: PresetBarProps) {
  const { t } = useTranslation();
  const [loadingKey, setLoadingKey] = useState<string | null>(null);

  const statuses = useMemo(() => {
    const map = new Map<string, ReturnType<typeof computePresetStatus>>();
    for (const preset of presets) {
      map.set(
        preset.id,
        computePresetStatus(preset, managedSkills, agentKeys, existsInWorkspace, statusMode)
      );
    }
    return map;
  }, [presets, managedSkills, agentKeys, existsInWorkspace, statusMode]);

  const visiblePresets = useMemo(
    () => presets.filter((p) => statuses.get(p.id)?.status !== "empty"),
    [presets, statuses]
  );

  const handleActivate = useCallback(async (preset: Preset) => {
    setLoadingKey(`${preset.id}-add`);
    try {
      const presetSkills = managedSkills.filter((s) => s.preset_ids.includes(preset.id));
      let added = 0, skipped = 0, failed = 0;
      const failures: string[] = [];
      for (const skill of presetSkills) {
        for (const agentKey of agentKeys) {
          if (existsInWorkspace(skill, agentKey)) { skipped++; continue; }
          try { await onAddSkill(skill, agentKey); added++; }
          catch (e) { failed++; failures.push(getErrorMessage(e, t("common.error"))); }
        }
      }
      if (added > 0) {
        toast.success(t("presetActions.addedToast", { added, skipped }));
      } else if (failed === 0) {
        toast.info(t("presetActions.nothingToAdd"));
      }
      if (failed > 0) {
        toast.error(
          [t("presetActions.partialFailedToast", { count: failed }), failures[0]]
            .filter(Boolean)
            .join(" — "),
        );
      }
      await onComplete();
    } catch (error) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setLoadingKey(null);
    }
  }, [agentKeys, existsInWorkspace, managedSkills, onAddSkill, onComplete, t]);

  const handleDeactivate = useCallback(async (preset: Preset) => {
    setLoadingKey(`${preset.id}-remove`);
    try {
      const presetSkills = managedSkills.filter((s) => s.preset_ids.includes(preset.id));
      let removed = 0, failed = 0;
      const failures: string[] = [];
      for (const skill of presetSkills) {
        for (const agentKey of agentKeys) {
          if (!existsInWorkspace(skill, agentKey)) continue;
          try { await onRemoveSkill(skill, agentKey); removed++; }
          catch (e) { failed++; failures.push(getErrorMessage(e, t("common.error"))); }
        }
      }
      if (removed > 0) {
        toast.success(t("presetActions.removedToast", { removed }));
      } else if (failed === 0) {
        toast.info(t("presetActions.nothingToRemove"));
      }
      if (failed > 0) {
        toast.error(
          [t("presetActions.partialFailedToast", { count: failed }), failures[0]]
            .filter(Boolean)
            .join(" — "),
        );
      }
      await onComplete();
    } catch (error) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setLoadingKey(null);
    }
  }, [agentKeys, existsInWorkspace, managedSkills, onComplete, onRemoveSkill, t]);

  if (visiblePresets.length === 0) return null;

  const busy = loadingKey !== null;

  return (
    <div className="flex min-w-0 flex-wrap items-center gap-1.5">
      <span className="shrink-0 text-[12px] text-muted">{t("sidebar.presets")}</span>
      <div className="flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto scrollbar-hide">
        {visiblePresets.map((preset) => {
          const s = statuses.get(preset.id)!;
          const presetIcon = getPresetIconOption(preset);
          const Icon = presetIcon.icon;
          const isLoading = loadingKey?.startsWith(preset.id) ?? false;

          return (
            <button
              key={preset.id}
              onClick={() => {
                if (busy) return;
                if (s.status === "active") handleDeactivate(preset);
                else handleActivate(preset);
              }}
              disabled={busy}
              title={preset.name}
              className={cn(
                "inline-flex shrink-0 items-center gap-1 rounded-full border px-2.5 py-0.5 text-[12px] font-medium transition-colors disabled:opacity-50",
                s.status === "active"
                  ? `${presetIcon.activeClass} ${presetIcon.colorClass}`
                  : s.status === "partial"
                  ? "border-amber-400/50 bg-amber-500/8 text-amber-600 dark:text-amber-400 hover:bg-amber-500/12"
                  : "border-border-subtle text-faint hover:border-border hover:text-muted"
              )}
            >
              {isLoading
                ? <Loader2 className="h-3 w-3 animate-spin" />
                : <Icon className="h-3 w-3" />}
              <span className="max-w-[140px] truncate">{preset.name}</span>
              {s.status === "active" && <Check className="h-3 w-3 shrink-0" />}
              {s.status === "partial" && (
                <span className="rounded-full bg-amber-500/20 px-1.5 py-px text-[10px] font-semibold">
                  {s.installed}/{s.total}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}
