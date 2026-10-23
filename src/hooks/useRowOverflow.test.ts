import { describe, expect, it } from "vitest";
import { heightForRows } from "./useRowOverflow";

describe("heightForRows", () => {
  it("counts the gaps between rows, not after the last one", () => {
    expect(heightForRows(24, 6, 2)).toBe(54);
    expect(heightForRows(24, 6, 1)).toBe(24);
  });

  it("grows by one row plus one gap each time", () => {
    const two = heightForRows(20, 4, 2);
    const three = heightForRows(20, 4, 3);
    expect(three - two).toBe(24);
  });
});
