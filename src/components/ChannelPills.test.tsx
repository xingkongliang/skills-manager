// @vitest-environment jsdom
import { act } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ChannelPills } from "./ChannelPills";
import { CHANNEL_ORDER } from "../lib/skillChannels";
import { cleanup, mount, pills } from "../test-utils/mount";

// The component translates through the same keys My Skills uses, so echoing the
// key back is enough to assert which label each pill got.
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

afterEach(cleanup);

function render(props: Partial<Parameters<typeof ChannelPills>[0]> = {}) {
  const container = mount(
    <ChannelPills
      channels={[...CHANNEL_ORDER]}
      selected={new Set()}
      onToggle={() => {}}
      {...props}
    />
  );
  return pills(container);
}

describe("ChannelPills", () => {
  it("renders the channels in the order it was given", () => {
    expect(render().map((pill) => pill.textContent)).toEqual([
      "mySkills.sourceFilter.local",
      "mySkills.sourceFilter.import",
      "mySkills.sourceFilter.git",
      "mySkills.sourceFilter.skillssh",
    ]);
  });

  it("marks the selected channels, and only those", () => {
    expect(render({ selected: new Set(["git"]) }).map((pill) => pill.getAttribute("aria-pressed"))).toEqual([
      "false",
      "false",
      "true",
      "false",
    ]);
  });

  it("reports the channel that was clicked", () => {
    const onToggle = vi.fn();
    const shown = render({ onToggle });
    act(() => shown[1].click());
    expect(onToggle).toHaveBeenCalledWith("import");
  });

  it("shows an unknown channel as its raw source_type", () => {
    // Dropping it would look like the filter cannot reach those skills at all.
    const shown = render({ channels: [...CHANNEL_ORDER, "well-known"] });
    expect(shown[4].textContent).toBe("well-known");
  });
});
