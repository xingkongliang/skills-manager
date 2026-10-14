import { describe, expect, it } from "vitest";
import { isItemDeployed, type AgentToggleItem } from "../lib/agentToggleState";

function item(overrides: Partial<AgentToggleItem>): AgentToggleItem {
  return {
    key: "opencode",
    displayName: "OpenCode",
    enabled: false,
    isAvailable: true,
    ...overrides,
  };
}

describe("isItemDeployed", () => {
  it("does not read preset membership (enabled) as synced (#384)", () => {
    // Joining a preset seeds enabled=1 for every installed agent, but the
    // skill_targets row — the actual deployment — is only written by a real
    // sync. The green check and the "已同步 x/y" summary must wait for it.
    expect(isItemDeployed(item({ enabled: true, deployed: false }))).toBe(false);
  });

  it("reads a skill_targets deployment as synced, independent of the flag", () => {
    expect(isItemDeployed(item({ enabled: true, deployed: true }))).toBe(true);
    // Deployment is a fact: it stays until the row is removed, even when
    // the intent flag is off (e.g. toggled off while the preset is inactive).
    expect(isItemDeployed(item({ enabled: false, deployed: true }))).toBe(true);
  });

  it("falls back to enabled where deployed is not tracked (project workspace)", () => {
    // ProjectDetail builds its items from actual deployment variants, where
    // enabled already means deployed, so it passes no deployed flag at all.
    expect(isItemDeployed(item({ enabled: true }))).toBe(true);
    expect(isItemDeployed(item({ enabled: false }))).toBe(false);
  });
});
