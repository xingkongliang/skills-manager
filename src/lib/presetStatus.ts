import type { ManagedSkill, Preset } from "./tauri";

export type PresetStatus = "active" | "partial" | "inactive" | "empty";

export interface PresetStatusResult {
  status: PresetStatus;
  installed: number;
  total: number;
}

/** Preset 状态统计的粒度。 */
export type PresetStatusMode = "agent-pair" | "logical-skill";

/** 计算 Preset 在当前工作区中的安装状态。 */
export function computePresetStatus(
  preset: Preset,
  skills: ManagedSkill[],
  agentKeys: string[],
  existsInWorkspace: (skill: ManagedSkill, agentKey: string) => boolean,
  mode: PresetStatusMode = "agent-pair"
): PresetStatusResult {
  const presetSkills = skills.filter((s) => s.preset_ids.includes(preset.id));
  if (presetSkills.length === 0 || agentKeys.length === 0) {
    return { status: "empty", installed: 0, total: 0 };
  }
  if (mode === "logical-skill") {
    const total = presetSkills.length;
    let installed = 0;
    for (const skill of presetSkills) {
      if (agentKeys.every((agentKey) => existsInWorkspace(skill, agentKey))) {
        installed++;
      }
    }
    if (installed === total) return { status: "active", installed, total };
    if (installed === 0) return { status: "inactive", installed, total };
    return { status: "partial", installed, total };
  }

  const total = presetSkills.length * agentKeys.length;
  let installed = 0;
  for (const skill of presetSkills) {
    for (const agentKey of agentKeys) {
      if (existsInWorkspace(skill, agentKey)) installed++;
    }
  }
  if (installed === total) return { status: "active", installed, total };
  if (installed === 0) return { status: "inactive", installed, total };
  return { status: "partial", installed, total };
}
