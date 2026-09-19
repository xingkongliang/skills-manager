import { useSyncExternalStore } from "react";
import * as api from "./tauri";

export const UNTAGGED_FILTER = "__untagged__";

/**
 * Drop tag filters whose pill is no longer on screen.
 * When the last skill carrying a tag is deleted the tag vanishes from the
 * available set, but a filter still selecting it would linger and silently
 * hide every remaining skill (the list looks empty for no visible reason).
 * `hasUntagged` mirrors the untagged pill's own render condition ("some skill
 * carries no tag") — that pill disappears the same way, so the sentinel has to
 * be reclaimed too.
 * Returns `prev` unchanged (same reference) when nothing is stale, so it is
 * safe to return directly from a `setState` updater without causing a loop.
 */
export function pruneStaleTagFilters(
  prev: Set<string>,
  availableTags: string[],
  hasUntagged: boolean
): Set<string> {
  if (prev.size === 0) return prev;
  const available = new Set(availableTags);
  if (hasUntagged) available.add(UNTAGGED_FILTER);
  const cleaned = new Set([...prev].filter((tag) => available.has(tag)));
  return cleaned.size === prev.size ? prev : cleaned;
}

const TAG_COLOR_CLASSES = [
  "bg-blue-500/15 text-blue-600 dark:text-blue-400",
  "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400",
  "bg-violet-500/15 text-violet-600 dark:text-violet-400",
  "bg-amber-500/15 text-amber-600 dark:text-amber-400",
  "bg-rose-500/15 text-rose-600 dark:text-rose-400",
  "bg-cyan-500/15 text-cyan-600 dark:text-cyan-400",
  "bg-orange-500/15 text-orange-600 dark:text-orange-400",
  "bg-pink-500/15 text-pink-600 dark:text-pink-400",
];

const TAG_ACTIVE_CLASSES = [
  "bg-blue-500 text-white dark:bg-blue-500",
  "bg-emerald-500 text-white dark:bg-emerald-500",
  "bg-violet-500 text-white dark:bg-violet-500",
  "bg-amber-500 text-white dark:bg-amber-500",
  "bg-rose-500 text-white dark:bg-rose-500",
  "bg-cyan-500 text-white dark:bg-cyan-500",
  "bg-orange-500 text-white dark:bg-orange-500",
  "bg-pink-500 text-white dark:bg-pink-500",
];

/** Swatch classes for the colour picker, one per palette slot. */
export const TAG_SWATCH_CLASSES = [
  "bg-blue-500",
  "bg-emerald-500",
  "bg-violet-500",
  "bg-amber-500",
  "bg-rose-500",
  "bg-cyan-500",
  "bg-orange-500",
  "bg-pink-500",
];

export const TAG_COLOR_COUNT = TAG_COLOR_CLASSES.length;

/**
 * Explicit per-tag colour picks, keyed by tag name, valued by palette index.
 * Lives in the `tag_colors` setting; the backend carries an entry across
 * `rename_tag` and drops it on `delete_tag` (see SkillStore::carry_tag_color).
 * Kept as a tiny external store rather than context so `getTagColor` keeps
 * its call signature and every pill picks the override up without plumbing.
 */
type TagColorMap = Readonly<Record<string, number>>;
let tagColors: TagColorMap = {};
const listeners = new Set<() => void>();

function emit() {
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot() {
  return tagColors;
}

/** Re-render on override changes. Call in any component that paints tag pills. */
export function useTagColors(): TagColorMap {
  return useSyncExternalStore(subscribe, getSnapshot);
}

/** Load overrides from the backend (startup). Failure leaves the defaults. */
export async function loadTagColors() {
  try {
    const raw = await api.getSettings(api.TAG_COLORS_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : {};
    const next: Record<string, number> = {};
    if (parsed && typeof parsed === "object") {
      for (const [tag, idx] of Object.entries(parsed as Record<string, unknown>)) {
        if (typeof idx === "number" && Number.isInteger(idx) && idx >= 0 && idx < TAG_COLOR_COUNT) {
          next[tag] = idx;
        }
      }
    }
    tagColors = next;
    emit();
  } catch {
    // Colour is cosmetic; keep whatever we had.
  }
}

/**
 * Pick a colour for one tag, or `null` to return it to the positional default.
 * Updates the in-memory store first so the UI responds immediately, then
 * persists; on a failed write the previous map is restored and the error is
 * re-thrown for the caller's toast.
 */
export async function setTagColor(tag: string, index: number | null) {
  const prev = tagColors;
  const next: Record<string, number> = { ...prev };
  if (index === null) delete next[tag];
  else next[tag] = index;
  tagColors = next;
  emit();
  try {
    await api.setSettings(api.TAG_COLORS_KEY, JSON.stringify(next));
  } catch (error) {
    tagColors = prev;
    emit();
    throw error;
  }
}

function resolveColorIndex(tag: string, allTags: string[]) {
  const override = tagColors[tag];
  if (override !== undefined) return override;
  const idx = allTags.indexOf(tag);
  return (idx === -1 ? 0 : idx) % TAG_COLOR_CLASSES.length;
}

/** The palette slot a tag currently renders with (override or default). */
export function getTagColorIndex(tag: string, allTags: string[]) {
  return resolveColorIndex(tag, allTags);
}

export function getTagColor(tag: string, allTags: string[]) {
  return TAG_COLOR_CLASSES[resolveColorIndex(tag, allTags)];
}

export function getTagActiveColor(tag: string, allTags: string[]) {
  return TAG_ACTIVE_CLASSES[resolveColorIndex(tag, allTags)];
}
