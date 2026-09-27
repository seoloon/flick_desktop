import { describe, expect, it } from "vitest";
import { hasAppHistory } from "./history";

describe("hasAppHistory", () => {
  it("is true only past the first in-app entry", () => {
    expect(hasAppHistory({ idx: 2, key: "k" })).toBe(true);
    expect(hasAppHistory({ idx: 0, key: "k" })).toBe(false);
    expect(hasAppHistory(null)).toBe(false);
    expect(hasAppHistory({})).toBe(false);
    expect(hasAppHistory({ idx: "3" })).toBe(false);
  });
});
