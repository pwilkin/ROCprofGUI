import { useContext, useEffect, useRef, useState } from "react";
import { readWave } from "../bridge";
import type { Capture, HiddenAnalysis, MarkerScope, Range } from "../types";
import { fmt, colors } from "../types";
import { markerSourceObservations } from "../markerSource";
import {
  weightedSourceHierarchy,
  clipSourceHierarchy,
  type SourceFrame,
  type SourceObservation,
} from "../sourceHierarchy";
import { Panel } from "./AnalysisViews";
import { PlotAxesContext, plotAxisKey } from "../plotAxes";
import { fitWeightedRange } from "../weightedViewport";
import { PlotExport } from "./PlotExport";
import { svgPlot } from "../plotExport";
import RangeBound from "./RangeBound";

export default function MarkerSourceView({
  capture,
  markers,
  scope,
  range,
  includeIdle,
  hidden,
  onSelect,
}: {
  capture: Capture;
  markers: MarkerScope[];
  scope: string;
  range: Range;
  includeIdle: boolean;
  hidden?: HiddenAnalysis;
  onSelect: (line: number) => void;
}) {
  const [frames, setFrames] = useState<SourceFrame[] | null>(null);
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const request = useRef<AbortController | null>(null);
  const svg = useRef<SVGSVGElement>(null);
  const plots = useContext(PlotAxesContext);
  const key = plotAxisKey(
    "marker-source",
    scope,
    hidden ? "nonhidden" : "total",
    String(includeIdle),
  );
  const total =
    frames?.filter((f) => f.depth === 0).reduce((s, f) => s + f.total, 0) || 0;
  const bounds: Range = [0, Math.max(1, total)];
  const viewport = plots?.ranges[key]
    ? fitWeightedRange(plots.ranges[key], total)
    : bounds;
  const visible = clipSourceHierarchy(frames || [], viewport);
  const height = Math.max(
    60,
    (visible.reduce((m, f) => Math.max(m, f.depth), 0) + 1) * 30,
  );
  const setViewport = (value: Range | null) => plots?.changeRange(key, value);
  useEffect(() => {
    request.current?.abort();
    setFrames(null);
    setStatus("");
    setBusy(false);
    return () => request.current?.abort();
  }, [capture, markers, range[0], range[1], includeIdle, hidden]);
  async function build() {
    request.current?.abort();
    const controller = new AbortController();
    request.current = controller;
    setBusy(true);
    setFrames(null);
    try {
      const hardware = new Set(
        markers.map((m) => `${m.se}:${m.simd}:${m.slot}`),
      );
      const candidates = capture.waves.filter(
        (w) =>
          hardware.has(`${w.se}:${w.simd}:${w.slot}`) &&
          w.start < range[1] &&
          w.end > range[0],
      );
      const instructions = new Map(
        capture.instructions.map((i) => [i.index, i]),
      );
      const observations = new Map<string, SourceObservation>();
      let tokens = 0,
        loaded = 0,
        processed = 0;
      for (const index of candidates) {
        controller.signal.throwIfAborted();
        setStatus(`Reading wave ${++loaded} of ${candidates.length}`);
        const wave = await readWave(capture, index.id);
        controller.signal.throwIfAborted();
        if (
          !markers.some(
            (m) =>
              m.se === index.se &&
              m.cu === wave.cu &&
              m.simd === index.simd &&
              m.slot === index.slot,
          )
        ) {
          if (wave.cu === null)
            throw Error(`Wave ${index.id} has no recorded CU`);
          continue;
        }
        tokens += wave.tokens.length;
        if (tokens > 2000000)
          throw Error(
            "More than 2,000,000 instruction occurrences; narrow the time or hardware scope",
          );
        for (const observation of markerSourceObservations(
          index,
          wave,
          markers,
          instructions,
          range,
          includeIdle,
          hidden,
        )) {
          const identity = JSON.stringify([
            observation.prefix.map((p) => p.key),
            observation.instruction.index,
          ]);
          const previous = observations.get(identity);
          if (previous) previous.weight += observation.weight;
          else observations.set(identity, observation);
          if (observations.size > 100000)
            throw Error(
              "More than 100,000 marker/ISA paths; narrow the time or hardware scope",
            );
          if (++processed % 4096 === 0) {
            await new Promise((resolve) => setTimeout(resolve, 0));
            controller.signal.throwIfAborted();
          }
        }
        await new Promise((resolve) => setTimeout(resolve, 0));
      }
      controller.signal.throwIfAborted();
      const next = weightedSourceHierarchy([...observations.values()], 100000);
      if (next.some((f) => !Number.isFinite(f.total)))
        throw Error("Marker/source cost total is not finite");
      setFrames(next);
      setStatus(
        `${loaded} wave files read · ${tokens.toLocaleString()} instruction occurrences examined`,
      );
    } catch (error) {
      if (!controller.signal.aborted) setStatus(String(error));
    } finally {
      if (request.current === controller) setBusy(false);
    }
  }
  return (
    <Panel
      title="Marker and source hierarchy"
      detail="Instruction cost grouped by recorded marker scopes, source frames and ISA."
    >
      <p className="muted small">
        Uses instruction start times in the visible range and exact
        SE/CU/SIMD/slot identity. Widths sum {hidden ? "nonhidden" : "total"}{" "}
        token cycles{includeIdle ? " including preceding idle" : ""}, not marker
        duration. Open markers remain active; point markers do not enclose
        instructions. Conflicting scope chains stay in an ambiguous bucket. Slot
        reuse cannot establish wave ownership.
      </p>
      <div className="source-remap-controls">
        <button disabled={busy} onClick={build}>
          Build marker/source hierarchy
        </button>
        {busy && (
          <button
            onClick={() => {
              request.current?.abort();
              setBusy(false);
              setStatus("Cancelled");
            }}
          >
            Cancel hierarchy
          </button>
        )}
        {status && <span role="status">{status}</span>}
      </div>
      {frames && (
        <>
          <div className="source-remap-controls">
            <RangeBound
              label="Marker source cost range"
              edge={0}
              range={viewport}
              bounds={bounds}
              onCommit={setViewport}
            />
            <RangeBound
              label="Marker source cost range"
              edge={1}
              range={viewport}
              bounds={bounds}
              onCommit={setViewport}
            />
            <button onClick={() => setViewport(null)}>
              Full marker/source range
            </button>
            <span>{fmt(total)} cycles</span>
            {visible.length <= 4000 && total > 0 && (
              <PlotExport
                format="svg"
                label="Marker source hierarchy"
                generate={() => {
                  if (!svg.current) throw Error("No hierarchy plot");
                  return svgPlot(
                    svg.current,
                    `${capture.name} · Marker/source hierarchy`,
                    `${scope} · start times ${range[0]}–${range[1]} · ${hidden ? "nonhidden" : "total"} cycles · idle ${includeIdle} · cumulative cost ${viewport[0]}–${viewport[1]}`,
                  );
                }}
              />
            )}
          </div>
          {visible.length > 4000 ? (
            <p role="alert">
              More than 4,000 visible frames. Narrow the cumulative cost range.
            </p>
          ) : total > 0 ? (
            <div style={{ overflow: "auto", maxHeight: 650 }}>
              <svg
                ref={svg}
                viewBox={`0 0 1000 ${height}`}
                style={{ width: "100%", minHeight: height }}
                role="img"
                aria-label="Combined marker source and ISA hierarchy"
              >
                {visible.map((frame) => (
                  <g
                    key={frame.key}
                    role="button"
                    tabIndex={0}
                    aria-label={`${frame.label}: ${fmt(frame.total)} cycles`}
                    onClick={() =>
                      frame.instruction
                        ? onSelect(frame.instruction.index)
                        : setViewport([frame.start, frame.end])
                    }
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        frame.instruction
                          ? onSelect(frame.instruction.index)
                          : setViewport([frame.start, frame.end]);
                      }
                    }}
                  >
                    <title>
                      {frame.label} · {fmt(frame.total)} cycles
                    </title>
                    <rect
                      x={frame.x * 1000}
                      y={frame.depth * 30}
                      width={frame.width * 1000}
                      height={28}
                      fill={
                        frame.instruction
                          ? "#e4ad7c"
                          : colors[frame.depth % colors.length]
                      }
                    />
                    <text
                      x={frame.x * 1000 + 3}
                      y={frame.depth * 30 + 19}
                      fill="#161b23"
                      fontSize="11"
                    >
                      {frame.label.slice(
                        0,
                        Math.max(0, Math.floor((frame.width * 1000) / 7) - 1),
                      )}
                    </text>
                  </g>
                ))}
              </svg>
            </div>
          ) : (
            <p>No mapped instruction cost in this scope and range.</p>
          )}
        </>
      )}
    </Panel>
  );
}
