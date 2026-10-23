import type { ManagedSkill } from "./tauri";

export interface SkillSourceInfo {
  /** Grouping unit. Two skills share a key only when they come from the same upstream. */
  key: string;
  /** What the user sees as the group title, or null when there is no source to name. */
  label: string | null;
  kind: SkillSourceKind;
}

export type SkillSourceKind = "repo" | "site" | "path" | "unknown";

export interface SkillSourceGroup extends SkillSourceInfo {
  count: number;
  skills: ManagedSkill[];
}

const stripSlashes = (value: string) => value.replace(/^\/+/, "").replace(/\/+$/, "");

/** The segments of a `path:` group key, most specific last. */
const pathSegments = (key: string) =>
  stripSlashes(key.replace(/^path:/, "")).split("/").filter(Boolean);

/**
 * Turn any way of naming a git repo into one key.
 *
 * SSH, HTTPS and scp forms all name the same repo, and so do a trailing `.git`
 * and different letter case — git treats all of them as the same remote. Both
 * the host and the path are case-folded: host names are case-insensitive, and
 * git hosts treat `owner/repo` case-insensitively for identity, so
 * `Obra/Superpowers` and `obra/superpowers` are one group.
 */
function parseGitRef(ref: string): SkillSourceInfo | null {
  // A trailing `#branch` / `?ref=` names a revision inside the repo, not a
  // different repo, so it is dropped before parsing: one repo, one group.
  const trimmed = ref.trim().replace(/[?#].*$/, "");
  if (!trimmed) return null;

  let host = "";
  let path = "";
  const scp = trimmed.match(/^[^@/\s]+@([^:/\s]+):(.+)$/); // git@github.com:owner/repo
  // scp without a user: github.com:owner/repo. The host needs a dot, so a
  // Windows drive path (`C:/x`) cannot be mistaken for one.
  const hostPath = trimmed.match(/^([\w.-]+\.[\w.-]+):(.+)$/);
  const url = trimmed.match(
    /^(?:https?|ssh|git):\/\/(?:[^@/\s]+@)?([^:/\s]+)(?::\d+)?\/(.+)$/
  );
  const shorthand = trimmed.match(/^([\w.-]+)\/([\w.-]+?)(?:\.git)?$/); // owner/repo

  if (scp) {
    host = scp[1];
    path = scp[2];
  } else if (url) {
    host = url[1];
    path = url[2];
  } else if (hostPath) {
    host = hostPath[1];
    path = hostPath[2];
  } else if (shorthand) {
    host = "github.com";
    path = `${shorthand[1]}/${shorthand[2]}`;
  } else {
    return null;
  }

  path = stripSlashes(path).replace(/\.git$/i, "");
  if (!path) return null;
  // The revision suffix is already gone, and callers pass the repo reference
  // rather than a directory inside it, so a monorepo's skills stay in one
  // group.
  path = path.toLowerCase();
  host = host.toLowerCase();

  const label = host === "github.com" ? path : `${host}/${path}`;
  return { key: `repo:${host}/${path}`, label, kind: "repo" };
}

/**
 * The many call sites differ only in which of the two references wins, so the
 * pair is normalised once: prefer the resolved URL and fall back to the raw one.
 */
function parseGitRefPair(resolved: string, ref: string): SkillSourceInfo | null {
  return parseGitRef(resolved || ref);
}

/**
 * skills.sh site skills are snapshots published by one site. The site is the
 * only grouping available — the skills are not a repo, so they group by domain.
 * `source_ref` holds the canonical `https://skills.sh/site/<domain>/<skill>`.
 */
function parseSiteRef(ref: string): SkillSourceInfo | null {
  const trimmed = ref.trim();
  if (!trimmed) return null;
  const segments = trimmed.replace(/[?#].*$/, "").split("/").filter(Boolean);
  const site = segments.indexOf("site");
  const domain = site >= 0 ? segments[site + 1] : undefined;
  if (domain) {
    const lower = domain.toLowerCase();
    return { key: `site:${lower}`, label: lower, kind: "site" };
  }
  try {
    const host = new URL(trimmed).host.toLowerCase();
    return host ? { key: `site:${host}`, label: host, kind: "site" } : null;
  } catch {
    return null;
  }
}

/**
 * Local and imported skills are sourced from a directory, and the path is the
 * source. Paths are not case-folded: on a case-sensitive filesystem two paths
 * differing only by case are two different sources.
 */
function parseLocalRef(ref: string): SkillSourceInfo | null {
  const trimmed = ref.trim().replace(/\\/g, "/").replace(/\/+$/, "");
  if (!trimmed) return null;
  const label = trimmed.split("/").filter(Boolean).pop() || trimmed;
  return { key: `path:${trimmed}`, label, kind: "path" };
}

/**
 * A skill whose `source_ref` is its own central copy has no external source:
 * adoption re-points it there once the original directory became a deployment
 * (see scenario_service::detach_source_refs_from_adoption_target). Grouping it
 * by that path would give every such skill a group of its own.
 */
function isDetached(skill: ManagedSkill) {
  return !!skill.source_ref && skill.source_ref === skill.central_path;
}

interface SourceContext {
  skill: ManagedSkill;
  ref: string;
  resolved: string;
}

const parseLocalUnlessDetached = ({ skill, ref }: SourceContext) =>
  isDetached(skill) ? null : parseLocalRef(ref);

/**
 * One resolver per `source_type`. Anything not listed here counts as an
 * unrecognised channel, and any resolver that cannot make sense of the
 * reference leaves it unplaced; both land in the `unknown` group.
 */
const SOURCE_RESOLVERS: Record<string, (ctx: SourceContext) => SkillSourceInfo | null> = {
  git: ({ ref, resolved }) => parseGitRefPair(resolved, ref),
  // `source_ref` is `owner/repo/<skill>`, so the repo URL has to win; the
  // reference is only a fallback when the resolved URL is missing.
  skillssh: ({ ref, resolved }) =>
    parseGitRef(resolved || ref) ??
    (ref ? parseGitRef(ref.split("/").slice(0, -1).join("/")) : null),
  "well-known": ({ ref, resolved }) => parseSiteRef(ref) || parseSiteRef(resolved),
  local: parseLocalUnlessDetached,
  import: parseLocalUnlessDetached,
};

export function getSkillSourceInfo(skill: ManagedSkill): SkillSourceInfo {
  const ref = skill.source_ref?.trim() || "";
  const resolved = skill.source_ref_resolved?.trim() || "";

  const info = SOURCE_RESOLVERS[skill.source_type]?.({ skill, ref, resolved });
  if (info) return info;

  // No generic git guess here: an unrecognised `source_type` whose
  // `source_ref` happens to have two segments (`docs/skills`) would otherwise
  // read as the repository `github.com/docs/skills`.
  return { key: "unknown", label: null, kind: "unknown" };
}

/** One row of the Source menu: the grouping key, its label, and how many skills it holds. */
export type SkillSourceCandidate = Pick<SkillSourceInfo, "key" | "label"> & { count: number };

/**
 * The Source menu for a list of skills. Call it with the result of the *other*
 * filters and before the Source filter, or picking one Source would remove the
 * others from the menu the user needs in order to switch.
 */
export function sourceCandidates(skills: ManagedSkill[]): SkillSourceCandidate[] {
  return buildSourceGroups(skills).map(({ key, label, count }) => ({ key, label, count }));
}

/**
 * A path group is labelled with its last segment, which is ambiguous more often
 * than not: `/work/a/skills` and `/home/b/skills` both read "skills". Widen
 * conflicting labels one parent segment at a time (`a/skills`, `b/skills`) until
 * they differ or the path runs out. Only the label moves — keys, and therefore
 * the grouping itself, are untouched. Repo and site labels are already unique
 * per source and are left alone.
 */
function disambiguatePathLabels(groups: SkillSourceGroup[]) {
  const pathGroups = groups.filter((group) => group.kind === "path");
  if (pathGroups.length < 2) return;

  const segments = new Map<SkillSourceGroup, string[]>();
  const depth = new Map<SkillSourceGroup, number>();
  for (const group of pathGroups) {
    segments.set(group, pathSegments(group.key));
    depth.set(group, 1);
  }

  const labelAt = (group: SkillSourceGroup, take: number) => {
    const parts = segments.get(group) ?? [];
    return parts.slice(Math.max(0, parts.length - take)).join("/") || group.key;
  };

  let widening = true;
  while (widening) {
    widening = false;
    const current = new Map(
      pathGroups.map((group) => [group, labelAt(group, depth.get(group) ?? 1)])
    );
    const seen = new Map<string, number>();
    for (const label of current.values()) seen.set(label, (seen.get(label) ?? 0) + 1);

    for (const group of pathGroups) {
      if ((seen.get(current.get(group) ?? "") ?? 0) < 2) continue;
      const parts = segments.get(group) ?? [];
      const take = depth.get(group) ?? 1;
      // Nothing left to widen with: the label stays as it is.
      if (take >= parts.length) continue;
      depth.set(group, take + 1);
      widening = true;
    }
  }

  for (const group of pathGroups) group.label = labelAt(group, depth.get(group) ?? 1);
}

/**
 * Groups skills by their upstream source.
 *
 * Input order is preserved inside a group, so grouping never reorders the list
 * the filters and sorting already produced. Groups are ordered large to small,
 * then by label; the unknown group is always last because it is a leftover
 * bucket rather than a source.
 */
export function buildSourceGroups(skills: ManagedSkill[]): SkillSourceGroup[] {
  const groups = new Map<string, SkillSourceGroup>();
  for (const skill of skills) {
    const info = getSkillSourceInfo(skill);
    const existing = groups.get(info.key);
    if (existing) {
      existing.count += 1;
      existing.skills.push(skill);
    } else {
      groups.set(info.key, { ...info, count: 1, skills: [skill] });
    }
  }

  const result = Array.from(groups.values());
  disambiguatePathLabels(result);

  return result.sort((a, b) => {
    if (a.kind === "unknown" || b.kind === "unknown") {
      return a.kind === b.kind ? 0 : a.kind === "unknown" ? 1 : -1;
    }
    return (
      b.count - a.count ||
      (a.label || a.key).localeCompare(b.label || b.key, undefined, {
        sensitivity: "base",
      })
    );
  });
}
