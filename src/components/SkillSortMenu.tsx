import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowDown, ArrowUp, ArrowUpDown, Check, ChevronDown } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";
import { SKILL_SORT_KEYS } from "../lib/skillSort";
import type { SkillSort, SkillSortDirection } from "../lib/skillSort";
import { ToggleSwitch } from "./ToggleSwitch";

interface Props {
  sort: SkillSort;
  onChange: (next: SkillSort) => void;
  /** Put the Preset's enabled skills first (default) or sort the whole list. */
  prioritizeEnabled: boolean;
  onPrioritizeEnabledChange: (next: boolean) => void;
}

const DIRECTIONS: SkillSortDirection[] = ["asc", "desc"];

/**
 * Sort picker for the My Skills library: by name (the default), creation time
 * or last update, each in either direction — plus a switch that drops the
 * "enabled skills first" grouping and sorts the whole list.
 */
export function SkillSortMenu({ sort, onChange, prioritizeEnabled, onPrioritizeEnabledChange }: Props) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const handlePointer = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      // The open menu consumes this Escape: it must not also reach the window
      // listeners behind it (multi-select mode would exit and drop the selection).
      e.stopPropagation();
      setOpen(false);
    };
    document.addEventListener("mousedown", handlePointer);
    document.addEventListener("keydown", handleEscape);
    return () => {
      document.removeEventListener("mousedown", handlePointer);
      document.removeEventListener("keydown", handleEscape);
    };
  }, [open]);

  const selectKey = useCallback(
    (key: SkillSort["key"]) => {
      setOpen(false);
      if (key !== sort.key) onChange({ ...sort, key });
    },
    [onChange, sort]
  );

  return (
    <div ref={containerRef} className="relative shrink-0">
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        title={t("mySkills.sort.label")}
        onClick={() => setOpen((prev) => !prev)}
        className={cn(
          "inline-flex items-center gap-1 rounded-md px-2.5 py-2 text-[13px] font-medium text-muted outline-none transition-colors hover:bg-surface-hover hover:text-secondary",
          open && "bg-surface-active text-secondary"
        )}
      >
        <ArrowUpDown className="h-3.5 w-3.5" />
        {t("mySkills.sort.label")}
        {prioritizeEnabled && (
          <span className="h-1.5 w-1.5 rounded-full bg-accent-light" aria-hidden="true" />
        )}
        <ChevronDown className={cn("h-3 w-3 transition-transform", open && "rotate-180")} />
      </button>
      {open && (
        <div
          role="menu"
          className="absolute left-0 top-full z-30 mt-1 w-[208px] rounded-lg border border-border bg-surface p-1 shadow-lg"
        >
          {SKILL_SORT_KEYS.map((key) => (
            <button
              key={key}
              type="button"
              role="menuitemradio"
              aria-checked={sort.key === key}
              onClick={() => selectKey(key)}
              className={cn(
                "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] outline-none transition-colors",
                sort.key === key
                  ? "bg-surface-active text-secondary"
                  : "text-secondary hover:bg-surface-hover"
              )}
            >
              <span className="flex h-3.5 w-3.5 shrink-0 items-center justify-center">
                {sort.key === key && <Check className="h-3.5 w-3.5" />}
              </span>
              {t(`mySkills.sort.keys.${key}`)}
            </button>
          ))}
          <div className="my-1 h-px bg-border-subtle" />
          <div className="flex items-center gap-1 px-1" role="group">
            {DIRECTIONS.map((direction) => (
              <button
                key={direction}
                type="button"
                aria-pressed={sort.direction === direction}
                onClick={() => onChange({ ...sort, direction })}
                className={cn(
                  "flex flex-1 items-center justify-center gap-1 rounded-md px-2 py-1.5 text-[12px] font-medium outline-none transition-colors",
                  sort.direction === direction
                    ? "bg-surface-active text-secondary"
                    : "text-muted hover:bg-surface-hover hover:text-secondary"
                )}
              >
                {direction === "asc" ? (
                  <ArrowUp className="h-3 w-3" />
                ) : (
                  <ArrowDown className="h-3 w-3" />
                )}
                {t(`mySkills.sort.${direction}`)}
              </button>
            ))}
          </div>
          <div className="my-1 h-px bg-border-subtle" />
          <div
            className="flex items-center gap-2 rounded-md px-2 py-1.5"
            title={t("mySkills.sort.prioritizeEnabledHint")}
          >
            <span className="flex-1 text-[13px] text-secondary">
              {t("mySkills.sort.prioritizeEnabled")}
            </span>
            <ToggleSwitch
              checked={prioritizeEnabled}
              onChange={() => onPrioritizeEnabledChange(!prioritizeEnabled)}
              title={t("mySkills.sort.prioritizeEnabled")}
            />
          </div>
        </div>
      )}
    </div>
  );
}
