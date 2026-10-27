import { describe, expect, it } from "vitest";
import { canRefresh, isUpdateAvailable, updatableSkills } from "./updateSelection";
import type { RefreshableSkill } from "./updateSelection";

const skill = (over: Partial<RefreshableSkill> = {}): RefreshableSkill => ({
  update_status: "update_available",
  source_type: "git",
  source_ref: "https://github.com/acme/skills.git",
  ...over,
});

describe("update selection", () => {
  it("picks only skills with a new revision waiting", () => {
    const picked = updatableSkills([
      skill(),
      skill({ update_status: "up_to_date" }),
      skill({ update_status: "unknown" }),
      skill({ update_status: "error" }),
    ]);
    expect(picked).toHaveLength(1);
  });

  it("never picks moved or source_gone, whatever their source path", () => {
    // Even a git skill with a perfectly good remote: these two states have no
    // new revision to apply, so "Update all" must leave them alone.
    for (const status of ["moved", "source_gone"]) {
      const row = skill({ update_status: status });
      expect(canRefresh(row)).toBe(false);
      expect(isUpdateAvailable(row)).toBe(false);
      expect(updatableSkills([row])).toEqual([]);
    }
  });

  it("offers a refresh to a local skill only while it still has a source path", () => {
    expect(canRefresh(skill({ source_type: "local", source_ref: "/tmp/x" }))).toBe(true);
    expect(canRefresh(skill({ source_type: "local", source_ref: null }))).toBe(false);
    expect(canRefresh(skill({ source_type: "import", source_ref: null }))).toBe(false);
  });

  it("does not offer one for a source type it cannot refresh", () => {
    expect(canRefresh(skill({ source_type: "well-known", source_ref: "x" }))).toBe(false);
  });
});
