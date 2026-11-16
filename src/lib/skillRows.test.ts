import { describe, expect, it } from "vitest";
import { buildSkillRows, renderedSkillIds } from "./skillRows";
import { countHiddenSelected } from "./selectionVisibility";
import type { SkillSourceGroup } from "./skillSource";
import type { ManagedSkill } from "./tauri";

const skill = (id: string): ManagedSkill => ({
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
});

const group = (key: string, ids: string[]): SkillSourceGroup => ({
  key,
  label: key,
  kind: "repo",
  count: ids.length,
  skills: ids.map(skill),
});

const repoA = group("repo:a", ["a1", "a2"]);
const repoB = group("repo:b", ["b1", "b2"]);
const single = group("repo:c", ["c1"]);

const build = (groups: SkillSourceGroup[] | null, openBySource = {}, searching = false) =>
  buildSkillRows({ groups, skills: [], openBySource, searching });

describe("buildSkillRows", () => {
  it("renders the filtered skills as-is when grouping is off", () => {
    const rows = buildSkillRows({
      groups: null,
      skills: [skill("a1"), skill("b1")],
      openBySource: {},
      searching: false,
    });
    expect(rows.map((row) => row.kind)).toEqual(["skill", "skill"]);
    expect(renderedSkillIds(rows)).toEqual(["a1", "b1"]);
  });

  it("collapses a source of several skills by default", () => {
    const rows = build([repoA, repoB]);
    expect(rows.filter((row) => row.kind === "group")).toHaveLength(2);
    expect(renderedSkillIds(rows)).toEqual([]);
  });

  it("keeps a source of one skill open, so its only skill is not hidden", () => {
    const rows = build([single]);
    expect(renderedSkillIds(rows)).toEqual(["c1"]);
  });

  it("honours an explicit choice over the default", () => {
    expect(renderedSkillIds(build([repoA], { "repo:a": true }))).toEqual(["a1", "a2"]);
    expect(renderedSkillIds(build([single], { "repo:c": false }))).toEqual([]);
  });

  it("opens every source during a search and says so on the header", () => {
    const rows = build([repoA, repoB], {}, true);
    expect(renderedSkillIds(rows)).toEqual(["a1", "a2", "b1", "b2"]);
    const headers = rows.filter((row) => row.kind === "group");
    expect(headers.every((row) => row.kind === "group" && row.expanded && !row.collapsible)).toBe(true);
  });

  it("keeps the header collapsible when nothing is being searched", () => {
    const headers = build([repoA, repoB]).filter((row) => row.kind === "group");
    expect(headers.every((row) => row.kind === "group" && row.collapsible)).toBe(true);
  });
});

describe("hidden selection, as the toolbar and the delete dialog report it", () => {
  const allIds = new Set(["a1", "a2", "b1", "b2"]);

  it("discloses the skills of a source collapsed after they were picked", () => {
    // Both sources collapsed by the user's own clicks, everything selected.
    const rows = build([repoA, repoB], { "repo:a": false, "repo:b": false });
    expect(renderedSkillIds(rows)).toEqual([]);
    expect(countHiddenSelected(allIds, renderedSkillIds(rows))).toBe(4);
  });

  it("discloses only the collapsed source when the other is open", () => {
    const rows = build([repoA, repoB], { "repo:a": true, "repo:b": false });
    expect(countHiddenSelected(allIds, renderedSkillIds(rows))).toBe(2);
  });

  it("discloses the same number whichever way the selection was made", () => {
    // Select-all swept both sources; picking row by row ends in the same state.
    const rows = build([repoA, repoB], { "repo:a": true, "repo:b": false });
    const sweptUp = countHiddenSelected(allIds, renderedSkillIds(rows));
    const picked = countHiddenSelected(new Set(["b1", "b2"]), renderedSkillIds(rows));
    expect(sweptUp).toBe(2);
    expect(picked).toBe(2);
  });

  it("discloses nothing when grouping is off or a search opens everything", () => {
    const ungrouped = buildSkillRows({
      groups: null,
      skills: [skill("a1"), skill("a2"), skill("b1"), skill("b2")],
      openBySource: {},
      searching: false,
    });
    expect(countHiddenSelected(allIds, renderedSkillIds(ungrouped))).toBe(0);
    expect(countHiddenSelected(allIds, renderedSkillIds(build([repoA, repoB], {}, true)))).toBe(0);
  });
});
