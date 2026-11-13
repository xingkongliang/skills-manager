import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SkillMarkdown } from "./SkillMarkdown";

function render(content: string) {
  return renderToStaticMarkup(<SkillMarkdown content={content} />);
}

const body = "# Visible skill\n\nFollow these instructions.\n\n---\n\nKeep this body separator.";

// Exercise the real preview, not a second implementation of its parser.
describe("SkillMarkdown frontmatter", () => {
  it.each([
    { name: "LF", newline: "\n" },
    { name: "CRLF", newline: "\r\n" },
  ])("hides metadata with $name line endings without changing the body", ({ newline }) => {
    const markdownBody = body.replaceAll("\n", newline);
    const content = ["---", "name: example", "description: Metadata only", "---", "", markdownBody].join(newline);
    expect(render(content)).toBe(render(markdownBody));
  });

  it.each([
    { name: "LF", newline: "\n" },
    { name: "CRLF", newline: "\r\n" },
  ])("hides metadata ending at EOF with $name line endings", ({ newline }) => {
    expect(render(["---", "name: example", "---"].join(newline))).toBe(render(""));
  });

  it.each([
    { name: "LF", newline: "\n" },
    { name: "CRLF", newline: "\r\n" },
  ])("handles empty metadata with $name line endings", ({ newline }) => {
    expect(render(["---", "---", body].join(newline))).toBe(render(body));
  });

  it("accepts mixed LF and CRLF delimiters", () => {
    expect(render(`---\r\nname: example\n---\n${body}`)).toBe(render(body));
    expect(render(`---\nname: example\r\n---\r\n${body}`)).toBe(render(body));
  });

  it("keeps a delimiter inside a YAML value from ending the metadata", () => {
    expect(render(`---\nname: example\ndescription: before---after\n---\n${body}`)).toBe(render(body));
  });

  it("preserves a document without frontmatter", () => {
    const markup = render(body);
    expect(markup).toContain("Visible skill");
    expect(markup).toContain("Follow these instructions.");
    expect(markup).toContain("<hr");
    expect(markup).toContain("Keep this body separator.");
  });

  it("preserves an unclosed frontmatter block", () => {
    const markup = render("---\nname: example\n\n# Visible skill\n\nFollow these instructions.");
    expect(markup).toContain("name: example");
    expect(markup).toContain("Follow these instructions.");
  });

  it("does not use a delimiter with trailing text as a closing delimiter", () => {
    const markup = render("---\nname: example\n--- not a delimiter\n\nVisible body");
    expect(markup).toContain("name: example");
    expect(markup).toContain("--- not a delimiter");
    expect(markup).toContain("Visible body");
  });

  it("does not strip a frontmatter-like block later in the document", () => {
    const markup = render("# Intro\n\n---\nname: example\n---\n\nVisible body");
    expect(markup).toContain("Intro");
    expect(markup).toContain("name: example");
    expect(markup).toContain("Visible body");
  });
});
