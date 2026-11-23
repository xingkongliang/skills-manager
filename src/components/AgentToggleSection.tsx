import { useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";
import { agentToggleState } from "../lib/agentToggleState";
import { AgentIcon } from "./AgentIcon";
import { ToggleSwitch } from "./ToggleSwitch";

export interface AgentToggleItem {
  key: string;
  displayName: string;
  /** Intent: enabled for this agent in the viewed preset. */
  enabled: boolean;
  /** Fact: the skill is deployed to this agent (a `skill_targets` row). */
  deployed: boolean;
  isAvailable: boolean;
  disabled?: boolean;
  badgeLabel?: string | null;
}

interface Props {
  items: AgentToggleItem[];
  togglingKey?: string | null;
  onToggle: (key: string, enabled: boolean) => void;
  /** Mark enabled-but-not-deployed rows as pending. Only true for the active preset. */
  showPendingSync: boolean;
  className?: string;
}

/**
 * Agent rows for a detail sheet's side column, shared by the library and the
 * project page. The dot is the fact (deployed or not), the switch the intent.
 */
export function AgentToggleSection({
  items,
  togglingKey,
  onToggle,
  showPendingSync,
  className,
}: Props) {
  const { t } = useTranslation();
  const [showUnavailable, setShowUnavailable] = useState(false);

  const availableItems = items.filter((item) => item.isAvailable);
  const unavailableItems = items.filter((item) => !item.isAvailable);
  // Counts the rows drawn with the green dot, so the summary and the rows
  // below it never disagree. A copy deployed by another preset but turned off
  // in the one being viewed is drawn as off and is not counted here.
  const syncedAvailableCount = availableItems.filter(
    (item) => agentToggleState(item, showPendingSync) === "deployed"
  ).length;

  return (
    <div className={className}>
      <div className="mt-1 text-[12px] text-muted tabular-nums">
        {t("mySkills.syncSummary", {
          synced: syncedAvailableCount,
          total: availableItems.length,
        })}
      </div>

      {availableItems.length > 0 && (
        <div className="mt-2">
          {availableItems.map((item) => (
            <AgentToggleRow
              key={item.key}
              item={item}
              loading={togglingKey === item.key}
              onToggle={onToggle}
              showPendingSync={showPendingSync}
            />
          ))}
        </div>
      )}

      {unavailableItems.length > 0 && (
        <div className="mt-1.5">
          <button
            type="button"
            onClick={() => setShowUnavailable((prev) => !prev)}
            aria-expanded={showUnavailable}
            className="inline-flex items-center gap-1 text-[12px] text-muted transition-colors hover:text-secondary"
          >
            {showUnavailable ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
            <span>{t("mySkills.agentUnavailableCount", { count: unavailableItems.length })}</span>
          </button>
          {showUnavailable && (
            <div className="mt-1">
              {unavailableItems.map((item) => (
                <AgentToggleRow
                  key={item.key}
                  item={item}
                  loading={togglingKey === item.key}
                  onToggle={onToggle}
                  showPendingSync={showPendingSync}
                />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function AgentToggleRow({
  item,
  loading,
  onToggle,
  showPendingSync,
}: {
  item: AgentToggleItem;
  loading: boolean;
  onToggle: (key: string, enabled: boolean) => void;
  showPendingSync: boolean;
}) {
  const { t } = useTranslation();
  const state = agentToggleState(item, showPendingSync);
  const pending = state === "pending";
  return (
    <div
      className={cn(
        "-mx-2 flex h-8 min-w-0 items-center gap-2 rounded-md px-2 transition-colors",
        !item.disabled && "hover:bg-surface-hover"
      )}
      data-state={state}
    >
      <span
        className={cn(
          "h-2 w-2 shrink-0 rounded-full",
          state === "deployed"
            ? "bg-accent-light"
            : pending
              ? "bg-amber-500"
              : "bg-surface-active"
        )}
      />
      <AgentIcon
        agentKey={item.key}
        displayName={item.displayName}
        className={cn("h-[18px] w-[18px] shrink-0 rounded-[4px]", item.disabled && "opacity-55")}
      />
      <span
        className={cn(
          "min-w-0 flex-1 truncate text-[13px]",
          item.disabled ? "text-muted" : "text-secondary"
        )}
        title={item.displayName}
      >
        {item.displayName}
      </span>
      {item.badgeLabel ? (
        <span className="shrink-0 rounded-full bg-surface-hover px-1.5 py-0.5 text-[11px] text-muted">
          {item.badgeLabel}
        </span>
      ) : pending ? (
        // The switch only flips intent; deploying a pending row is this pill's job.
        <button
          type="button"
          onClick={() => onToggle(item.key, true)}
          disabled={loading}
          title={t("mySkills.agentTogglePendingSyncHint")}
          className="shrink-0 rounded-full bg-amber-500/12 px-1.5 py-0.5 text-[11px] font-medium text-amber-600 transition-colors hover:bg-amber-500/20 disabled:opacity-50 dark:text-amber-400"
        >
          {t("mySkills.agentTogglePendingSync")}
        </button>
      ) : null}
      <ToggleSwitch
        checked={item.enabled}
        disabled={item.disabled}
        loading={loading}
        onChange={() => onToggle(item.key, !item.enabled)}
        title={item.displayName}
      />
    </div>
  );
}
