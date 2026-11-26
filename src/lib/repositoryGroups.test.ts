import { beforeEach, describe, expect, it, vi } from "vitest";
import { buildRepositorySkillGroups, buildRepositoryViewGroups, getRepositorySkillIdentity, prepareRepositorySkills, repositoryAvailableCount, repositoryDeploymentState } from "./repositoryGroups";
import type { ManagedSkill, SkillRepository } from "./tauri";
import * as api from "./tauri";

vi.mock("./tauri", () => ({ importRepositorySkills: vi.fn(), getManagedSkills: vi.fn() }));

const skill = (id: string, overrides: Partial<ManagedSkill> = {}): ManagedSkill => ({
  id, name: id, description: null, source_type: "git", source_ref: "https://github.com/Owner/Skills.git",
  source_ref_resolved: null, source_subpath: id, source_branch: null, source_revision: null,
  remote_revision: null, update_status: "up_to_date", last_checked_at: null, last_check_error: null,
  central_path: `/fixture/${id}`, enabled: true, created_at: 0, updated_at: 0, status: "ok",
  targets: [], preset_ids: [], tags: [], ...overrides,
});

const repository = (overrides: Partial<SkillRepository> = {}): SkillRepository => ({
  id: "saved", identity: "repo:github.com/owner/skills#ref=", name: "owner/skills",
  source_ref: "owner/skills", clone_url: "https://github.com/owner/skills.git", branch: null,
  documentation_url: null, added_at: 0, updated_at: 0, last_scanned_at: null,
  snapshot_revision: null, scan_error: null, members: [], skill_ids: [], ...overrides,
});

beforeEach(() => vi.clearAllMocks());

describe("repository identity and legacy grouping", () => {
  it("joins SSH, HTTPS and skills.sh members while keeping case-sensitive refs distinct", () => {
    const groups = buildRepositorySkillGroups([
      skill("https", { source_branch: "main" }),
      skill("ssh", { source_ref: "git@github.com:owner/skills", source_branch: "main" }),
      skill("skills.sh", { source_type: "skillssh", source_ref: "owner/skills/demo", source_ref_resolved: "https://github.com/owner/skills", source_branch: "main" }),
      skill("other-ref", { source_branch: "Main" }),
      skill("default-ref"),
    ]);
    expect(groups).toHaveLength(3);
    expect(groups.find((group) => group.branch === "main")?.skills.map((item) => item.id)).toEqual(["https", "ssh", "skills.sh"]);
    expect(new Set(groups.map((group) => group.key))).toEqual(new Set([
      "repo:github.com/owner/skills#ref=main", "repo:github.com/owner/skills#ref=Main", "repo:github.com/owner/skills#ref=",
    ]));
  });

  it("uses legacy ref suffixes without merging a branch containing slash with another ref", () => {
    expect(getRepositorySkillIdentity(skill("old", { source_ref: "owner/skills#feature/example" }))).toBe("repo:github.com/owner/skills#ref=feature%2Fexample");
    expect(getRepositorySkillIdentity(skill("old", { source_ref: "https://github.com/owner/skills?ref=release%2F1" }))).toBe("repo:github.com/owner/skills#ref=release%2F1");
  });

  it("uses authoritative catalog membership when a default ref resolved to main at install", () => {
    const saved = repository({ skill_ids: ["resolved"] });
    const [group] = buildRepositorySkillGroups([skill("resolved", { source_branch: "main" })], [saved]);
    expect(group.key).toBe(saved.identity);
    expect(group.branch).toBeNull();
    expect(group.repository).toBe(saved);
  });

  it("retains non-repository skills in the grouped view without guessing repos from paths", () => {
    const local = skill("local", { source_type: "local", source_ref: "/work/local" });
    expect(getRepositorySkillIdentity(local)).toBeNull();
    expect(buildRepositoryViewGroups([skill("git"), local]).flatMap((group) => group.skills.map((item) => item.id))).toEqual(["git", "local"]);
  });
});

describe("repository deployment scope", () => {
  it("counts only the selected Agent's actual copies regardless of preset membership", () => {
    const members = [skill("a", { preset_ids: ["preset"] }), skill("b"), skill("c")];
    const exists = (item: ManagedSkill, agent: string) => agent === "codex" ? item.id === "b" : true;
    expect(repositoryDeploymentState(members, "codex", exists)).toEqual({ enabled: 1, total: 3, state: "partial" });
    expect(repositoryDeploymentState(members, "other", exists).state).toBe("on");
  });

  it("counts confirmed unimported upstream members but excludes removed and stale entries", async () => {
    const members = [skill("a")];
    const [group] = buildRepositorySkillGroups(members, [repository({ skill_ids: ["a"], members: [
      { rel_path: "a", name: "a", description: null, installed_skill_ids: ["a"], upstream_present: true, is_stale: false },
      { rel_path: "b", name: "b", description: null, installed_skill_ids: [], upstream_present: true, is_stale: false },
      { rel_path: "gone", name: "gone", description: null, installed_skill_ids: [], upstream_present: false, is_stale: false },
      { rel_path: "stale", name: "stale", description: null, installed_skill_ids: [], upstream_present: true, is_stale: true },
    ] })]);
    expect(repositoryAvailableCount(group)).toBe(2);
  });

  it("enables existing imported collections offline without cloning or scanning", async () => {
    const [group] = buildRepositorySkillGroups([skill("a")], [repository({ skill_ids: ["a"] })]);
    expect(await prepareRepositorySkills(group)).toEqual(group.skills);
    expect(api.importRepositorySkills).not.toHaveBeenCalled();
  });
});
