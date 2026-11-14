import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";

const doc = "# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";

// The module decides once, at import, whether the engine can take remark-gfm.
async function renderWith(lookbehind: boolean) {
  if (!lookbehind) {
    const Native = RegExp;
    vi.stubGlobal(
      "RegExp",
      new Proxy(Native, {
        construct(target, args: [string | RegExp, string?]) {
          const source = args[0] instanceof Native ? args[0].source : String(args[0]);
          if (/\(\?<[=!]/.test(source)) {
            throw new SyntaxError("Invalid regular expression: invalid group specifier name");
          }
          return new target(...args);
        },
      })
    );
  }
  vi.resetModules();
  const { SkillMarkdown } = await import("./SkillMarkdown");
  return renderToStaticMarkup(<SkillMarkdown content={doc} />);
}

afterEach(() => {
  vi.unstubAllGlobals();
});

// #613: WebKit before Safari 16.4 cannot compile remark-gfm's lookbehind regex.
describe("SkillMarkdown on engines without regex lookbehind", () => {
  it("renders plain Markdown instead of GFM", async () => {
    const markup = await renderWith(false);
    expect(markup).toContain("Title");
    expect(markup).not.toContain("<table");
  });

  it("keeps GFM where lookbehind works", async () => {
    expect(await renderWith(true)).toContain("<table");
  });
});
