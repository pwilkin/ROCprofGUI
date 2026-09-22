import MarkerSourceView from "./MarkerSourceView";
import { markerInstruction } from "../markerSource";
import { useAnalysisPreference } from "../analysisPreferences";
import type { TimelinePreferences } from "../timelinePreferences";
import RangeBound from "./RangeBound";
import { PlotAxesContext, plotAxisKey } from "../plotAxes";
import { clipWeightedInterval, fitWeightedRange } from "../weightedViewport";
import { PlotExport } from "./PlotExport";
import { svgPlot } from "../plotExport";
import { useContext, useMemo, useRef } from "react";
import type { Capture, Range, HiddenAnalysis } from "../types";
import { fmt, colors } from "../types";
import { markerTree, type MarkerNode } from "../markerAnalysis";
import { Timeline } from "./Plots";
import { Panel, Empty } from "./AnalysisViews";
export default function MarkersView({
  capture,
  range,
  onRange,
  onSource,
  preferences,
  onPreferences,
  includeIdle = false,
  hidden,
}: {
  includeIdle?: boolean;
  hidden?: HiddenAnalysis;
  capture: Capture;
  range: Range;
  onRange: (r: Range) => void;
  onSource: (line: number) => void;
  preferences: TimelinePreferences;
  onPreferences: (preferences: TimelinePreferences) => void;
}) {
  const [scope, setScope] = useAnalysisPreference<string>(
      "markers.scope",
      "all",
    ),
    [selected, setSelected] = useAnalysisPreference<number | null>(
      "markers.selected",
      null,
    ),
    [page, setPage] = useAnalysisPreference<number>("markers.page", 0);
  const flame = useRef<SVGSVGElement>(null);
  const markers = capture.markers || [];
  const location = (m: (typeof markers)[number]) =>
    `SE ${m.se} · CU ${m.cu} · SIMD ${m.simd} · slot ${m.slot}`;
  const locations = [...new Set(markers.map(location))];
  const scoped = useMemo(
    () => markers.filter((m) => scope === "all" || location(m) === scope),
    [markers, scope],
  );
  const visible = scoped.filter(
    (m) => m.start <= range[1] && (m.end === null || m.end >= range[0]),
  );
  const roots = useMemo(() => markerTree(scoped, range), [scoped, range]);
  const total = roots.reduce((sum, n) => sum + n.duration, 0);
  const plots = useContext(PlotAxesContext);
  const viewportKey = plotAxisKey("marker-flamegraph", scope);
  const storedViewport = plots?.ranges[viewportKey];
  const domain: Range = [0, Math.max(1, total)];
  const viewport = storedViewport
    ? fitWeightedRange(storedViewport, total)
    : domain;
  const setViewport = (next: Range | null) =>
    plots?.changeRange(viewportKey, next);
  function navigate(factor: number, shift = 0) {
    const span = viewport[1] - viewport[0];
    const middle = (viewport[0] + viewport[1]) / 2 + shift * span;
    setViewport(
      fitWeightedRange(
        [middle - (span * factor) / 2, middle + (span * factor) / 2],
        total,
      ),
    );
  }
  const rectangles: {
    node: MarkerNode;
    x: number;
    width: number;
    depth: number;
  }[] = [];
  function layout(
    nodes: MarkerNode[],
    left: number,
    width: number,
    depth: number,
    denominator: number,
  ) {
    if (depth >= 64) return;
    let x = left;
    for (const node of nodes) {
      const span = denominator > 0 ? (width * node.duration) / denominator : 0;
      if (span > 0) {
        const clipped = clipWeightedInterval(x, x + span, viewport);
        if (clipped)
          rectangles.push({
            node,
            x: clipped.x * 1000,
            width: clipped.width * 1000,
            depth,
          });
        layout(node.children, x, span, depth + 1, node.duration);
      }
      x += span;
    }
  }
  layout(roots, 0, total, 0, total);
  const current = markers.find((m) => m.id === selected);
  const sourceLine = current
    ? markerInstruction(current, capture.instructions)
    : undefined;
  const safePage = Math.min(
    page,
    Math.max(0, Math.ceil(visible.length / 100) - 1),
  );
  if (!Number.isFinite(total))
    return (
      <Empty
        title="Marker duration total is not finite"
        detail="The marker flamegraph cannot represent this aggregate duration."
      />
    );
  if (!markers.length)
    return (
      <Empty
        title="No resolved instrumentation markers"
        detail="Marker scopes require shader-data records and a funcmap identifying their active code object. Raw shader-data values are not assumed to be markers. See Capture evidence for import diagnostics."
      />
    );
  return (
    <>
      <MarkerSourceView
        capture={capture}
        markers={scoped}
        scope={scope}
        range={range}
        includeIdle={includeIdle}
        hidden={hidden}
        onSelect={onSource}
      />
      <Panel
        title="Marker scopes"
        detail="Per-wave instrumentation scopes on the capture clock. Open scopes have unknown duration; point markers have no duration."
      >
        <label>
          Hardware scope{" "}
          <select
            aria-label="Marker hardware scope"
            value={scope}
            onChange={(e) => {
              setScope(e.target.value);
              setPage(0);
            }}
          >
            <option value="all">All recorded locations</option>
            {locations.map((l) => (
              <option key={l}>{l}</option>
            ))}
          </select>
        </label>
        <Timeline
          preferenceId="markers"
          preferences={preferences}
          onPreferences={onPreferences}
          exportLabel={`${capture.name} · Marker scopes · ${scope}`}
          unit={capture.unit}
          range={range}
          bounds={[capture.start, capture.end]}
          onRange={onRange}
          marks={scoped.map((m) => ({
            start: m.start,
            end: m.end ?? capture.end,
            lane: `${location(m)} · depth ${m.depth}`,
            label: `${m.name}${m.end === null ? " · incomplete" : ""}${m.point ? " · point" : ""}`,
            key: String(m.id),
            color: colors[m.depth % colors.length],
            selected: m.id === selected,
          }))}
          onSelect={(m) => setSelected(+m.key)}
          height={275}
        />
      </Panel>
      <Panel
        title="Marker flamegraph"
        tools={
          rectangles.length ? (
            <PlotExport
              format="svg"
              label="Marker flamegraph"
              generate={() => {
                if (!flame.current) throw Error("No marker plot");
                return svgPlot(
                  flame.current,
                  `${capture.name} · Marker flamegraph`,
                  `${scope} · range ${range[0]}–${range[1]} ${capture.unit} · root total ${total}; visible cumulative duration ${viewport[0]}–${viewport[1]}; open scopes/points excluded; first 64 levels`,
                );
              }}
            />
          ) : undefined
        }
        detail={`${fmt(total)} summed root-scope ${capture.unit} in the visible range. Inclusive child durations are not added to the root total. Open scopes and points are excluded from widths.`}
      >
        <div className="flame-viewport toolbar-inline">
          <span>Cumulative root duration ({capture.unit})</span>
          <RangeBound
            label="Marker flamegraph range"
            edge={0}
            range={viewport}
            bounds={domain}
            onCommit={setViewport}
          />
          <RangeBound
            label="Marker flamegraph range"
            edge={1}
            range={viewport}
            bounds={domain}
            onCommit={setViewport}
          />
          <button
            aria-label="Pan marker flamegraph left"
            onClick={() => navigate(1, -0.5)}
          >
            ←
          </button>
          <button
            aria-label="Zoom marker flamegraph in"
            onClick={() => navigate(0.5)}
          >
            +
          </button>
          <button
            aria-label="Zoom marker flamegraph out"
            onClick={() => navigate(2)}
          >
            −
          </button>
          <button
            aria-label="Pan marker flamegraph right"
            onClick={() => navigate(1, 0.5)}
          >
            →
          </button>
          <button onClick={() => setViewport(null)}>
            Full marker flamegraph range
          </button>
          <span>
            {storedViewport ? "Independent viewport" : "Full weight range"} ·
            workspace time filters duration totals
          </span>
        </div>
        {rectangles.length ? (
          <svg
            ref={flame}
            className="marker-flame"
            viewBox={`0 0 1000 ${rectangles.reduce((depth, r) => Math.max(depth, r.depth), 0) * 30 + 30}`}
            role="img"
            aria-label="Nested marker duration flamegraph"
          >
            {rectangles.map((r, i) => (
              <g
                key={i}
                role="button"
                tabIndex={0}
                aria-label={`${r.node.name}: ${fmt(r.node.duration)} ${capture.unit}`}
                onClick={() => setSelected(r.node.ids[0])}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    setSelected(r.node.ids[0]);
                  }
                }}
              >
                <title>
                  {r.node.name} · {fmt(r.node.duration)} {capture.unit} ·{" "}
                  {r.node.count} occurrences
                </title>
                <rect
                  x={r.x}
                  y={r.depth * 30 + 1}
                  width={r.width}
                  height={27}
                  stroke="var(--panel)"
                  strokeWidth={1}
                  fill={colors[(r.depth + 3) % colors.length]}
                  opacity={0.8}
                />
                {r.width > 100 && (
                  <text fill="#111820" x={r.x + 7} y={r.depth * 30 + 19}>
                    {r.node.name.slice(0, Math.floor(r.width / 8))}
                  </text>
                )}
              </g>
            ))}
          </svg>
        ) : (
          <p>No complete duration scopes in this selection.</p>
        )}
        {scoped.some((m) => m.depth >= 64) && (
          <p className="notice">
            Flamegraph displays the first 64 nesting levels. Deeper records
            remain in the scope list.
          </p>
        )}
      </Panel>
      {current && (
        <Panel
          title={current.name}
          detail={`${location(current)} · code object ${current.codeObject} · marker ${current.markerId}`}
        >
          <p>
            {current.point
              ? "Point marker"
              : current.end === null
                ? "Incomplete scope: no matching exit recorded"
                : `${fmt(current.end - current.start)} ${capture.unit}`}{" "}
            · {current.source || "No source location recorded"}
          </p>
          <div className="toolbar-inline">
            <button
              disabled={current.end === null || current.point}
              onClick={() =>
                current.end !== null &&
                onRange([
                  current.start,
                  Math.max(current.start + 1, current.end),
                ])
              }
            >
              Zoom to marker
            </button>
            <button
              disabled={!sourceLine}
              onClick={() => sourceLine && onSource(sourceLine.index)}
            >
              Go to marker source
            </button>
            {current.parent !== null && (
              <button onClick={() => setSelected(current.parent)}>
                Select parent scope
              </button>
            )}
          </div>
        </Panel>
      )}
      <Panel
        title="Recorded marker instances"
        detail={`${visible.length} instances intersect the visible range`}
      >
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>Marker</th>
                <th>Location</th>
                <th>Start</th>
                <th>Duration</th>
                <th>Depth</th>
              </tr>
            </thead>
            <tbody>
              {visible.slice(safePage * 100, (safePage + 1) * 100).map((m) => (
                <tr key={m.id} className={m.id === selected ? "selected" : ""}>
                  <td>
                    <button onClick={() => setSelected(m.id)}>{m.name}</button>
                  </td>
                  <td>{location(m)}</td>
                  <td>{fmt(m.start)}</td>
                  <td>
                    {m.point
                      ? "Point"
                      : m.end === null
                        ? "Incomplete"
                        : fmt(m.end - m.start)}
                  </td>
                  <td>{m.depth}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="toolbar-inline">
          <button disabled={!safePage} onClick={() => setPage(safePage - 1)}>
            Previous markers
          </button>
          <span>Page {safePage + 1}</span>
          <button
            disabled={(safePage + 1) * 100 >= visible.length}
            onClick={() => setPage(safePage + 1)}
          >
            Next markers
          </button>
        </div>
      </Panel>
    </>
  );
}
