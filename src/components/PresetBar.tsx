import { useCallback, useMemo, useRef, useState } from "react";
import { Check, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import { computePresetStatus } from "../lib/presetStatus";
import { getPresetIconOption } from "../lib/presetIcons";
import type {
  ManagedSkill,
  Preset,
  PresetApplyMode,
  PresetApplyReport,
} from "../lib/tauri";
import { getErrorMessage } from "../lib/error";

export interface PresetBarProps {
  presets: Preset[];
  managedSkills: ManagedSkill[];
  agentKeys: string[];
  scopeKey: string;
  existsInWorkspace: (skill: ManagedSkill, agentKey: string) => boolean;
  onApplyPreset: (preset: Preset, mode: PresetApplyMode) => Promise<PresetApplyReport>;
  onComplete: () => Promise<void>;
}

interface QueuedPresetOperation {
  preset: Preset;
  mode: PresetApplyMode;
  applyPreset: PresetBarProps["onApplyPreset"];
}

const getPendingOperationKey = (scopeKey: string, presetId: string) =>
  JSON.stringify([scopeKey, presetId]);

export function PresetBar({
  presets,
  managedSkills,
  agentKeys,
  scopeKey,
  existsInWorkspace,
  onApplyPreset,
  onComplete,
}: PresetBarProps) {
  const { t } = useTranslation();
  const queueRef = useRef<QueuedPresetOperation[]>([]);
  const processingRef = useRef(false);
  const pendingOperationKeysRef = useRef(new Set<string>());
  const onCompleteRef = useRef(onComplete);
  onCompleteRef.current = onComplete;
  const [pendingOperationKeys, setPendingOperationKeys] = useState<Set<string>>(
    () => new Set()
  );

  const statuses = useMemo(() => {
    const map = new Map<string, ReturnType<typeof computePresetStatus>>();
    for (const preset of presets) {
      map.set(preset.id, computePresetStatus(preset, managedSkills, agentKeys, existsInWorkspace));
    }
    return map;
  }, [presets, managedSkills, agentKeys, existsInWorkspace]);

  const showResultToast = useCallback((mode: PresetApplyMode, report: PresetApplyReport) => {
    const failed = report.failures.length;
    if (mode === "add") {
      if (report.applied > 0) {
        toast.success(t("presetActions.addedToast", {
          added: report.applied,
          skipped: report.skipped,
        }));
      } else if (failed === 0) {
        toast.info(t("presetActions.nothingToAdd"));
      }
    } else {
      if (report.applied > 0) {
        toast.success(t("presetActions.removedToast", { removed: report.applied }));
      } else if (failed === 0) {
        toast.info(t("presetActions.nothingToRemove"));
      }
    }
    if (failed > 0) {
      // Toast 保持简短，只展示前三项可操作信息；完整报告写入控制台，
      // 既避免大量失败撑满界面，也让排障时不会丢失后续错误。
      const failurePreview = report.failures
        .slice(0, 3)
        .map(({ skillId, toolKey, message }) => `${skillId} · ${toolKey}: ${message}`)
        .join("\n");
      console.error("[PresetBar] Preset apply failures", report.failures);
      toast.error(t("presetActions.partialFailedToast", { count: failed }), {
        description: failurePreview,
      });
    }
  }, [t]);

  const drainQueue = useCallback(async () => {
    if (processingRef.current) return;
    processingRef.current = true;

    try {
      // 队列按点击顺序串行执行，避免多个 Preset 同时复制重叠目录或竞争
      // SQLite；刷新期间新加入的操作会在刷新结束后继续处理。
      do {
        let operation = queueRef.current.shift();
        while (operation) {
          try {
            const report = await operation.applyPreset(operation.preset, operation.mode);
            showResultToast(operation.mode, report);
          } catch (error) {
            toast.error(getErrorMessage(error, t("common.error")));
          }
          operation = queueRef.current.shift();
        }

        try {
          // apply 回调必须快照精确 Agent；完成刷新则必须使用当前页面的最新
          // 回调，避免从 Agent A 导航到 Agent B 后旧请求把 A 的列表写进 B 页面。
          await onCompleteRef.current();
        } catch (error) {
          toast.error(getErrorMessage(error, t("common.error")));
        }
      } while (queueRef.current.length > 0);
    } finally {
      pendingOperationKeysRef.current.clear();
      setPendingOperationKeys(new Set());
      processingRef.current = false;
    }
  }, [showResultToast, t]);

  const enqueuePreset = useCallback((preset: Preset, mode: PresetApplyMode) => {
    // Preset ID 只在同一作用域内去重；Agent A/B 或 Project A/B 即使使用同名
    // Preset，也必须能够各自加入同一条 FIFO 队列。
    const operationKey = getPendingOperationKey(scopeKey, preset.id);
    if (pendingOperationKeysRef.current.has(operationKey)) return;

    pendingOperationKeysRef.current.add(operationKey);
    setPendingOperationKeys(new Set(pendingOperationKeysRef.current));
    // 路由切换时组件可能被复用；把当前 Agent 对应的回调快照到队列项，
    // 防止 Agent B 的后续点击被仍在运行的 Agent A 队列错误提交到 A。
    queueRef.current.push({
      preset,
      mode,
      applyPreset: onApplyPreset,
    });
    void drainQueue();
  }, [drainQueue, onApplyPreset, scopeKey]);

  if (presets.length === 0) return null;

  return (
    <div className="flex min-w-0 flex-wrap items-start gap-1.5">
      <span className="shrink-0 pt-0.5 text-[12px] text-muted">{t("sidebar.presets")}</span>
      <div
        data-testid="preset-list"
        className="flex min-w-0 flex-1 flex-wrap items-center gap-1.5"
      >
        {presets.map((preset) => {
          const s = statuses.get(preset.id)!;
          const presetIcon = getPresetIconOption(preset);
          const Icon = presetIcon.icon;
          const isPending = pendingOperationKeys.has(
            getPendingOperationKey(scopeKey, preset.id)
          );

          return (
            <button
              key={preset.id}
              onClick={() => {
                enqueuePreset(preset, s.status === "active" ? "remove" : "add");
              }}
              disabled={isPending || s.status === "empty"}
              aria-busy={isPending}
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
              {isPending
                ? <Loader2 className="h-3 w-3 animate-spin" />
                : <Icon className="h-3 w-3" />}
              <span className="whitespace-nowrap">{preset.name}</span>
              {s.status === "active" && <Check className="h-3 w-3 shrink-0" />}
              {s.status === "partial" && (
                <span className="rounded-full bg-amber-500/20 px-1.5 py-px text-[10px] font-semibold">
                  {s.installed}/{s.total}
                </span>
              )}
              {s.status === "empty" && (
                <span className="rounded-full bg-surface-hover px-1.5 py-px text-[10px] font-semibold">
                  0
                </span>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}
