import { waveQuery } from "./waveQuery";
import { loadWaveStatesPreference } from "./importPreferences";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { Bundle, Capture, Wave } from "./types";
import { requestCatalog, requestCatalogCommand } from "./catalogTransfer";
import { requestProject, acknowledgeProject } from "./projectTransfer";
export const desktop = isTauri();
let catalogImport: AbortController | null = null;
export async function attachSpm(
  captureId: string,
  se: number,
): Promise<Capture | null> {
  if (catalogImport) throw Error("An import is already running");
  const controller = new AbortController();
  catalogImport = controller;
  try {
    return await requestCatalogCommand(
      "attach_spm",
      { captureId, se },
      crypto.randomUUID(),
      invoke,
      controller.signal,
    );
  } finally {
    if (catalogImport === controller) catalogImport = null;
  }
}
export interface CodeObjectExtraction {
  recordedUri: string;
  remapped: boolean;
  path: string;
  source: string;
  offset: string;
  bytes: string;
  sha256: string;
}
export const extractCodeObject = (
  captureId: string,
  objectId: string,
  chooseSource = false,
  sourcePath?: string,
) =>
  invoke<CodeObjectExtraction | null>("extract_code_object", {
    captureId,
    objectId,
    ...(chooseSource ? { chooseSource: true } : {}),
    ...(sourcePath ? { sourcePath } : {}),
  });
let demo: Bundle;
export async function example(): Promise<Capture> {
  demo = await (await fetch("/demo.json")).json();
  return { ...demo.capture, id: "demo" };
}
export interface AttRun {
  prefix: string;
  run: number;
  shaderEngines: number[];
  bytes: number;
  path: string;
}
export interface RunChoice {
  runs: AttRun[];
}
export async function openCapturePath(
  path: string,
  samplingMethod?: string,
): Promise<Capture> {
  if (catalogImport) throw Error("An import is already running");
  const controller = new AbortController();
  catalogImport = controller;
  try {
    return await requestCatalog(
      path,
      crypto.randomUUID(),
      invoke,
      controller.signal,
      loadWaveStatesPreference(),
      samplingMethod,
    );
  } finally {
    if (catalogImport === controller) catalogImport = null;
  }
}
// The third copy of the ATT filename grammar (with att_runs.rs's SE_MARKER /
// split_stem and decode.py's regex — keep the three in step). A bare single
// `.att` file that is NOT a decoded `<prefix>_shader_engine_<SE>_<RUN>.att` run
// member needs the raw-ATT decoder setup; a shader-engine run member imports
// directly. The two trailing fields are all-ASCII-digit, `_`-separated.
export function isRawAttSetupPath(path: string): boolean {
  return /\.att$/i.test(path) && !/_shader_engine_\d+_\d+\.att$/i.test(path);
}
export async function openCapture(
  directory: boolean,
): Promise<
  Capture | RunChoice | { samplingChoice: string } | { attSetup: string } | null
> {
  if (!desktop)
    throw Error(
      "Directory and raw trace import run in the Rust desktop app. Use the example capture here, or launch npm run tauri dev.",
    );
  const path = await open({
    directory,
    multiple: false,
    ...(directory
      ? {}
      : {
          filters: [
            {
              // Loom compile reports arrive as .json (e.g. *.compile-report.json,
              // *.summary.json, *.details.json) and are recognised by content in
              // the Rust import dispatcher, so the "json" entry already admits them.
              name: "ROCm and RADV captures",
              extensions: [
                "json",
                "csv",
                "att",
                "rocprof-att",
                "rocprof-capture",
                "rocpd",
                "rpd",
                "db",
                "sqlite",
                "rgp",
                "irpf",
              ],
            },
          ],
        }),
  });
  if (typeof path !== "string") return null;
  if (!directory && isRawAttSetupPath(path)) return { attSetup: path };
  if (directory) {
    const runs = await invoke<AttRun[]>("discover_att_runs", { path });
    if (runs.length > 1) return { runs };
    if (runs.length === 1) return openCapturePath(runs[0].path);
  }
  try {
    return await openCapturePath(path);
  } catch (error) {
    if (
      String(error).includes(
        "Mixed host-trap and stochastic sampling methods require an explicit method selection",
      )
    ) {
      return { samplingChoice: path };
    }
    throw error;
  }
}
export async function readWave(capture: Capture, id: string, signal?: AbortSignal): Promise<Wave> {
  return capture.id === "demo"
    ? demo.waves[id]
    : waveQuery("read_wave", { captureId: capture.id, waveId: id }, signal);
}
export async function closeCapture(id: string) {
  if (id !== "demo" && desktop)
    await invoke("close_capture", { captureId: id });
}

export async function readWaveWindow(
  capture: Capture,
  se: number,
  start: number,
  end: number,
  signal?: AbortSignal,
): Promise<Wave[]> {
  if (capture.id !== "demo")
    return waveQuery("read_wave_window", {
      captureId: capture.id,
      se,
      start,
      end,
    }, signal);
  return capture.waves
    .filter((w) => w.se === se && w.start < end && w.end > start)
    .map((w) => demo.waves[w.id]);
}

export async function analyzeHidden(
  capture: Capture,
  signal?: AbortSignal,
): Promise<import("./types").HiddenAnalysis> {
  if (capture.id === "demo") return (await fetch("/demo-hidden.json")).json();
  return waveQuery("analyze_hidden", { captureId: capture.id }, signal);
}

export async function cancelImport() {
  catalogImport?.abort();
  if (desktop) await invoke("cancel_import");
}

export interface SavedProjectEntry {
  captureId: string;
  state: import("./session").TabState;
  sources: import("./sourceFiles").SourceMappings;
}
export interface OpenedProject {
  pendingTransfer?: string;
  captures: Capture[];
  project: {
    schemaVersion: number;
    definitionLibrary?: import("./definitionLibrary").DefinitionLibrary | null;
    active: number;
    entries: {
      input: string | null;
      fingerprint?: {
        files: Record<string, { size: number; sha256: string }>;
      } | null;
      state: import("./session").TabState;
      sources: import("./sourceFiles").SourceMappings;
    }[];
  };
}
export const saveProject = (
  entries: SavedProjectEntry[],
  active: number,
  definitionLibrary: import("./definitionLibrary").DefinitionLibrary,
) => invoke<boolean>("save_project", { entries, active, definitionLibrary });
export async function openProject(): Promise<OpenedProject | null> {
  if (catalogImport) throw Error("An import is already running");
  const controller = new AbortController();
  catalogImport = controller;
  try {
    return await requestProject(crypto.randomUUID(), invoke, controller.signal);
  } finally {
    if (catalogImport === controller) catalogImport = null;
  }
}
export const acknowledgeOpenedProject = (opened: OpenedProject) =>
  acknowledgeProject(opened.pendingTransfer, invoke);

export const diagnosticContext = (captureId: string): Promise<unknown> =>
  desktop
    ? invoke("diagnostic_context", { captureId })
    : Promise.resolve({
        environment: "browser preview",
        inputPath: null,
        inputHash: null,
        decoder: { status: "not available in browser preview" },
      });
export async function saveDiagnostics(bytes: Uint8Array) {
  if (desktop)
    return invoke<boolean>("export_diagnostics", { bytes: Array.from(bytes) });
  const url = URL.createObjectURL(
    new Blob([bytes], { type: "application/json" }),
  );
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = "rocprof-diagnostics.json";
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  return true;
}

export const exportCaptureArchive = (captureId: string) =>
  invoke<boolean>("export_capture_archive", { captureId });

// Live profiling: thin wrappers over the live_profile_* desktop commands, which
// share the same core as the MCP profile_* tools. Each takes the same argument
// object the MCP tool would, passed through verbatim.
export type LiveProfileResult = Record<string, unknown>;
export const liveProfileStart = (args: Record<string, unknown>) =>
  invoke<LiveProfileResult>("live_profile_start", { args });
export const liveProfileTrigger = (args: Record<string, unknown>) =>
  invoke<LiveProfileResult>("live_profile_trigger", { args });
export const liveProfileStatus = (id: string) =>
  invoke<LiveProfileResult>("live_profile_status", { args: { id } });
export const liveProfileStop = (id: string) =>
  invoke<LiveProfileResult>("live_profile_stop", { args: { id } });

export async function readWaveStats(
  capture: Capture,
  se: number,
  start: number,
  end: number,
  signal?: AbortSignal,
): Promise<import("./waveStats").WaveStatsRow[]> {
  if (capture.id !== "demo")
    return waveQuery("read_wave_stats", { captureId: capture.id, se, start, end }, signal);
  const rows = new Map<number, import("./waveStats").WaveStatsRow>();
  for (const wave of await readWaveWindow(capture, se, start, end)) {
    for (const token of wave.tokens) {
      if (token.start < start || token.start >= end) continue;
      const row = rows.get(token.instruction) ?? {
        instruction: token.instruction,
        hits: 0,
        latency: 0,
        stall: 0,
      };
      row.hits++;
      row.latency += token.duration;
      row.stall += token.stall;
      rows.set(token.instruction, row);
    }
  }
  return [...rows.values()];
}
