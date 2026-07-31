import type {
  ManagedSkill,
  PresetApplyMode,
  PresetApplyReport,
  ProjectSkill,
} from "./tauri";

export interface ApplyProjectPresetOperationOptions {
  presetId: string;
  mode: PresetApplyMode;
  managedSkills: ManagedSkill[];
  agentKeys: string[];
  loadProjectSkills: () => Promise<ProjectSkill[]>;
  addSkill: (skillId: string, agentKey: string) => Promise<void>;
  removeSkill: (skillRelativePath: string, agentKey: string) => Promise<void>;
  formatError: (error: unknown) => string;
}

const projectVariantKey = (skillId: string, agentKey: string) =>
  `${skillId}::${agentKey}`;

export function indexProjectPresetVariants(skills: ProjectSkill[]) {
  const variants = new Map<string, ProjectSkill>();
  for (const skill of skills) {
    if (!skill.center_skill_id) continue;
    variants.set(projectVariantKey(skill.center_skill_id, skill.agent), skill);
  }
  return variants;
}

/**
 * 将一个 Preset 操作应用到 Project Workspace。
 *
 * PresetBar 会把多次点击排进 FIFO，但出于性能考虑只在整批操作完成后刷新 React
 * 状态。因此每个队列项必须在真正开始时重新扫描项目目录，不能使用入队时的旧快照；
 * 否则两个含重叠 Skill 的 Preset 会重复导出，或重复删除同一目录。
 */
export async function applyProjectPresetOperation({
  presetId,
  mode,
  managedSkills,
  agentKeys,
  loadProjectSkills,
  addSkill,
  removeSkill,
  formatError,
}: ApplyProjectPresetOperationOptions): Promise<PresetApplyReport> {
  const liveVariants = indexProjectPresetVariants(await loadProjectSkills());
  const report: PresetApplyReport = { applied: 0, skipped: 0, failures: [] };
  const presetSkills = managedSkills.filter((skill) =>
    skill.preset_ids.includes(presetId)
  );

  for (const skill of presetSkills) {
    for (const agentKey of agentKeys) {
      const existingVariant =
        liveVariants.get(projectVariantKey(skill.id, agentKey)) ?? null;
      if (
        (mode === "add" && existingVariant) ||
        (mode === "remove" && !existingVariant)
      ) {
        report.skipped += 1;
        continue;
      }

      try {
        if (mode === "add") {
          await addSkill(skill.id, agentKey);
        } else {
          await removeSkill(existingVariant!.relative_path, agentKey);
        }
        report.applied += 1;
      } catch (error) {
        report.failures.push({
          skillId: skill.id,
          toolKey: agentKey,
          message: formatError(error),
        });
      }
    }
  }

  return report;
}
