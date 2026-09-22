export type DockTree =
  | { kind: "panel"; view: string }
  | {
      kind: "split";
      axis: "horizontal" | "vertical";
      ratio: number;
      first: DockTree;
      second: DockTree;
    };
export const dockLeaves = (tree?: DockTree): string[] =>
  !tree
    ? []
    : tree.kind === "panel"
      ? [tree.view]
      : [...dockLeaves(tree.first), ...dockLeaves(tree.second)];
export function removeDockPanel(
  tree: DockTree | undefined,
  view: string,
): DockTree | undefined {
  if (!tree) return;
  if (tree.kind === "panel") return tree.view === view ? undefined : tree;
  const first = removeDockPanel(tree.first, view),
    second = removeDockPanel(tree.second, view);
  return first && second ? { ...tree, first, second } : first || second;
}
export function reconcileDockTree(
  tree: DockTree | undefined,
  pinned: string[],
): DockTree | undefined {
  for (const view of dockLeaves(tree))
    if (!pinned.includes(view)) tree = removeDockPanel(tree, view);
  for (const view of pinned)
    if (!dockLeaves(tree).includes(view)) {
      const panel: DockTree = { kind: "panel", view };
      tree = tree
        ? {
            kind: "split",
            axis: "vertical",
            ratio: 50,
            first: tree,
            second: panel,
          }
        : panel;
    }
  return tree;
}
export function placeDockPanel(
  tree: DockTree,
  source: string,
  target: string,
  position: string,
): DockTree {
  if (
    source === target ||
    !dockLeaves(tree).includes(source) ||
    !dockLeaves(tree).includes(target)
  )
    return tree;
  const pruned = removeDockPanel(tree, source)!;
  const visit = (node: DockTree): DockTree => {
    if (node.kind === "split")
      return { ...node, first: visit(node.first), second: visit(node.second) };
    if (node.view !== target) return node;
    const panel: DockTree = { kind: "panel", view: source };
    const before = position === "left" || position === "above";
    return {
      kind: "split",
      axis:
        position === "left" || position === "right" ? "horizontal" : "vertical",
      ratio: 50,
      first: before ? panel : node,
      second: before ? node : panel,
    };
  };
  return visit(pruned);
}
export function updateDockSplit(
  tree: DockTree,
  path: string,
  update: { axis?: "horizontal" | "vertical"; ratio?: number },
): DockTree {
  if (tree.kind === "panel") return tree;
  if (!path) return { ...tree, ...update };
  const branch = path[0] === "0" ? "first" : "second";
  return {
    ...tree,
    [branch]: updateDockSplit(tree[branch], path.slice(1), update),
  };
}
export const dockHeight = (tree: DockTree): number =>
  tree.kind === "panel"
    ? 460
    : 38 +
      (tree.axis === "vertical"
        ? dockHeight(tree.first) + dockHeight(tree.second)
        : Math.max(dockHeight(tree.first), dockHeight(tree.second)));
