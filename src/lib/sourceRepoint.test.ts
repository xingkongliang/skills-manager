import { describe, expect, it } from "vitest";
import { repointChoice } from "./sourceRepoint";
import type { MoveCandidate } from "./tauri";

const candidate = (
  subpath: string,
  confidence: MoveCandidate["confidence"],
  reason: MoveCandidate["reason"] = "directory_name",
): MoveCandidate => ({ subpath, confidence, reason });

describe("repointChoice", () => {
  it("offers a one-click re-point when the content is identical", () => {
    expect(repointChoice([candidate("moved-here", "high", "content_hash")])).toEqual({
      kind: "oneClick",
      subpath: "moved-here",
    });
  });

  it("never auto-picks among several candidates, even if all are high", () => {
    const choice = repointChoice([
      candidate("a", "high", "content_hash"),
      candidate("b", "high", "content_hash"),
    ]);
    expect(choice.kind).toBe("choose");
  });

  it("shows a lone low-confidence candidate instead of re-pointing at it", () => {
    const choice = repointChoice([candidate("maybe", "low")]);
    expect(choice).toEqual({
      kind: "choose",
      candidates: [candidate("maybe", "low")],
    });
  });

  it("offers nothing when there is no candidate", () => {
    expect(repointChoice([])).toEqual({ kind: "none" });
  });
});
