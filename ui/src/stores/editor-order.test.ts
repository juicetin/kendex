import { describe, expect, it } from "vitest";
import { unreadFold } from "./editor-order";

// The mark is per place and newest-wins, like the manifests beside it: a
// pass answers for the places it reached, and an older one cannot put back
// what a newer read of one place already settled.
describe("folding how each place's read went", () => {
  it("sets and clears per place", () => {
    const fold = unreadFold();
    expect(fold([], [["a", true]], 1)).toEqual(["a"]);
    expect(fold(["a"], [["b", true]], 2)).toEqual(["a", "b"]);
    expect(fold(["a", "b"], [["a", false]], 3)).toEqual(["b"]);
  });

  it("lets no older read answer for a place a newer one settled", () => {
    const fold = unreadFold();
    // The newer read lands first: this place is fine.
    expect(fold(["a"], [["a", false]], 5)).toEqual([]);
    // The older pass returns afterwards, still carrying its failure.
    expect(fold([], [["a", true]], 2)).toEqual([]);
    // And it still answers for a place it was the newest to reach.
    expect(fold([], [["c", true]], 2)).toEqual(["c"]);
  });

  it("hands back the same list when nothing moved", () => {
    const fold = unreadFold();
    const first = fold([], [["a", true]], 1);
    expect(fold(first, [["a", true]], 2)).toBe(first);
  });
});
