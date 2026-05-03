import type { ProjectSkill } from "../lib/tauri";

export function getProjectSkillVariantKey(variant: Pick<ProjectSkill, "agent" | "relative_path">) {
  return `${variant.agent}::${variant.relative_path.toLowerCase()}`;
}

export function applyProjectSkillEnabledState(
  allSkills: ProjectSkill[],
  variants: Pick<ProjectSkill, "agent" | "relative_path">[],
  enabled: boolean,
) {
  const variantKeys = new Set(variants.map(getProjectSkillVariantKey));
  return allSkills.map((skill) =>
    variantKeys.has(getProjectSkillVariantKey(skill))
      ? { ...skill, enabled }
      : skill
  );
}
