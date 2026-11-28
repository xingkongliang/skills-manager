import { describe, expect, it } from "vitest";
import {
  buildGitPreviewSelections,
  countGitPreviewSelection,
  duplicateGitPreviewNames,
  filterGitPreviewSelections,
  setVisibleGitPreviewSelection,
  type GitPreviewSelection,
} from "./gitPreview";
import type { GitSkillPreview } from "./tauri";

const preview = (rel_path: string, installed: boolean): GitSkillPreview => ({
  rel_path,
  name: rel_path.split("/").pop() ?? rel_path,
  description: `${rel_path} description`,
  installed,
});

const selection = (
  rel_path: string,
  selected: boolean,
  installed: boolean
): GitPreviewSelection => ({
  rel_path,
  name: rel_path,
  description: null,
  selected,
  installed,
});

describe("buildGitPreviewSelections", () => {
  it("leaves skills the library already holds unchecked", () => {
    const selections = buildGitPreviewSelections([
      preview("skills/alpha", true),
      preview("skills/beta", false),
    ]);

    expect(selections.map((s) => [s.rel_path, s.selected])).toEqual([
      ["skills/alpha", false],
      ["skills/beta", true],
    ]);
  });

  it("selects every skill on a first visit and keeps the row content", () => {
    const selections = buildGitPreviewSelections([preview("skills/alpha", false)]);

    expect(selections).toEqual([
      {
        rel_path: "skills/alpha",
        name: "alpha",
        description: "skills/alpha description",
        selected: true,
        installed: false,
      },
    ]);
  });
});

describe("filterGitPreviewSelections", () => {
  const rows = [
    selection("skills/a", true, false),
    selection("skills/b", false, true),
    selection("skills/c", true, true),
  ];

  it("shows only installed skills", () => {
    expect(
      filterGitPreviewSelections(rows, "installed").map((s) => s.rel_path)
    ).toEqual(["skills/b", "skills/c"]);
  });

  it("shows only skills the library does not hold", () => {
    expect(
      filterGitPreviewSelections(rows, "uninstalled").map((s) => s.rel_path)
    ).toEqual(["skills/a"]);
  });

  it("keeps every row under the default filter", () => {
    expect(filterGitPreviewSelections(rows, "all")).toBe(rows);
  });
});

describe("countGitPreviewSelection", () => {
  it("counts checked rows out of every discovered row", () => {
    expect(
      countGitPreviewSelection([
        selection("skills/a", true, false),
        selection("skills/b", false, true),
        selection("skills/c", false, false),
      ])
    ).toEqual({ selected: 1, total: 3 });
  });
});

describe("duplicateGitPreviewNames", () => {
  it("names the rows that need their folder printed", () => {
    // A repo that vendors a copy of a skill under an example folder lists two
    // rows called `prolog-query`; only one of them is installed, so the dialog
    // has to show where each lives.
    const duplicates = duplicateGitPreviewNames([
      { ...selection("agent-web-demo/skills/prolog-query", true, false), name: "prolog-query" },
      { ...selection("prolog-query", false, true), name: "prolog-query" },
      { ...selection("api-change-comment", true, true), name: "api-change-comment" },
    ]);

    expect([...duplicates]).toEqual(["prolog-query"]);
  });

  it("is empty when every name is unique", () => {
    expect(
      duplicateGitPreviewNames([
        selection("skills/a", true, false),
        selection("skills/b", true, false),
      ])
    ).toEqual(new Set());
  });
});

describe("setVisibleGitPreviewSelection", () => {
  it("changes only the rows the filter shows", () => {
    const visible = new Set(["skills/b", "skills/c"]);
    const rows = [
      selection("skills/a", true, false),
      selection("skills/b", false, true),
      selection("skills/c", false, true),
    ];

    expect(
      setVisibleGitPreviewSelection(rows, visible, true).map((s) => [
        s.rel_path,
        s.selected,
      ])
    ).toEqual([
      ["skills/a", true],
      ["skills/b", true],
      ["skills/c", true],
    ]);

    expect(
      setVisibleGitPreviewSelection(rows, visible, false).map((s) => [
        s.rel_path,
        s.selected,
      ])
    ).toEqual([
      ["skills/a", true],
      ["skills/b", false],
      ["skills/c", false],
    ]);
  });
});
