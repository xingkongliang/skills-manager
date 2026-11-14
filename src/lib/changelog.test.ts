import { describe, expect, it, vi } from "vitest";
import changelogEn from "../../CHANGELOG.md?raw";
import changelogZh from "../../CHANGELOG-zh.md?raw";
import packageJson from "../../package.json?raw";
import {
  MAX_WHATS_NEW_ENTRIES,
  SECTION_TITLES,
  compareVersions,
  decideWhatsNew,
  parseChangelog,
  selectWhatsNew,
  type ChangelogEntry,
} from "./changelog";

const FIXTURE = `# Changelog

Intro text.

## [1.10.0] - 2026-03-02

### Release Overview
- Ten is here.

### User-facing
- **Big thing** — it works.
- Small thing.

### Developer & Governance
- Internal refactor.

## [1.9.0] - 2026-03-01

### 发布概览
- 九。

### 用户可见更新
- 中文条目。

### 开发者与治理更新
- 内部重构。

## [1.8.0] - 2026-02-01

### Release Overview
- Only internals.

### Developer & Governance
- CI.

## [1.7.0] - 2026-01-01

### Added
- Old format entry.
`;

describe("parseChangelog", () => {
  const entries = parseChangelog(FIXTURE);

  it("splits releases and keeps only overview and user-facing sections", () => {
    expect(entries.map((e) => e.version)).toEqual(["1.10.0", "1.9.0", "1.8.0", "1.7.0"]);
    expect(entries[0]).toEqual({
      version: "1.10.0",
      date: "2026-03-02",
      overview: "- Ten is here.",
      userFacing: "- **Big thing** — it works.\n- Small thing.",
    });
    expect(entries[1].overview).toBe("- 九。");
    expect(entries[1].userFacing).toBe("- 中文条目。");
  });

  it("ignores the developer section", () => {
    const all = entries.map((e) => e.overview + e.userFacing).join("\n");
    expect(all).not.toContain("Internal refactor");
    expect(all).not.toContain("内部重构");
    expect(entries[2].userFacing).toBe("");
  });

  it("reads old-format releases as empty", () => {
    expect(entries[3]).toMatchObject({ overview: "", userFacing: "" });
  });

  // What scripts/prepare-release.mjs writes before anyone fills it in; the
  // Prepare Release workflow can ship it as is.
  it("reads an unfilled release template as empty", () => {
    const template = [
      "## [2.0.0] - 2026-12-01", "",
      "### Release Overview", "- ", "",
      "### User-facing", "- ", "",
      "### Developer & Governance", "- ", "",
      "## [1.9.0] - 2026-11-01", "",
      "### 发布概览", "-", "",
      "### 用户可见更新", "- ", "", "",
      "## [1.8.0] - 2026-10-01", "",
      "### User-facing", "- Real item.", "- ", "",
    ].join("\n");
    const parsed = parseChangelog(template);
    expect(parsed[0]).toMatchObject({ version: "2.0.0", overview: "", userFacing: "" });
    expect(parsed[1]).toMatchObject({ version: "1.9.0", overview: "", userFacing: "" });
    // Only a section with nothing else counts as empty.
    expect(parsed[2].userFacing).toBe("- Real item.\n-");
    expect(selectWhatsNew(parsed, "2.0.0", null)).toEqual([]);
    expect(selectWhatsNew(parsed, "2.0.0", "1.8.0")).toEqual([]);
  });

  it("keeps the first entry of a duplicated version", () => {
    const dup = parseChangelog(
      "## [1.0.0] - 2026-01-02\n\n### User-facing\n- first\n## [1.0.0] - 2026-01-01\n\n### User-facing\n- second\n"
    );
    expect(dup).toHaveLength(1);
    expect(dup[0].userFacing).toBe("- first");
  });
});

describe("compareVersions", () => {
  it("compares numerically, not as strings", () => {
    expect(compareVersions("1.9.0", "1.10.0")).toBeLessThan(0);
    expect(compareVersions("1.10.0", "1.9.0")).toBeGreaterThan(0);
    expect(compareVersions("2.0.0", "1.99.99")).toBeGreaterThan(0);
    expect(compareVersions("1.2.3", "1.2.3")).toBe(0);
    expect(compareVersions("1.2.3", "1.2.10")).toBeLessThan(0);
  });
});

function release(version: string, userFacing = `- change in ${version}`): ChangelogEntry {
  return { version, date: "2026-01-01", overview: `- ${version}`, userFacing };
}

describe("selectWhatsNew", () => {
  const entries = parseChangelog(FIXTURE);

  it("returns releases after lastSeen up to current, newest first, without empty ones", () => {
    expect(selectWhatsNew(entries, "1.10.0", "1.7.0").map((e) => e.version)).toEqual([
      "1.10.0",
      "1.9.0",
    ]);
  });

  it("excludes lastSeen itself and anything newer than current", () => {
    expect(selectWhatsNew(entries, "1.9.0", "1.9.0")).toEqual([]);
    expect(selectWhatsNew(entries, "1.9.0", "1.8.0").map((e) => e.version)).toEqual(["1.9.0"]);
  });

  it("returns only current when lastSeen is null", () => {
    // 1.9.0 has user-facing changes too, so it must be left out explicitly.
    expect(selectWhatsNew(entries, "1.10.0", null).map((e) => e.version)).toEqual(["1.10.0"]);
    expect(selectWhatsNew(entries, "1.9.0", null).map((e) => e.version)).toEqual(["1.9.0"]);
    expect(selectWhatsNew(entries, "1.8.0", null)).toEqual([]);
  });

  it("caps a long jump at the newest MAX_WHATS_NEW_ENTRIES", () => {
    const many = ["1.0.8", "1.0.7", "1.0.6", "1.0.5", "1.0.4", "1.0.3", "1.0.2", "1.0.1"].map((v) =>
      release(v)
    );
    const picked = selectWhatsNew(many, "1.0.8", "1.0.0");
    expect(MAX_WHATS_NEW_ENTRIES).toBe(5);
    expect(picked.map((e) => e.version)).toEqual(["1.0.8", "1.0.7", "1.0.6", "1.0.5", "1.0.4"]);
  });
});

describe("decideWhatsNew", () => {
  const entries = [release("1.3.0"), release("1.2.0", ""), release("1.1.0")];
  const decide = (current: string, lastSeen: string | null, hasLibrary = true) => {
    const loadEntries = vi.fn().mockResolvedValue(entries);
    return decideWhatsNew({ current, lastSeen, hasLibrary, loadEntries }).then((d) => ({
      ...d,
      versions: d.entries.map((e) => e.version),
      loaded: loadEntries.mock.calls.length > 0,
    }));
  };

  it("shows the new releases after an update and records on close", async () => {
    const d = await decide("1.3.0", "1.1.0");
    expect(d.versions).toEqual(["1.3.0"]);
    expect(d.record).toBe("onClose");
  });

  it("records right away when the skipped releases have nothing user-facing", async () => {
    const d = await decide("1.2.0", "1.1.0");
    expect(d.versions).toEqual([]);
    expect(d.record).toBe("now");
  });

  it("does nothing when the version is unchanged, without loading the changelog", async () => {
    const d = await decide("1.3.0", "1.3.0");
    expect(d).toMatchObject({ versions: [], record: "none", loaded: false });
  });

  it("records the lower version after a downgrade", async () => {
    const d = await decide("1.1.0", "1.3.0");
    expect(d).toMatchObject({ versions: [], record: "now", loaded: false });
  });

  it("treats a missing record with a library as an update from before this dialog", async () => {
    const d = await decide("1.3.0", null, true);
    expect(d.versions).toEqual(["1.3.0"]);
    expect(d.record).toBe("onClose");
  });

  it("treats a missing record with an empty library as a fresh install", async () => {
    const d = await decide("1.3.0", null, false);
    expect(d).toMatchObject({ versions: [], record: "now", loaded: false });
  });
});

// The parser depends on the exact `##` / `###` headings. If the CHANGELOG
// format drifts, these fail in CI instead of the dialog going quietly empty.
// They must stay green on an unfilled release template, which is a supported
// way to cut a release (the dialog then simply does not open).
const RECENT_RELEASES = 10;

/** Every `###` heading in the newest releases, read from the raw text. */
function recentHeadings(markdown: string): string[] {
  return markdown
    .split(/^## \[/m)
    .slice(1, RECENT_RELEASES + 1)
    .flatMap((release) => release.split(/\r?\n/))
    .filter((line) => line.startsWith("### "))
    .map((line) => line.slice(4).trim());
}

describe("the real CHANGELOG files", () => {
  const en = parseChangelog(changelogEn);
  const zh = parseChangelog(changelogZh);
  const current = (JSON.parse(packageJson) as { version: string }).version;

  it("list the same releases in both languages", () => {
    expect(en.length).toBeGreaterThan(RECENT_RELEASES);
    expect(en.map((e) => `${e.version} ${e.date}`)).toEqual(zh.map((e) => `${e.version} ${e.date}`));
  });

  it("have the current version in both languages", () => {
    expect(en.map((e) => e.version)).toContain(current);
    expect(zh.map((e) => e.version)).toContain(current);
  });

  it("use only the known section headings in recent releases", () => {
    for (const [markdown, titles] of [
      [changelogEn, SECTION_TITLES.en],
      [changelogZh, SECTION_TITLES.zh],
    ] as const) {
      const known: string[] = Object.values(titles);
      const headings = recentHeadings(markdown);
      expect(headings.length).toBeGreaterThan(0);
      expect(headings.filter((heading) => !known.includes(heading))).toEqual([]);
    }
  });
});
