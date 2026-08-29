export type SkillSortMode = "none" | "asc" | "desc";

export const SKILL_SORT_MODE_LS_KEY = "skills-manager.skillSort";

/** Reads the persisted sort mode, falling back to "none" on bad values. */
export function getStoredSkillSortMode(): SkillSortMode {
  try {
    const stored = localStorage.getItem(SKILL_SORT_MODE_LS_KEY);
    return stored === "asc" || stored === "desc" ? stored : "none";
  } catch {
    return "none";
  }
}

/**
 * Case-insensitive, numeric-aware locale comparison. This is what "dictionary
 * order" means for skill lists: `a2` sorts before `a10`, and "Zebra" next to
 * "apple" is treated by the letter rather than by ASCII case.
 */
export function compareSkillNames(a: string, b: string): number {
  return a.localeCompare(b, undefined, { sensitivity: "base", numeric: true });
}

/**
 * Returns a new array sorted by dictionary order of the name returned by
 * `getName`. `mode` selects ascending (A–Z) or descending (Z–A); "none" is
 * handled by callers and should not be passed here. The input is untouched.
 */
export function sortSkillsByName<T>(
  skills: readonly T[],
  getName: (skill: T) => string,
  mode: Exclude<SkillSortMode, "none"> = "asc",
): T[] {
  const direction = mode === "desc" ? -1 : 1;
  return [...skills].sort((a, b) => direction * compareSkillNames(getName(a), getName(b)));
}
