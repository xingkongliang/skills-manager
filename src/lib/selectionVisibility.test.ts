import { describe, expect, it } from "vitest";
import { countHiddenSelected } from "./selectionVisibility";

const ids = (...values: string[]) => new Set(values);

describe("countHiddenSelected", () => {
  it("hides nothing when there is no selection", () => {
    expect(countHiddenSelected(ids(), [])).toBe(0);
    expect(countHiddenSelected(ids(), ["a", "b"])).toBe(0);
  });

  it("hides nothing when every selected skill is rendered", () => {
    expect(countHiddenSelected(ids("a", "b"), ["a", "b", "c"])).toBe(0);
  });

  it("counts the skills of a collapsed source swept up by select-all", () => {
    // Three skills selected, only the one in the open source is rendered.
    expect(countHiddenSelected(ids("a", "b", "c"), ["a"])).toBe(2);
  });

  it("counts a source collapsed after its skills were picked", () => {
    // Same arithmetic, different cause: the selection was made row by row.
    const picked = ids("b", "c");
    expect(countHiddenSelected(picked, ["a"])).toBe(2);
  });

  it("hides nothing when grouping is off", () => {
    // Ungrouped, every filtered skill has a row.
    expect(countHiddenSelected(ids("a", "b", "c"), ["a", "b", "c"])).toBe(0);
  });

  it("hides nothing while a search forces every source open", () => {
    expect(countHiddenSelected(ids("a", "b"), ["a", "b", "c"])).toBe(0);
  });

  it("treats an id with no row as hidden, leaving pruning to the caller", () => {
    // `useMultiSelect` drops ids whose skill is gone before this runs, so a
    // missing row here can only mean a display reason, which is what we count.
    expect(countHiddenSelected(ids("gone"), [])).toBe(1);
  });

  it("counts each selected skill once even if a row repeats", () => {
    expect(countHiddenSelected(ids("a", "b"), ["a", "a"])).toBe(1);
  });
});
