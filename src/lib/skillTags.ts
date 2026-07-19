export const UNTAGGED_FILTER = "__untagged__";

/**
 * Drop tag filters that no longer match any existing tag.
 * When the last skill carrying a tag is deleted the tag vanishes from the
 * available set, but a filter still selecting it would linger and silently
 * hide every remaining skill (the list looks empty for no visible reason).
 * The always-valid UNTAGGED sentinel is never pruned.
 * Returns `prev` unchanged (same reference) when nothing is stale, so it is
 * safe to return directly from a `setState` updater without causing a loop.
 */
export function pruneStaleTagFilters(prev: Set<string>, availableTags: string[]): Set<string> {
  if (prev.size === 0) return prev;
  const available = new Set(availableTags);
  available.add(UNTAGGED_FILTER);
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

function resolveColorIndex(tag: string, allTags: string[]) {
  const idx = allTags.indexOf(tag);
  return (idx === -1 ? 0 : idx) % TAG_COLOR_CLASSES.length;
}

export function getTagColor(tag: string, allTags: string[]) {
  return TAG_COLOR_CLASSES[resolveColorIndex(tag, allTags)];
}

export function getTagActiveColor(tag: string, allTags: string[]) {
  return TAG_ACTIVE_CLASSES[resolveColorIndex(tag, allTags)];
}
