import type { ManagedSkill } from "./tauri";

/** The fields the refresh rules read, so a test needs only those. */
export type RefreshableSkill = Pick<
  ManagedSkill,
  "update_status" | "source_type" | "source_ref"
>;

/**
 * Whether an update action can do anything for this skill.
 *
 * `moved` and `source_gone` cannot: one waits for the user to re-point its
 * source, the other has nothing left upstream. Offering "Update" for either is
 * the failed-forever button this replaces.
 */
export function canRefresh(skill: RefreshableSkill): boolean {
  return (
    skill.update_status !== "moved" &&
    skill.update_status !== "source_gone" &&
    (skill.source_type === "git" ||
      skill.source_type === "skillssh" ||
      ((skill.source_type === "local" || skill.source_type === "import") &&
        !!skill.source_ref))
  );
}

/**
 * The skills "Update all" picks: only those with a new revision waiting.
 *
 * Exactly the status the round treats as "there is an update to apply", and
 * then the same refresh rule as a single-skill click — so a moved or deleted
 * skill can never be swept into a batch.
 */
export function isUpdateAvailable(skill: RefreshableSkill): boolean {
  return skill.update_status === "update_available" && canRefresh(skill);
}

/** The subset of `skills` that "Update all" would run. */
export function updatableSkills<T extends RefreshableSkill>(skills: T[]): T[] {
  return skills.filter(isUpdateAvailable);
}
