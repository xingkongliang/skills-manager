import { useCallback, useEffect, useState } from "react";

export type SkillViewMode = "grid" | "list";

/**
 * My Skills view preferences, kept under one key so adding a preference does
 * not add a second storage entry to keep in sync.
 *
 * The stored value used to be the bare string `"grid"` / `"list"`; that form is
 * still read, so an existing installation keeps its view mode.
 */
interface ViewPreference {
  viewMode: SkillViewMode;
  groupBySource: boolean;
}

const STORAGE_KEY = "skills-manager.skillViewMode";

const DEFAULT_PREFERENCE: ViewPreference = { viewMode: "grid", groupBySource: false };

function readPreference(): ViewPreference {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "grid" || stored === "list") {
      return { viewMode: stored, groupBySource: false };
    }
    if (stored) {
      const parsed = JSON.parse(stored) as Partial<ViewPreference>;
      return {
        viewMode: parsed.viewMode === "list" ? "list" : "grid",
        groupBySource: parsed.groupBySource === true,
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

  return [preference.viewMode, setViewMode, preference.groupBySource, setGroupBySource] as const;
}
