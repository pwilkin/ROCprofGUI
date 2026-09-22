import type { Instruction } from "./types";
export interface WaveStatsRow { instruction: number; hits: number; latency: number; stall: number }
export function applyWaveStats(code: Instruction[], rows: WaveStatsRow[] | null, mean = false): Instruction[] {
  if (!rows) return [];
  const byInstruction = new Map(rows.map(row => [row.instruction, row]));
  return code.map(instruction => {
    const row = byInstruction.get(instruction.index);
    const hits = row?.hits ?? 0;
    return { ...instruction, hits, latency: row ? row.latency / (mean && hits ? hits : 1) : 0, stall: row?.stall ?? 0, idle: 0 };
  });
}
