import { describe, expect, it } from "vitest";
import { toggleAll, toggleOne, toggleRange } from "./newSkillSelection";

const rows = ["a", "b", "c", "d", "e"];

describe("toggleOne", () => {
  it("adds a key that was not chosen and removes one that was", () => {
    expect([...toggleOne(new Set(), "b")]).toEqual(["b"]);
    expect([...toggleOne(new Set(["b"]), "b")]).toEqual([]);
  });
});

describe("toggleRange", () => {
  it("takes everything between the anchor and the row", () => {
    expect([...toggleRange(new Set(["b"]), rows, "b", "d")]).toEqual(["b", "c", "d"]);
  });

  it("works upwards as well as downwards", () => {
    expect([...toggleRange(new Set(), rows, "d", "b")]).toEqual(["b", "c", "d"]);
  });

  it("clears the run when every row in it was already chosen", () => {
    const all = new Set(["b", "c", "d"]);
    expect([...toggleRange(all, rows, "b", "d")]).toEqual([]);
  });

  it("fills the gaps when only part of the run was chosen", () => {
    expect([...toggleRange(new Set(["c"]), rows, "b", "d")]).toEqual(["c", "b", "d"]);
  });

  it("falls back to a plain toggle when the anchor is gone", () => {
    expect([...toggleRange(new Set(), rows, "gone", "c")]).toEqual(["c"]);
  });
});

describe("toggleAll", () => {
  it("chooses everything, then nothing", () => {
    expect([...toggleAll(new Set(), rows)]).toEqual(rows);
    expect([...toggleAll(new Set(rows), rows)]).toEqual([]);
  });

  it("chooses everything when only some are chosen", () => {
    expect([...toggleAll(new Set(["a"]), rows)]).toEqual(rows);
  });
});
