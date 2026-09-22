import type { Instruction, Range } from "./types";
import { clipWeightedInterval } from "./weightedViewport";

export interface SourceFrame {
  key: string;
  label: string;
  total: number;
  depth: number;
  start: number;
  end: number;
  instruction?: Instruction;
}
interface Node {
  key: string;
  label: string;
  total: number;
  children: Map<string, Node>;
  instruction?: Instruction;
}
export function sourceHierarchy(
  instructions: Instruction[],
  metric: "latency" | "pcSamples" | "pcIssued" | "pcStalled",
): SourceFrame[] {
  return weightedSourceHierarchy(
    instructions.map((instruction) => ({
      instruction,
      weight: instruction[metric] ?? 0,
      prefix: [],
    })),
  );
}
export interface SourceObservation {
  instruction: Instruction;
  weight: number;
  prefix: { key: string; label: string }[];
}
export function weightedSourceHierarchy(
  observations: SourceObservation[],
  maxFrames = Infinity,
): SourceFrame[] {
  const root: Node = { key: "", label: "", total: 0, children: new Map() };
  let nextKey = 0;
  for (const { instruction, weight, prefix } of observations) {
    if (!(weight > 0)) continue;
    const locations = instruction.source
      .split(" -> ")
      .filter(Boolean)
      .reverse();
    const file = locations[0]?.match(/^(.*):[0-9]+$/)?.[1] || "Unmapped ISA";
    const labels = [
      ...prefix.map((p) => p.label),
      file,
      ...locations,
      instruction.text,
    ];
    let parent = root;
    labels.forEach((label, depth) => {
      const leaf = depth === labels.length - 1;
      const identity = JSON.stringify(
        depth < prefix.length
          ? ["marker", prefix[depth].key]
          : [leaf, leaf ? [instruction.codeObject, instruction.index] : label],
      );
      let child = parent.children.get(identity);
      if (!child) {
        if (nextKey >= maxFrames)
          throw Error(
            `More than ${maxFrames.toLocaleString()} hierarchy frames; narrow the scope`,
          );
        child = {
          key: String(nextKey++),
          label,
          total: 0,
          children: new Map(),
          instruction: leaf ? instruction : undefined,
        };
        parent.children.set(identity, child);
      }
      child.total += weight;
      parent = child;
    });
  }
  const frames: SourceFrame[] = [];
  const pending = [{ node: root, depth: -1, start: 0 }];
  while (pending.length) {
    const { node, depth, start } = pending.pop()!;
    if (depth >= 0)
      frames.push({
        key: node.key,
        label: node.label,
        total: node.total,
        depth,
        start,
        end: start + node.total,
        instruction: node.instruction,
      });
    let offset = start;
    const children = [...node.children.values()]
      .sort((a, b) => b.total - a.total || a.label.localeCompare(b.label))
      .map((child) => {
        const entry = { node: child, depth: depth + 1, start: offset };
        offset += child.total;
        return entry;
      });
    for (let index = children.length - 1; index >= 0; index--)
      pending.push(children[index]);
  }
  return frames;
}
export function clipSourceHierarchy(frames: SourceFrame[], range: Range) {
  return frames.flatMap((frame) => {
    const visible = clipWeightedInterval(frame.start, frame.end, range);
    return visible ? [{ ...frame, ...visible }] : [];
  });
}
