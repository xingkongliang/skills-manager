import { describe, expect, it, vi } from "vitest";
import { applyProjectPresetOperation } from "./projectPresetApply";
import type { ManagedSkill, ProjectSkill } from "./tauri";

const managedSkill = (
  id: string,
  presetIds: string[]
): ManagedSkill => ({
  id,
  name: id,
  description: null,
  source_type: "local",
  source_ref: null,
  source_ref_resolved: null,
  source_subpath: null,
  source_branch: null,
  source_revision: null,
  remote_revision: null,
  update_status: "unknown",
  last_checked_at: null,
  last_check_error: null,
  central_path: `C:\\skills\\${id}`,
  enabled: true,
  created_at: 0,
  updated_at: 0,
  status: "ready",
  targets: [],
  preset_ids: presetIds,
  tags: [],
});

const projectSkill = (
  centerSkillId: string,
  agent: string
): ProjectSkill => ({
  name: centerSkillId,
  dir_name: centerSkillId,
  relative_path: centerSkillId,
  description: null,
  path: `C:\\project\\${centerSkillId}`,
  files: ["SKILL.md"],
  enabled: true,
  agent,
  agent_display_name: agent,
  tags: [],
  in_center: true,
  sync_status: "in_sync",
  center_skill_id: centerSkillId,
});

describe("applyProjectPresetOperation", () => {
  it("每次队列操作都重新读取项目状态，因此重叠 Preset 不会重复导出", async () => {
    const shared = managedSkill("shared", ["preset-a", "preset-b"]);
    let liveSkills: ProjectSkill[] = [];
    const loadProjectSkills = vi.fn(async () => [...liveSkills]);
    const addSkill = vi.fn(async (skillId: string, agentKey: string) => {
      liveSkills = [projectSkill(skillId, agentKey)];
    });

    const common = {
      mode: "add" as const,
      managedSkills: [shared],
      agentKeys: ["codex"],
      loadProjectSkills,
      addSkill,
      removeSkill: vi.fn(),
      formatError: (error: unknown) => String(error),
    };

    const first = await applyProjectPresetOperation({
      ...common,
      presetId: "preset-a",
    });
    const second = await applyProjectPresetOperation({
      ...common,
      presetId: "preset-b",
    });

    expect(first).toEqual({ applied: 1, skipped: 0, failures: [] });
    expect(second).toEqual({ applied: 0, skipped: 1, failures: [] });
    expect(loadProjectSkills).toHaveBeenCalledTimes(2);
    expect(addSkill).toHaveBeenCalledTimes(1);
  });

  it("移除操作也使用实时状态，因此重叠 Preset 不会重复删除", async () => {
    const shared = managedSkill("shared", ["preset-a", "preset-b"]);
    let liveSkills: ProjectSkill[] = [projectSkill("shared", "codex")];
    const loadProjectSkills = vi.fn(async () => [...liveSkills]);
    const removeSkill = vi.fn(async () => {
      liveSkills = [];
    });

    const common = {
      mode: "remove" as const,
      managedSkills: [shared],
      agentKeys: ["codex"],
      loadProjectSkills,
      addSkill: vi.fn(),
      removeSkill,
      formatError: (error: unknown) => String(error),
    };

    const first = await applyProjectPresetOperation({
      ...common,
      presetId: "preset-a",
    });
    const second = await applyProjectPresetOperation({
      ...common,
      presetId: "preset-b",
    });

    expect(first).toEqual({ applied: 1, skipped: 0, failures: [] });
    expect(second).toEqual({ applied: 0, skipped: 1, failures: [] });
    expect(loadProjectSkills).toHaveBeenCalledTimes(2);
    expect(removeSkill).toHaveBeenCalledTimes(1);
  });
});
