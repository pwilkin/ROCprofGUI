import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { desktop } from "../bridge";
import { useAnalysisPreference } from "../analysisPreferences";
import {
  readTraceAssociations,
  type TraceAssociation,
} from "../traceAssociations";
import type { Capture } from "../types";

export default function TraceAssociations({
  capture,
  onOpen,
}: {
  capture: Capture;
  onOpen: (path: string) => void;
}) {
  const [stored, setStored] = useAnalysisPreference<string>(
    "att.associations",
    "[]",
  );
  const rows = readTraceAssociations(stored) ?? [];
  const [mode, setMode] = useState<TraceAssociation["mode"]>("unknown");
  const [dispatch, setDispatch] = useState("");
  const [search, setSearch] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const alive = useRef(false);
  const generation = useRef(0);
  const currentCapture = useRef(capture);
  const currentStored = useRef(stored);
  currentCapture.current = capture;
  currentStored.current = stored;
  useEffect(() => {
    alive.current = true;
    generation.current++;
    setBusy(false);
    setError("");
    setMode("unknown");
    setDispatch("");
    setSearch("");
    return () => {
      alive.current = false;
      generation.current++;
    };
  }, [capture]);
  const matches = capture.dispatches.filter((d) =>
    `${d.id} ${d.kernel} ${d.agent}`
      .toLowerCase()
      .includes(search.toLowerCase()),
  );
  async function choose(relocate?: TraceAssociation) {
    setError("");
    if (
      !relocate && mode === "single-dispatch" &&
      capture.dispatches.filter((d) => d.id === dispatch).length !== 1
    ) {
      setError("Select one uniquely identified dispatch from this capture.");
      return;
    }
    const requestGeneration = generation.current;
    const valid = () => alive.current && generation.current === requestGeneration && currentCapture.current === capture;
    setBusy(true);
    try {
      const path = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "ATT manifest", extensions: ["rocprof-att"] }],
      });
      if (!valid() || !path || typeof path !== "string") return;
      if (currentStored.current !== stored)
        throw Error("Trace associations changed while the picker was open. Select the manifest again.");
      const next = relocate ? JSON.stringify(rows.map(row => row.path === relocate.path
        ? {...row, relocation: path === row.path ? "" : path} : row)) : JSON.stringify([
        ...rows.filter((row) => row.path !== path),
        { path, mode, dispatch: mode === "single-dispatch" ? dispatch : "" },
      ]);
      if (!readTraceAssociations(next))
        throw Error("Association limit exceeded or invalid ATT manifest path.");
      setStored(next);
    } catch (e) {
      if (valid()) setError(String(e));
    } finally {
      if (valid()) setBusy(false);
    }
  }
  return (
    <section className="panel trace-associations">
      <h3>Associated ATT manifests</h3>
      <p>
        Explicit user associations, saved with this capture in the project.
        Filename matches do not establish dispatch ownership. Opening a manifest
        decodes a separate capture with its own clock; it does not align or
        merge timestamps.
      </p>
      <label>
        Trace collection mode{" "}
        <select
          aria-label="Trace collection mode"
          value={mode}
          onChange={(e) => {
            setMode(e.target.value as TraceAssociation["mode"]);
            setDispatch("");
          }}
        >
          <option value="unknown">Unknown ownership</option>
          <option value="single-dispatch">
            Single dispatch (user asserted)
          </option>
          <option value="region">Region / consecutive kernels</option>
        </select>
      </label>
      {mode === "single-dispatch" && (
        <>
          <label>
            Find associated dispatch{" "}
            <input
              aria-label="Find associated dispatch"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </label>
          <label>
            Associated dispatch{" "}
            <select
              aria-label="Associated dispatch"
              value={dispatch}
              onChange={(e) => setDispatch(e.target.value)}
            >
              <option value="">Select a dispatch…</option>
              {matches.slice(0, 200).map((d, i) => (
                <option key={`${d.id}:${i}`} value={d.id}>
                  {d.id} · {d.kernel} · {d.agent}
                </option>
              ))}
            </select>
          </label>
          {matches.length > 200 && (
            <p>
              Showing 200 of {matches.length} matching dispatches. Narrow the
              search.
            </p>
          )}
        </>
      )}
      <button disabled={!desktop || busy} onClick={() => void choose()}>
        Associate ATT manifest…
      </button>
      {error && <p role="alert">{error}</p>}
      {rows.map((row) => (
        <div className="trace-association" key={row.path}>
          <p>Recorded manifest: {row.path}</p>
          {row.relocation && <p>Selected replacement: {row.relocation}. Collection-time identity is unverified.</p>}
          <p>
            {row.mode === "single-dispatch"
              ? `User-asserted dispatch: ${row.dispatch}`
              : row.mode === "region"
                ? "Region / consecutive-kernel trace; no single-dispatch assignment"
                : "Unknown dispatch ownership"}
          </p>
          {row.dispatch &&
            capture.dispatches.filter((d) => d.id === row.dispatch).length !==
              1 && (
              <p role="alert">
                Recorded dispatch is missing or ambiguous in this capture.
              </p>
            )}
          <div className="trace-association-actions">
          <button disabled={!desktop || busy} onClick={() => onOpen(row.relocation || row.path)}>
            Open associated trace
          </button>
          <button disabled={!desktop || busy} onClick={() => void choose(row)}>
            Locate moved manifest…
          </button>
          {row.relocation && <button disabled={busy} onClick={() => setStored(JSON.stringify(
            rows.map(other => { if (other.path !== row.path) return other; const {relocation, ...original} = other; return original; })
          ))}>Forget manifest replacement</button>}
          <button
            disabled={busy}
            onClick={() =>
              setStored(
                JSON.stringify(rows.filter((other) => other.path !== row.path)),
              )
            }
          >
            Remove trace association
          </button>
          </div>
        </div>
      ))}
      <p>
        References do not embed files or verify collection-time identity.
        Opening validates the current files. Saving an opened trace in a project
        includes its normal input fingerprints.
      </p>
    </section>
  );
}
