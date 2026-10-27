import { describe, expect, it } from "vitest";
import { groupPendingRemovals, removalPathCount } from "./pendingRemovals";
import type { PendingRemovalGroup } from "./tauri";

const group = (overrides: Partial<PendingRemovalGroup>): PendingRemovalGroup => ({
  skill_id: "skill-1",
  skill_name: "media-use",
  source: "https://example.com/acme/skills",
  revision: "rev2",
  location: "library",
  paths: [],
  ...overrides,
});

describe("groupPendingRemovals", () => {
  it("puts one row per skill, with the library first and every path counted", () => {
    const rows = groupPendingRemovals([
      group({ paths: ["a.mjs"] }),
      group({ location: "claude_code", paths: ["b.mjs"] }),
      group({ skill_id: "skill-2", skill_name: "other", paths: ["c.txt"] }),
    ]);

    expect(rows.map((row) => row.id)).toEqual(["skill-1", "skill-2"]);
    expect(rows[0].locations.map((entry) => entry.location)).toEqual(["library", "claude_code"]);
    expect(removalPathCount(rows[0])).toBe(2);
    expect(removalPathCount(rows[1])).toBe(1);
  });
});
