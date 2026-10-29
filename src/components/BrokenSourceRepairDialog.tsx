import { useCallback, useEffect, useRef, useState } from "react";
import { FolderInput, FolderSearch, X, Wrench, Loader2, FolderOpen } from "lucide-react";
import { useTranslation } from "react-i18next";
import { open as dialogOpen } from "@tauri-apps/plugin-dialog";
import { cn } from "../utils";
import { toast } from "sonner";
import {
  scanBrokenLocalSources,
  relinkLocalSourcesBatch,
  type BrokenLocalSourceEntry,
  type RelinkSourceItem,
} from "../lib/tauri";
import { getErrorMessage } from "../lib/error";

interface Props {
  open: boolean;
  onClose: () => void;
  /** Called after at least one skill was re-pointed, so the list refreshes. */
  onRepaired: () => void;
}

const ROOT_STORAGE_KEY = "sm.brokenSourceScanRoot";

/**
 * Display form for long paths: lead with the informative tail. Inside the
 * scan root show the root-relative part; otherwise keep the last two
 * segments. The full path always stays available via `title`.
 */
const shortenPath = (path: string, root: string): string => {
  const cleanRoot = root.replace(/\/+$/, "");
  if (cleanRoot && path.startsWith(cleanRoot + "/")) {
    return path.slice(cleanRoot.length + 1);
  }
  const parts = path.split("/").filter(Boolean);
  if (parts.length <= 2) return path;
  return "…/" + parts.slice(-2).join("/");
};

type RowState = {
  entry: BrokenLocalSourceEntry;
  /** Chosen replacement path; null = not selected for repair. */
  chosen: string | null;
};

export function BrokenSourceRepairDialog({ open, onClose, onRepaired }: Props) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(false);
  const [rows, setRows] = useState<RowState[]>([]);
  const [root, setRoot] = useState<string>("");
  const [applying, setApplying] = useState(false);
  /** Errors from the last apply batch, keyed by skill_id. Non-empty = the
   * dialog stays open showing only the failed rows for retry. */
  const [lastErrors, setLastErrors] = useState<Map<string, string>>(new Map());
  const requestIdRef = useRef(0);

  const runScan = useCallback(async (scanRoot: string | null) => {
    const requestId = ++requestIdRef.current;
    setLoading(true);
    try {
      const report = await scanBrokenLocalSources(scanRoot);
      if (requestId !== requestIdRef.current) return;
      const next: RowState[] = report.entries.map((entry) => ({
        entry,
        // Default to the first same-name suggestion; a manual pick overrides.
        chosen: entry.suggestions[0] ?? null,
      }));
      setRows(next);
      setLastErrors(new Map());
    } catch (error: unknown) {
      if (requestId === requestIdRef.current) {
        setRows([]);
        setLastErrors(new Map());
        toast.error(getErrorMessage(error, t("common.error")));
      }
    } finally {
      if (requestId === requestIdRef.current) setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    if (!open) return;
    // Scan WITH the remembered root, not null — displaying it while scanning
    // rootless left every row suggestionless and its checkbox dead until the
    // user re-picked the root.
    const saved = localStorage.getItem(ROOT_STORAGE_KEY) ?? "";
    setRoot(saved);
    runScan(saved || null);
  }, [open, runScan]);

  if (!open) return null;

  const pickRoot = async () => {
    const selected = await dialogOpen({ directory: true, multiple: false });
    if (!selected || Array.isArray(selected)) return;
    setRoot(selected);
    localStorage.setItem(ROOT_STORAGE_KEY, selected);
    await runScan(selected);
  };

  const rescanWithoutRoot = async () => {
    setRoot("");
    localStorage.removeItem(ROOT_STORAGE_KEY);
    await runScan(null);
  };

  const setChosen = (skillId: string, path: string | null) => {
    setRows((prev) =>
      prev.map((row) => (row.entry.skill_id === skillId ? { ...row, chosen: path } : row)),
    );
  };

  const pickManually = async (skillId: string) => {
    const selected = await dialogOpen({ directory: true, multiple: false });
    if (!selected || Array.isArray(selected)) return;
    setChosen(skillId, selected);
  };

  const selectedItems: RelinkSourceItem[] = rows
    .filter((row) => row.chosen)
    .map((row) => ({ skill_id: row.entry.skill_id, source_path: row.chosen as string }));

  const handleApply = async () => {
    if (selectedItems.length === 0 || applying) return;
    setApplying(true);
    try {
      const results = await relinkLocalSourcesBatch(selectedItems);
      const failed = results.filter((r) => !r.ok);
      const fixed = results.length - failed.length;
      if (fixed > 0) onRepaired();
      if (failed.length === 0) {
        // Every row repaired: close the dialog and let the toast carry the
        // short confirmation.
        toast.success(t("mySkills.repairDialog.allDone", { fixed }), {
          duration: 3000,
        });
        onClose();
        return;
      }
      // Some rows failed: stay on the dialog with only the failed rows and
      // their selections, so a corrected path can be retried right away.
      const errMap = new Map(
        failed.map((r) => [r.skill_id, r.error ?? "Unknown error"]),
      );
      setLastErrors(errMap);
      setRows((prev) =>
        prev
          .filter((row) => errMap.has(row.entry.skill_id))
          // A failed row's selected path can drop out of the suggestion list
          // (typically a manually picked directory that disappeared). Reset it
          // to the first suggestion — otherwise the dropdown shows a directory
          // the retry would not submit, and retrying fails on the old path.
          .map((row) =>
            row.chosen && !row.entry.suggestions.includes(row.chosen)
              ? { ...row, chosen: row.entry.suggestions[0] ?? null }
              : row,
          ),
      );
      toast.warning(
        t("mySkills.repairDialog.someFailed", { fixed, failed: failed.length }),
        { duration: 3000 },
      );
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setApplying(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div className="relative bg-surface border border-border rounded-xl w-full max-w-[640px] max-h-[80vh] flex flex-col p-5 shadow-2xl">
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-[13px] font-semibold text-primary flex items-center gap-2">
            <Wrench className="w-4 h-4 text-accent-light" />
            {t("mySkills.repairDialog.title")}
          </h2>
          <button
            onClick={onClose}
            className="text-muted hover:text-secondary p-1 rounded-md transition-colors outline-none"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <p className="mb-3 rounded-lg bg-surface-hover px-3 py-2 text-[12px] leading-snug text-muted">
          {t("mySkills.repairDialog.intro")}
        </p>

        <div className="flex flex-wrap items-center gap-2 mb-3">
          <button
            type="button"
            onClick={pickRoot}
            disabled={loading || applying}
            className="inline-flex items-center gap-1.5 rounded-md border border-border-subtle px-3 py-1.5 text-[12px] font-medium text-secondary transition-colors hover:bg-surface-hover disabled:opacity-50"
          >
            <FolderSearch className="h-3.5 w-3.5" />
            {t("mySkills.repairDialog.pickRoot")}
          </button>
          {root && (
            <>
              <span className="font-mono text-[11px] text-muted truncate max-w-[280px]" title={root}>
                {shortenPath(root, "")}
              </span>
              <button
                type="button"
                onClick={rescanWithoutRoot}
                disabled={loading || applying}
                className="text-[11px] text-faint hover:text-secondary underline underline-offset-2 disabled:opacity-50"
              >
                {t("mySkills.repairDialog.clearRoot")}
              </button>
            </>
          )}
        </div>

        {lastErrors.size > 0 && (
          <p className="mb-3 rounded-lg bg-red-500/10 px-3 py-2 text-[12px] leading-snug text-red-600 dark:text-red-400">
            {t("mySkills.repairDialog.failedHint", { count: lastErrors.size })}
          </p>
        )}

        <div className="flex-1 min-h-0 overflow-y-auto border border-border-subtle rounded-lg">
          {loading ? (
            <div className="flex items-center justify-center gap-2 py-10 text-[12px] text-muted">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              {t("mySkills.repairDialog.scanning")}
            </div>
          ) : rows.length === 0 ? (
            <div className="py-10 text-center text-[12px] text-muted">
              {t("mySkills.repairDialog.noBroken")}
            </div>
          ) : (
            <ul className="divide-y divide-border-subtle">
              {rows.map(({ entry, chosen }) => (
                <li key={entry.skill_id} className="px-3 py-2.5">
                  <div className="flex items-center gap-2">
                    <input
                      type="checkbox"
                      checked={!!chosen}
                      onChange={(e) =>
                        setChosen(
                          entry.skill_id,
                          e.target.checked ? entry.suggestions[0] ?? null : null,
                        )
                      }
                      // Nothing to check when there is no suggestion and no
                      // manual pick yet — an empty-string chosen would render
                      // as unchecked anyway.
                      disabled={applying || (!chosen && entry.suggestions.length === 0)}
                      className="accent-[var(--accent)] shrink-0"
                    />
                    <span className="text-[13px] font-medium text-secondary truncate">
                      {entry.name}
                    </span>
                    <span
                      className="font-mono text-[11px] text-faint truncate"
                      title={entry.old_source_ref}
                    >
                      {shortenPath(entry.old_source_ref, root)}
                    </span>
                  </div>
                  <div className="mt-1.5 flex items-center gap-2 pl-6">
                    {entry.suggestions.length > 1 ? (
                      <select
                        value={chosen ?? ""}
                        onChange={(e) => setChosen(entry.skill_id, e.target.value || null)}
                        disabled={applying}
                        className="h-6 flex-1 min-w-0 rounded-md border border-border-subtle bg-transparent px-1.5 font-mono text-[11px] text-secondary outline-none focus:border-accent"
                      >
                        {!chosen && <option value="">{t("mySkills.repairDialog.chooseSuggestion")}</option>}
                        {entry.suggestions.map((s) => (
                          <option key={s} value={s}>{shortenPath(s, root)}</option>
                        ))}
                      </select>
                    ) : chosen ? (
                      <span className="font-mono text-[11px] text-emerald-600 dark:text-emerald-400 truncate" title={chosen}>
                        {shortenPath(chosen, root)}
                      </span>
                    ) : (
                      <span className="text-[11px] text-faint">
                        {entry.suggestions.length === 0 && root
                          ? t("mySkills.repairDialog.noSuggestion")
                          : t("mySkills.repairDialog.noSuggestionNoRoot")}
                      </span>
                    )}
                    <button
                      type="button"
                      onClick={() => pickManually(entry.skill_id)}
                      disabled={applying}
                      className="ml-auto inline-flex shrink-0 items-center gap-1 rounded-md px-2 py-0.5 text-[11px] text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
                    >
                      <FolderOpen className="h-3 w-3" />
                      {t("mySkills.repairDialog.pickManually")}
                    </button>
                  </div>
                  {lastErrors.get(entry.skill_id) && (
                    <p className="mt-1 pl-6 text-[11px] leading-snug text-red-600 dark:text-red-400">
                      {lastErrors.get(entry.skill_id)}
                    </p>
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>

        <div className="mt-4 flex items-center justify-between gap-3">
          <span className="text-[12px] text-muted">
            {t("mySkills.repairDialog.selectedCount", { count: selectedItems.length })}
          </span>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={onClose}
              disabled={applying}
              className="rounded-md px-3 py-1.5 text-[12px] font-medium text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
            >
              {t("common.cancel")}
            </button>
            <button
              type="button"
              onClick={handleApply}
              disabled={applying || loading || selectedItems.length === 0}
              className={cn(
                "inline-flex items-center gap-1.5 rounded-md bg-accent px-3 py-1.5 text-[12px] font-medium text-white transition-colors hover:bg-accent/90 disabled:opacity-50",
              )}
            >
              {applying ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <FolderInput className="h-3.5 w-3.5" />
              )}
              {t("mySkills.repairDialog.apply", { count: selectedItems.length })}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
