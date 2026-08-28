import type { ManagedSkill } from "./tauri";

export interface SkillSourceInfo {
  key: string;
  label: string;
  type: string;
  raw: string;
}

export interface SkillSourceGroup {
  key: string;
  label: string;
  type: string;
  raw: string;
  count: number;
  skills: ManagedSkill[];
}

const cleanPath = (value: string) =>
  value.replace(/^\/+/, "").replace(/\/+$/, "").replace(/\.git$/i, "");

function parseGitRef(
  ref: string,
  type: string,
  raw: string
): SkillSourceInfo | null {
  const trimmed = ref.trim();
  if (!trimmed) return null;

  let host = "";
  let path = "";
  const scp = trimmed.match(/^git@([^:/]+):(.+)$/i);
  const url = trimmed.match(
    /^(?:https?|ssh):\/\/(?:[^@/]+@)?([^\s:/]+)(?::\d+)?\/(.+)$/i
  );
  const shorthand = trimmed.match(
    /^([a-zA-Z0-9._-]+)\/([a-zA-Z0-9._-]+)(\.git)?$/
  );

  if (scp) {
    host = scp[1];
    path = scp[2];
  } else if (url) {
    host = url[1];
    path = url[2];
  } else if (shorthand) {
    host = "github.com";
    path = `${shorthand[1]}/${shorthand[2]}`;
  } else {
    const slash = trimmed.indexOf("/");
    if (slash > 0 && /^[a-zA-Z0-9.-]+$/.test(trimmed.slice(0, slash))) {
      host = trimmed.slice(0, slash);
      path = trimmed.slice(slash + 1);
    } else {
      path = trimmed;
    }
  }

  path = cleanPath(path);
  if (!path) return null;

  const hostLower = host.toLowerCase();
  if (hostLower === "github.com") {
    return { key: path.toLowerCase(), label: path, type, raw };
  }
  if (host) {
    const label = `${host}/${path}`;
    return {
      key: `${hostLower}/${path.toLowerCase()}`,
      label,
      type,
      raw,
    };
  }
  return { key: path.toLowerCase(), label: path, type, raw };
}

function parseSkillShRef(ref: string, type: string, raw: string): SkillSourceInfo | null {
  const trimmed = ref.trim();
  if (!trimmed) return null;
  const parts = trimmed.split("/").filter(Boolean);
  // For a full skillssh reference like `owner/repo/skill-id`, the source is
  // `owner/repo`. Shorter references are treated as the source itself.
  const source = parts.length > 2 ? parts.slice(0, -1).join("/") : trimmed;
  if (!source) return null;
  const label = `skills.sh/${source}`;
  return { key: `skills.sh:${source.toLowerCase()}`, label, type, raw };
}

function parseLocalRef(ref: string, type: string, raw: string): SkillSourceInfo {
  const trimmed = ref.trim().replace(/\\/g, "/").replace(/\/+$/, "");
  if (!trimmed) {
    return { key: `unknown:${type}`, label: "", type, raw: raw || trimmed };
  }
  const normalized = trimmed.toLowerCase();
  const label = trimmed.split("/").filter(Boolean).pop() || trimmed;
  return { key: `${type}:${normalized}`, label, type, raw: raw || trimmed };
}

export function getSkillSourceInfo(skill: ManagedSkill): SkillSourceInfo {
  const resolved = skill.source_ref_resolved || "";
  const ref = skill.source_ref || "";
  const raw = ref || resolved;

  if (skill.source_type === "git" && resolved) {
    const gitInfo = parseGitRef(resolved, skill.source_type, raw);
    if (gitInfo) return gitInfo;
  }

  if (skill.source_type === "skillssh") {
    if (resolved) {
      const gitInfo = parseGitRef(resolved, skill.source_type, raw);
      if (gitInfo) return gitInfo;
    }
    if (ref) {
      const skillShInfo = parseSkillShRef(ref, skill.source_type, raw);
      if (skillShInfo) return skillShInfo;
    }
  }

  if (skill.source_type === "local" || skill.source_type === "import") {
    return parseLocalRef(resolved, skill.source_type, raw);
  }

  if (raw) {
    return {
      key: `${skill.source_type}:${raw.toLowerCase()}`,
      label: raw,
      type: skill.source_type,
      raw,
    };
  }
  return {
    key: `unknown:${skill.source_type}`,
    label: "",
    type: skill.source_type,
    raw,
  };
}

/**
 * Groups skills by normalized source.
 *
 * The input order is preserved inside each group. Group order is deterministic:
 * larger groups first, then locale-aware label order (falling back to key).
 */
export function buildSourceGroups(skills: ManagedSkill[]): SkillSourceGroup[] {
  const map = new Map<string, SkillSourceGroup>();
  for (const skill of skills) {
    const info = getSkillSourceInfo(skill);
    const existing = map.get(info.key);
    if (existing) {
      existing.count += 1;
      existing.skills.push(skill);
    } else {
      map.set(info.key, {
        key: info.key,
        label: info.label,
        type: info.type,
        raw: info.raw,
        count: 1,
        skills: [skill],
      });
    }
  }

  const groups = Array.from(map.values());
  groups.sort(
    (a, b) =>
      b.count - a.count ||
      (a.label || a.key).localeCompare(b.label || b.key, undefined, {
        sensitivity: "base",
      })
  );
  return groups;
}
