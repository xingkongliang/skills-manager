import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
    return twMerge(clsx(inputs));
}

/** Normalize a tag for dedup comparison: lowercase, remove all spaces */
export function normalizeTag(tag: string): string {
  return tag.toLowerCase().replace(/\s+/g, "");
}

/** Merge new tags into existing tags, deduplicating by normalized form */
export function mergeTags(existing: string[], incoming: string[]): string[] {
  const seen = new Set(existing.map(normalizeTag));
  const merged = [...existing];
  for (const tag of incoming) {
    const key = normalizeTag(tag);
    if (!seen.has(key)) {
      seen.add(key);
      merged.push(tag);
    }
  }
  return merged;
}

/** Truncate skill content: keep full frontmatter + first N chars of body */
export function truncateSkillContent(content: string, maxChars = 300): string {
  const fmEnd = content.indexOf("\n---\n", 4);
  if (fmEnd === -1) return content.slice(0, maxChars);
  const frontmatter = content.slice(0, fmEnd + 5);
  const body = content.slice(fmEnd + 5);
  return frontmatter + body.slice(0, maxChars);
}

/** Build reverse map from AI consolidation mapping and apply to a skill's tags.
 *  Returns null if no change needed. */
export function applyTagMapping(
  mapping: Record<string, string[]>,
  skillTags: string[],
): string[] | null {
  const reverseMap = new Map<string, string>();
  for (const [canonical, originals] of Object.entries(mapping)) {
    if (Array.isArray(originals)) {
      for (const original of originals) {
        reverseMap.set(original.toLowerCase().trim(), canonical);
      }
    }
  }

  const newTags: string[] = [];
  const seen = new Set<string>();
  let changed = false;
  for (const tag of skillTags) {
    const canonical = reverseMap.get(tag.toLowerCase().trim()) ?? tag;
    if (canonical !== tag) changed = true;
    const key = normalizeTag(canonical);
    if (!seen.has(key)) {
      seen.add(key);
      newTags.push(canonical);
    }
  }
  if (!changed && newTags.length === skillTags.length) return null;
  return newTags;
}
