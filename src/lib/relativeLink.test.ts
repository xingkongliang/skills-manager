import { describe, expect, it } from "vitest";
import { relativeLinkPath } from "./relativeLink";

describe("relativeLinkPath", () => {
  it.each([
    ["references/ooxml.md", "references/ooxml.md"],
    ["./references/ooxml.md", "references/ooxml.md"],
    ["././scripts/run.sh", "scripts/run.sh"],
    ["references/ooxml.md#schema", "references/ooxml.md"],
    ["references/ooxml.md?raw=1", "references/ooxml.md"],
    ["docs/my%20notes.md", "docs/my notes.md"],
    ["../outside.md", "../outside.md"],
  ])("%s -> %s", (href, expected) => {
    expect(relativeLinkPath(href)).toBe(expected);
  });

  it.each([
    undefined,
    "",
    "#section",
    "/abs/path.md",
    "//host/path",
    "https://example.com/a.md",
    "mailto:a@b.c",
    "file:///etc/passwd",
    "?only=query",
  ])("%s is not a relative link", (href) => {
    expect(relativeLinkPath(href)).toBeNull();
  });
});
