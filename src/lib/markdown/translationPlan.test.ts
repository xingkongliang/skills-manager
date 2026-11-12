import { describe, expect, it } from "vitest";
import { buildTranslationPlan, restoreProtectedText, splitTranslationText } from "./translationPlan";

describe("Markdown translation plan", () => {
  it("excludes metadata and code but visits nested tight lists and cells exactly once", () => {
    const plan = buildTranslationPlan('---\nname: hidden\n---\n# Guide\n\n- Read `SKILL.md` first.\n  - Open the guide.\n\n| Name | Description |\n|---|---|\n| API | Call the service |\n\n```sh\necho hello\n```');
    expect(plan.blocks).toHaveLength(7);
    expect(plan.blocks.map((b) => b.request).join("\n")).not.toMatch(/hidden|echo hello/);
    const item = plan.blocks.find((b) => b.tokens.includes("SKILL.md"))!;
    expect(item.request).not.toContain("SKILL.md");
    expect(restoreProtectedText(item.request.replace("Read", "阅读"), item.tokens)).toContain("SKILL.md");
  });
  it("refuses damaged or duplicated protected tokens", () => {
    expect(() => restoreProtectedText("lost", ["cmd"])).toThrow();
    expect(() => restoreProtectedText("⟦S0⟧ ⟦S0⟧", ["cmd"])).toThrow();
    expect(() => restoreProtectedText("⟦S9⟧", [])).toThrow();
  });
  it("skips Chinese and code-only blocks but translates mixed English prose", () => {
    const blocks = buildTranslationPlan("纯中文段落。\n\n`git status`\n\nUse 中文 in this example.").blocks;
    expect(blocks.map((b) => b.skipped)).toEqual([true, true, false]);
  });
  it("splits long text without cutting protected tokens or surrogate pairs", () => {
    const text = "Hello 😀 world. ".repeat(500) + "⟦S0⟧";
    const pieces = splitTranslationText(text);
    expect(pieces.join("")).toBe(text);
    expect(pieces.every((p) => p.length <= 3500 && !/[\uD800-\uDBFF]$/.test(p))).toBe(true);
    expect(pieces.some((p) => p.includes("⟦S0⟧"))).toBe(true);
  });
  it("keeps heading ids stable and deduplicates identical headings", () => {
    const plan = buildTranslationPlan("# Hello World\n\n# Hello World");
    expect([...plan.anchors.values()]).toEqual(["hello-world", "hello-world-1"]);
  });
  it("reads literal token examples in original mode without throwing", () => {
    const plan = buildTranslationPlan("# Literal ⟦S0⟧\n\nUse ⟦S0⟧ with `code`.");
    const block = plan.blocks[1];
    expect(restoreProtectedText(block.request, block.tokens)).toBe("Use ⟦S0⟧ with code.");
  });
  it("does not collide with a heading that already ends in a numeric suffix", () => {
    expect([...buildTranslationPlan("# Foo\n\n# Foo-1\n\n# Foo").anchors.values()]).toEqual(["foo", "foo-1", "foo-2"]);
  });
});
