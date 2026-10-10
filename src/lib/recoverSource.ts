import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import * as api from "./tauri";
import type { ManagedSkill, RecoverSourcePick, SkillsShSkill } from "./tauri";

/**
 * Shared by the single-skill and the batch recovery dialogs.
 *
 * The search half of this feature grew a real bug when it lived inside one
 * dialog: two in-flight searches could share a generation, so a slow earlier
 * answer overwrote a newer one and the box said one thing while the list showed
 * another. Extracting it means the batch dialog inherits the fix instead of
 * reimplementing it.
 */

/** Which directory a lost skill was in, best guess first. */
export function searchSeeds(skill: ManagedSkill): string[] {
  const basename = (path: string | null): string | null => {
    if (!path) return null;
    const parts = path.split(/[\\/]/).filter(Boolean);
    return parts.length > 0 ? parts[parts.length - 1] : null;
  };
  return [basename(skill.source_ref), skill.name, basename(skill.central_path)].filter(
    (value): value is string => !!value && value.length > 0
  );
}

/** Folds the differences between how the two names were written. */
export function normalize(value: string): string {
  return value.toLowerCase().replace(/[-_\s]+/g, "");
}

export type Confidence = "exact" | "similar" | "other";

export function confidenceOf(candidate: SkillsShSkill, seed: string): Confidence {
  const a = normalize(candidate.skill_id);
  const b = normalize(seed);
  if (a === b) return "exact";
  if (a.includes(b) || b.includes(a)) return "similar";
  return "other";
}

/** Accepts a full URL or the `owner/repo` shorthand skills.sh itself uses. */
export function toRepoUrl(input: string): string {
  const trimmed = input.trim();
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed)) return trimmed;
  if (/^[\w.-]+\/[\w.-]+$/.test(trimmed)) return `https://github.com/${trimmed}.git`;
  return trimmed;
}

/** The last path segment, for showing which directory a candidate is. */
export function repoLabel(pick: RecoverSourcePick): string {
  try {
    const url = new URL(pick.repoUrl);
    return url.hostname.replace(/^www\./, "") + url.pathname.replace(/\.git$/, "");
  } catch {
    return pick.repoUrl;
  }
}

/** A skills.sh hit, as the source it points at. */
export function pickFromCandidate(hit: SkillsShSkill): RecoverSourcePick {
  return {
    repoUrl: `https://github.com/${hit.source}.git`,
    locatorSource: hit.source,
    locatorSkillId: hit.skill_id,
  };
}

export function useSkillsshSearch(seed: string, enabled = true) {
  const [candidates, setCandidates] = useState<SkillsShSkill[]>([]);
  const [searching, setSearching] = useState(false);
  const [searchFailed, setSearchFailed] = useState(false);
  /** Whether a search has come back yet — an untouched box is not "no results". */
  const [searched, setSearched] = useState(false);

  const generation = useRef(0);
  /** The seed already auto-searched for; null means "not yet". */
  const autoSearched = useRef<string | null>(null);

  const runSearch = useCallback(async (query: string) => {
    if (!query.trim()) return;
    // Claim the generation for *this* query, not for this box: a user who
    // searches again without waiting must not have the first answer land on
    // top of the second.
    const mine = ++generation.current;
    setSearching(true);
    setSearchFailed(false);
    try {
      const hits = await api.searchSkillssh(query, 30);
      if (mine !== generation.current) return;
      setCandidates(hits);
      setSearched(true);
    } catch {
      if (mine !== generation.current) return;
      // Discovery is an accelerator, not a gate: a marketplace that is down,
      // unreachable or unconfigured must not take the manual path with it.
      setCandidates([]);
      setSearchFailed(true);
      setSearched(true);
    } finally {
      if (mine === generation.current) setSearching(false);
    }
  }, []);

  // Seed once per distinct query, so a usable candidate is usually already on
  // screen. Keyed on the query rather than on "mounted", so returning to a
  // previous skill does not re-search but a new one always does.
  useEffect(() => {
    if (!enabled || !seed) return;
    if (autoSearched.current === seed) return;
    autoSearched.current = seed;
    void runSearch(seed);
  }, [enabled, seed, runSearch]);

  /** Drop everything and orphan any answer still in flight. */
  const reset = useCallback(() => {
    generation.current += 1;
    autoSearched.current = null;
    setCandidates([]);
    setSearching(false);
    setSearchFailed(false);
    setSearched(false);
  }, []);

  const grouped = useMemo(() => {
    const order: Confidence[] = ["exact", "similar", "other"];
    return order
      .map((level) => ({
        level,
        hits: candidates.filter((hit) => confidenceOf(hit, seed) === level),
      }))
      .filter((group) => group.hits.length > 0);
  }, [candidates, seed]);

  return { grouped, searching, searchFailed, searched, runSearch, reset };
}