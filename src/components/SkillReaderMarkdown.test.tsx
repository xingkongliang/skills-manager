import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SkillReaderMarkdown } from "./SkillReaderMarkdown";
import { buildTranslationPlan } from "../lib/markdown/translationPlan";

describe("SkillReaderMarkdown bilingual rendering", () => {
  it("renders each tight-list and table translation once, with escaped provider text", () => {
    const content = "# Title\n\n- Read `SKILL.md`.\n  - Nested paragraph.\n\n| Name | Detail |\n|---|---|\n| API | Call service |";
    const plan = buildTranslationPlan(content);
    const translations = Object.fromEntries(plan.blocks.map((b) => [b.id, "<script>translated</script>"]));
    const markup = renderToStaticMarkup(<SkillReaderMarkdown content={content} plan={plan} translations={translations} />);
    expect(markup.match(/reader-translation/g)).toHaveLength(7);
    expect(markup).toContain("&lt;script&gt;translated&lt;/script&gt;");
    expect(markup).not.toContain("<script>");
    expect(markup).toContain("<code");
    expect(markup.match(/id="title"/g)).toHaveLength(1);
    expect(markup.match(/<table/g)).toHaveLength(1);
  });
});
