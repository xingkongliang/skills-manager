import { describe, expect, it } from "vitest";
import { agentToggleState, nextToggleValue } from "./agentToggleState";

describe("agentToggleState", () => {
  it.each([
    // enabled, deployed, active preset, expected
    [false, false, true, "off"],
    [false, true, true, "off"], // deployed by something else, not enabled here
    [true, true, true, "deployed"],
    [true, false, true, "pending"],
    [false, false, false, "off"],
    [false, true, false, "off"],
    [true, true, false, "deployed"],
    [true, false, false, "enabled"], // nothing deploys from an inactive preset
  ] as const)("enabled=%s deployed=%s active=%s -> %s", (enabled, deployed, active, expected) => {
    expect(agentToggleState({ enabled, deployed }, active)).toBe(expected);
  });
});

describe("nextToggleValue", () => {
  it("deploys a pending row instead of turning it off", () => {
    expect(nextToggleValue("pending")).toBe(true);
  });

  it("turns an off row on and an enabled or deployed row off", () => {
    expect(nextToggleValue("off")).toBe(true);
    expect(nextToggleValue("deployed")).toBe(false);
    expect(nextToggleValue("enabled")).toBe(false);
  });
});
