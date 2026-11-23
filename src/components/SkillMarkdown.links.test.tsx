// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SkillMarkdown } from "./SkillMarkdown";

const content = "See [notes](./references/notes.md#top) and [site](https://example.com).";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

/** Clicks the link and reports whether something cancelled the navigation. */
function click(text: string): boolean {
  const link = [...container.querySelectorAll("a")].find((a) => a.textContent === text)!;
  const event = new MouseEvent("click", { bubbles: true, cancelable: true });
  link.dispatchEvent(event);
  return event.defaultPrevented;
}

describe("SkillMarkdown relative links", () => {
  it("hands a relative link to onRelativeLink instead of following it", async () => {
    const onRelativeLink = vi.fn();
    await act(async () => root.render(<SkillMarkdown content={content} onRelativeLink={onRelativeLink} />));
    expect(click("notes")).toBe(true);
    expect(onRelativeLink).toHaveBeenCalledWith("references/notes.md");
    expect(click("site")).toBe(false);
    expect(onRelativeLink).toHaveBeenCalledTimes(1);
  });

  it("leaves every link alone without onRelativeLink", async () => {
    await act(async () => root.render(<SkillMarkdown content={content} />));
    expect(click("notes")).toBe(false);
    expect(click("site")).toBe(false);
  });
});
