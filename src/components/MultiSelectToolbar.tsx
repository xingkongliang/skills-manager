import { Trash2, CheckCircle2, Circle, Sparkles, Loader2 } from "lucide-react";
import { cn } from "../utils";

interface MultiSelectToolbarLabels {
  hint: string;
  selected: string;
  delete: string;
  enable: string;
  disable: string;
  selectAll: string;
  deselectAll: string;
  cancel: string;
  aiTag?: string;
}

interface MultiSelectToolbarProps {
  selectedCount: number;
  isAllSelected: boolean;
  anyDisabled: boolean;
  showToggle: boolean;
  labels: MultiSelectToolbarLabels;
  onDelete: () => void;
  onToggle: () => void;
  onSelectAll: () => void;
  onCancel: () => void;
  onAiTag?: () => void;
  aiTagging?: boolean;
}

export function MultiSelectToolbar({
  selectedCount,
  isAllSelected,
  anyDisabled,
  showToggle,
  labels,
  onDelete,
  onToggle,
  onSelectAll,
  onCancel,
  onAiTag,
  aiTagging,
}: MultiSelectToolbarProps) {
  return (
    <div className="flex items-center gap-2 px-1 py-1.5">
      <span className="text-[13px] text-muted">
        {selectedCount > 0 ? labels.selected : labels.hint}
      </span>
      {selectedCount > 0 && (
        <>
          <button
            onClick={onDelete}
            className="inline-flex items-center gap-1.5 rounded-md bg-red-600/90 px-2.5 py-1 text-[13px] font-medium text-white hover:bg-red-500 transition-colors"
          >
            <Trash2 className="h-3.5 w-3.5" />
            {labels.delete}
          </button>
          {showToggle && (
            <button
              onClick={onToggle}
              className={cn(
                "inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[13px] font-medium text-white transition-colors",
                anyDisabled
                  ? "bg-emerald-600/90 hover:bg-emerald-500"
                  : "bg-amber-600/90 hover:bg-amber-500"
              )}
            >
              {anyDisabled
                ? <CheckCircle2 className="h-3.5 w-3.5" />
                : <Circle className="h-3.5 w-3.5" />}
              {anyDisabled ? labels.enable : labels.disable}
            </button>
          )}
          {onAiTag && (
            <button
              onClick={onAiTag}
              disabled={aiTagging}
              className="inline-flex items-center gap-1.5 rounded-md bg-accent/90 px-2.5 py-1 text-[13px] font-medium text-white hover:bg-accent transition-colors disabled:opacity-50"
            >
              {aiTagging
                ? <Loader2 className="h-3.5 w-3.5 animate-spin" />
                : <Sparkles className="h-3.5 w-3.5" />}
              {labels.aiTag}
            </button>
          )}
        </>
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
