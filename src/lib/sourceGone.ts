import type { ManagedSkill } from "./tauri";

/** The status the backend writes when a Source no longer holds a skill (#655). */
export const SOURCE_GONE = "source_gone";

/**
 * One skill the cleanup would delete, with everything the confirmation has to
 * disclose: where it came from, what the library still holds, and what else
 * goes with it.
 */
export interface SourceGoneRow {
  id: string;
  name: string;
  /** The subpath it was installed from — the one the Source deleted. */
  subpath: string | null;
  /** The library copy on disk. Deleting the skill deletes this. */
  centralPath: string;
  deployments: number;
  presets: number;
}

/**
 * The skills whose Source deleted them, and only those (#655).
 *
 * `source_missing` is deliberately excluded: a Local or Import skill whose
 * original path is gone still has its library copy as the only one left, so
 * deleting it is a different decision from deleting something that still exists
 * upstream and can be reinstalled. `update_available` and `error` are not
 * terminal, so they are not candidates either.
 */
export function sourceGoneRows(skills: ManagedSkill[]): SourceGoneRow[] {
  return skills
    .filter((skill) => skill.update_status === SOURCE_GONE)
    .map((skill) => ({
      id: skill.id,
      name: skill.name,
      subpath: skill.source_subpath,
      centralPath: skill.central_path,
      deployments: skill.targets.length,
      presets: skill.preset_ids.length,
    }));
}
