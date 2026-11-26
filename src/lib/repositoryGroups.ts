import type { ManagedSkill, SkillRepository, ToolInfo } from "./tauri";
import { getManagedSkills, importRepositorySkills } from "./tauri";
import { buildSourceGroups, getSkillSourceInfo, type SkillSourceGroup } from "./skillSource";

export interface RepositorySkillGroup extends SkillSourceGroup {
  branch: string | null;
  repository?: SkillRepository;
}

/** Explicit refs remain distinct, including a default ref and an explicit main. */
function sourceBranch(skill: ManagedSkill): string | null {
  const recorded = skill.source_branch?.trim();
  if (recorded) return recorded;
  for (const ref of [skill.source_ref, skill.source_ref_resolved]) {
    if (!ref) continue;
    const query = ref.match(/[?&]ref=([^&#]+)/)?.[1];
    const fragment = ref.match(/#(.+)$/)?.[1];
    const encoded = query || fragment;
    if (!encoded) continue;
    try { return decodeURIComponent(encoded); } catch { return encoded; }
  }
  return null;
}

/** Matches the catalog's identity without relying on an install-time collection row. */
export function getRepositorySkillIdentity(skill: ManagedSkill): string | null {
  if (skill.source_type !== "git" && skill.source_type !== "skillssh") return null;
  const source = getSkillSourceInfo(skill);
  if (source.kind !== "repo") return null;
  return `${source.key}#ref=${encodeURIComponent(sourceBranch(skill) ?? "")}`;
}

/** Older Git and skills.sh installs join the same repository/ref automatically. */
export function buildRepositorySkillGroups(skills: ManagedSkill[], repositories: SkillRepository[] = []): RepositorySkillGroup[] {
  const groups = new Map<string, RepositorySkillGroup>();
  const repositoriesBySkill = new Map(repositories.flatMap((repository) => repository.skill_ids.map((id) => [id, repository] as const)));
  const repositoriesByIdentity = new Map(repositories.map((repository) => [repository.identity, repository]));
  for (const skill of skills) {
    const derivedKey = getRepositorySkillIdentity(skill);
    const repository = repositoriesBySkill.get(skill.id) ?? (derivedKey ? repositoriesByIdentity.get(derivedKey) : undefined);
    const key = repository?.identity ?? derivedKey;
    if (!key) continue;
    const existing = groups.get(key);
    if (existing) {
      existing.skills.push(skill);
      existing.count += 1;
    } else {
      groups.set(key, {
        ...getSkillSourceInfo(skill), key, kind: "repo", label: repository?.name ?? getSkillSourceInfo(skill).label,
        branch: repository ? repository.branch : sourceBranch(skill), repository, count: 1, skills: [skill],
      });
    }
  }
  return [...groups.values()].sort((a, b) =>
    (a.label ?? a.key).localeCompare(b.label ?? b.key, undefined, { sensitivity: "base" })
    || (a.branch ?? "").localeCompare(b.branch ?? "")
  );
}

/** Keep local, website and unknown skills available in the repository view too. */
export function buildRepositoryViewGroups(skills: ManagedSkill[], repositories: SkillRepository[] = []): SkillSourceGroup[] {
  const repositoryGroups = buildRepositorySkillGroups(skills, repositories);
  const groupedIds = new Set(repositoryGroups.flatMap((group) => group.skills.map((skill) => skill.id)));
  return [
    ...repositoryGroups,
    ...buildSourceGroups(skills.filter((skill) => !groupedIds.has(skill.id))),
  ];
}

/** Import catalog members first, then hand the full selection to one batch planner. */
export function repositoryAvailableCount(group: RepositorySkillGroup): number {
  return group.skills.length + (group.repository?.members.filter((member) => member.upstream_present === true && !member.is_stale && member.installed_skill_ids.length === 0).length ?? 0);
}

export async function prepareRepositorySkills(group: RepositorySkillGroup, discoverUpstream = false): Promise<ManagedSkill[]> {
  if (!group.repository || (!discoverUpstream && repositoryAvailableCount(group) === group.skills.length)) return group.skills;
  const result = await importRepositorySkills(group.repository.id);
  window.dispatchEvent(new Event("repository-catalog-changed"));
  const ids = new Set(result.repository.skill_ids);
  return (await getManagedSkills()).filter((skill) => ids.has(skill.id));
}

export function repositoryDeploymentState(
  skills: ManagedSkill[],
  agentKey: string,
  exists: (skill: ManagedSkill, agentKey: string) => boolean,
) {
  const enabled = skills.filter((skill) => exists(skill, agentKey)).length;
  return { enabled, total: skills.length, state: enabled === 0 ? "off" : enabled === skills.length ? "on" : "partial" } as const;
}

/** Lexical comparison for the shared-directory hint; the backend resolves aliases. */
function normalizedSkillsPath(path: string): string {
  const windows = /^[a-z]:[\\/]/i.test(path) || /^\\\\/.test(path);
  const normalized = path.replace(/\\/g, "/").replace(/\/{2,}/g, "/").replace(/\/$/, "");
  const segments: string[] = [];
  for (const segment of normalized.split("/")) {
    if (segment === ".") continue;
    if (segment === ".." && segments.length > 0 && segments.at(-1) !== "..") segments.pop();
    else segments.push(segment);
  }
  const result = segments.join("/");
  return windows ? result.toLowerCase() : result;
}

export function sharedRepositoryAgents(agentKey: string, tools: ToolInfo[], scope: "global" | "project"): ToolInfo[] {
  const selected = tools.find((tool) => tool.key === agentKey);
  const pathFor = (tool: ToolInfo) => scope === "global" ? tool.skills_dir : tool.project_relative_skills_dir;
  const path = selected && pathFor(selected);
  if (!path) return [];
  const key = normalizedSkillsPath(path);
  return tools.filter((tool) => tool.key !== agentKey && tool.installed && !!pathFor(tool) && normalizedSkillsPath(pathFor(tool)!) === key);
}
