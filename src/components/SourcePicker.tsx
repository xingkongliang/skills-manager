import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderTree, Square, SquareCheck, X } from "lucide-react";
import { cn } from "../utils";
import type { SkillSourceCandidate } from "../lib/skillSource";

interface Props {
  /**
   * The Sources on offer for the list being filtered — see `sourceCandidates`,
   * which must be computed before the Source filter itself is applied.
   */
  candidates: SkillSourceCandidate[];
  selected: ReadonlySet<string>;
  onChange: (next: Set<string>) => void;
  className?: string;
}

/**
 * The Source filter: a menu rather than pills, because a library can hold
 * dozens of repositories and directories. My Skills and the drawer that adds
 * skills to an Agent or a project both render this, so picking a Source works
 * the same in both.
 */
export function SourcePicker({ candidates, selected, onChange, className }: Props) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const nothingToOffer = candidates.length === 0;

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  const toggle = (key: string) => {
    const next = new Set(selected);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    onChange(next);
  };

  return (
    <div className={cn("relative", className)}>
      <button
        type="button"
        onClick={() => setOpen((wasOpen) => !wasOpen)}
        aria-expanded={open}
        aria-haspopup="true"
        disabled={nothingToOffer}
        className={cn(
          "inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors disabled:opacity-40",
          selected.size > 0
            ? "bg-accent text-white dark:bg-accent dark:text-white"
            : "bg-surface-hover text-muted hover:text-secondary"
        )}
      >
        <FolderTree className="h-3 w-3" />
        {t("mySkills.sourcePicker.label")}
        {selected.size > 0 && ` (${selected.size})`}
      </button>
      {open && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          {/* overscroll-contain stops a wheel that reaches the end of the list
              from scrolling the page underneath it. */}
          <div className="absolute left-0 top-full z-50 mt-1 max-h-72 w-64 overflow-y-auto overscroll-contain rounded-lg border border-border bg-surface py-1 shadow-2xl">
            {selected.size > 0 && (
              <button
                type="button"
                onClick={() => onChange(new Set())}
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[12px] text-muted hover:bg-surface-hover hover:text-secondary"
              >
                <X className="h-3 w-3" />
                {t("mySkills.sourcePicker.clear")}
              </button>
            )}
            {candidates.map((candidate) => {
              const label = candidate.label || t("mySkills.sourcePicker.unknownSource");
              const checked = selected.has(candidate.key);
              return (
                <button
                  key={candidate.key}
                  type="button"
                  aria-pressed={checked}
                  onClick={() => toggle(candidate.key)}
                  className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[12px] text-secondary hover:bg-surface-hover"
                  title={label}
                >
                  {checked ? (
                    <SquareCheck className="h-3.5 w-3.5 shrink-0 text-accent" />
                  ) : (
                    <Square className="h-3.5 w-3.5 shrink-0 text-faint" />
                  )}
                  <span className="min-w-0 flex-1 truncate">{label}</span>
                  <span className="shrink-0 text-[11px] text-faint">{candidate.count}</span>
                </button>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}
