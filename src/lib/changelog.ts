/**
 * Reads the bundled CHANGELOG for the "What's new" dialog. Pure functions only;
 * the dialog and its auto-open wrapper live in `components/WhatsNewDialog.tsx`.
 *
 * Only two sections of each release are shown: the overview and the
 * user-facing list. "Developer & Governance" is for contributors, not users.
 */

export interface ChangelogEntry {
  version: string;
  date: string;
  /** Markdown of the "Release Overview" section, trimmed; "" when absent. */
  overview: string;
  /** Markdown of the "User-facing" section, trimmed; "" when absent. */
  userFacing: string;
}

/** Settings key holding the last version whose "What's new" the user closed. */
export const WHATS_NEW_SEEN_KEY = "whats_new_seen_version";

/**
 * At most this many releases in one dialog after a multi-version jump. Picked
 * by hand as "enough to cover a few skipped updates without a wall of text",
 * not derived from anything.
 */
export const MAX_WHATS_NEW_ENTRIES = 5;

const VERSION_HEADING = /^## \[(\d+\.\d+\.\d+)\] - (\d{4}-\d{2}-\d{2})\s*$/;
const SECTION_HEADING = /^### (.+?)\s*$/;
const OVERVIEW_TITLES = new Set(["Release Overview", "发布概览"]);
const USER_FACING_TITLES = new Set(["User-facing", "用户可见更新"]);

/**
 * Splits a CHANGELOG into releases, newest first as written. A version that
 * appears twice keeps its first entry (CHANGELOG.md has a leftover empty
 * template above the real 1.14.3), so versions are unique and safe as keys.
 */
export function parseChangelog(markdown: string): ChangelogEntry[] {
  const entries: ChangelogEntry[] = [];
  const seen = new Set<string>();
  let entry: ChangelogEntry | null = null;
  let section: "overview" | "userFacing" | null = null;
  let buffer: string[] = [];

  const flushSection = () => {
    if (entry && section) entry[section] = buffer.join("\n").trim();
    buffer = [];
  };

  for (const line of markdown.split(/\r?\n/)) {
    if (line.startsWith("## ")) {
      flushSection();
      section = null;
      entry = null;
      const match = VERSION_HEADING.exec(line);
      if (match && !seen.has(match[1])) {
        seen.add(match[1]);
        entry = { version: match[1], date: match[2], overview: "", userFacing: "" };
        entries.push(entry);
      }
      continue;
    }
    if (!entry) continue;
    const heading = SECTION_HEADING.exec(line);
    if (heading) {
      flushSection();
      const title = heading[1];
      section = OVERVIEW_TITLES.has(title)
        ? "overview"
        : USER_FACING_TITLES.has(title)
          ? "userFacing"
          : null;
      continue;
    }
    if (section) buffer.push(line);
  }
  flushSection();
  return entries;
}

/** Compares x.y.z versions numerically: negative when a < b. */
export function compareVersions(a: string, b: string): number {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    const diff = (pa[i] || 0) - (pb[i] || 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

/**
 * Releases to show, newest first. With `lastSeen`, everything after it up to
 * `current`, capped at MAX_WHATS_NEW_ENTRIES; without, `current` alone.
 * Releases with no user-facing changes are dropped.
 */
export function selectWhatsNew(
  entries: ChangelogEntry[],
  current: string,
  lastSeen: string | null
): ChangelogEntry[] {
  return entries
    .filter((entry) => {
      if (!entry.userFacing) return false;
      if (lastSeen === null) return compareVersions(entry.version, current) === 0;
      return (
        compareVersions(entry.version, lastSeen) > 0 &&
        compareVersions(entry.version, current) <= 0
      );
    })
    .sort((a, b) => compareVersions(b.version, a.version))
    .slice(0, MAX_WHATS_NEW_ENTRIES);
}

export interface WhatsNewDecision {
  /** Releases to show; empty means no dialog. */
  entries: ChangelogEntry[];
  /** When to store `current` as seen: right away, once the dialog closes, or not at all. */
  record: "now" | "onClose" | "none";
}

/**
 * Whether to open "What's new" at launch. `loadEntries` is only called when a
 * dialog is possible, so an unchanged version never loads the CHANGELOG chunk.
 * A missing `lastSeen` with a non-empty library means an update from a version
 * that predates this dialog; with an empty library it is a fresh install.
 */
export async function decideWhatsNew({
  current,
  lastSeen,
  hasLibrary,
  loadEntries,
}: {
  current: string;
  lastSeen: string | null;
  hasLibrary: boolean;
  loadEntries: () => Promise<ChangelogEntry[]>;
}): Promise<WhatsNewDecision> {
  if (lastSeen !== null) {
    const order = compareVersions(current, lastSeen);
    if (order === 0) return { entries: [], record: "none" };
    if (order < 0) return { entries: [], record: "now" };
  } else if (!hasLibrary) {
    return { entries: [], record: "now" };
  }
  const entries = selectWhatsNew(await loadEntries(), current, lastSeen);
  return entries.length > 0 ? { entries, record: "onClose" } : { entries: [], record: "now" };
}

/**
 * The CHANGELOG bundled with this build, in the UI language: Chinese for zh
 * and zh-TW (as the i18n fallback does), English otherwise. Each file is its
 * own chunk, fetched only here.
 */
export async function loadChangelog(language: string): Promise<ChangelogEntry[]> {
  const raw =
    language === "zh" || language === "zh-TW"
      ? (await import("../../CHANGELOG-zh.md?raw")).default
      : (await import("../../CHANGELOG.md?raw")).default;
  return parseChangelog(raw);
}
