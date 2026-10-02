import { useState } from "react";
import {
  desktop,
  liveProfileStart,
  liveProfileTrigger,
  liveProfileStatus,
  liveProfileStop,
} from "../bridge";

type Json = Record<string, unknown>;

// Compact desktop surface for the four live-profiling commands. It drives the
// same core the MCP tools use (profile_start/trigger/status/stop); the logic is
// not forked. rocprofv3 windows can be timed (windowMs) or triggered (end on
// demand), and carry the same stream/counter/att/kernel-filter selection.
export default function LiveProfiling() {
  const [tracer, setTracer] = useState("rocprofv3");
  const [argv, setArgv] = useState("");
  const [cwd, setCwd] = useState("");
  const [profiler, setProfiler] = useState("");
  const [windowMs, setWindowMs] = useState("5000");
  const [triggered, setTriggered] = useState(false);
  const [traces, setTraces] = useState("");
  const [counters, setCounters] = useState("");
  const [att, setAtt] = useState(false);
  const [includeRegex, setIncludeRegex] = useState("");
  const [excludeRegex, setExcludeRegex] = useState("");
  const [profile, setProfile] = useState<Json | null>(null);
  const [status, setStatus] = useState<Json | null>(null);
  const [result, setResult] = useState<Json | null>(null);
  const [windows, setWindows] = useState<{ count: number; mode: string }[]>([]);
  const [openTriggered, setOpenTriggered] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const rocprofv3 = tracer === "rocprofv3";
  const list = (s: string) => s.split(/[\s,]+/).filter(Boolean);
  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError("");
    try {
      await fn();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const start = () =>
    run(async () => {
      const args: Json = { argv: list(argv), cwd, tracer };
      if (rocprofv3 && profiler.trim()) args.profiler = profiler.trim();
      const started = await liveProfileStart(args);
      setProfile(started);
      setStatus(null);
      setResult(null);
      setWindows([]);
      setOpenTriggered(false);
    });

  const trigger = () =>
    run(async () => {
      if (!profile) return;
      const args: Json = { id: profile.id as string };
      if (rocprofv3) {
        if (triggered) args.triggered = true;
        else args.windowMs = Number(windowMs);
        if (traces.trim()) args.traces = list(traces);
        if (counters.trim()) args.counters = list(counters);
        if (att) args.att = {};
        if (includeRegex.trim()) args.kernelIncludeRegex = includeRegex.trim();
        if (excludeRegex.trim()) args.kernelExcludeRegex = excludeRegex.trim();
      }
      const res = await liveProfileTrigger(args);
      if (typeof res.window === "number")
        setWindows((w) => [...w, { count: res.window as number, mode: String(res.mode ?? "timed") }]);
      if (res.mode === "triggered") setOpenTriggered(true);
      setStatus(res);
    });

  const endWindow = () =>
    run(async () => {
      if (!profile) return;
      const res = await liveProfileTrigger({ id: profile.id as string, end: true });
      setOpenTriggered(false);
      setStatus(res);
    });

  const poll = () =>
    run(async () => {
      if (!profile) return;
      setStatus(await liveProfileStatus(profile.id as string));
    });

  const stop = () =>
    run(async () => {
      if (!profile) return;
      const res = await liveProfileStop(profile.id as string);
      setResult(res);
      setProfile(null);
      setStatus(null);
      setOpenTriggered(false);
    });

  const warnings = (value: Json | null): string[] =>
    Array.isArray(value?.warnings) ? (value!.warnings as string[]) : [];

  if (!desktop)
    return (
      <div className="content live-profiling">
        <h1>Live profiling</h1>
        <p>
          Live profiling launches an application and attaches a profiler on the
          local machine, so it runs only in the desktop app. Start it with{" "}
          <code>npm run tauri dev</code> or the packaged build; the browser
          preview has no profiling server.
        </p>
      </div>
    );

  return (
    <div className="content live-profiling">
      <h1>Live profiling</h1>
      <p>
        Launch an application and gate GPU capture through the same core the MCP
        profile tools use. RADV captures RGP frames on a file trigger; rocprofv3
        attaches for timed or triggered windows.
      </p>
      {!profile ? (
        <section className="live-form">
          <label>
            Tracer
            <select value={tracer} onChange={(e) => setTracer(e.target.value)} disabled={busy}>
              <option value="rocprofv3">rocprofv3 (HIP/ROCm attach)</option>
              <option value="radv">radv (Vulkan RGP)</option>
            </select>
          </label>
          <label>
            Command
            <textarea
              value={argv}
              onChange={(e) => setArgv(e.target.value)}
              placeholder="/path/to/llama-server --model model.gguf -ngl 99"
              disabled={busy}
            />
          </label>
          <label>
            Working directory
            <input value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder="/path/to/cwd" disabled={busy} />
          </label>
          {rocprofv3 && (
            <label>
              rocprofv3 path (optional)
              <input
                value={profiler}
                onChange={(e) => setProfiler(e.target.value)}
                placeholder="/opt/rocm/bin/rocprofv3"
                disabled={busy}
              />
            </label>
          )}
          <button onClick={start} disabled={busy || !argv.trim() || !cwd.trim()}>
            Start profiling
          </button>
        </section>
      ) : (
        <section className="live-session">
          <p className="live-id">
            Running <strong>{String(profile.id)}</strong> · pid {String(profile.pid)}
          </p>
          {typeof profile.triggerFile === "string" && (
            <p>
              RADV trigger file: <code>{profile.triggerFile}</code>
            </p>
          )}
          {rocprofv3 ? (
            <>
              <label>
                Window mode
                <select
                  value={triggered ? "triggered" : "timed"}
                  onChange={(e) => setTriggered(e.target.value === "triggered")}
                  disabled={busy || openTriggered}
                >
                  <option value="timed">Timed (windowMs)</option>
                  <option value="triggered">Triggered (end on demand)</option>
                </select>
              </label>
              {!triggered && (
                <label>
                  Window (ms)
                  <input value={windowMs} onChange={(e) => setWindowMs(e.target.value)} disabled={busy} />
                </label>
              )}
              <label>
                Traces
                <input
                  value={traces}
                  onChange={(e) => setTraces(e.target.value)}
                  placeholder="kernel, hip"
                  disabled={busy}
                />
              </label>
              <label>
                Counters
                <input value={counters} onChange={(e) => setCounters(e.target.value)} disabled={busy} />
              </label>
              <label className="live-check">
                <input type="checkbox" checked={att} onChange={(e) => setAtt(e.target.checked)} disabled={busy} />
                ATT thread trace
              </label>
              <label>
                Kernel include regex
                <input value={includeRegex} onChange={(e) => setIncludeRegex(e.target.value)} disabled={busy} />
              </label>
              <label>
                Kernel exclude regex
                <input value={excludeRegex} onChange={(e) => setExcludeRegex(e.target.value)} disabled={busy} />
              </label>
              <div className="live-actions">
                <button onClick={trigger} disabled={busy || openTriggered}>
                  Trigger window
                </button>
                {openTriggered && (
                  <button onClick={endWindow} disabled={busy}>
                    End window
                  </button>
                )}
                <button onClick={poll} disabled={busy}>
                  Refresh status
                </button>
                <button onClick={stop} disabled={busy}>
                  Stop and collect
                </button>
              </div>
            </>
          ) : (
            <div className="live-actions">
              <button onClick={trigger} disabled={busy}>
                Arm RGP trigger
              </button>
              <button onClick={poll} disabled={busy}>
                Refresh status
              </button>
              <button onClick={stop} disabled={busy}>
                Stop and collect
              </button>
            </div>
          )}
          {windows.length > 0 && (
            <div className="live-windows">
              <strong>Windows</strong>
              <ul>
                {windows.map((w) => (
                  <li key={w.count}>
                    window-{w.count} · {w.mode}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {status && (
            <p className="live-status">
              {status.windowRunning
                ? "Window running…"
                : status.triggerPending
                  ? "Trigger pending…"
                  : "Idle"}
              {typeof status.attachAttempts === "number" ? ` · attach attempts: ${status.attachAttempts}` : ""}
            </p>
          )}
          {warnings(status).map((w, i) => (
            <p key={i} role="alert" className="live-warning">
              {w}
            </p>
          ))}
        </section>
      )}
      {result && (
        <section className="live-result">
          <h2>Collected</h2>
          <p>
            {Array.isArray(result.files) ? (result.files as unknown[]).length : 0} file(s) in{" "}
            <code>{String(result.directory)}</code>
          </p>
          {warnings(result).map((w, i) => (
            <p key={i} role="alert" className="live-warning">
              {w}
            </p>
          ))}
        </section>
      )}
      {error && (
        <p role="alert" className="live-error">
          {error}
        </p>
      )}
    </div>
  );
}
