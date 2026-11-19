import { describe, expect, it } from "vitest";
import {
  recordedPathUrl,
  repairGroups,
  repoUrl,
  sourceKey,
  type ScanItem,
} from "./sourceRepair";
import type { ManagedSkill } from "./tauri";

function skill(overrides: Partial<ManagedSkill> = {}): ManagedSkill {
  return {
    id: "b",
    name: "b",
    description: null,
    source_type: "git",
    source_ref: "https://github.com/owner/repo.git",
    source_ref_resolved: "https://github.com/owner/repo.git",
    source_subpath: "skills/b",
    source_branch: "main",
    source_revision: "rev-1",
    remote_revision: "rev-2",
    update_status: "source_gone",
    last_checked_at: 1,
    last_check_error: null,
    central_path: "/library/b",
    enabled: true,
    created_at: 1,
    updated_at: 1,
    status: "ok",
    targets: [],
    preset_ids: [],
    tags: [],
    ...overrides,
  };
}

function scan(overrides: Partial<ScanItem> = {}): ScanItem {
  return {
    skill_id: "b",
    placement: "gone",
    candidates: [],
    local_changed: false,
    installed_revision_present: true,
    ...overrides,
  };
}

describe("repoUrl", () => {
  it("turns the clone URLs it can into a browsable repository", () => {
    expect(repoUrl("https://github.com/owner/repo.git")).toBe(
      "https://github.com/owner/repo"
    );
    expect(repoUrl("git@github.com:owner/repo.git")).toBe(
      "https://github.com/owner/repo"
    );
  });

  it("leaves a host it cannot guess alone", () => {
    expect(repoUrl("owner/repo")).toBeNull();
  });
});

describe("recordedPathUrl", () => {
  it("points at the recorded subpath on the tracked branch", () => {
    expect(recordedPathUrl({
      cloneUrl: skill().source_ref_resolved,
      branch: skill().source_branch,
      subpath: skill().source_subpath,
    })).toBe(
      "https://github.com/owner/repo/tree/main/skills/b"
    );
  });

  it("has nothing to point at for a root install or an unknown host", () => {
    expect(
      recordedPathUrl({ cloneUrl: "https://github.com/owner/repo", branch: "main", subpath: null })
    ).toBeNull();
    expect(
      recordedPathUrl({ cloneUrl: "owner/repo", branch: "main", subpath: "skills/b" })
    ).toBeNull();
  });
});

describe("sourceKey", () => {
  it("groups the skills of one Source, and separates branches", () => {
    expect(sourceKey(skill())).toBe(sourceKey(skill({ id: "c", name: "c" })));
    expect(sourceKey(skill())).not.toBe(sourceKey(skill({ source_branch: "dev" })));
  });
});

describe("repairGroups", () => {
  it("keeps a broken Source away from the skills that are merely gone", () => {
    const groups = repairGroups(
      [
        skill({ id: "gone", name: "gone" }),
        skill({ id: "broken", name: "broken", update_status: "error" }),
      ],
      {}
    );

    expect(groups.gone.map((row) => row.id)).toEqual(["gone"]);
    expect(groups.unreachable.map((row) => row.id)).toEqual(["broken"]);
  });

  it("lets a scan overrule what the check could not know", () => {
    const groups = repairGroups(
      [skill()],
      {
        [sourceKey(skill())]: [
          scan({
            placement: "moved",
            candidates: [{ subpath: "other/b", evidence: "content_hash" }],
          }),
        ],
      }
    );

    expect(groups.gone).toEqual([]);
    expect(groups.moved.map((row) => row.id)).toEqual(["b"]);
    expect(groups.moved[0].candidates[0].subpath).toBe("other/b");
  });

  it("carries the facts the actions need", () => {
    const groups = repairGroups(
      [skill({ targets: [], preset_ids: ["p1"] })],
      {
        [sourceKey(skill())]: [scan({ local_changed: true, installed_revision_present: false })],
      }
    );

    const [row] = groups.gone;
    expect(row.localChanged).toBe(true);
    expect(row.installedRevisionPresent).toBe(false);
    expect(row.presets).toBe(1);
    expect(row.subpath).toBe("skills/b");
    expect(row.centralPath).toBe("/library/b");
  });
});
