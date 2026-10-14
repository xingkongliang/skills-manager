import { useState } from "react";
import {
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  Circle,
  CircleDashed,
  Loader2,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";
import { isItemDeployed, type AgentToggleItem } from "../lib/agentToggleState";
import { AgentIcon } from "./AgentIcon";

export type { AgentToggleItem };

interface Props {
  items: AgentToggleItem[];
  togglingKey?: string | null;
  onToggle: (key: string, enabled: boolean) => void;
  className?: string;
}

export function AgentToggleSection({
  items,
  togglingKey,
  onToggle,
  className,
}: Props) {
  const { t } = useTranslation();
  const [showUnavailable, setShowUnavailable] = useState(false);

  const availableItems = items.filter((item) => item.isAvailable);
  const unavailableItems = items.filter((item) => !item.isAvailable);
  const syncedAvailableCount = availableItems.filter((item) =>
    isItemDeployed(item)
  ).length;

  return (
    <div className={cn("rounded-xl border border-border-subtle", className)}>
      <div className="border-b border-border-subtle px-6 py-2.5">
        <div className="flex items-center justify-between gap-2 text-[13px]">
          <div className="flex min-w-0 items-center gap-2">
            <span className="font-medium text-secondary">{t("mySkills.agentTogglesTitle")}</span>
            <span className="rounded-full border border-border-subtle bg-surface px-2 py-0.5 text-[12px] text-muted">
              {t("mySkills.syncSummary", {
                synced: syncedAvailableCount,
                total: availableItems.length,
              })}
            </span>
          </div>
        </div>

        {availableItems.length > 0 && (
          <div className="mt-2 grid grid-cols-2 gap-1.5 md:grid-cols-3">
            {availableItems.map((item) => (
              <AgentToggle
                key={item.key}
                item={item}
                loading={togglingKey === item.key}
                onToggle={onToggle}
              />
            ))}
          </div>
        )}

        {unavailableItems.length > 0 && (
          <div className="mt-2">
            <button
              type="button"
              onClick={() => setShowUnavailable((prev) => !prev)}
              className="inline-flex items-center gap-1 text-[12px] text-muted transition-colors hover:text-secondary"
            >
              {showUnavailable ? <ChevronUp className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />}
              <span>{t("mySkills.agentUnavailableCount", { count: unavailableItems.length })}</span>
            </button>
            {showUnavailable && (
              <div className="mt-1.5 grid grid-cols-2 gap-1.5 md:grid-cols-3">
                {unavailableItems.map((item) => (
                  <AgentToggle
                    key={item.key}
                    item={item}
                    loading={togglingKey === item.key}
                    onToggle={onToggle}
                  />
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function AgentToggle({
  item,
  loading,
  onToggle,
}: {
  item: AgentToggleItem;
  loading: boolean;
  onToggle: (key: string, enabled: boolean) => void;
}) {
  const { t } = useTranslation();
  const disabled = Boolean(item.disabled || loading);
  const pendingSync = item.enabled && !isItemDeployed(item);
  const badge = item.badgeLabel ?? (pendingSync ? t("mySkills.agentTogglePendingSync") : null);
  return (
    <button
      type="button"
      onClick={() => onToggle(item.key, !item.enabled)}
      disabled={disabled}
      className={cn(
        "flex w-full items-center gap-2 rounded-md border px-2 py-1.5 text-left text-[12px] transition-colors",
        item.enabled ? "border-border bg-surface" : "border-border-subtle bg-bg-secondary",
        !disabled && "hover:bg-surface-hover",
        disabled && "opacity-55"
      )}
      title={badge ?? undefined}
    >
      <span className="shrink-0">
        {loading ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin text-muted" />
        ) : isItemDeployed(item) ? (
          <CheckCircle2 className="h-3.5 w-3.5 text-emerald-500" />
        ) : item.enabled ? (
          <CircleDashed className="h-3.5 w-3.5 text-amber-500" />
        ) : (
          <Circle className="h-3.5 w-3.5 text-muted" />
        )}
      </span>
      <AgentIcon
        agentKey={item.key}
        displayName={item.displayName}
        className="h-5 w-5 rounded-[4px]"
      />
      <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-secondary">
        {item.displayName}
      </span>
      {badge && (
        <span className="shrink-0 rounded-full border border-border-subtle bg-bg-secondary px-1.5 py-0.5 text-[11px] text-muted">
          {badge}
        </span>
      )}
    </button>
  );
}
