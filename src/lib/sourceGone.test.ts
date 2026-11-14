import { describe, expect, it } from "vitest";
import { SOURCE_GONE, sourceGoneRows } from "./sourceGone";
import type { ManagedSkill } from "./tauri";

function skill(overrides: Partial<ManagedSkill> = {}): ManagedSkill {
  return {
    id: "b",
    name: "b",
    description: null,
    source_type: "git",
    source_ref: "https://example.test/mono.git",
    source_ref_resolved: "https://example.test/mono.git",
    source_subpath: "skills/b",
    source_branch: null,
    source_revision: "rev-1",
    remote_revision: "rev-2",
    update_status: SOURCE_GONE,
    last_checked_at: 1,
    last_check_error: null,
    central_path: "/library/b",
    enabled: true,
    created_at: 1,
    updated_at: 1,
    status: "ok",
    targets: [
      {
        id: "t1",
        skill_id: "b",
        tool: "claude-code",
        target_path: "/home/.claude/skills/b",
        mode: "symlink",
        status: "synced",
        synced_at: 1,
      },
    ],
    preset_ids: ["p1", "p2"],
    tags: [],
    ...overrides,
  };
}

describe("sourceGoneRows", () => {
  it("keeps only the skills whose Source deleted them", () => {
    const rows = sourceGoneRows([
      skill(),
      skill({ id: "local", update_status: "source_missing" }),
      skill({ id: "behind", update_status: "update_available" }),
      skill({ id: "broken", update_status: "error" }),
    ]);

    expect(rows.map((row) => row.id)).toEqual(["b"]);
  });

  it("tells the user what the confirmation would take away", () => {
    const [row] = sourceGoneRows([skill()]);

    expect(row).toEqual({
      id: "b",
      name: "b",
      subpath: "skills/b",
      centralPath: "/library/b",
      deployments: 1,
      presets: 2,
    });
  });

  it("has nothing to offer when the Source still holds every skill", () => {
    expect(sourceGoneRows([skill({ update_status: "up_to_date" })])).toEqual([]);
  });
});
