import { waveQuery } from "./waveQuery";
import { categories, colors, type Capture, type Range, type WaveIndex } from "./types";
import type { Mark } from "./components/Plots";

export interface WaveOverview {
  range: Range;
  utilization?: boolean;
  representedTokens: number;
  waves: { wave: WaveIndex; cu: number | null; groups: { category: number; pipe?: string | null; shared?: boolean; start: number; end: number; count: number; zoomRange: Range }[] }[];
}
export function readWaveOverview(capture: Capture, se: number, start: number, end: number, utilization = false, separateLds = false, signal?: AbortSignal): Promise<WaveOverview> {
  return waveQuery("read_wave_overview", { captureId: capture.id, se, start, end, utilization, separateLds }, signal);
}
// Enforced in crates/profile-core/src/wave_index.rs; a change there must be mirrored here.
const MAX_WINDOW_WAVES = 128;
const MAX_VIEWPORT_TOKENS = 500_000;
export function isWaveDetailBudgetError(error: unknown): boolean {
  const message = String(error), waves = MAX_WINDOW_WAVES, tokens = MAX_VIEWPORT_TOKENS.toLocaleString("en-US");
  return message.includes(`Visible wave query exceeds ${tokens} tokens`) ||
    message.includes(`more than ${waves} waves`) ||
    message.includes(`more than ${waves} detailed waves`) ||
    message.includes(`Wave query exceeds ${waves} waves`);
}
export function overviewDescription(overview: WaveOverview): string {
  if (overview.utilization) return `Utilization overview: ${overview.representedTokens.toLocaleString()} projected issue events grouped by wave, pipe and start-time bucket. Group spans may include gaps; they are not measured utilization percentages. Click a group to zoom to exact events.`;
  return `CU overview: ${overview.representedTokens.toLocaleString()} recorded events grouped by wave, category and start-time bucket. Group spans may include gaps; they are not occupancy measurements. Click a group to zoom to exact events.`;
}
export function overviewMarks(overview: WaveOverview): Mark[] {
  return overview.waves.flatMap(({ wave, cu, groups }) => groups.map((group, n) => ({
    key: `overview:${wave.id}:${n}`,
    start: group.start, end: group.end,
    lane: `${cu === null ? `CU unknown [${wave.id}]` : `CU ${cu}`} · ${overview.utilization ? `${group.shared ? "Shared" : `S${wave.simd}`} · ${group.pipe}` : `S${wave.simd} slot${wave.slot}`}`,
    shaderEngine: wave.se,
    color: colors[group.category] || colors[0],
    label: `${group.count} ${categories[group.category] || "OTHER"} events · SE ${wave.se} · wave ${wave.wid} · overview; zoom to inspect`,
    aggregateCount: group.count,
    zoomRange: group.zoomRange,
  })));
}
