import { describe, expect, it } from "vitest";
import {
  DEFAULT_SKILL_SORT,
  compareForList,
  compareSkills,
  isSkillSortDirection,
  isSkillSortKey,
  isDefaultSkillSort,
  sortSkills,
} from "./skillSort";
import type { SkillListOrder, SkillSort } from "./skillSort";
import type { ManagedSkill } from "./tauri";

const skill = (id: string, overrides: Partial<ManagedSkill> = {}): ManagedSkill => ({
  id,
  name: id,
  description: null,
  source_type: "git",
  source_ref: null,
  source_ref_resolved: null,
  source_subpath: null,
  source_branch: null,
  source_revision: null,
  remote_revision: null,
  update_status: "local_only",
  last_checked_at: null,
  last_check_error: null,
  central_path: `/fixture/${id}`,
  enabled: true,
  created_at: 0,
  updated_at: 0,
  status: "ok",
  targets: [],
  preset_ids: [],
  tags: [],
  ...overrides,
});

const ids = (skills: ManagedSkill[]) => skills.map((s) => s.id);

describe("defaults", () => {
  it("sorts by name ascending out of the box", () => {
    expect(DEFAULT_SKILL_SORT).toEqual({ key: "name", direction: "asc" });
    expect(isDefaultSkillSort(DEFAULT_SKILL_SORT)).toBe(true);
    expect(isDefaultSkillSort({ key: "name", direction: "desc" })).toBe(false);
    expect(isDefaultSkillSort({ key: "updated", direction: "asc" })).toBe(false);
  });

  it("validates stored keys and directions", () => {
    expect(isSkillSortKey("created")).toBe(true);
    expect(isSkillSortKey("custom")).toBe(false);
    expect(isSkillSortDirection("desc")).toBe(true);
    expect(isSkillSortDirection("down")).toBe(false);
  });
});

describe("name", () => {
  const skills = [skill("c", { name: "charlie" }), skill("a", { name: "Alpha" }), skill("b", { name: "bravo" })];

  it("ascending ignores case and orders naturally", () => {
    expect(ids(sortSkills(skills, { key: "name", direction: "asc" }))).toEqual(["a", "b", "c"]);
  });

  it("descending reverses the same order", () => {
    expect(ids(sortSkills(skills, { key: "name", direction: "desc" }))).toEqual(["c", "b", "a"]);
  });

  it("uses the rendered name when one is given", () => {
    // Two skills share the display name, so the page renders the directory name.
    const dupes = [skill("one", { name: "tool" }), skill("two", { name: "tool" })];
    const getName = (s: ManagedSkill) => (s.id === "one" ? "tool-a" : "tool-b");
    expect(ids(sortSkills(dupes, DEFAULT_SKILL_SORT, getName))).toEqual(["one", "two"]);
  });
});

describe("timestamps", () => {
  const skills = [
    skill("old", { created_at: 100, updated_at: 300 }),
    skill("new", { created_at: 300, updated_at: 100 }),
    skill("mid", { created_at: 200, updated_at: 200 }),
  ];

  it("orders by creation time in both directions", () => {
    expect(ids(sortSkills(skills, { key: "created", direction: "asc" }))).toEqual(["old", "mid", "new"]);
    expect(ids(sortSkills(skills, { key: "created", direction: "desc" }))).toEqual(["new", "mid", "old"]);
  });

  it("orders by update time in both directions", () => {
    expect(ids(sortSkills(skills, { key: "updated", direction: "asc" }))).toEqual(["new", "mid", "old"]);
    expect(ids(sortSkills(skills, { key: "updated", direction: "desc" }))).toEqual(["old", "mid", "new"]);
  });
});

describe("ties", () => {
  const sort: SkillSort = { key: "created", direction: "desc" };

  it("falls back to the name, then to the id, regardless of direction", () => {
    const skills = [
      skill("z", { name: "same", created_at: 10 }),
      skill("a", { name: "same", created_at: 10 }),
      skill("m", { name: "before", created_at: 10 }),
    ];
    expect(ids(sortSkills(skills, sort))).toEqual(["m", "a", "z"]);
  });

  it("is a total order, so the input order never leaks through", () => {
    const skills = [skill("b", { created_at: 5 }), skill("a", { created_at: 5 })];
    expect(ids(sortSkills(skills, { key: "created", direction: "asc" }))).toEqual(["a", "b"]);
    expect(ids(sortSkills([...skills].reverse(), { key: "created", direction: "asc" }))).toEqual(["a", "b"]);
  });
});

describe("sortSkills", () => {
  it("does not mutate the input", () => {
    const skills = [skill("b"), skill("a")];
    const sorted = sortSkills(skills, DEFAULT_SKILL_SORT);
    expect(ids(skills)).toEqual(["b", "a"]);
    expect(ids(sorted)).toEqual(["a", "b"]);
  });

  it("compareSkills agrees with the sorted output", () => {
    const a = skill("a", { name: "alpha", updated_at: 2 });
    const b = skill("b", { name: "beta", updated_at: 1 });
    const sort: SkillSort = { key: "updated", direction: "asc" };
    expect(compareSkills(a, b, sort)).toBeGreaterThan(0);
    expect(compareSkills(b, a, sort)).toBeLessThan(0);
    expect(compareSkills(a, a, sort)).toBe(0);
  });
});
describe("compareForList", () => {
  const on = (id: string, name: string, extra: Partial<ManagedSkill> = {}) =>
    skill(id, { name, preset_ids: ["p1"], ...extra });
  const off = (id: string, name: string, extra: Partial<ManagedSkill> = {}) =>
    skill(id, { name, preset_ids: [], ...extra });

  const list = [
    off("d2", "aaa-disabled"),
    on("e2", "zzz-enabled"),
    on("e1", "mmm-enabled"),
    off("d1", "bbb-disabled"),
  ];

  const order = (overrides: Partial<SkillListOrder> = {}): SkillListOrder => ({
    presetId: "p1",
    presetOrder: [],
    prioritizeEnabled: true,
    ...overrides,
  });

  it("puts the enabled skills of the viewed Preset first by default", () => {
    const sorted = [...list].sort((a, b) => compareForList(a, b, DEFAULT_SKILL_SORT, order()));
    expect(sorted.slice(0, 2).map((s) => s.id)).toEqual(["e1", "e2"]);
    expect(sorted.slice(2).map((s) => s.id)).toEqual(["d2", "d1"]);
  });

  it("sorts the whole list when prioritising enabled skills is off", () => {
    const sorted = [...list].sort((a, b) =>
      compareForList(a, b, DEFAULT_SKILL_SORT, order({ prioritizeEnabled: false }))
    );
    expect(sorted.map((s) => s.name)).toEqual([
      "aaa-disabled",
      "bbb-disabled",
      "mmm-enabled",
      "zzz-enabled",
    ]);
  });

  it("keeps the manually dragged order inside each group for the default sort", () => {
    const sorted = [...list].sort((a, b) =>
      compareForList(a, b, DEFAULT_SKILL_SORT, order({ presetOrder: ["e2", "e1"] }))
    );
    expect(sorted.slice(0, 2).map((s) => s.id)).toEqual(["e2", "e1"]);
  });

  it("lets an explicit sort override the manual order inside the group", () => {
    const sort: SkillSort = { key: "name", direction: "desc" };
    // Manual order says e1 then e2; the requested sort says the opposite.
    const sorted = [...list].sort((a, b) =>
      compareForList(a, b, sort, order({ presetOrder: ["e1", "e2"] }))
    );
    expect(sorted.slice(0, 2).map((s) => s.id)).toEqual(["e2", "e1"]);
    expect(sorted.slice(2).map((s) => s.id)).toEqual(["d1", "d2"]);
  });

  it("ignores both groupings outside a Preset", () => {
    const sorted = [...list].sort((a, b) =>
      compareForList(a, b, DEFAULT_SKILL_SORT, order({ presetId: null, presetOrder: ["e2", "e1"] }))
    );
    expect(sorted.map((s) => s.name)).toEqual([
      "aaa-disabled",
      "bbb-disabled",
      "mmm-enabled",
      "zzz-enabled",
    ]);
  });
});