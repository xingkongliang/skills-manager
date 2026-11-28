import type { ManagedSkill } from "./tauri";

/**
 * How the My Skills page orders the library.
 *
 * `name` is the default (ascending). `created` / `updated` read the timestamps
 * the backend stores on every managed skill, so they keep working for skills
 * that were adopted rather than installed.
 */
export type SkillSortKey = "name" | "created" | "updated";
export type SkillSortDirection = "asc" | "desc";

export interface SkillSort {
  key: SkillSortKey;
  direction: SkillSortDirection;
}

export const SKILL_SORT_KEYS: readonly SkillSortKey[] = ["name", "created", "updated"];

export const DEFAULT_SKILL_SORT: SkillSort = { key: "name", direction: "asc" };

export function isSkillSortKey(value: unknown): value is SkillSortKey {
  return typeof value === "string" && (SKILL_SORT_KEYS as readonly string[]).includes(value);
}

export function isSkillSortDirection(value: unknown): value is SkillSortDirection {
  return value === "asc" || value === "desc";
}

/** True for the factory default, which a Preset view treats as "manual order". */
export function isDefaultSkillSort(sort: SkillSort): boolean {
  return sort.key === "name" && sort.direction === "asc";
}

/** Two skills often share a name (the same skill installed from two sources). */
function compareNames(a: ManagedSkill, b: ManagedSkill, getName?: (skill: ManagedSkill) => string) {
  const left = getName ? getName(a) : a.name;
  const right = getName ? getName(b) : b.name;
  return left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" });
}

function compareTimestamps(a: ManagedSkill, b: ManagedSkill, key: "created" | "updated") {
  const left = key === "created" ? a.created_at : a.updated_at;
  const right = key === "created" ? b.created_at : b.updated_at;
  return left - right;
}

/**
 * Comparator for the chosen sort. `getName` lets the page sort by the name it
 * actually renders (duplicate skill names are shown with their directory name).
 *
 * Ties fall back to the name (always ascending) and then to the id, so a list
 * of skills sharing a timestamp never shuffles between renders.
 */
export function compareSkills(
  a: ManagedSkill,
  b: ManagedSkill,
  sort: SkillSort,
  getName?: (skill: ManagedSkill) => string
): number {
  const sign = sort.direction === "asc" ? 1 : -1;
  const primary =
    sort.key === "name" ? compareNames(a, b, getName) : compareTimestamps(a, b, sort.key);

  if (primary !== 0) return primary * sign;

  const byName = compareNames(a, b, getName);
  if (byName !== 0) return byName;
  return a.id.localeCompare(b.id);
}

/** Non-mutating sort, used by the page and by tests. */
export function sortSkills(
  skills: readonly ManagedSkill[],
  sort: SkillSort,
  getName?: (skill: ManagedSkill) => string
): ManagedSkill[] {
  return [...skills].sort((a, b) => compareSkills(a, b, sort, getName));
}

/** Where the list is shown, which decides whether enabled state still matters. */
export interface SkillListOrder {
  /** Preset being viewed, or null for the whole library. */
  presetId: string | null;
  /** Stored manual order of that Preset (dragged by the user). */
  presetOrder: readonly string[];
  /**
   * "优先展示已开启": when true the skills enabled in the viewed Preset come
   * first; when false the list follows the chosen sort alone.
   */
  prioritizeEnabled: boolean;
}

/**
 * Comparator used by the My Skills list.
 *
 * Inside a Preset the enabled skills come first (that is what the Preset toggle
 * is about) and the user's sort applies within each group; the default name
 * ascending order keeps the manually dragged order instead, otherwise a drag
 * would be undone by the sort on the very next render. Turning
 * `prioritizeEnabled` off drops both behaviours and sorts the whole list.
 */
export function compareForList(
  a: ManagedSkill,
  b: ManagedSkill,
  sort: SkillSort,
  order: SkillListOrder,
  getName?: (skill: ManagedSkill) => string
): number {
  const { presetId, presetOrder, prioritizeEnabled } = order;

  if (presetId && prioritizeEnabled) {
    const aEnabled = a.preset_ids.includes(presetId) ? 0 : 1;
    const bEnabled = b.preset_ids.includes(presetId) ? 0 : 1;
    if (aEnabled !== bEnabled) return aEnabled - bEnabled;

    if (isDefaultSkillSort(sort)) {
      const aIndex = presetOrder.indexOf(a.id);
      const bIndex = presetOrder.indexOf(b.id);
      if (aIndex !== -1 && bIndex !== -1) return aIndex - bIndex;
      if (aIndex !== -1) return -1;
      if (bIndex !== -1) return 1;
    }
  }

  return compareSkills(a, b, sort, getName);
}
