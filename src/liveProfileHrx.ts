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

// What the panel says about the tracer before a run: its three limits.
export const HRX_LIMITS = [
  "Records the whole run: the application writes its own IREE HAL profile from start to exit, so there is no window to trigger. Stop and collect interrupts it (SIGINT) so the bundle is finalized.",
  "Dispatch-level only: kernel names, grids and device time. No waves, no ISA, no hardware counters.",
  "On a GPU + NPU group the XDNA device may record no dispatches: its driver implements no dispatch-events family.",
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
}

export const emptyHrxForm = (): HrxForm => ({
  familyMode: "default",
  families: [...HRX_DEFAULT_FAMILIES],
  extraFamilies: "",
  flushIntervalMs: "",
  filterFunction: "",
  allowPartial: false,
});

// Builds the `hrx` object of a profile_start request from the panel's fields.
// Unset fields are left out so the core's defaults apply.
export function hrxOptions(form: HrxForm): Json {
  const options: Json = {};
  if (form.familyMode === "custom") {
    const families = [...form.families, ...form.extraFamilies.split(/[\s,]+/).filter(Boolean)];
    if (families.length === 0) throw new Error("hrx families cannot be empty");
    const unknown = families.find((family) => !(HRX_FAMILIES as readonly string[]).includes(family));
    if (unknown !== undefined) throw new Error(`Unknown hrx data family ${unknown}`);
    if (new Set(families).size !== families.length) throw new Error("hrx families must be unique");
    options.families = families;
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
    lines.push(`TRUNCATED ${String(family.family)}: ${count(family.dropped)} record(s) dropped by the producer`);
  lines.push("Waves, ISA and hardware counters: none (the bundle is dispatch-level).");
  return lines;
}
