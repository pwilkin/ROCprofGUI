export interface WaveIndex {
  id: string;
  se: number;
  simd: number;
  slot: number;
  wid: number;
  start: number;
  end: number;
}
export interface Interval {
  start: number;
  end: number;
  se: number;
  cu: number;
  simd: number;
  slot: number;
  kernel: string;
}
export interface Instruction {
  index: number;
  text: string;
  source: string;
  address: string;
  codeObject: string;
  hits: number;
  latency: number;
  stall: number;
  idle: number;
  pcSamples: number;
  pcIssued?: number | null;
  pcStalled?: number | null;
  stallReasons?: number[] | null;
}
export interface Series {
  spmClock?: { anchorSe: number; realtimeTimestamps: string[] } | null;
  definition?: {
    id: string;
    description: string;
    block: string;
    expression: string;
    isDerived: boolean | null;
    isConstant: boolean | null;
  } | null;
  att?: {
    counter: string;
    simdMask: number;
    simds: number[];
    explicit: boolean;
  } | null;
  name: string;
  unit: string;
  scope: string;
  points: [number, number][];
}
export interface Dispatch {
  shaderEngine?: number | null;
  id: string;
  kernel: string;
  start: number;
  end: number;
  agent: string;
  queue: string;
  vgpr: number | null;
  sgpr: number | null;
  lds: number | null;
  scratch: number | null;
  grid: string;
  workgroup: string;
}
export interface DecoderEvent {
  id: string;
  se: number;
  time: number;
  kind: string;
  fields: Record<string, string>;
}
export interface OtherSimd {
  se: number;
  wgp: number | null;
  source: string;
  start: number;
  duration: number;
  category: number | null;
}
export interface CodeObjectRecord {
  id: string;
  objectId: string;
  scope: string;
  agent: string | null;
  uri: string | null;
  storage: string | null;
  loadBase: string | null;
  loadSize: string | null;
  loadDelta: string | null;
}
export interface Capture {
  pcSamplingObservations?: { columns: string[]; rows: string[][] } | null;
  seTimeOffsets?: Record<string, number>;
  codeObjects?: CodeObjectRecord[];
  otherSimd?: OtherSimd[];
  decoderEvents?: DecoderEvent[];
  realtime?: {
    frequencyHz: string | null;
    anchors: { se: number; shaderClock: string; realtimeClock: string }[];
  } | null;
  eventCounters?: EventCounter[];
  hostCalls?: {
    id: string;
    scope?: string;
    category?: string | null;
    linkBasis?: string | null;
    pid: string;
    tid: string;
    name: string;
    arguments: string | null;
    start: number;
    end: number;
    operations: string[];
  }[];
  monitorIntervals?: {
    id: string;
    deviceType: string;
    deviceId: string;
    name: string;
    start: number;
    end: number;
    value: string | null;
    unit: string;
  }[];
  id: string;
  name: string;
  format: string;
  architecture: string;
  unit: string;
  origin: string;
  start: number;
  end: number;
  warnings: string[];
  waves: WaveIndex[];
  intervals: Interval[];
  instructions: Instruction[];
  counters: Series[];
  dispatches: Dispatch[];
  sources: Record<string, string>;
  sourceFiles?: string[];
  metadata: Record<string, string>;
  markers?: MarkerScope[];
}
export interface Token {
  start: number;
  duration: number;
  stall: number;
  category: number;
  instruction: number;
}
export interface Wave {
  tokenIdentities?: { index: number; occurrence: number; displaySlot: number }[];
  viewport?: Range;
  id: string;
  start: number;
  end: number;
  cu: number | null;
  tokens: Token[];
  states: [number, number, number][];
  waits: [number, [number, number][]][] | null;
  info: Record<string, number>;
}
export interface Bundle {
  capture: Capture;
  waves: Record<string, Wave>;
}
export type Range = [number, number];
export const categories = [
  "OTHER",
  "SMEM",
  "SALU",
  "VMEM",
  "FLAT",
  "LDS",
  "VALU",
  "BRANCH",
  "NEXT",
  "IMMED",
  "TRAP",
  "MESSAGE",
  "BVH",
  "MATRIX",
];
export const colors = [
  "#87929f",
  "#b794f6",
  "#a3b886",
  "#68b6e8",
  "#7394e6",
  "#eeb669",
  "#78c4a4",
  "#c199e7",
  "#70818a",
  "#c1a1a6",
  "#8e99a5",
  "#87929f",
  "#e38d90",
  "#c199e7",
];
const standardNumberFormat = new Intl.NumberFormat("en-US", {
  maximumFractionDigits: 1,
  notation: "standard",
});
const compactNumberFormat = new Intl.NumberFormat("en-US", {
  maximumFractionDigits: 1,
  notation: "compact",
});
export const fmt = (n: number) =>
  (Math.abs(n) >= 1e6 ? compactNumberFormat : standardNumberFormat).format(n);

export interface HiddenCost {
  idle: number;
  stall: number;
  issue: number;
}
export interface HiddenToken {
  start: number;
  instruction: number;
  duration: number;
  stall: number;
  idle: number;
  hidden: HiddenCost;
}
export interface HiddenAnalysis {
  warnings?: string[];
  byInstruction: Record<string, HiddenCost>;
  byWave: Record<string, HiddenToken[]>;
  model: string;
}

export interface MarkerScope {
  id: number;
  parent: number | null;
  markerId: number;
  codeObject: string;
  name: string;
  kind: string;
  source: string;
  start: number;
  end: number | null;
  point: boolean;
  depth: number;
  se: number;
  cu: number;
  simd: number;
  slot: number;
}

export interface EventCounter {
  id: string;
  counterId: string;
  event: string | null;
  dispatches: string[];
  name: string;
  unit: string;
  scope: string;
  value: number | null;
}
