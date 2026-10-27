import type { MoveCandidate } from "./tauri";

/**
 * What the row should offer for a `moved` skill.
 *
 * The backend never ranks low-confidence candidates: it hands over every place
 * whose name matched. A single high-confidence candidate is the only case where
 * the path is not a choice — the bytes are identical — so it is the only case
 * with a single click. Anything else has to be picked, path by path.
 */
export type RepointChoice =
  | { kind: "none" }
  | { kind: "oneClick"; subpath: string }
  | { kind: "choose"; candidates: MoveCandidate[] };

export function repointChoice(candidates: MoveCandidate[]): RepointChoice {
  if (candidates.length === 0) return { kind: "none" };
  if (candidates.length === 1 && candidates[0].confidence === "high") {
    return { kind: "oneClick", subpath: candidates[0].subpath };
  }
  return { kind: "choose", candidates };
}
