import { describe, expect, it } from "vitest";
import { relativeLinkPath, resolveRelativePath } from "./relativeLink";

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

describe("resolveRelativePath", () => {
  it.each([
    ["references/a.md", "b.md", "references/b.md"],
    ["references/a.md", "../SKILL.md", "SKILL.md"],
    ["references/a.md", "./sub/c.md", "references/sub/c.md"],
    ["references/deep/a.md", "../../scripts/run.sh", "scripts/run.sh"],
    ["references/a.md", "x/../b.md", "references/b.md"],
    // The root document resolves exactly as a root-relative path did before.
    ["SKILL.md", "references/ooxml.md", "references/ooxml.md"],
  ])("%s + %s -> %s", (fromFile, path, expected) => {
    expect(resolveRelativePath(fromFile, path)).toBe(expected);
  });

  it.each([
    ["references/a.md", "../../x"],
    ["SKILL.md", "../outside.md"],
    ["references/a.md", ".."],
  ])("%s + %s climbs out of the skill", (fromFile, path) => {
    expect(resolveRelativePath(fromFile, path)).toBeNull();
  });
});
