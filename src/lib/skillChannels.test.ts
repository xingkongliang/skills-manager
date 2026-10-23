import { describe, expect, it } from "vitest";
import { CHANNEL_ORDER, channelLabel, channelsForSkills, isKnownChannel } from "./skillChannels";

const skill = (source_type: string) => ({ source_type });

describe("channelsForSkills", () => {
  it("lists every known channel even when the library holds none of them", () => {
    // The row must not change shape as skills come and go.
    expect(channelsForSkills([])).toEqual([...CHANNEL_ORDER]);
    expect(channelsForSkills([skill("git")])).toEqual([...CHANNEL_ORDER]);
  });

  it("keeps the documented order regardless of what the library holds", () => {
    const mixed = [skill("skillssh"), skill("local"), skill("git"), skill("import")];
    expect(channelsForSkills(mixed)).toEqual(["local", "import", "git", "skillssh"]);
  });

  it("appends a channel this build does not name, once and sorted", () => {
    const extra = [skill("well-known"), skill("well-known"), skill("zz-legacy")];
    expect(channelsForSkills(extra)).toEqual([
      ...CHANNEL_ORDER,
      "well-known",
      "zz-legacy",
    ]);
  });

  it("ignores an empty source_type", () => {
    expect(channelsForSkills([skill("")])).toEqual([...CHANNEL_ORDER]);
  });
});

describe("isKnownChannel", () => {
  it("knows the four channels the library can hold", () => {
    expect(CHANNEL_ORDER.every(isKnownChannel)).toBe(true);
  });

  it("does not claim the site channel is known", () => {
    // `well-known` has no pill of its own; it renders as its raw source_type.
    expect(isKnownChannel("well-known")).toBe(false);
  });
});

describe("channelLabel", () => {
  const t = (key: string) => key;

  it("translates the channels this build knows", () => {
    expect(channelLabel(t, "git")).toBe("mySkills.sourceFilter.git");
    expect(channelLabel(t, "skillssh")).toBe("mySkills.sourceFilter.skillssh");
  });

  it("falls back to the raw value so an unknown channel still gets a pill", () => {
    expect(channelLabel(t, "well-known")).toBe("well-known");
  });
});
