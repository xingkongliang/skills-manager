/**
 * How one agent row in the skill detail sheet reads, from the viewed preset's
 * intent (`enabled`) and the fact on disk (`deployed`, a `skill_targets` row).
 *
 * - `off`: not enabled in the viewed preset, whether or not it is deployed.
 * - `deployed`: enabled and deployed.
 * - `pending`: enabled but not deployed, in the active preset. Only the active
 *   preset deploys, so this is the one place "pending" means something.
 * - `enabled`: enabled but not deployed, in a preset that is not active.
 */
export type AgentToggleState = "off" | "deployed" | "pending" | "enabled";

export function agentToggleState(
  item: { enabled: boolean; deployed: boolean },
  showPendingSync: boolean
): AgentToggleState {
  if (!item.enabled) return "off";
  if (item.deployed) return "deployed";
  return showPendingSync ? "pending" : "enabled";
}

/** The `enabled` value a click asks for: a pending row deploys, it does not turn off. */
export function nextToggleValue(state: AgentToggleState): boolean {
  return state === "off" || state === "pending";
}
