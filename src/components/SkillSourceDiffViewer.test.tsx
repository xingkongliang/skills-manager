// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SkillSourceDiffEntry } from "../lib/tauri";
import { SkillSourceDiffViewer } from "./SkillSourceDiffViewer";
import { DocumentDiffViewer } from "./DocumentDiffViewer";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => (opts ? `${key}${JSON.stringify(opts)}` : key),
  }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function textEntry(path: string, before = "a\n", after = "b\n"): SkillSourceDiffEntry {
  return {
    relative_path: path,
    status: "modified",
    content_kind: "text",
    original_text: before,
    updated_text: after,
    executable_before: false,
    executable_after: false,
  };
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const diffTables = () => container.querySelectorAll("table").length;
const headerButtons = () => Array.from(container.querySelectorAll<HTMLButtonElement>("button[aria-expanded]"));

describe("SkillSourceDiffViewer", () => {
  it("shows a short list open, as before", () => {
    act(() => root.render(<SkillSourceDiffViewer entries={[textEntry("a.md"), textEntry("b.md")]} />));
    expect(diffTables()).toBe(2);
    expect(headerButtons()).toHaveLength(0);
  });

  // #658: a long list renders no diff until a file is opened, and only one
  // file's diff at a time.
  it("starts a long list collapsed and opens one file at a time", () => {
    const entries = Array.from({ length: 200 }, (_, i) => textEntry(`f${i}.md`));
    act(() => root.render(<SkillSourceDiffViewer entries={entries} />));
    expect(diffTables()).toBe(0);
    expect(container.textContent).toContain("mySkills.sourceDiff.collapsedHint");

    act(() => headerButtons()[3].click());
    expect(diffTables()).toBe(1);
    expect(headerButtons()[3].getAttribute("aria-expanded")).toBe("true");

    act(() => headerButtons()[7].click());
    expect(diffTables()).toBe(1);
    expect(headerButtons()[3].getAttribute("aria-expanded")).toBe("false");

    act(() => headerButtons()[7].click());
    expect(diffTables()).toBe(0);
  });

  it("collapses a short list whose diff is over the budget", () => {
    const huge = "x\n".repeat(5000);
    act(() => root.render(<SkillSourceDiffViewer entries={[textEntry("big.md", "", huge)]} />));
    expect(headerButtons()).toHaveLength(1);
  });

  it("says when the list stops before every changed file", () => {
    const entries = Array.from({ length: 4 }, (_, i) => textEntry(`f${i}.md`));
    act(() => root.render(<SkillSourceDiffViewer entries={entries} truncated />));
    expect(container.textContent).toContain('mySkills.sourceDiff.truncated{"count":4}');
  });
});

describe("DocumentDiffViewer", () => {
  it("summarizes a pair too large to compare instead of computing it", () => {
    const lines = (n: number, tag: string) => Array.from({ length: n }, (_, i) => `${tag}${i}`).join("\n");
    act(() => root.render(<DocumentDiffViewer original={lines(1200, "a")} updated={lines(1200, "b")} />));
    expect(container.textContent).toContain("common.diffTooLarge");
    expect(diffTables()).toBe(0);
  });

  it("still diffs a pair within the budget", () => {
    act(() => root.render(<DocumentDiffViewer original={"same\nold\n"} updated={"same\nnew\n"} />));
    expect(diffTables()).toBe(1);
  });
});
