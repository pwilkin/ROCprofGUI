import { statSync } from "node:fs";
import { dirname, join } from "node:path";
import { occupancy } from "../src/analysis";
import { activityCost, hardwareSummary } from "../src/summary";
import { sourceHierarchy } from "../src/sourceHierarchy";
import { markerTree } from "../src/markerAnalysis";
import { scalarDefinition, scalarSeries } from "../src/scalarCounters";
import { diagnosticManifest } from "../src/diagnostics";
import { Tensor, tensorDefinitions, withTensorBudget } from "../src/tensor";
import type { Capture, Instruction, Range, Series, MarkerScope } from "../src/types";
import { budgets, readBoundedJson, validateManifest } from "./mcp-analysis-transport";

import { kernelAnalysis } from "./mcp-analysis-kernels";

async function main() {
  const manifest = validateManifest(readBoundedJson(process.argv[2], budgets.manifestBytes));
  const folder = dirname(process.argv[2]);
  if (manifest.mode === "kernel-stream") { process.stdout.write(JSON.stringify(await kernelAnalysis(folder, manifest))); return; }
  if (!manifest.payload) throw Error("Timeline streaming mode is not yet connected");
  const path = join(folder, manifest.payload.file);
  if (statSync(path).size !== manifest.payload.bytes) throw Error("Analysis payload byte count mismatch");
  const { capture: c, args: a } = readBoundedJson(path, budgets.projectedBytes);
  if (a.analysis !== manifest.analysis) throw Error("Analysis payload kind mismatch");
  let result: unknown;
  switch (manifest.analysis) {
    case "diagnostics": result = await diagnosticManifest({ capture: c as Capture, workspace: a.workspace || {}, sources: {}, definitions: { version: 1, selected: "", files: [] }, hiddenPending: false, wave: null, runtime: { environment: "MCP", backendVersion: "0.2.1" } }); break;
    case "occupancy": result = occupancy({ intervals: c.intervals as Capture["intervals"] }, a.se, a.kernel); break;
    case "activity": result = activityCost(c.instructions as Pick<Instruction, "idle" | "stall" | "latency">[]); break;
    case "hardware": result = hardwareSummary(c.architecture as string, c.counters as Series[]); break;
    case "hotspots": {
      const metric: "latency" | "pcSamples" | "pcIssued" | "pcStalled" = a.metric || "latency";
      result = [...c.instructions as Instruction[]].sort((x, y) => (y[metric] || 0) - (x[metric] || 0)); break;
    }
    case "source": result = sourceHierarchy(c.instructions as Instruction[], a.metric || "latency"); break;
    case "markers": result = markerTree(c.markers as MarkerScope[], a.range as Range || [c.start, c.end]); break;
    case "scalar": result = scalarSeries(scalarDefinition(a.name || "MCP counter", a.formula, c.counters as Series[]), c.counters as Series[]); break;
    case "tensor": result = withTensorBudget(8_000_000, () => {
      const raw = Object.fromEntries(Object.entries(a.raw || {}).map(([name, v]: [string, any]) => [name, new Tensor(v.shape, v.values)]));
      return tensorDefinitions(a.definitions, raw);
    }); break;
    default: throw Error("Unknown projected analysis");
  }
  process.stdout.write(JSON.stringify(result, (_k, v) => v instanceof Map ? Object.fromEntries(v) : v));
}
main().catch(error => { console.error(String(error)); process.exitCode = 1; });
