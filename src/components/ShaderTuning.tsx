import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Play, Square, Search, Save, Code2 } from "lucide-react";

type Config = {
  sourceTree: string;
  cwd: string;
  argv: string[];
  env: Record<string, string>;
  occurrence: number;
  groups?: number[];
  name?: string;
  buildDir?: string;
  layer: string;
  replay: string;
};
type Metric = {
  value: number | null;
  unit: string;
  reason?: string | null;
  delta?: number;
  deltaPercent?: number | null;
};
type Conditions = {
  mode: "stateless" | "sustained";
  memory: "device" | "app" | "gtt" | "auto";
  flushMb: number;
  repeats: number;
};
type Run = {
  number: number;
  variant: boolean;
  iterations: number;
  metrics: Record<string, Metric>;
  conditions?: Conditions;
  counters?: boolean;
  driver?: { version: string; deviceType?: string; device?: string; memory?: string };
  comparedWith?: number | null;
  validation?: {
    status: string;
    mismatchedRegions?: string[];
    groups?: { group: number; bytes: number; mismatched: number }[];
    sustainedMatch?: boolean;
  };
  notes?: string[];
  trace?: string | { error: string } | null;
  edits?: string[];
  directory: string;
};
type Dispatch = {
  name: string;
  labels: string[];
  groups: number[];
  device?: { name: string };
  pushHex: string;
  specValues: { id: number; value: number; hex: string }[];
  bindings: { set: number; binding: number; group: number; offset: number; range: number }[];
  buffers: { group: number; bytes: number; writtenBytes: number }[];
  unknownFeatures: number[];
};
type Session = {
  mode: "vulkan";
  phase?: string | null;
  kernel: string;
  directory: string;
  traceDispatch: { id: string; grid: string; workgroup: string };
  config?: Config;
  captured?: string;
  dispatch?: Dispatch;
  candidates?: { variant: string; sources: string[]; runtimePatch: { added: unknown[]; removed: unknown[] } }[];
  recipe?: { variant: string; source: string; glslc: string; argv: string[]; runtimePatch: { added: unknown[]; removed: unknown[] } };
  files?: { path: string; hash: string; code: string }[];
  isa?: string;
  lastSave?: { path: string; backup: string };
  runs: Run[];
};
type Status = { session: Session; running: boolean; error: string | null; log: string };

const number = (v: number) =>
  new Intl.NumberFormat("en-US", { maximumSignificantDigits: 6 }).format(v);
const metricOrder = [
  "GPU duration",
  "Launch throughput",
  "Replay spread",
  "GPU cycles",
  "Clock",
  "L0 hit rate",
  "L1 hit rate",
  "L2 hit rate",
  "VGPRs",
  "SGPRs",
  "Max waves per SIMD",
  "LDS",
  "Scratch",
  "Spilled VGPRs",
  "Instructions",
  "WMMA instructions",
  "Compute throughput",
  "Memory throughput",
];
const metricRank = (name: string) =>
  metricOrder.includes(name) ? metricOrder.indexOf(name) : metricOrder.length;
const fileName = (path: string) => path.split("/").pop() || path;
const BEST_CASE: Conditions = { mode: "stateless", memory: "device", flushMb: 0, repeats: 1 };
const APPLICATION_LIKE: Conditions = { mode: "sustained", memory: "auto", flushMb: 0, repeats: 3 };
const memoryNames: Record<string, string> = {
  device: "device-local memory",
  app: "the memory type ggml uses on unified-memory GPUs",
  gtt: "system memory (GTT)",
  auto: "system memory on integrated GPUs, device-local otherwise",
};
/** One line saying how a run's replay was made, for the run header. */
function describeConditions(run: Run) {
  const c = run.conditions || BEST_CASE,
    parts = [
      c.mode === "sustained" ? "back to back after one restore" : "inputs restored before every dispatch",
      `buffers in ${memoryNames[run.driver?.memory || c.memory] || c.memory}`,
    ];
  if (c.flushMb) parts.push(`${c.flushMb} MiB cache flush before each dispatch`);
  if (c.repeats > 1) parts.push(`${c.repeats} replays with fresh allocations`);
  return parts.join(" · ");
}
const shortConditions = (run: Run) => {
  const c = run.conditions || BEST_CASE;
  if (JSON.stringify(c) === JSON.stringify(BEST_CASE)) return "best case";
  return [c.mode, c.memory, c.repeats > 1 ? `×${c.repeats}` : ""].filter(Boolean).join(" ");
};

export function ShaderTuning({ id }: { id: string }) {
  const [status, setStatus] = useState<Status | null>(null),
    [config, setConfig] = useState<Config | null>(null),
    [argv, setArgv] = useState("[]"),
    [env, setEnv] = useState("{}"),
    [edits, setEdits] = useState<Record<string, string>>({}),
    [file, setFile] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [iterations, setIterations] = useState(20),
    [work, setWork] = useState<Record<string, string>>({}),
    [runIndex, setRunIndex] = useState(-1),
    [validate, setValidate] = useState(true),
    [trace, setTrace] = useState(false),
    [conditions, setConditions] = useState<Conditions>(BEST_CASE),
    [counters, setCounters] = useState(true);
  const initialized = useRef(false),
    filesLoaded = useRef(false),
    stopped = useRef(false),
    mcpRevision = useRef(-1);
  useEffect(() => {
    const control = (event: Event) => {
      const s = (event as CustomEvent).detail;
      if (s.workspace?.tuningId === id && mcpRevision.current !== s.workspace.tuningRevision) {
        mcpRevision.current = s.workspace.tuningRevision;
        initialized.current = false;
        filesLoaded.current = false;
        setRunIndex(-1);
      }
    };
    window.addEventListener("mcp-control", control);
    return () => window.removeEventListener("mcp-control", control);
  }, [id]);
  useEffect(() => {
    stopped.current = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await invoke<Status>("tuning_status", { id });
        if (stopped.current) return;
        setStatus(next);
        setBusy(next.running);
        if (!initialized.current && next.session.config) {
          setConfig(next.session.config);
          setArgv(JSON.stringify(next.session.config.argv));
          setEnv(JSON.stringify(next.session.config.env, null, 2));
          initialized.current = true;
        }
        if (next.session.files && !filesLoaded.current) {
          setEdits(Object.fromEntries(next.session.files.map((f) => [f.path, f.code])));
          setFile(next.session.recipe?.source || next.session.files[0]?.path || "");
          filesLoaded.current = true;
        }
      } catch (e) {
        if (!stopped.current) setError(String(e));
      }
      if (!stopped.current) timer = setTimeout(poll, 750);
    };
    void poll();
    return () => {
      stopped.current = true;
      clearTimeout(timer);
    };
  }, [id]);
  const session = status?.session,
    runs = session?.runs || [],
    selected = runIndex < 0 ? runs.at(-1) : runs[runIndex],
    originals = Object.fromEntries((session?.files || []).map((f) => [f.path, f.code])),
    changed = Object.fromEntries(Object.entries(edits).filter(([path, text]) => originals[path] !== text));
  async function action(op: string, extra: Record<string, unknown> = {}) {
    setError("");
    try {
      if (!config) throw Error("Session is still loading");
      const parsedArgv = JSON.parse(argv),
        parsedEnv = JSON.parse(env);
      if (!Array.isArray(parsedArgv)) throw Error("The application command must be a JSON array of arguments");
      if (!parsedEnv || typeof parsedEnv !== "object" || Array.isArray(parsedEnv))
        throw Error("The environment must be a JSON object of strings");
      await invoke("tuning_start", {
        id,
        request: { op, config: { ...config, argv: parsedArgv, env: parsedEnv }, ...extra },
      });
      setBusy(true);
    } catch (e) {
      setError(String(e));
    }
  }
  const field = (key: keyof Config, label: string) => (
    <label>
      {label}
      <input
        value={String(config?.[key] ?? "")}
        onChange={(e) =>
          config &&
          setConfig({ ...config, [key]: key === "occurrence" ? Number(e.target.value) : e.target.value })
        }
      />
    </label>
  );
  const dispatch = session?.dispatch;
  return (
    <div className="tuning-window">
      <header className="tuning-header">
        <Code2 />
        <div>
          <strong>Shader laboratory</strong>
          <p>{session?.kernel || "Opening session…"}</p>
        </div>
        <span className="badge">
          {busy ? "RUNNING" : session?.captured ? "DISPATCH CAPTURED" : "SETUP"}
        </span>
      </header>
      <div className="tuning-content">
        {busy && (
          <button onClick={() => void invoke("tuning_cancel", { id }).catch((e) => setError(String(e)))}>
            <Square size={15} />
            Cancel operation
          </button>
        )}
        {(error || status?.error) && (
          <div className="error-notice" role="alert">
            {error || status?.error}
          </div>
        )}
        {busy && session?.phase && <p role="status">{session.phase}…</p>}
        <details open={!session?.captured}>
          <summary>1 · Application and capture</summary>
          <div className="tuning-form">
            {field("sourceTree", "Source tree")}
            <button
              disabled={busy}
              onClick={async () => {
                const path = await open({ directory: true, multiple: false });
                if (typeof path === "string" && config) setConfig({ ...config, sourceTree: path });
              }}
            >
              Choose source tree
            </button>
            {field("cwd", "Application working directory")}
            <label>
              Application command · JSON argument array
              <textarea value={argv} onChange={(e) => setArgv(e.target.value)} rows={3} />
            </label>
            <label>
              Environment · JSON object (driver selection, GGML_VK_DEBUG_MARKERS…)
              <textarea aria-label="Application environment" value={env} onChange={(e) => setEnv(e.target.value)} rows={4} />
            </label>
            {field("occurrence", "Matching dispatch occurrence · zero-based, among dispatches with this name and workgroup count")}
            <p>
              Selected trace dispatch: {session?.traceDispatch.id} · Grid {session?.traceDispatch.grid} · Workgroup{" "}
              {session?.traceDispatch.workgroup}. The capture layer stops at the dispatch whose pipeline name and
              workgroup count match; re-running the application can change which occurrence that is.
            </p>
            <details>
              <summary>Runtime and build overrides</summary>
              {field("name", "Pipeline name override (default: the traced kernel name)")}
              {field("buildDir", "Shader build directory (default: build* under the source tree)")}
              {field("layer", "Capture layer manifest")}
              {field("replay", "vkreplay executable")}
            </details>
            <div className="tuning-actions">
              <button disabled={busy || !!session?.captured} onClick={() => void action("capture")}>
                <Play size={15} />
                Run application & capture dispatch
              </button>
              <button disabled={busy || !session?.captured || !!session?.recipe} onClick={() => void action("search")}>
                <Search size={15} />
                Find shader source
              </button>
            </div>
            {session?.candidates && !session.recipe && (
              <ul>
                {session.candidates.map((c, i) => (
                  <li key={c.variant}>
                    <button disabled={busy} onClick={() => void action("select", { index: i })}>
                      {c.variant} · {c.sources.join(", ")}
                      {c.runtimePatch.added.length + c.runtimePatch.removed.length
                        ? ` · ${c.runtimePatch.added.length + c.runtimePatch.removed.length} run-time SPIR-V edits`
                        : ""}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </details>
        {session?.captured && (
          <>
            <div className="tuning-section-title">
              <h2>2 · Edit & replay</h2>
              {session.recipe && (
                <span>
                  {session.recipe.variant} · {session.recipe.source}
                </span>
              )}
            </div>
            {session.recipe && session.files ? (
              <>
                <div className="tuning-actions" role="tablist" aria-label="Shader source files">
                  {session.files.map((f) => (
                    <button
                      key={f.path}
                      role="tab"
                      aria-selected={f.path === file}
                      className={f.path === file ? "active" : ""}
                      onClick={() => setFile(f.path)}
                    >
                      {fileName(f.path)}
                      {changed[f.path] !== undefined ? " •" : ""}
                    </button>
                  ))}
                </div>
                <div className="tuning-editors">
                  <label>
                    {fileName(file)}
                    <textarea
                      aria-label="Shader source editor"
                      spellCheck={false}
                      value={edits[file] ?? ""}
                      onChange={(e) => setEdits({ ...edits, [file]: e.target.value })}
                    />
                  </label>
                  <label>
                    ISA · last run
                    <pre aria-label="Shader ISA">{session.isa || "Run the captured baseline to see the compiled ISA."}</pre>
                  </label>
                </div>
                <details>
                  <summary>Recovered compile command & work estimates</summary>
                  <pre>{[session.recipe.glslc, ...session.recipe.argv].join(" ")}</pre>
                  <p>
                    The application edits this shader's SPIR-V at run time (
                    {session.recipe.runtimePatch.added.length} instructions added,{" "}
                    {session.recipe.runtimePatch.removed.length} removed); every edited variant receives the same edits
                    before it runs. Variants compile from a private copy of the shader directory; the source tree
                    changes only on Save.
                  </p>
                  <div className="tuning-form">
                    {Object.entries({ flops: "FLOPs per dispatch", bytes: "Bytes transferred per dispatch" }).map(
                      ([key, label]) => (
                        <label key={key}>
                          {label}
                          <input
                            type="number"
                            min="0"
                            value={work[key] || ""}
                            onChange={(e) => setWork({ ...work, [key]: e.target.value })}
                          />
                        </label>
                      ),
                    )}
                  </div>
                </details>
              </>
            ) : (
              <p>Find the shader source to edit it. The captured SPIR-V can be replayed now.</p>
            )}
            <fieldset className="tuning-conditions">
              <legend>Replay conditions</legend>
              <label>
                Dispatches
                <select
                  aria-label="Replay mode"
                  value={conditions.mode}
                  onChange={(e) => setConditions({ ...conditions, mode: e.target.value as Conditions["mode"] })}
                >
                  <option value="stateless">Stateless: inputs restored before each</option>
                  <option value="sustained">Sustained: back to back, like an application</option>
                </select>
              </label>
              <label>
                Buffers in
                <select
                  aria-label="Replay memory"
                  value={conditions.memory}
                  onChange={(e) => setConditions({ ...conditions, memory: e.target.value as Conditions["memory"] })}
                >
                  <option value="device">Device-local memory</option>
                  <option value="app">ggml's unified-memory type</option>
                  <option value="gtt">System memory (GTT)</option>
                  <option value="auto">Auto: system memory on integrated GPUs</option>
                </select>
              </label>
              <label>
                Replays
                <input
                  aria-label="Replays with fresh allocations"
                  type="number"
                  min="1"
                  max="10"
                  value={conditions.repeats}
                  onChange={(e) => setConditions({ ...conditions, repeats: Number(e.target.value) })}
                />
              </label>
              <label>
                Cache flush (MiB)
                <input
                  aria-label="Cache flush before each dispatch in MiB"
                  type="number"
                  min="0"
                  max="1024"
                  value={conditions.flushMb}
                  onChange={(e) => setConditions({ ...conditions, flushMb: Number(e.target.value) })}
                />
              </label>
              <button type="button" onClick={() => setConditions(BEST_CASE)}>
                Best case
              </button>
              <button type="button" onClick={() => setConditions(APPLICATION_LIKE)}>
                Application-like
              </button>
              <p>
                The default replay is a best case. An application keeps the GPU busy, which can lower its clock, and
                on an integrated GPU its large buffers usually live in system memory, where a kernel's cache behavior
                can depend on which pages each allocation gets. Several replays show that spread.
              </p>
            </fieldset>
            <div className="tuning-actions">
              <label>
                <input type="checkbox" checked={validate} onChange={(e) => setValidate(e.target.checked)} /> Check
                outputs against the application's own results
              </label>
              <label>
                <input type="checkbox" checked={trace} onChange={(e) => setTrace(e.target.checked)} /> Trace the run
                (RADV thread trace)
              </label>
              <label>
                <input type="checkbox" checked={counters} onChange={(e) => setCounters(e.target.checked)} /> Collect
                performance counters (cycles, clock, cache hit rates)
              </label>
              <label>
                Iterations
                <input
                  aria-label="Replay iterations"
                  type="number"
                  min="1"
                  max="10000"
                  value={iterations}
                  onChange={(e) => setIterations(Number(e.target.value))}
                />
              </label>
              <button
                disabled={busy}
                onClick={() =>
                  void action("run", { variant: false, iterations, validate, trace, work, conditions, autoCounters: counters })
                }
              >
                Run captured baseline
              </button>
              <button
                disabled={busy || !session.recipe}
                onClick={() =>
                  void action("run", {
                    variant: true,
                    edits: changed,
                    iterations,
                    validate,
                    trace,
                    work,
                    conditions,
                    autoCounters: counters,
                  })
                }
              >
                <Play size={15} />
                Build & run edited shader
              </button>
              <button
                disabled={busy || !session.recipe || !Object.keys(changed).length}
                onClick={() => void action("save", { edits: changed })}
              >
                <Save size={15} />
                Save edited files
              </button>
            </div>
            {session.lastSave && (
              <p role="status">
                Saved {session.lastSave.path}. Backup: {session.lastSave.backup}
              </p>
            )}
          </>
        )}
        <div className="tuning-section-title">
          <h2>3 · Run comparison</h2>
          <select aria-label="Tuning run" value={runIndex} onChange={(e) => setRunIndex(Number(e.target.value))}>
            <option value={-1}>Latest run</option>
            {runs.map((r, i) => (
              <option key={r.number} value={i}>
                Run {r.number} · {r.variant ? `edited (${(r.edits || []).map(fileName).join(", ")})` : "captured"} ·{" "}
                {shortConditions(r)}
              </option>
            ))}
          </select>
        </div>
        {selected ? (
          <>
            <p>
              Run {selected.number} · {selected.iterations} iterations · {describeConditions(selected)} ·{" "}
              {selected.comparedWith === undefined
                ? `deltas against run ${selected.number - 1}`
                : selected.comparedWith === null
                  ? "no earlier run under these conditions to compare with"
                  : `deltas against run ${selected.comparedWith}, the latest under the same conditions`}
            </p>
            {selected.driver && (
              <p>
                Replayed on {selected.driver.device || "the captured GPU"} ({selected.driver.deviceType || "unknown type"})
                with driver {selected.driver.version}.
              </p>
            )}
            <p role="status">
              Output validation: {selected.validation?.status || "not checked"}
              {selected.validation?.mismatchedRegions?.length
                ? ` · ${selected.validation.mismatchedRegions.join(", ")} differ`
                : "."}
              {selected.validation?.sustainedMatch !== undefined &&
                ` Two back-to-back dispatches: ${selected.validation.sustainedMatch ? "same outputs" : "outputs differ"}.`}
            </p>
            {(selected.notes || []).map((n) => (
              <p key={n} role="note">
                {n}
              </p>
            ))}
            {selected.trace && (
              <p>
                {typeof selected.trace === "string"
                  ? `Thread trace of this run: ${selected.trace} (open it with Open capture).`
                  : `Trace failed: ${selected.trace.error}`}
              </p>
            )}
            <div className="tuning-metrics">
              {Object.entries(selected.metrics)
                .sort(([a], [b]) => metricRank(a) - metricRank(b))
                .map(([name, m]) => (
                  <article key={name} title={m.reason || undefined}>
                    <h3>{name}</h3>
                    <strong>
                      {m.value === null ? "Not collected" : number(m.value)}{" "}
                      <small>{m.value === null ? "" : m.unit}</small>
                    </strong>
                    {m.delta !== undefined && (
                      <p className="metric-delta">
                        Δ {m.delta > 0 ? "+" : ""}
                        {number(m.delta)} {m.unit}
                        {m.deltaPercent != null ? ` (${m.deltaPercent > 0 ? "+" : ""}${number(m.deltaPercent)}%)` : ""}
                      </p>
                    )}
                    {m.reason && <p>{m.reason}</p>}
                  </article>
                ))}
            </div>
          </>
        ) : (
          <p>No completed runs yet. Capture the dispatch, then run the captured baseline.</p>
        )}
        {dispatch && (
          <details>
            <summary>Captured dispatch, constants and buffers</summary>
            <p>
              {dispatch.name} · workgroups {dispatch.groups.join(" × ")} · {dispatch.device?.name}
            </p>
            <p>Debug labels: {dispatch.labels.join(" › ") || "none"}</p>
            <table>
              <thead>
                <tr>
                  <th>Binding</th>
                  <th>Buffer</th>
                  <th>Offset</th>
                  <th>Range</th>
                  <th>Bytes the application wrote</th>
                </tr>
              </thead>
              <tbody>
                {dispatch.bindings.map((b) => (
                  <tr key={`${b.set}:${b.binding}`}>
                    <td>
                      set {b.set} · binding {b.binding}
                    </td>
                    <td>{b.group}</td>
                    <td>{b.offset}</td>
                    <td>{number(b.range)}</td>
                    <td>{number(dispatch.buffers[b.group]?.writtenBytes ?? 0)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p>
              Specialization constants:{" "}
              {dispatch.specValues.map((s) => `${s.id}=${s.value}`).join(", ") || "none"}
            </p>
            <p>Push constants: {dispatch.pushHex || "none"}</p>
            {dispatch.unknownFeatures.length > 0 && (
              <p role="note">
                The application enabled device feature structures the replay cannot reproduce:{" "}
                {dispatch.unknownFeatures.join(", ")}.
              </p>
            )}
          </details>
        )}
        <details>
          <summary>Job log & artifacts</summary>
          <p>{session?.directory}</p>
          <pre>{status?.log || "No commands have run yet."}</pre>
        </details>
      </div>
    </div>
  );
}
