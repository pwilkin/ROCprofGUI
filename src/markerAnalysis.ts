import type { MarkerScope, Range } from "./types";
export interface MarkerNode {
  key: string;
  name: string;
  duration: number;
  count: number;
  open: number;
  points: number;
  ids: number[];
  children: MarkerNode[];
}
export function markerTree(markers: MarkerScope[], range: Range): MarkerNode[] {
  const byId = new Map(markers.map((m) => [m.id, m])),
    roots: MarkerNode[] = [];
  const ordered = [...markers].sort(
    (a, b) => a.depth - b.depth || a.start - b.start,
  );
  const paths = new Map<number, MarkerNode>();
  for (const marker of ordered) {
    if (
      marker.start > range[1] ||
      (marker.end !== null && marker.end < range[0])
    )
      continue;
    const parent =
      marker.parent === null ? undefined : paths.get(marker.parent);
    const list = parent?.children || roots;
    const key = `${marker.codeObject}:${marker.markerId}:${marker.kind}`;
    let node = list.find((n) => n.key === key);
    if (!node) {
      node = {
        key,
        name: marker.name,
        duration: 0,
        count: 0,
        open: 0,
        points: 0,
        ids: [],
        children: [],
      };
      list.push(node);
    }
    node.count++;
    node.ids.push(marker.id);
    paths.set(marker.id, node);
    if (marker.point) node.points++;
    else if (marker.end === null) node.open++;
    else
      node.duration += Math.max(
        0,
        Math.min(range[1], marker.end) - Math.max(range[0], marker.start),
      );
    if (marker.parent !== null && !byId.has(marker.parent))
      node.name = marker.name + " (parent outside selection)";
  }
  return roots;
}
