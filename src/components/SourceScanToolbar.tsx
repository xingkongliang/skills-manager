/* eslint-disable react-refresh/only-export-components */
import { useTranslation } from "react-i18next";
import { DownloadCloud, Loader2, Trash2 } from "lucide-react";
import { cn } from "../utils";

/** Display filter for an expanded source's skill rows (selection is unaffected). */
export type SourceRowFilter = "all" | "installed" | "uninstalled";

/**
 * Whether a row is visible under a filter. Single source of truth shared by
 * the row list rendering and the select/deselect-visible actions, so the two
 * can never drift apart.
 */
export function sourceRowMatchesFilter(
  row: { installed: boolean },
  filter: SourceRowFilter,
): boolean {
  if (filter === "installed") return row.installed;
  if (filter === "uninstalled") return !row.installed;
  return true;
}

interface Props {
  filter: SourceRowFilter;
  onFilterChange: (filter: SourceRowFilter) => void;
  /** Checked rows across ALL rows, filter-independent (install count). */
  selectedCount: number;
  /** Checked rows that are installed and hold a backend id (remove count). */
  removableCount: number;
  /** Any install/remove in flight or this source refreshing — pauses every action. */
  busy: boolean;
  /** This source is the one installing (spinner on the install button). */
  installing: boolean;
  /** This source is the one removing (spinner on the remove button). */
  removing: boolean;
  onSelectAllVisible: () => void;
  onDeselectAllVisible: () => void;
  onInstallSelected: () => void;
  /** Opens the removal confirm dialog; the parent owns the dialog. */
  onRemoveSelected: () => void;
}

export function SourceScanToolbar({
  filter,
  onFilterChange,
  selectedCount,
  removableCount,
  busy,
  installing,
  removing,
  onSelectAllVisible,
  onDeselectAllVisible,
  onInstallSelected,
  onRemoveSelected,
}: Props) {
  const { t } = useTranslation();

  return (
    <div className="flex flex-wrap items-center justify-between gap-2">
      <div className="flex items-center gap-2 text-[13px]">
        <div className="app-segmented">
          {(["all", "installed", "uninstalled"] as const).map((mode) => (
            <button
              key={mode}
              type="button"
              onClick={() => onFilterChange(mode)}
              className={cn(
                "app-segmented-button",
                filter === mode && "app-segmented-button-active",
              )}
            >
              {t(`install.sources.filters.${mode}`)}
            </button>
          ))}
        </div>
        <span className="text-faint">·</span>
        <button
          type="button"
          onClick={onSelectAllVisible}
          disabled={busy}
          className="text-accent-light hover:underline"
        >
          {t("install.sources.selectAll")}
        </button>
        <button
          type="button"
          onClick={onDeselectAllVisible}
          disabled={busy}
          className="text-muted hover:underline"
        >
          {t("install.sources.deselectAll")}
        </button>
      </div>
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={onRemoveSelected}
          disabled={busy || removableCount === 0}
          className="inline-flex items-center gap-1.5 rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-1.5 text-[13px] font-medium text-red-400 transition-colors hover:bg-red-500/20 disabled:opacity-50"
        >
          {removing ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Trash2 className="h-3.5 w-3.5" />
          )}
          {t("install.sources.removeSelected", { count: removableCount })}
        </button>
        <button
          type="button"
          onClick={onInstallSelected}
          disabled={busy || selectedCount === 0}
          className="inline-flex items-center gap-1.5 rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white transition-colors hover:bg-accent disabled:opacity-50"
        >
          {installing ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <DownloadCloud className="h-3.5 w-3.5" />
          )}
          {t("install.sources.installSelected", {
            count: selectedCount,
          })}
        </button>
      </div>
    </div>
  );
}
