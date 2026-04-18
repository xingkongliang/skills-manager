import { useState } from "react";
import { CheckCircle2, ChevronDown, ChevronUp, Circle, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";

export interface AgentToggleItem {
  key: string;
  displayName: string;
  enabled: boolean;
  isAvailable: boolean;
  disabled?: boolean;
  badgeLabel?: string | null;
}

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
  const [isExpanded, setIsExpanded] = useState(false);

  const availableCount = items.filter((item) => item.isAvailable).length;
  const enabledAvailableCount = items.filter((item) => item.isAvailable && item.enabled).length;
  const unavailableCount = items.length - availableCount;

  return (
    <div className={cn("rounded-xl border border-border-subtle", className)}>
      <div className="border-b border-border-subtle px-6 py-2.5">
        <div className="flex items-center justify-between gap-2 text-[13px]">
          <div className="flex min-w-0 items-center gap-2">
            <span className="font-medium text-secondary">{t("mySkills.agentTogglesTitle")}</span>
            <span className="rounded-full border border-border-subtle bg-surface px-2 py-0.5 text-[12px] text-muted">
              {t("mySkills.syncSummary", {
                synced: enabledAvailableCount,
                total: availableCount,
              })}
            </span>
            {unavailableCount > 0 && (
              <span className="rounded-full border border-border-subtle bg-surface px-2 py-0.5 text-[12px] text-muted">
                {t("mySkills.agentUnavailableCount", { count: unavailableCount })}
              </span>
            )}
          </div>
          <button
            type="button"
            onClick={() => setIsExpanded((prev) => !prev)}
            aria-expanded={isExpanded}
            aria-controls="skill-agent-toggle-list"
            className="inline-flex shrink-0 items-center gap-1 rounded-[6px] border border-border-subtle bg-surface px-2 py-1 text-[12px] text-muted transition-colors hover:text-secondary"
            title={
              isExpanded
                ? t("mySkills.collapseAgentToggles")
                : t("mySkills.expandAgentToggles")
            }
          >
            <span>
              {isExpanded
                ? t("mySkills.collapseAgentToggles")
                : t("mySkills.expandAgentToggles")}
            </span>
            {isExpanded ? (
              <ChevronUp className="h-3.5 w-3.5" />
            ) : (
              <ChevronDown className="h-3.5 w-3.5" />
            )}
          </button>
        </div>
        {isExpanded && (
          <div id="skill-agent-toggle-list" className="mt-2 grid grid-cols-2 gap-1.5 md:grid-cols-3">
            {items.map((item) => {
              const loading = togglingKey === item.key;
              const disabled = Boolean(item.disabled || loading);
              return (
                <button
                  key={item.key}
                  type="button"
                  onClick={() => onToggle(item.key, !item.enabled)}
                  disabled={disabled}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-[6px] border px-2 py-1.5 text-left text-[12px] transition-colors",
                    item.enabled ? "border-border bg-surface" : "border-border-subtle bg-bg-secondary",
                    !disabled && "hover:bg-surface-hover",
                    disabled && "opacity-55"
                  )}
                  title={item.badgeLabel ?? undefined}
                >
                  <span className="shrink-0">
                    {loading ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin text-muted" />
                    ) : item.enabled ? (
                      <CheckCircle2 className="h-3.5 w-3.5 text-emerald-500" />
                    ) : (
                      <Circle className="h-3.5 w-3.5 text-muted" />
                    )}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-[12.5px] font-medium text-secondary">
                    {item.displayName}
                  </span>
                  {item.badgeLabel && (
                    <span className="shrink-0 rounded-full border border-border-subtle bg-bg-secondary px-1.5 py-0.5 text-[11px] text-muted">
                      {item.badgeLabel}
                    </span>
                  )}
                </button>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
