import { useCallback, useState } from "react";

type SkillViewMode = "grid" | "list";

const STORAGE_KEY = "skills-manager.skillViewMode";

export function useSkillViewMode() {
  const [viewMode, setViewModeState] = useState<SkillViewMode>(() => {
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      if (stored === "grid" || stored === "list") return stored;
    } catch {
      // Keep the view usable when browser storage is unavailable.
    }
    return "grid";
  });

  const setViewMode = useCallback((next: SkillViewMode) => {
    setViewModeState(next);
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // The selection still works for the current page.
    }
  }, []);

  return [viewMode, setViewMode] as const;
}
