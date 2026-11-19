import { SOURCE_GONE } from "./sourceGone";
import type { ManagedSkill, MoveCandidate } from "./tauri";

/**
 * What a scan of one Source said about one skill. Mirrors the backend's
 * `SourceRepairItem`, minus the parts this page derives from the skill itself.
 */
export interface ScanItem {
  skill_id: string;
  placement: RepairGroup;
  candidates: RepairCandidate[];
  local_changed: boolean;
  /** `null` when the Source could not be reached: nobody looked. */
  installed_revision_present: boolean | null;
}

export type RepairCandidate = MoveCandidate;

export type RepairGroup = "at" | "moved" | "gone" | "unreachable";

/** The order the page shows them in: most repairable first. */
export const REPAIR_GROUP_ORDER: readonly RepairGroup[] = [
  "at",
  "moved",
  "gone",
  "unreachable",
];

export interface RepairRow {
  id: string;
  name: string;
  sourceKey: string;
  cloneUrl: string | null;
  branch: string | null;
  subpath: string | null;
  /** Browses the Source, when the clone URL says enough to build one. */
  repoUrl: string | null;
  /** The recorded path; a 404 there is the clearest evidence there is. */
  pathUrl: string | null;
  centralPath: string;
  deployments: number;
  presets: number;
  group: RepairGroup;
  candidates: RepairCandidate[];
  /** The library copy differs from what was installed: overwriting loses it. */
  localChanged: boolean;
  /** `null` when no scan has looked at the Source's history. */
  installedRevisionPresent: boolean | null;
  scanned: boolean;
}

/** One Source is one clone: the clone URL plus the branch it tracks. */
export function sourceKey(skill: ManagedSkill): string {
  const url = skill.source_ref_resolved ?? skill.source_ref ?? "";
  return `${url}#${skill.source_branch ?? ""}`;
}

/**
 * A browsable repository URL, when the clone URL says enough to build one.
 * `owner/repo` shorthand is deliberately refused: guessing the host would send
 * the user to the wrong place, which is worse than offering nothing.
 */
export function repoUrl(cloneUrl: string | null): string | null {
  if (!cloneUrl) return null;
  const scp = cloneUrl.match(/^git@([^:]+):(.+?)(?:\.git)?$/);
  if (scp) return `https://${scp[1]}/${scp[2].replace(/\.git$/, "")}`;
  const http = cloneUrl.match(/^https?:\/\/[^/]+\/.+$/);
  if (http) return cloneUrl.replace(/\.git$/, "");
  return null;
}

/**
 * The URL of the path the record still points at.
 *
 * It may 404 — and that is the point: seeing the repository without that folder
 * settles "moved or deleted" faster than any wording in this app.
 */
export function recordedPathUrl(where: {
  cloneUrl: string | null;
  branch: string | null;
  subpath: string | null;
}): string | null {
  const repo = repoUrl(where.cloneUrl);
  const subpath = where.subpath?.replace(/^\/+|\/+$/g, "");
  if (!repo || !subpath || !repo.startsWith("https://github.com/")) return null;
  return `${repo}/tree/${where.branch ?? "HEAD"}/${subpath}`;
}

/**
 * Group the skills that need a decision, using a scan when there is one.
 *
 * Without a scan the check's answer is the only one available: `source_gone`
 * means "not where an update would look", and `error` means the Source could
 * not be asked at all. A scan overrules both, because it looked by hand.
 */
export function repairGroups(
  skills: ManagedSkill[],
  scans: Record<string, ScanItem[]>
): Record<RepairGroup, RepairRow[]> {
  const groups: Record<RepairGroup, RepairRow[]> = {
    at: [],
    moved: [],
    gone: [],
    unreachable: [],
  };
  for (const skill of skills) {
    if (skill.update_status !== SOURCE_GONE && skill.update_status !== "error") {
      continue;
    }
    const key = sourceKey(skill);
    const fromScan = scans[key]?.find((item) => item.skill_id === skill.id);
    const group: RepairGroup =
      fromScan?.placement ??
      (skill.update_status === "error" ? "unreachable" : "gone");
    const cloneUrl = skill.source_ref_resolved ?? skill.source_ref;
    groups[group].push({
      id: skill.id,
      name: skill.name,
      sourceKey: key,
      cloneUrl,
      branch: skill.source_branch,
      subpath: skill.source_subpath,
      repoUrl: repoUrl(cloneUrl),
      pathUrl: recordedPathUrl({
        cloneUrl,
        branch: skill.source_branch,
        subpath: skill.source_subpath,
      }),
      centralPath: skill.central_path,
      deployments: skill.targets.length,
      presets: skill.preset_ids.length,
      group,
      candidates: fromScan?.candidates ?? [],
      localChanged: fromScan?.local_changed ?? false,
      installedRevisionPresent: fromScan?.installed_revision_present ?? null,
      scanned: fromScan !== undefined,
    });
  }
  return groups;
}
