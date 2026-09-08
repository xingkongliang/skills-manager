import type { ReactNode } from "react";
import { Loader2 } from "lucide-react";
import { cn } from "../utils";
import { CardActionMenu } from "./CardActionMenu";

export interface BulkAction {
  key: string;
  label: string;
  icon: ReactNode;
  onSelect: () => void;
  /** Primary = the action this page exists for; danger = destructive. Default: secondary. */
  tone?: "primary" | "secondary" | "danger";
  busy?: boolean;
  disabled?: boolean;
}

interface MultiSelectToolbarLabels {
  hint: string;
  selected: string;
  selectAll: string;
  deselectAll: string;
  cancel: string;
  more: string;
}

interface MultiSelectToolbarProps {
  selectedCount: number;
  isAllSelected: boolean;
  /** Buttons shown inline. Keep this to the two or three actions users reach for. */
  actions: BulkAction[];
  /** Low-frequency actions, collapsed into the "…" menu. */
  overflowActions?: BulkAction[];
  labels: MultiSelectToolbarLabels;
  onSelectAll: () => void;
  onCancel: () => void;
}

const TONE_CLASS: Record<NonNullable<BulkAction["tone"]>, string> = {
  primary:
    "border border-accent-border bg-accent-dark text-white hover:bg-accent",
  secondary:
    "border border-border-subtle bg-surface text-secondary hover:bg-surface-hover",
  danger:
    "border border-transparent text-danger hover:bg-danger-bg",
};

export function MultiSelectToolbar({
  selectedCount,
  isAllSelected,
  actions,
  overflowActions = [],
  labels,
  onSelectAll,
  onCancel,
}: MultiSelectToolbarProps) {
  const hasSelection = selectedCount > 0;
  const inlineActions = hasSelection ? actions : [];
  const menuActions = hasSelection ? overflowActions : [];

  return (
    <div className="flex items-center gap-2 px-1 py-1.5">
      <span className="text-[13px] text-muted tabular-nums">
        {hasSelection ? labels.selected : labels.hint}
      </span>

      {inlineActions.map((action) => (
        <button
          key={action.key}
          onClick={action.onSelect}
          disabled={action.disabled || action.busy}
          className={cn(
            "inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1 text-[13px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50",
            TONE_CLASS[action.tone ?? "secondary"]
          )}
        >
          {action.busy
            ? <Loader2 className="h-3.5 w-3.5 animate-spin" />
            : action.icon}
          {action.label}
        </button>
      ))}

      {menuActions.length > 0 && (
        <CardActionMenu
          label={labels.more}
          actions={menuActions.map((action) => ({
            key: action.key,
            label: action.label,
            icon: action.busy
              ? <Loader2 className="h-3.5 w-3.5 animate-spin" />
              : action.icon,
            onSelect: action.onSelect,
            danger: action.tone === "danger",
            disabled: action.disabled || action.busy,
          }))}
        />
      )}

      <button
        onClick={onSelectAll}
        className="rounded-md px-2.5 py-1 text-[13px] font-medium text-muted hover:text-secondary hover:bg-surface-hover transition-colors"
      >
        {isAllSelected ? labels.deselectAll : labels.selectAll}
      </button>
      <button
        onClick={onCancel}
        className="rounded-md px-2.5 py-1 text-[13px] font-medium text-muted hover:text-secondary hover:bg-surface-hover transition-colors"
      >
        {labels.cancel}
      </button>
    </div>
  );
}
