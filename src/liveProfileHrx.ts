// Request and result shaping for the hrx tracer of the live-profiling panel.
// The request object is the MCP `hrx` object verbatim, and the checks repeat the
// Rust core's (src-tauri/src/mcp.rs, hrx_profiling_flags) so a bad field is
// reported before a process is started; the core remains the authority.

type Json = Record<string, unknown>;

// Every IREE HAL profiling data family the core accepts (HRX_FAMILIES).
export const HRX_FAMILIES = [
  "queue-events",
  "host-execution",
  "device-queue-events",
  "dispatch-events",
  "memory-events",
  "device-metrics",
  "command-region-events",
  "counters",
  "counter-ranges",
  "executable-metadata",
  "executable-traces",
] as const;

// Families requested when the request names none (HRX_DEFAULT_FAMILIES). They
// are also the only ones the native AMD GPU driver implements.
export const HRX_DEFAULT_FAMILIES = ["queue-events", "dispatch-events", "executable-metadata"] as const;

// Periodic flush the core applies when the request names none (HRX_DEFAULT_FLUSH_MS).
export const HRX_DEFAULT_FLUSH_MS = 25;

// The family that asks the driver for SQTT thread traces (HRX_TRACE_FAMILY).
export const HRX_TRACE_FAMILY = "executable-traces";

// Options of `hrx.trace` with the range the core accepts (HRX_TRACE_OPTIONS).
export const HRX_TRACE_OPTIONS = [
  { name: "targetCu", low: 0, high: 15 },
  { name: "simdSelect", low: 0, high: 3 },
  { name: "shaderEngineMask", low: 1, high: 255 },
  { name: "bufferSize", low: 128 * 1024, high: 512 * 1024 * 1024 - 4096 },
  { name: "maxTraces", low: 1, high: 4096 },
] as const;

// The family that asks the driver for dispatch-scoped hardware counters
// (HRX_COUNTER_FAMILY).
export const HRX_COUNTER_FAMILY = "counters";

// Set aliases the native AMD GPU driver expands (HRX_COUNTER_SETS).
export const HRX_COUNTER_SETS = ["sqg", "instructions"] as const;

// Bound on sampled dispatches a request may ask for (HRX_COUNTER_MAX_SAMPLES).
export const HRX_COUNTER_MAX_SAMPLES = 1_000_000;

// How a hardware counter of the native AMD GPU driver may be read. An event
// count is the dispatch's own when the capture reports an exclusive sample
// window; a cycle count never is: the queue enable that isolates event counts
// does not isolate cycles (measured on gfx1151).
export type HrxCounterKind = "event" | "cycles";

export interface HrxCounterInfo {
  name: string;
  // Set alias that selects the counter, when one does.
  set?: (typeof HRX_COUNTER_SETS)[number];
  // Hardware block and the instances a value is summed over.
  block: "SQG" | "SQ";
  instances: "shader engine" | "workgroup processor";
  kind: HrxCounterKind;
  description: string;
}

// Every counter the driver accepts (HRX_COUNTER_NAMES), classified.
export const HRX_COUNTERS: readonly HrxCounterInfo[] = [
  { name: "SQG_WAVES", set: "sqg", block: "SQG", instances: "shader engine", kind: "event", description: "waves launched" },
  { name: "SQG_WAVES_32", set: "sqg", block: "SQG", instances: "shader engine", kind: "event", description: "wave32 waves launched" },
  { name: "SQG_WAVES_64", set: "sqg", block: "SQG", instances: "shader engine", kind: "event", description: "wave64 waves launched" },
  { name: "SQG_ITEMS", set: "sqg", block: "SQG", instances: "shader engine", kind: "event", description: "work items launched" },
  { name: "SQG_BUSY_CYCLES", set: "sqg", block: "SQG", instances: "shader engine", kind: "cycles", description: "cycles with a wave in the shader engine" },
  { name: "SQG_WAVE_CYCLES", block: "SQG", instances: "shader engine", kind: "cycles", description: "wave-cycles: cycles summed over live waves" },
  { name: "SQG_CYCLES", block: "SQG", instances: "shader engine", kind: "cycles", description: "clock cycles while counting" },
  { name: "SQG_WAVES_SAVED", block: "SQG", instances: "shader engine", kind: "event", description: "waves saved by a context switch" },
  { name: "SQ_WAVES", set: "instructions", block: "SQ", instances: "workgroup processor", kind: "event", description: "waves launched, per workgroup processor" },
  { name: "SQ_INSTS_VALU", set: "instructions", block: "SQ", instances: "workgroup processor", kind: "event", description: "vector ALU instructions executed" },
  { name: "SQ_INSTS_SALU", set: "instructions", block: "SQ", instances: "workgroup processor", kind: "event", description: "scalar ALU instructions executed" },
  { name: "SQ_INSTS_ALL", set: "instructions", block: "SQ", instances: "workgroup processor", kind: "event", description: "instructions executed" },
];

// Classifies a counter name of an hrx capture. Names the driver does not list
// are classified by their suffix: hardware cycle counters end in _CYCLES.
export function hrxCounterKind(name: string): HrxCounterKind {
  const known = HRX_COUNTERS.find((counter) => counter.name === name);
  if (known) return known.kind;
  return /(^|_)cycles$/i.test(name) ? "cycles" : "event";
}

// What the panel says next to the hardware-counters option.
export const HRX_COUNTER_LIMITS = [
  "Window sums, not a census: each value is a counter's increase between a read just before a dispatch and one just after it. The hardware counts every counting-enabled queue of the process in that window.",
  "The driver lets one of its queues into a sampled window at a time, so event counts (waves, work items, instructions) are the dispatch's own among that device's queues when the capture reports an exclusive window. Cycle counters (SQG_BUSY_CYCLES) are never per-dispatch: other queues raise them.",
  "Only the first dispatches matching the filter are sampled, up to the bound below; the capture states how many of its dispatches carry values. Needs the native AMD GPU driver's PM4 counter producer (gfx11.5).",
] as const;

// What the panel says about the tracer before a run: its three limits.
export const HRX_LIMITS = [
  "Records the whole run: the application writes its own IREE HAL profile from start to exit, so there is no window to trigger. Stop and collect interrupts it (SIGINT) so the bundle is finalized.",
  "Dispatch-level by default: kernel names, grids and device time. Waves and ISA need the thread-trace option below, hardware counters the counters option.",
  "On a GPU + NPU group the XDNA device may record no dispatches: its driver implements no dispatch-events family.",
] as const;

// What the panel says next to the thread-trace option: what a trace is and is not.
export const HRX_TRACE_LIMITS = [
  "A sample, not a census: SQTT traces one SIMD of one workgroup processor per shader engine, about 2% of a dispatch's waves on gfx1151.",
  "Loom dispatches through recorded command buffers, so each trace brackets one whole recorded execution of the first operations matching the filter. A full buffer truncates it; the capture states the covered window and what was dropped.",
  "Needs the native AMD GPU driver's PM4 thread-trace producer (gfx11.5). A device that does not produce the family is reported after the run.",
] as const;

export interface HrxForm {
  // "default" sends no families, so the core applies HRX_DEFAULT_FAMILIES.
  familyMode: "default" | "custom";
  // Families ticked in custom mode.
  families: string[];
  // Further family names typed in custom mode, comma or space separated.
  extraFamilies: string;
  // Blank means the core default; 0 disables the periodic flush.
  flushIntervalMs: string;
  // Blank means every function.
  filterFunction: string;
  allowPartial: boolean;
  // Adds the executable-traces family and sends the trace fields below.
  trace: boolean;
  // Thread-trace selection; blank means the driver default.
  traceTargetCu: string;
  traceSimdSelect: string;
  traceShaderEngineMask: string;
  // Per shader engine, in MiB.
  traceBufferMib: string;
  traceMaxTraces: string;
  // Adds the counters family and sends the counter fields below.
  counters: boolean;
  // "sqg" is the driver's default set; "sqg+instructions" adds the
  // per-workgroup-processor instruction counts.
  counterSet: "sqg" | "sqg+instructions";
  // Bound on sampled dispatches; blank means the driver default (4096).
  counterMaxSamples: string;
}

export const emptyHrxForm = (): HrxForm => ({
  familyMode: "default",
  families: [...HRX_DEFAULT_FAMILIES],
  extraFamilies: "",
  flushIntervalMs: "",
  filterFunction: "",
  allowPartial: false,
  trace: false,
  traceTargetCu: "",
  traceSimdSelect: "",
  traceShaderEngineMask: "",
  traceBufferMib: "",
  traceMaxTraces: "",
  counters: false,
  counterSet: "sqg",
  counterMaxSamples: "",
});

// One `hrx.trace` field: blank is absent, anything else must be an integer in
// the core's range for that option.
function traceOption(name: string, raw: string, toValue: (n: number) => number = (n) => n): number | undefined {
  const text = raw.trim();
  if (text === "") return undefined;
  const option = HRX_TRACE_OPTIONS.find((o) => o.name === name)!;
  const value = toValue(Number(text));
  if (!Number.isInteger(value) || value < option.low || value > option.high)
    throw new Error(`hrx trace ${name} must be an integer between ${option.low} and ${option.high}`);
  return value;
}

// Builds the `hrx` object of a profile_start request from the panel's fields.
// Unset fields are left out so the core's defaults apply.
export function hrxOptions(form: HrxForm): Json {
  const options: Json = {};
  if (form.familyMode === "custom") {
    const families = [...form.families, ...form.extraFamilies.split(/[\s,]+/).filter(Boolean)];
    if (form.trace && !families.includes(HRX_TRACE_FAMILY)) families.push(HRX_TRACE_FAMILY);
    if (form.counters && !families.includes(HRX_COUNTER_FAMILY)) families.push(HRX_COUNTER_FAMILY);
    if (families.length === 0) throw new Error("hrx families cannot be empty");
    const unknown = families.find((family) => !(HRX_FAMILIES as readonly string[]).includes(family));
    if (unknown !== undefined) throw new Error(`Unknown hrx data family ${unknown}`);
    if (new Set(families).size !== families.length) throw new Error("hrx families must be unique");
    options.families = families;
  } else if (form.trace || form.counters) {
    // The default set no longer applies once a family is added to it.
    options.families = [
      ...HRX_DEFAULT_FAMILIES,
      ...(form.trace ? [HRX_TRACE_FAMILY] : []),
      ...(form.counters ? [HRX_COUNTER_FAMILY] : []),
    ];
  }
  if (form.trace) {
    const bufferSize = traceOption("bufferSize", form.traceBufferMib, (mib) => mib * 1024 * 1024);
    if (bufferSize !== undefined && bufferSize % 4096 !== 0)
      throw new Error("hrx trace bufferSize must be a multiple of 4096 bytes");
    const trace: Json = {};
    const fields: [string, number | undefined][] = [
      ["targetCu", traceOption("targetCu", form.traceTargetCu)],
      ["simdSelect", traceOption("simdSelect", form.traceSimdSelect)],
      ["shaderEngineMask", traceOption("shaderEngineMask", form.traceShaderEngineMask)],
      ["bufferSize", bufferSize],
      ["maxTraces", traceOption("maxTraces", form.traceMaxTraces)],
    ];
    for (const [name, value] of fields) if (value !== undefined) trace[name] = value;
    if (Object.keys(trace).length > 0) options.trace = trace;
  }
  if (form.counters) {
    // The set is always named: with no counter name every device of a group
    // would start its own default set.
    const counters: Json = { names: form.counterSet === "sqg" ? ["sqg"] : ["sqg", "instructions"] };
    const limit = form.counterMaxSamples.trim();
    if (limit !== "") {
      const samples = Number(limit);
      if (!Number.isInteger(samples) || samples < 1 || samples > HRX_COUNTER_MAX_SAMPLES)
        throw new Error(`hrx counters maxSamples must be an integer between 1 and ${HRX_COUNTER_MAX_SAMPLES}`);
      counters.maxSamples = samples;
    }
    options.counters = counters;
  }
  const flush = form.flushIntervalMs.trim();
  if (flush !== "") {
    const ms = Number(flush);
    if (!Number.isInteger(ms) || ms < 0 || ms > 60000)
      throw new Error("hrx flushIntervalMs must be an integer between 0 and 60000");
    options.flushIntervalMs = ms;
  }
  const filter = form.filterFunction.trim();
  if (filter !== "") {
    if (filter.length > 256 || filter.includes("\0"))
      throw new Error("hrx filterFunction must be a nonempty glob of at most 256 characters");
    options.filterFunction = filter;
  }
  if (form.allowPartial) options.allowPartial = true;
  return options;
}

const count = (value: unknown) => (typeof value === "number" ? value.toLocaleString("en-US") : "?");

// One line for a live_profile_status answer of an hrx run.
export function hrxStatusLine(status: Json): string {
  const bytes = `bundle ${count(status.bundleBytes)} bytes so far`;
  if (status.running === false) {
    const code = typeof status.exitCode === "number" ? `code ${status.exitCode}` : "no exit code";
    return `The application has exited (${code}) · ${bytes} · stop and collect to read it`;
  }
  return `Recording the whole run · ${bytes}`;
}

// Whether a live_profile_stop answer comes from the hrx tracer: only it carries
// a `bundle` field (null when no readable bundle was written).
export const isHrxResult = (result: Json): boolean => "bundle" in result;

// The lines for a bundle that holds counter samples: what was counted, how the
// values may be read, and what the producer said about the sample window.
function hrxCounterLines(bundle: Json): string[] {
  const names = Array.isArray(bundle.counterNames) ? (bundle.counterNames as string[]) : [];
  const cycles = names.filter((name) => hrxCounterKind(name) === "cycles");
  const lines = [
    `Hardware counters: ${count(bundle.counterSamples)} sample(s) of ${names.length > 0 ? names.join(", ") : "counters the bundle does not name"}. Values are window sums over the sampled dispatches, not a census${cycles.length > 0 ? `; ${cycles.join(", ")} ${cycles.length > 1 ? "are" : "is"} never per-dispatch` : ""}.`,
  ];
  const capture = bundle.counterCapture as Json | null | undefined;
  if (!capture) {
    lines.push(
      "Counter sample window: unknown. The bundle has no capture record, so it cannot tell whether other queues of the process overlapped a window; read values as upper bounds.",
    );
    return lines;
  }
  const foreign = typeof capture.foreignState === "number" ? capture.foreignState : 0;
  lines.push(
    foreign > 0
      ? `Counter sample window: ANOTHER COLLECTOR reprogrammed the counters during ${count(foreign)} sampled operation(s); do not read the values as per-dispatch counts.`
      : capture.exclusiveWindow
        ? "Counter sample window: exclusive among the producing device's queues, per the driver. Event counts are each sampled dispatch's own there; queues of the process outside that device session are not visible to the capture."
        : "Counter sample window: exclusivity not reported, so values may include other queues of the process; read them as upper bounds.",
  );
  lines.push(
    `Sampled dispatches: ${count(capture.sampledDispatches)} in ${count(capture.sampledOperations)} operation(s), bound ${count(capture.sampleLimit)}${capture.limitReached ? " (reached)" : ""} · ${count(capture.skippedOperations)} matching operation(s) ran unsampled · ${count(capture.droppedSamples)} sample(s) dropped · ${count(capture.invalidSamples)} without hardware values`,
  );
  return lines;
}

// The lines the panel shows for a collected hrx run: what the bundle holds, per
// device session, and what it cannot hold. A missing or unfinalized bundle is
// stated, never rendered as an empty success.
export function hrxBundleLines(result: Json): string[] {
  const files = Array.isArray(result.files) ? (result.files as unknown[]) : [];
  const bundle = result.bundle as Json | null | undefined;
  const lines: string[] = [];
  if (!bundle) {
    lines.push(
      files.length > 0
        ? `Profile bundle ${String(files[0])} could not be read.`
        : "No profile bundle was written.",
    );
    return lines;
  }
  const path = files.length > 0 ? String(files[0]) : "(path not reported)";
  lines.push(
    `Profile bundle: ${path} · ${count(bundle.bytes)} bytes · ${bundle.finalized ? "finalized" : "NOT finalized (cannot be imported)"}`,
  );
  const producers = Array.isArray(bundle.producers) ? (bundle.producers as string[]) : [];
  lines.push(
    `Device sessions: ${count(bundle.sessions)}${producers.length > 0 ? ` (${producers.join(", ")})` : ""}`,
  );
  lines.push(
    `Dispatch events: ${count(bundle.dispatchEvents)} · queue events: ${count(bundle.queueEvents)} · functions: ${count(bundle.functions)}`,
  );
  const perProducer = Array.isArray(bundle.perProducer) ? (bundle.perProducer as Json[]) : [];
  if (perProducer.length > 1)
    for (const entry of perProducer) {
      const dispatches =
        entry.dispatchEvents === 0 ? "no dispatch events" : `${count(entry.dispatchEvents)} dispatch events`;
      lines.push(`${String(entry.producer) || "(unnamed)"}: ${dispatches}, ${count(entry.queueEvents)} queue events`);
    }
  const truncated = Array.isArray(bundle.truncated) ? (bundle.truncated as Json[]) : [];
  for (const family of truncated)
    lines.push(
      family.family === HRX_TRACE_FAMILY
        ? `TRUNCATED thread trace: a buffer filled, tokens were dropped (${count(family.dropped)} counted) or the stop did not finish`
        : `TRUNCATED ${String(family.family)}: ${count(family.dropped)} record(s) dropped by the producer`,
    );
  const traced = typeof bundle.executableTraces === "number" && bundle.executableTraces > 0;
  const counted = typeof bundle.counterSamples === "number" && bundle.counterSamples > 0;
  const trace = `Thread trace: ${count(bundle.executableTraces)} partition(s), ${count(bundle.executableTraceBytes)} bytes of SQTT. Open the capture to decode waves and ISA rows: a sample of one SIMD of one workgroup processor per shader engine, not a census.`;
  if (traced) lines.push(counted ? trace : `${trace} Hardware counters: none.`);
  else if (counted) lines.push("Waves and ISA: none (no thread trace in the bundle).");
  else lines.push("Waves, ISA and hardware counters: none (the bundle is dispatch-level).");
  if (counted) lines.push(...hrxCounterLines(bundle));
  return lines;
}
