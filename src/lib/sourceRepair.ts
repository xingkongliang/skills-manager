import type { ManagedSkill } from "./tauri";

/**
 * What a scan of one Source said about one skill. Mirrors the backend's
 * `SourceRepairItem`, minus the parts this page derives from the skill itself.
 */
export interface ScanItem {
  skill_id: string;
  placement: "at" | "moved" | "gone" | "unreachable";
  candidates: RepairCandidate[];
  local_changed: boolean;
  installed_revision_present: boolean;
}

export interface RepairCandidate {
  subpath: string;
  evidence: "content_hash" | "directory_name";
}

export type RepairGroup = "at" | "moved" | "gone" | "unreachable";

export interface RepairRow {
  id: string;
  name: string;
  sourceKey: string;
  cloneUrl: string | null;
  branch: string | null;
  subpath: string | null;
  centralPath: string;
  deployments: number;
  presets: number;
  group: RepairGroup;
  candidates: RepairCandidate[];
  /** The library copy differs from what was installed: overwriting loses it. */
  localChanged: boolean;
  /** False when the Source's history no longer holds the installed revision. */
  installedRevisionPresent: boolean;
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
export function recordedPathUrl(skill: ManagedSkill): string | null {
  const repo = repoUrl(skill.source_ref_resolved ?? skill.source_ref);
  const subpath = skill.source_subpath?.replace(/^\/+|\/+$/g, "");
  if (!repo || !subpath || !repo.startsWith("https://github.com/")) return null;
  return `${repo}/tree/${skill.source_branch ?? "HEAD"}/${subpath}`;
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
    if (skill.update_status !== "source_gone" && skill.update_status !== "error") {
      continue;
    }
    const key = sourceKey(skill);
    const fromScan = scans[key]?.find((item) => item.skill_id === skill.id);
    const group: RepairGroup =
      fromScan?.placement ??
      (skill.update_status === "error" ? "unreachable" : "gone");
    groups[group].push({
      id: skill.id,
      name: skill.name,
      sourceKey: key,
      cloneUrl: skill.source_ref_resolved ?? skill.source_ref,
      branch: skill.source_branch,
      subpath: skill.source_subpath,
      centralPath: skill.central_path,
      deployments: skill.targets.length,
      presets: skill.preset_ids.length,
      group,
      candidates: fromScan?.candidates ?? [],
      localChanged: fromScan?.local_changed ?? false,
      installedRevisionPresent: fromScan?.installed_revision_present ?? true,
      scanned: fromScan !== undefined,
    });
  }
  return groups;
}
