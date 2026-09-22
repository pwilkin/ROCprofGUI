import { describe, expect, it } from "vitest";
import {
  dockLeaves,
  reconcileDockTree,
  placeDockPanel,
  updateDockSplit,
  removeDockPanel,
} from "./dockTree";
describe("nested dock trees", () => {
  it("nests relative placement, edits splits and collapses removed branches", () => {
    let tree = reconcileDockTree(undefined, ["a", "b", "c"])!;
    tree = placeDockPanel(tree, "b", "a", "right");
    tree = placeDockPanel(tree, "c", "b", "below");
    expect(tree).toEqual({
      kind: "split",
      axis: "horizontal",
      ratio: 50,
      first: { kind: "panel", view: "a" },
      second: {
        kind: "split",
        axis: "vertical",
        ratio: 50,
        first: { kind: "panel", view: "b" },
        second: { kind: "panel", view: "c" },
      },
    });
    expect(updateDockSplit(tree, "1", { ratio: 70 })).toMatchObject({
      second: { ratio: 70 },
    });
    expect(dockLeaves(removeDockPanel(tree, "b"))).toEqual(["a", "c"]);
    expect(reconcileDockTree(tree, [])).toBeUndefined();
  });
  it("preserves one leaf per pinned panel through repeated moves and pin changes", () => {
    const panels = Array.from({ length: 16 }, (_, i) => String(i));
    let tree = reconcileDockTree(undefined, panels)!;
    for (let i = 0; i < 200; i++) {
      tree = placeDockPanel(
        tree,
        panels[i % 16],
        panels[(i * 7 + 3) % 16],
        ["left", "right", "above", "below"][i % 4],
      );
      expect(dockLeaves(tree).sort()).toEqual([...panels].sort());
    }
    expect(
      dockLeaves(reconcileDockTree(tree, ["3", "5", "new"])).sort(),
    ).toEqual(["3", "5", "new"]);
  });
});
