import type { GitSkillPreview } from "./tauri";

/** One selectable row of the git install preview. */
export interface GitPreviewSelection {
  rel_path: string;
  name: string;
  description: string | null;
  selected: boolean;
  /** The library already holds this skill — used for the row's badge. */
  installed: boolean;
}

/** Which rows the preview shows; the selection itself is kept across filters. */
export type GitPreviewStatusFilter = "all" | "installed" | "uninstalled";

export interface GitPreviewSelectionCount {
  selected: number;
  total: number;
}

/**
 * Rows start selected so a first visit imports everything it found, except the
 * skills the library already holds: those stay visible (and selectable) but
 * unchecked, so re-visiting a repository imports only what is new.
 */
export function buildGitPreviewSelections(
  skills: GitSkillPreview[]
): GitPreviewSelection[] {
  return skills.map((skill) => ({
    rel_path: skill.rel_path,
    name: skill.name,
    description: skill.description,
    selected: !skill.installed,
    installed: skill.installed,
  }));
}

/** The rows a status filter shows. Hidden rows keep their selection. */
export function filterGitPreviewSelections(
  selections: GitPreviewSelection[],
  filter: GitPreviewStatusFilter
): GitPreviewSelection[] {
  if (filter === "all") return selections;
  const installed = filter === "installed";
  return selections.filter((selection) => selection.installed === installed);
}

/** Checked rows out of every discovered row, whatever the filter hides. */
export function countGitPreviewSelection(
  selections: GitPreviewSelection[]
): GitPreviewSelectionCount {
  return {
    selected: selections.filter((selection) => selection.selected).length,
    total: selections.length,
  };
}

/**
 * Display names that more than one row carries.
 *
 * A repository can hold the same skill twice — a root copy plus one vendored
 * into an example project (`agent-web-demo/skills/prolog-query`) — with the same
 * `name:` in both `SKILL.md` files. Only the folder tells them apart, and only
 * one of them may be installed, so the dialog prints the folder for these rows.
 */
export function duplicateGitPreviewNames(
  selections: GitPreviewSelection[]
): Set<string> {
  const seen = new Set<string>();
  const duplicates = new Set<string>();
  for (const selection of selections) {
    if (seen.has(selection.name)) duplicates.add(selection.name);
    seen.add(selection.name);
  }
  return duplicates;
}

/**
 * Select or clear the rows the filter currently shows. "Select all" under a
 * filter means what the user can see, so rows behind another filter keep their
 * state.
 */
export function setVisibleGitPreviewSelection(
  selections: GitPreviewSelection[],
  visible: ReadonlySet<string>,
  selected: boolean
): GitPreviewSelection[] {
  return selections.map((selection) =>
    visible.has(selection.rel_path) ? { ...selection, selected } : selection
  );
}
