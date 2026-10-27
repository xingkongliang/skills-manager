import type { PendingRemovalGroup } from "./tauri";

/** One skill in the review panel, with its paths grouped by where they live. */
export interface PendingRemovalSkill {
  id: string;
  name: string;
  source: string;
  revision: string;
  locations: { location: string; paths: string[] }[];
}

/**
 * Collapse the backend's per-location groups into one row per skill.
 *
 * Approval is per skill, so the checkbox and the shift-range operate on these
 * rows. The backend already orders the library first; sorting here keeps that
 * promise even if a caller builds the array by hand.
 */
export function groupPendingRemovals(groups: PendingRemovalGroup[]): PendingRemovalSkill[] {
  const bySkill = new Map<string, PendingRemovalSkill>();
  for (const group of groups) {
    let skill = bySkill.get(group.skill_id);
    if (!skill) {
      skill = {
        id: group.skill_id,
        name: group.skill_name,
        source: group.source,
        revision: group.revision,
        locations: [],
      };
      bySkill.set(group.skill_id, skill);
    }
    const existing = skill.locations.find((entry) => entry.location === group.location);
    if (existing) existing.paths.push(...group.paths);
    else skill.locations.push({ location: group.location, paths: [...group.paths] });
  }

  return [...bySkill.values()].map((skill) => ({
    ...skill,
    locations: skill.locations.sort((a, b) => {
      const libraryFirst = Number(b.location === "library") - Number(a.location === "library");
      return libraryFirst !== 0 ? libraryFirst : a.location.localeCompare(b.location);
    }),
  }));
}

export const removalPathCount = (skill: PendingRemovalSkill): number =>
  skill.locations.reduce((total, entry) => total + entry.paths.length, 0);
