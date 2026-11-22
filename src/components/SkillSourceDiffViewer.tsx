import { useState } from "react";
import { ChevronRight } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";
import { DocumentDiffViewer } from "./DocumentDiffViewer";
import { diffExceedsBudget } from "../lib/diffBudget";
import type { SkillSourceDiffEntry } from "../lib/tauri";

interface Props {
  entries: SkillSourceDiffEntry[];
  /** More files differ than `entries` lists (the backend stops at its limit). */
  truncated?: boolean;
  className?: string;
}

const STATUS_TONE: Record<SkillSourceDiffEntry["status"], string> = {
  added: "border-emerald-300 bg-emerald-50 text-emerald-700 dark:bg-emerald-950/40 dark:text-emerald-300",
  removed: "border-red-300 bg-red-50 text-red-700 dark:bg-red-950/40 dark:text-red-300",
  modified: "border-sky-300 bg-sky-50 text-sky-700 dark:bg-sky-950/40 dark:text-sky-300",
};

/** Up to this many files are all shown open, as before; a longer list starts
 * collapsed and opens one file at a time, so the rendered diffs stay bounded
 * however many files changed (#658). */
const EXPAND_ALL_MAX = 3;

function fitsBudget(entry: SkillSourceDiffEntry): boolean {
  return entry.content_kind !== "text" || !diffExceedsBudget(entry.original_text ?? "", entry.updated_text ?? "");
}

export function SkillSourceDiffViewer({ entries, truncated = false, className }: Props) {
  const { t } = useTranslation();
  const [openPath, setOpenPath] = useState<string | null>(null);

  if (entries.length === 0) {
    return (
      <div className={cn("rounded-xl border border-border-subtle bg-bg-secondary px-4 py-6 text-center", className)}>
        <div className="text-[13px] font-medium text-secondary">{t("mySkills.sourceDiff.noChanges")}</div>
      </div>
    );
  }

  const expandAll = entries.length <= EXPAND_ALL_MAX && entries.every(fitsBudget);

  return (
    <div className={cn("space-y-4", className)}>
      {(truncated || !expandAll) && (
        <div className="text-[12px] text-muted">
          {truncated
            ? t("mySkills.sourceDiff.truncated", { count: entries.length })
            : t("mySkills.sourceDiff.collapsedHint", { count: entries.length })}
        </div>
      )}
      {entries.map((entry) => {
        const open = expandAll || openPath === entry.relative_path;
        const header = (
          <>
            {!expandAll && (
              <ChevronRight
                className={cn("h-3.5 w-3.5 shrink-0 text-muted transition-transform", open && "rotate-90")}
              />
            )}
            <span
              className={cn(
                "inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-medium",
                STATUS_TONE[entry.status]
              )}
            >
              {t(`mySkills.sourceDiff.status.${entry.status}`)}
            </span>
            <span className="min-w-0 flex-1 truncate font-mono text-[13px] text-secondary" title={entry.relative_path}>
              {entry.relative_path}
            </span>
            {entry.status === "modified" && entry.executable_before !== entry.executable_after && (
              <span className="inline-flex items-center rounded-full border border-border-subtle bg-bg-secondary px-2 py-0.5 text-[11px] text-muted">
                {t("mySkills.sourceDiff.execBit", {
                  before: entry.executable_before ? "0755" : "0644",
                  after: entry.executable_after ? "0755" : "0644",
                })}
              </span>
            )}
          </>
        );
        const headerClass = cn(
          "flex w-full flex-wrap items-center gap-2 bg-surface px-3 py-2 text-left",
          open && "border-b border-border-subtle"
        );

        return (
          <div key={entry.relative_path} className="overflow-hidden rounded-xl border border-border-subtle">
            {expandAll ? (
              <div className={headerClass}>{header}</div>
            ) : (
              <button
                type="button"
                className={cn(headerClass, "hover:bg-surface-hover")}
                aria-expanded={open}
                onClick={() => setOpenPath(open ? null : entry.relative_path)}
              >
                {header}
              </button>
            )}

            {open &&
              (entry.content_kind === "text" ? (
                <DocumentDiffViewer
                  original={entry.original_text ?? ""}
                  updated={entry.updated_text ?? ""}
                  className="!space-y-0 !rounded-none !border-0"
                />
              ) : (
                <div className="px-4 py-4 text-[12px] text-muted">
                  {t(`mySkills.sourceDiff.summary.${entry.content_kind}`)}
                </div>
              ))}
          </div>
        );
      })}
    </div>
  );
}
