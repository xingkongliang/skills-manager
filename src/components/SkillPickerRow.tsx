import type { MouseEvent } from "react";
import { Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";
import type { ManagedSkill } from "../lib/tauri";
import type { PickerStatus } from "../lib/skillPickerStatus";
import { getTagColor } from "../lib/skillTags";

interface Props {
  skill: ManagedSkill;
  status: PickerStatus;
  allTags: string[];
  sourceLabel: string;
  selected: boolean;
  onToggle: (e: MouseEvent<HTMLDivElement>) => void;
  busy?: boolean;
}

export function SkillPickerRow({
  skill,
  status,
  allTags,
  sourceLabel,
  selected,
  onToggle,
  busy,
}: Props) {
  const { t } = useTranslation();
  const selectable = status === "available" && !busy;

  const statusLabel: Record<PickerStatus, string> = {
    available: "",
    installed: t("addFromLibrary.status.installed"),
    conflict: t("addFromLibrary.status.conflict"),
    unavailable: t("addFromLibrary.status.unavailable"),
  };

  const tooltip =
    status === "conflict"
      ? t("addFromLibrary.tooltip.conflict")
      : status === "installed"
        ? t("addFromLibrary.tooltip.installed")
        : status === "unavailable"
          ? t("addFromLibrary.tooltip.unavailable")
          : undefined;

  return (
    <div
      onClick={selectable ? onToggle : undefined}
      title={tooltip}
      className={cn(
        "flex items-center gap-3 px-5 py-2.5 transition-colors",
        selectable && "cursor-pointer hover:bg-surface-hover",
        selectable && selected && "bg-accent-bg/40",
        !selectable && "opacity-60",
      )}
    >
      <div
        className={cn(
          "flex h-4 w-4 shrink-0 items-center justify-center rounded border transition-colors",
          selectable && selected
            ? "border-accent bg-accent text-white"
            : selectable
              ? "border-border"
              : "border-border-subtle",
        )}
      >
        {selectable && selected && (
          <svg viewBox="0 0 16 16" fill="currentColor" className="h-3 w-3">
            <path d="M13.854 3.646a.5.5 0 0 1 0 .708l-7 7a.5.5 0 0 1-.708 0l-3.5-3.5a.5.5 0 1 1 .708-.708L6.5 10.293l6.646-6.647a.5.5 0 0 1 .708 0z" />
          </svg>
        )}
      </div>

      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate text-[13px] font-medium text-primary">{skill.name}</span>
          <span className="shrink-0 rounded-full bg-surface-hover px-1.5 py-0.5 text-[11px] font-medium text-muted">
            {sourceLabel}
          </span>
        </div>
        {skill.description && (
          <div className="mt-0.5 truncate text-[12px] text-muted">{skill.description}</div>
        )}
        {skill.tags.length > 0 && (
          <div className="mt-1 flex flex-wrap gap-1">
            {skill.tags.map((tag) => (
              <span
                key={tag}
                className={cn(
                  "inline-flex items-center rounded-full px-1.5 py-0.5 text-[10.5px] font-medium",
                  getTagColor(tag, allTags),
                )}
              >
                {tag}
              </span>
            ))}
          </div>
        )}
      </div>

      {busy ? (
        <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-muted" />
      ) : status !== "available" ? (
        <span
          className={cn(
            "shrink-0 rounded-full px-2 py-0.5 text-[11px] font-medium",
            status === "installed" && "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
            status === "conflict" && "bg-rose-500/10 text-rose-600 dark:text-rose-400",
            status === "unavailable" && "bg-surface-hover text-muted",
          )}
        >
          {statusLabel[status]}
        </span>
      ) : null}
    </div>
  );
}
