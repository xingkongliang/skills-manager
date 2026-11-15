// @vitest-environment jsdom
import { act } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SourcePicker } from "./SourcePicker";
import type { SkillSourceCandidate } from "../lib/skillSource";
import { cleanup, mount, pills } from "../test-utils/mount";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

afterEach(cleanup);

const candidateList: SkillSourceCandidate[] = [
  { key: "repo:a", label: "owner/repo", count: 12 },
  { key: "unknown", label: null, count: 2 },
];

function render(props: Partial<Parameters<typeof SourcePicker>[0]> = {}) {
  const container = mount(
    <SourcePicker
      candidates={candidateList}
      selected={new Set()}
      onChange={() => {}}
      {...props}
    />
  );
  // The trigger is the first button; everything after it lives in the menu.
  const buttons = () => pills(container);
  const trigger = () => buttons()[0];
  const rows = () => buttons().slice(1);
  return { container, buttons, trigger, rows };
}

describe("SourcePicker", () => {
  it("is disabled when the list holds no Source to pick", () => {
    expect(render({ candidates: [] }).trigger().disabled).toBe(true);
  });

  it("counts the picked Sources on the button", () => {
    expect(render({ selected: new Set(["repo:a"]) }).trigger().textContent).toContain("(1)");
    expect(render().trigger().textContent).not.toContain("(");
  });

  it("opens a menu of every candidate, with its skill count", () => {
    const picker = render();
    act(() => picker.trigger().click());
    expect(picker.rows().map((row) => row.textContent)).toEqual([
      "owner/repo12",
      "mySkills.sourcePicker.unknownSource2",
    ]);
  });

  it("offers no clear row until something is picked", () => {
    const empty = render();
    act(() => empty.trigger().click());
    expect(empty.rows()).toHaveLength(2);

    cleanup();
    const picked = render({ selected: new Set(["repo:a"]) });
    act(() => picked.trigger().click());
    expect(picked.rows()).toHaveLength(3);
    expect(picked.rows()[0].textContent).toBe("mySkills.sourcePicker.clear");
  });

  it("reports the Source that was clicked, on top of what was already picked", () => {
    const onChange = vi.fn();
    const picker = render({ selected: new Set(["unknown"]), onChange });
    act(() => picker.trigger().click());
    act(() => picker.rows()[1].click());
    expect(onChange).toHaveBeenCalledWith(new Set(["unknown", "repo:a"]));
  });

  it("clears every picked Source through the menu's clear row", () => {
    const onChange = vi.fn();
    const picker = render({ selected: new Set(["repo:a"]), onChange });
    act(() => picker.trigger().click());
    act(() => picker.rows()[0].click());
    expect(onChange).toHaveBeenCalledWith(new Set());
  });

  it("closes the menu on Escape", () => {
    const picker = render();
    act(() => picker.trigger().click());
    expect(picker.buttons()).toHaveLength(3);
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(picker.buttons()).toHaveLength(1);
  });
});
