import { describe, expect, it } from "vitest";
import { classifyReaderLink, ReaderHistory } from "./skillReader";

describe("reader links", () => {
  it("separates relative files and fragments from external and blocked schemes", () => {
    expect(classifyReaderLink("references/guide.md#setup")).toBe("internal");
    expect(classifyReaderLink("#setup")).toBe("internal");
    expect(classifyReaderLink("https://example.com/a")).toBe("external");
    for (const href of ["javascript:alert(1)", "data:text/html,hi", "file:///etc/passwd", "/etc/passwd", "//example.com/a", "C:\\test"]) {
      expect(classifyReaderLink(href)).toBe("blocked");
    }
  });
});

describe("reader navigation", () => {
  it("restores per-entry scroll and discards forward history after branching", () => {
    const h = new ReaderHistory();
    h.push("SKILL.md");
    h.saveScroll(125);
    h.push("references/a.md", "setup");
    h.saveScroll(360);
    expect(h.move(-1)).toEqual({ path: "SKILL.md", fragment: null, scrollTop: 125 });
    expect(h.move(1)?.scrollTop).toBe(360);
    expect(h.current?.fragment).toBe("setup");
    h.move(-1);
    expect(h.move(1)?.fragment).toBeNull();
    h.move(-1);
    h.push("other.md");
    expect(h.canForward).toBe(false);
  });
  it("restores the selected file and scroll when a source snapshot is refreshed", () => {
    const h = new ReaderHistory();
    h.push("SKILL.md"); h.push("references/guide.md", "setup"); h.saveScroll(900);
    expect(h.initial("SKILL.md")).toEqual({ path: "references/guide.md", fragment: null, scrollTop: 900 });
    expect(h.replaceCurrent("README.md")).toEqual({ path: "README.md", fragment: null, scrollTop: 0 });
  });
});
