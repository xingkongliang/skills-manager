import { useCallback, useEffect, useState } from "react";
import { DEFAULT_SKILL_SORT, isSkillSortDirection, isSkillSortKey } from "../lib/skillSort";
import type { SkillSort } from "../lib/skillSort";

export type SkillViewMode = "grid" | "list";

/**
 * My Skills view preferences, kept under one key so adding a preference does
 * not add a second storage entry to keep in sync.
 *
 * The stored value used to be the bare string `"grid"` / `"list"`; that form is
 * still read, so an existing installation keeps its view mode. `sort` and
 * `prioritizeEnabled` arrived later, so a stored preference without them falls
 * back to their defaults (name ascending, prioritize enabled).
 */
interface ViewPreference {
  viewMode: SkillViewMode;
  groupBySource: boolean;
  sort: SkillSort;
  /** "优先展示已开启": list the Preset's enabled skills first. Defaults to on. */
  prioritizeEnabled: boolean;
}

const STORAGE_KEY = "skills-manager.skillViewMode";

const DEFAULT_PREFERENCE: ViewPreference = {
  viewMode: "grid",
  groupBySource: false,
  sort: DEFAULT_SKILL_SORT,
  prioritizeEnabled: true,
};

function readSort(value: unknown): SkillSort {
  if (!value || typeof value !== "object") return DEFAULT_SKILL_SORT;
  const candidate = value as Partial<SkillSort>;
  return {
    key: isSkillSortKey(candidate.key) ? candidate.key : DEFAULT_SKILL_SORT.key,
    direction: isSkillSortDirection(candidate.direction)
      ? candidate.direction
      : DEFAULT_SKILL_SORT.direction,
  };
}

function readPreference(): ViewPreference {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "grid" || stored === "list") {
      return { ...DEFAULT_PREFERENCE, viewMode: stored };
    }
    if (stored) {
      const parsed = JSON.parse(stored) as Partial<ViewPreference>;
      return {
        viewMode: parsed.viewMode === "list" ? "list" : "grid",
        groupBySource: parsed.groupBySource === true,
        sort: readSort(parsed.sort),
        // Absent in preferences stored before this switch existed: default is on.
        prioritizeEnabled: parsed.prioritizeEnabled !== false,
      };
    }
  } catch {
    // Keep the view usable when browser storage is unavailable or holds junk.
  }
  return DEFAULT_PREFERENCE;
}

export function useSkillViewMode() {
  const [preference, setPreference] = useState<ViewPreference>(readPreference);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(preference));
    } catch {
      // The selection still works for the current page.
    }
  }, [preference]);

  const setViewMode = useCallback((next: SkillViewMode) => {
    setPreference((prev) => ({ ...prev, viewMode: next }));
  }, []);

  const setGroupBySource = useCallback((next: boolean) => {
    setPreference((prev) => ({ ...prev, groupBySource: next }));
  }, []);

  const setSort = useCallback((next: SkillSort) => {
    setPreference((prev) => ({ ...prev, sort: next }));
  }, []);

  const setPrioritizeEnabled = useCallback((next: boolean) => {
    setPreference((prev) => ({ ...prev, prioritizeEnabled: next }));
  }, []);

  return [
    preference.viewMode,
    setViewMode,
    preference.groupBySource,
    setGroupBySource,
    preference.sort,
    setSort,
    preference.prioritizeEnabled,
    setPrioritizeEnabled,
  ] as const;
}
