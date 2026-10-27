import { useMemo, useState } from "react";
import { Check, ChevronDown, ChevronRight, FileWarning, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";
import type { PendingRemovalGroup } from "../lib/tauri";
import {
  groupPendingRemovals,
  removalPathCount,
  type PendingRemovalSkill,
} from "../lib/pendingRemovals";
import { toggleAll, toggleOne, toggleRange } from "../lib/newSkillSelection";

interface Props {
  groups: PendingRemovalGroup[];
  approving: boolean;
  onApprove: (skillIds: string[]) => void;
  onClose: () => void;
}

/**
 * The skills whose last update would have deleted files.
 *
 * One row per skill, expandable to the exact paths, grouped by the directory
 * that holds them. Nothing is selected by default: approving is a destructive
 * act, and the shift-range/all gestures are the same ones the new-skill prompt
 * and the library use.
 */
export function PendingRemovalsDialog({ groups, approving, onApprove, onClose }: Props) {
  const { t } = useTranslation();
  const rows = useMemo(() => groupPendingRemovals(groups), [groups]);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  // Shift-click ranges need a starting row; it moves with every plain click.
  const [anchor, setAnchor] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  const ordered = useMemo(() => rows.map((row) => row.id), [rows]);
  const selected = rows.filter((row) => chosen.has(row.id));
  const allChosen = ordered.length > 0 && selected.length === ordered.length;
  const totalPaths = rows.reduce((total, row) => total + removalPathCount(row), 0);

  if (rows.length === 0) return null;

  const toggle = (row: PendingRemovalSkill, shiftKey: boolean) => {
    if (shiftKey && anchor) {
      setChosen((prev) => toggleRange(prev, ordered, anchor, row.id));
      return;
    }
    setChosen((prev) => toggleOne(prev, row.id));
    setAnchor(row.id);
  };

  const toggleExpanded = (id: string) => setExpanded((prev) => toggleOne(prev, id));

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div className="relative flex max-h-[80vh] w-full max-w-xl flex-col rounded-xl border border-border bg-surface p-5 shadow-2xl">
        <div className="mb-1 flex items-center justify-between">
          <h2 className="text-[13px] font-semibold text-primary">
            {t("mySkills.pendingRemovals.title", { count: rows.length })}
          </h2>
          <button
            onClick={onClose}
            disabled={approving}
            className="rounded-md p-1 text-muted transition-colors hover:text-secondary disabled:opacity-50"
            aria-label={t("common.close")}
          >
            <X className="h-4 w-4" />
          </button>
        </div>
        <p className="mb-4 text-[12px] text-muted">
          {t("mySkills.pendingRemovals.hint", { count: totalPaths })}
        </p>

        <div className="-mx-1 min-h-0 flex-1 overflow-y-auto px-1">
          {rows.map((row) => {
            const checked = chosen.has(row.id);
            const open = expanded.has(row.id);
            const pathCount = removalPathCount(row);
            return (
              <div
                key={row.id}
                className={cn(
                  "mb-1 rounded-md transition-colors",
                  checked ? "bg-surface-active" : "hover:bg-surface-hover"
                )}
              >
                <div className="flex items-center gap-2 px-2 py-1.5">
                  <button
                    type="button"
                    onClick={(event) => toggle(row, event.shiftKey)}
                    className="flex min-w-0 flex-1 items-center gap-2 text-left"
                  >
                    <span
                      className={cn(
                        "flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-md border",
                        checked ? "border-accent bg-accent text-white" : "border-border"
                      )}
                    >
                      {checked && <Check className="h-2.5 w-2.5" />}
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="flex items-center gap-1.5">
                        <span className="truncate text-[13px] text-secondary">{row.name}</span>
                        <span className="shrink-0 text-[11px] text-faint">
                          {t("mySkills.pendingRemovals.paths", { count: pathCount })}
                        </span>
                      </span>
                      <span className="block truncate text-[11px] text-faint" title={row.source}>
                        {row.source || row.id}
                      </span>
                    </span>
                  </button>
                  <button
                    type="button"
                    onClick={() => toggleExpanded(row.id)}
                    className="shrink-0 rounded-md p-1 text-faint transition-colors hover:bg-surface-hover hover:text-secondary"
                    aria-label={
                      open
                        ? t("mySkills.pendingRemovals.collapse")
                        : t("mySkills.pendingRemovals.expand")
                    }
                  >
                    {open ? (
                      <ChevronDown className="h-3.5 w-3.5" />
                    ) : (
                      <ChevronRight className="h-3.5 w-3.5" />
                    )}
                  </button>
                </div>
                {open && (
                  <div className="border-t border-border-subtle px-3 pb-2 pt-2">
                    {row.locations.map((entry) => (
                      <div key={entry.location} className="mb-2 last:mb-0">
                        <div className="mb-1 flex items-center gap-1 text-[11px] font-medium text-tertiary">
                          <FileWarning className="h-3 w-3" />
                          {entry.location === "library"
                            ? t("mySkills.pendingRemovals.library")
                            : entry.location}
                        </div>
                        {entry.paths.map((path) => (
                          <div
                            key={`${entry.location}:${path}`}
                            className="whitespace-pre-wrap break-all font-mono text-[11px] text-faint"
                          >
                            {path}
                          </div>
                        ))}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>

        <div className="mt-4 flex items-center justify-between gap-2">
          <button
            type="button"
            onClick={() => setChosen((prev) => toggleAll(prev, ordered))}
            className="rounded-md px-2 py-0.5 text-[12px] font-medium text-muted transition-colors hover:bg-surface-hover hover:text-secondary"
          >
            {allChosen ? t("mySkills.deselectAll") : t("mySkills.selectAll")}
          </button>
          <div className="flex gap-2">
            <button
              onClick={onClose}
              disabled={approving}
              className="rounded-lg px-3 py-1.5 text-[13px] font-medium text-tertiary transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
            >
              {t("mySkills.pendingRemovals.close")}
            </button>
            <button
              onClick={() => onApprove(selected.map((row) => row.id))}
              disabled={approving || selected.length === 0}
              className="rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
            >
              {t("mySkills.pendingRemovals.approve", { count: selected.length })}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
