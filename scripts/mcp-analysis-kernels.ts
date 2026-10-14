import { open } from "node:fs/promises";
import { join } from "node:path";
import { createKernelStatsAccumulator } from "../src/analysis";
import protocol from "../src/mcpAnalysisProtocol.json";
import {
  budgets,
  kernelSample,
  readRecords,
  type Manifest,
} from "./mcp-analysis-transport";
export function boundedKernelAccumulator(
  limits = {
    groups: budgets.kernelGroups,
    names: budgets.kernelNameBytes,
    pairs: budgets.kernelIntervals,
  },
) {
  const stats = {
    groups: 0,
    nameBytes: 0,
    pairs: 0,
    retainedDispatchRecords: 0,
  };
  const refusal = (bound: string) =>
    Error(
      `MCP_KERNEL_STATE_BUDGET: exact full-capture aggregation exceeds ${bound} budget. limit/query do not reduce aggregate input. Use capture_query on dispatches and intervals with positive limit and nextOffset for offline exact aggregation, or import a smaller capture. For activeTime, union intervals globally; do not sum per-part unions. No partial totals were returned.`,
    );
  const accumulator = createKernelStatsAccumulator({
    beforeKernel(name, next) {
      const bytes = Buffer.byteLength(name);
      if (next > limits.groups) throw refusal(`${limits.groups} groups`);
      if (stats.nameBytes + bytes > limits.names)
        throw refusal(`${limits.names} kernel-name bytes`);
      stats.groups = next;
      stats.nameBytes += bytes;
    },
    beforeInterval(next) {
      if (next > limits.pairs) throw refusal(`${limits.pairs} intervals`);
      stats.pairs = next;
    },
  });
  return { accumulator, stats };
}
export async function kernelAnalysis(folder: string, manifest: Manifest) {
  const { accumulator } = boundedKernelAccumulator();
  for (const input of readRecords(folder, manifest.streams![0]))
    accumulator.dispatch(kernelSample(input));
  for (const input of readRecords(folder, manifest.streams![1]))
    accumulator.interval(kernelSample(input));
  const rows = accumulator.finish();
  const file = await open(join(folder, protocol.files.kernelResult), "wx");
  let bytes = 0;
  try {
    for (const row of rows) {
      const line = JSON.stringify(row) + "\n",
        size = Buffer.byteLength(line);
      if (
        size > budgets.recordBytes ||
        bytes + size > budgets.kernelResultBytes
      )
        throw Error(
          `MCP_KERNEL_STATE_BUDGET: aggregate result exceeds ${budgets.kernelResultBytes} bytes; use offline paged dispatch/interval aggregation. No partial totals were returned.`,
        );
      await file.writeFile(line, "utf8");
      bytes += size;
    }
  } finally {
    await file.close();
  }
  return {
    protocol: protocol.protocol,
    version: protocol.version,
    kind: "kernel-rows",
    file: protocol.files.kernelResult,
    rows: rows.length,
    bytes,
  };
}
