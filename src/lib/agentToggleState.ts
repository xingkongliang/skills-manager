export interface AgentToggleItem {
  key: string;
  displayName: string;
  enabled: boolean;
  /**
   * Whether the skill is actually deployed for this agent (a `skill_targets`
   * row). Optional: the project workspace passes `undefined` and falls back
   * to `enabled`, which already means a real deployment there (#384).
   */
  deployed?: boolean;
  isAvailable: boolean;
  disabled?: boolean;
  badgeLabel?: string | null;
}

/// The check mark and the "已同步 x/y" summary must reflect what is actually
/// on disk, not the preset's intent flag: `enabled=1` rows are seeded for
/// every installed adapter when a skill joins a preset, long before any
/// files are written (#384).
export function isItemDeployed(item: AgentToggleItem): boolean {
  return item.deployed ?? item.enabled;
}
