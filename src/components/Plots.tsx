import { DisplayScaleContext } from "../displayScale";
import { lineSamples, lineSampleBudget } from "../lineSamples";
import { useAnalysisPreference } from "../analysisPreferences";
import TimelineExport from "./TimelineExport";
import { timelineOverlays } from "../timelineOverlays";
import { timelineHits } from "../timelineSelection";
import { mipLevel, mipMarks } from "../timelineMip";
import {
  discreteZoomRange,
  stepZoomRange,
  waveMipLevels,
} from "../timelineZoom";
import { timelineGlyphs } from "../timelineDetail";
import { paintTimelineGlyphs } from "../timelinePaint";
import { plotColor } from "../theme";
import RangeBound from "./RangeBound";
import { PlotAxesContext } from "../plotAxes";
import { axisBounds, axisFraction } from "../chartAxis";
import { PlotExport } from "./PlotExport";
import { canvasPlot, svgPlot } from "../plotExport";
import { useEffect, useLayoutEffect, useRef, useState, useId, useContext, useMemo } from "react";
import type { Range } from "../types";
import { fmt } from "../types";
import { insideRect } from "../timelineLayout";
import {
  laneHeights,
  parseTimelinePreferences,
  timelinePreferenceKey,
} from "../timelinePreferences";
import type { TimelinePreferences } from "../timelinePreferences";
import { boundedRange } from "../analysis";
export interface Mark {
  tokenCategory?: number;
  mipGroup?: string;
  shaderEngine?: number;
  overlayWidth?: number;
  recordKind?: "residency" | "dispatch" | "marker" | "event" | "activity";
  recordIndex?: number;
  aggregateCount?: number;
  zoomRange?: Range;
  start: number;
  end: number;
  lane: string;
  color: string;
  label: string;
  key: string;
  selected?: boolean;
  waveId?: string;
  tokenIndex?: number;
  tokenOccurrence?: number;
  instruction?: number;
  displaySlot?: number;
}
export function Timeline({
  marks,
  range,
  bounds,
  onRange,
  onSelect,
  unit = "cycles",
  height = 215,
  cursor,
  preferenceId = "default",
  preferences: controlledPreferences,
  onPreferences,
  exportLabel = "Timeline",
  loadExportMarks,
  baseClock,
}: {
  marks: Mark[];
  range: Range;
  bounds: Range;
  onRange: (r: Range) => void;
  onSelect?: (m: Mark) => void;
  unit?: string;
  height?: number;
  cursor?: number;
  preferenceId?: string;
  preferences?: TimelinePreferences;
  onPreferences?: (preferences: TimelinePreferences) => void;
  exportLabel?: string;
  loadExportMarks?: (range: Range, signal: AbortSignal) => Promise<Mark[]>;
  baseClock?: 1 | 4;
}) {
  const display = useContext(DisplayScaleContext);
  const canvas = useRef<HTMLCanvasElement>(null),
    host = useRef<HTMLDivElement>(null);
  const [localPreferences, setPreferences] = useState(() => {
    try {
      return parseTimelinePreferences(
        localStorage.getItem(timelinePreferenceKey(preferenceId)),
      );
    } catch {
      return parseTimelinePreferences(null);
    }
  });
  const preferences = controlledPreferences ?? localPreferences;
  const [saveError, setSaveError] = useState(false);
  const { stacked, laneHeight, detailPixels, zoomMode } = preferences;
  const updatePreferences = (patch: Partial<TimelinePreferences>) => {
    const next = { ...preferences, ...patch };
    setPreferences(next);
    onPreferences?.(next);
    try {
      localStorage.setItem(
        timelinePreferenceKey(preferenceId),
        JSON.stringify(next),
      );
      setSaveError(false);
    } catch {
      setSaveError(true);
    }
  };
  const hasSlots = marks.some((m) => m.displaySlot !== undefined);
  const [width, setWidth] = useState(900),
    [page, setPage] = useState(0),
    [hover, setHover] = useState<Mark | null>(null),
    [measurement, setMeasurement] = useState<Range | null>(null);
  const drag = useRef<{ x: number; range: Range; mode: string } | null>(null);
  const pointer = useRef<{ x: number; y: number } | null>(null);
  const upstreamLevel = baseClock ? mipLevel(range, width, baseClock) : 0;
  const upstreamMode = detailPixels === -1 && baseClock !== undefined;
  const displayMarks = useMemo(
    () => (upstreamMode ? mipMarks(marks, baseClock!, upstreamLevel) : marks),
    [marks, upstreamMode, baseClock, upstreamLevel],
  );
  const laneNames = useMemo(() => {
    const names = new Set<string>();
    for (const mark of displayMarks) names.add(mark.lane);
    return [...names];
  }, [displayMarks]);
  const perPage = Math.max(1, Math.floor((height - 27) / laneHeight));
  const safePage = Math.min(
    page,
    Math.max(0, Math.ceil(laneNames.length / perPage) - 1),
  );
  const lanes = laneNames.slice(safePage * perPage, (safePage + 1) * perPage),
    left = 155,
    span = range[1] - range[0];
  const glyphs = useMemo(
    () =>
      timelineGlyphs(
        displayMarks,
        lanes,
        range,
        width,
        laneHeight,
        stacked,
        upstreamMode ? 0 : Math.max(0, detailPixels),
        upstreamMode ? { level: upstreamLevel, base: baseClock! } : undefined,
      ),
    [
      displayMarks,
      upstreamMode,
      upstreamLevel,
      baseClock,
      JSON.stringify(lanes),
      range[0],
      range[1],
      width,
      laneHeight,
      stacked,
      detailPixels,
    ],
  );
  const overlays = useMemo(
    () => timelineOverlays(marks, lanes, range, width, laneHeight),
    [marks, JSON.stringify(lanes), range[0], range[1], width, laneHeight],
  );
  const overlayCount = new Set(overlays.map((g) => g.mark.key)).size;
  const [overlapChoices, setOverlapChoices] = useState<Mark[]>([]);
  const [overlapPage, setOverlapPage] = useState(0);
  useEffect(() => {
    setHover(null);
    setOverlapChoices([]);
    setOverlapPage(0);
    pointer.current = null;
  }, [glyphs, overlays]);
  const aggregateEvents = glyphs.reduce(
    (n, g) => n + (g.mark.aggregateCount || 1),
    0,
  );
  const toX = (t: number) =>
    left + ((t - range[0]) / span) * (width - left - 14);
  const toTime = (x: number) =>
    range[0] + ((x - left) / (width - left - 14)) * span;
  useEffect(() => {
    const observer = new ResizeObserver((entries) =>
      setWidth(entries[0].contentRect.width),
    );
    if (host.current) observer.observe(host.current);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    const el = canvas.current;
    if (!el) return;
    const ratio = display.ratio;
    el.width = width * ratio;
    el.height = height * ratio;
    const ctx = el.getContext("2d")!;
    ctx.scale(ratio, ratio);
    ctx.lineWidth = display.strokeScale;
    ctx.clearRect(0, 0, width, height);
    ctx.font = "11px ui-monospace, monospace";
    for (let i = 0; i <= 5; i++) {
      const x = left + (i * (width - left - 14)) / 5;
      ctx.strokeStyle = plotColor("--border");
      ctx.beginPath();
      ctx.moveTo(x, 25);
      ctx.lineTo(x, height);
      ctx.stroke();
      ctx.fillStyle = plotColor("--plot-muted");
      ctx.fillText(fmt(range[0] + (i * span) / 5), Math.min(x, width - 60), 15);
    }
    lanes.forEach((lane, i) => {
      ctx.fillStyle = plotColor(
        i % 2 ? "--plot-alternate" : "--plot-background",
      );
      ctx.fillRect(0, 27 + i * laneHeight, width, laneHeight);
      ctx.fillStyle = plotColor("--plot-muted");
      ctx.fillText(
        lane.slice(0, 23),
        12,
        27 + laneHeight / 2 + 4 + i * laneHeight,
      );
    });
    paintTimelineGlyphs(ctx, glyphs, overlays, () => plotColor("--plot-selection"));
    if (cursor !== undefined && cursor >= range[0] && cursor <= range[1]) {
      ctx.strokeStyle = plotColor("--accent");
      ctx.lineWidth = display.strokeScale;
      ctx.beginPath();
      ctx.moveTo(toX(cursor), 24);
      ctx.lineTo(toX(cursor), height);
      ctx.stroke();
    }
    if (measurement) {
      const a = toX(measurement[0]),
        b = toX(measurement[1]);
      ctx.fillStyle = "#eab66b33";
      ctx.fillRect(Math.min(a, b), 25, Math.abs(b - a), height - 25);
      ctx.strokeStyle = "#eab66b";
      ctx.strokeRect(Math.min(a, b), 25, Math.abs(b - a), height - 25);
    }
  });
  useEffect(() => {
    const element = canvas.current;
    if (!element) return;
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      if (event.deltaY === 0) return;
      if (zoomMode === "mip" && baseClock !== undefined) {
        onRange(
          stepZoomRange(
            range,
            bounds,
            width,
            baseClock,
            event.deltaY > 0 ? 1 : -1,
            (event.clientX - element.getBoundingClientRect().left - left) /
              Math.max(1, width - left - 14),
          ),
        );
        return;
      }
      const t = toTime(event.clientX - element.getBoundingClientRect().left),
        factor = event.deltaY > 0 ? 1.2 : 0.83;
      onRange(
        boundedRange(
          [t - (t - range[0]) * factor, t + (range[1] - t) * factor],
          bounds,
        ),
      );
    };
    element.addEventListener("wheel", wheel, { passive: false });
    return () => element.removeEventListener("wheel", wheel);
  }, [range, width, bounds, onRange, zoomMode, baseClock]);
  const hit = (x: number, y: number) =>
    overlays.findLast((g) => insideRect(g.rect, x, y))?.mark ||
    glyphs.findLast((g) => insideRect(g.rect, x, y))?.mark ||
    null;
  const selectMark = (m: Mark) => {
    setOverlapChoices([]);
    if (m.aggregateCount) {
      const [start, end] = m.zoomRange || [m.start, m.end];
      const padding = Math.max(1, (end - start) * 0.1);
      const next = boundedRange([start - padding, end + padding], bounds);
      if (next[1] - next[0] >= span) updatePreferences({ detailPixels: 0 });
      else onRange(next);
    } else onSelect?.(m);
  };
  const chooseOverlaps = () => {
    if (!pointer.current) return;
    setOverlapPage(0);
    setOverlapChoices(
      timelineHits(glyphs, overlays, pointer.current.x, pointer.current.y),
    );
  };
  return (
    <div className="timeline" ref={host}>
      <div className="source-remap-controls">
        <label>
          Zoom{" "}
          <select
            aria-label="Timeline zoom mode"
            value={zoomMode}
            onChange={(e) => {
              const next = e.target.value as "continuous" | "mip";
              updatePreferences({ zoomMode: next });
              if (next === "mip" && baseClock !== undefined)
                onRange(
                  discreteZoomRange(
                    range,
                    bounds,
                    width,
                    baseClock,
                    upstreamLevel,
                  ),
                );
            }}
          >
            <option value="continuous">Continuous</option>
            <option value="mip" disabled={baseClock === undefined}>
              Upstream mip steps
            </option>
          </select>
        </label>
        {zoomMode === "mip" && baseClock !== undefined && (
          <>
            <button
              aria-label="Zoom in one mip level"
              disabled={upstreamLevel <= -2}
              onClick={() =>
                onRange(stepZoomRange(range, bounds, width, baseClock, -1))
              }
            >
              Zoom in
            </button>
            <label>
              Mip level{" "}
              <select
                aria-label="Timeline mip level"
                value={upstreamLevel}
                onChange={(e) =>
                  onRange(
                    discreteZoomRange(
                      range,
                      bounds,
                      width,
                      baseClock,
                      Number(e.target.value),
                    ),
                  )
                }
              >
                {waveMipLevels.map((level) => (
                  <option key={level} value={level}>
                    {level}
                  </option>
                ))}
              </select>
            </label>
            <button
              aria-label="Zoom out one mip level"
              disabled={
                upstreamLevel >= 10 ||
                range[1] - range[0] >= bounds[1] - bounds[0]
              }
              onClick={() =>
                onRange(stepZoomRange(range, bounds, width, baseClock, 1))
              }
            >
              Zoom out
            </button>
            <span>
              Wheel zoom stays anchored at the pointer. Capture bounds limit the
              widest step.
            </span>
          </>
        )}
        {zoomMode === "mip" && baseClock === undefined && (
          <span>Unknown clock model · using continuous zoom.</span>
        )}
        <PlotExport
          format="png"
          label={exportLabel}
          generate={() => {
            if (!canvas.current) throw Error("Plot is unavailable");
            return canvasPlot(
              canvas.current,
              exportLabel,
              `Range ${range[0]}–${range[1]} ${unit} · lanes ${safePage * perPage + 1}–${Math.min((safePage + 1) * perPage, laneNames.length)} of ${laneNames.length} · ${display.mode} strokes at DPR ${display.ratio} · visible page · detail ${upstreamMode ? `upstream mip ${upstreamLevel}` : detailPixels > 0 ? `${detailPixels}px aggregation` : marks.some((mark) => mark.aggregateCount !== undefined) ? "overview groups" : "exact"} · ${aggregateEvents} events in ${glyphs.length} marks · ${overlayCount} decoder event overlays`,
            );
          }}
        />
        <TimelineExport
          label={exportLabel}
          unit={unit}
          bounds={bounds}
          range={range}
          stacked={stacked}
          load={loadExportMarks || (async () => marks)}
        />
        {!!overlayCount && (
          <span aria-label="Timeline overlay summary">
            {overlayCount} decoder events over matching SE rows
          </span>
        )}
        <button
          disabled={!pointer.current}
          onClick={chooseOverlaps}
          title="Choose records at the last pointer position; shortcut O while the timeline is focused"
        >
          Choose overlaps
        </button>
        <label>
          Detail{" "}
          <select
            aria-label="Timeline detail"
            value={detailPixels}
            onChange={(e) => {
              updatePreferences({ detailPixels: Number(e.target.value) });
              setHover(null);
            }}
          >
            <option value={0}>{marks.some((mark) => mark.aggregateCount !== undefined) ? "Overview groups" : "Exact events"}</option>
            {(baseClock !== undefined || detailPixels === -1) && (
              <option value={-1} disabled={baseClock === undefined}>
                Upstream mip · automatic
              </option>
            )}
            <option value={2}>Fine · 2 px</option>
            <option value={4}>Balanced · 4 px</option>
            <option value={8}>Overview · 8 px</option>
          </select>
        </label>
        <span aria-label="Timeline detail summary">
          {detailPixels === -1 &&
            !upstreamMode &&
            "Unknown clock model · using exact events · "}
          {upstreamMode && `Mip ${upstreamLevel} · ${baseClock}-clock base · `}
          {aggregateEvents.toLocaleString()} events ·{" "}
          {glyphs.length.toLocaleString()} drawn marks
          {detailPixels ? " · groups zoom on click" : ""}
        </span>
        <label>
          Lane height{" "}
          <select
            aria-label="Timeline lane height"
            value={laneHeight}
            onChange={(e) => {
              updatePreferences({ laneHeight: Number(e.target.value) });
              setHover(null);
            }}
          >
            {laneHeights.map((h, i) => (
              <option key={h} value={h}>
                {["Compact", "Normal", "Spacious", "Large"][i]}
              </option>
            ))}
          </select>
        </label>
        <span>
          W/S to increase/decrease lane height. Saved for this timeline view.
        </span>
        {saveError && (
          <span role="status">
            Preferences could not be saved; changes apply to this view only.
          </span>
        )}
      </div>
      {hasSlots && (
        <div className="source-remap-controls">
          <label>
            Token layout{" "}
            <select
              aria-label="Token layout"
              value={stacked ? "stacked" : "flat"}
              onChange={(e) =>
                updatePreferences({ stacked: e.target.value === "stacked" })
              }
            >
              <option value="stacked">Upstream equal-clock stacking</option>
              <option value="flat">Flat bars</option>
            </select>
          </label>
          <span>
            Four display slots separate simultaneous starts; additional
            collisions share slot 3. These are not hardware wave slots.
          </span>
        </div>
      )}
      <canvas
        ref={canvas}
        style={{ width: "100%", height }}
        aria-label="Interactive timeline. Drag to pan, scroll to zoom, Shift-drag to measure. Alt-click or O to choose overlapping records at the pointer."
        tabIndex={0}
        onContextMenu={(e) => e.preventDefault()}
        onKeyDown={(e) => {
          if (e.key === "Escape") setOverlapChoices([]);
          if (e.key.toLowerCase() === "o") {
            e.preventDefault();
            chooseOverlaps();
          }
          if (e.key.toLowerCase() === "w" || e.key.toLowerCase() === "s") {
            e.preventDefault();
            const index = laneHeights.indexOf(
              laneHeight as (typeof laneHeights)[number],
            );
            updatePreferences({
              laneHeight:
                laneHeights[
                  Math.max(
                    0,
                    Math.min(
                      laneHeights.length - 1,
                      index + (e.key.toLowerCase() === "w" ? 1 : -1),
                    ),
                  )
                ],
            });
            setHover(null);
          }
          if (e.key === "a" || e.key === "d") {
            const d = span * 0.15 * (e.key === "a" ? -1 : 1);
            onRange(boundedRange([range[0] + d, range[1] + d], bounds));
          }
        }}
        onPointerDown={(e) => {
          const x = e.nativeEvent.offsetX;
          drag.current = {
            x,
            range: [...range],
            mode: e.shiftKey || e.button === 2 ? "measure" : "pan",
          };
          e.currentTarget.setPointerCapture(e.pointerId);
          if (drag.current.mode === "measure")
            setMeasurement([toTime(x), toTime(x)]);
        }}
        onPointerMove={(e) => {
          const x = e.nativeEvent.offsetX;
          pointer.current = { x, y: e.nativeEvent.offsetY };
          setHover(hit(x, e.nativeEvent.offsetY));
          if (drag.current) {
            if (drag.current.mode === "measure")
              setMeasurement([toTime(drag.current.x), toTime(x)]);
            else {
              const d =
                ((drag.current.x - x) / (width - left - 14)) *
                (drag.current.range[1] - drag.current.range[0]);
              onRange(
                boundedRange(
                  [drag.current.range[0] + d, drag.current.range[1] + d],
                  bounds,
                ),
              );
            }
          }
        }}
        onPointerUp={(e) => {
          if (
            drag.current &&
            drag.current.mode === "pan" &&
            Math.abs(drag.current.x - e.nativeEvent.offsetX) < 3
          ) {
            if (e.altKey) {
              setOverlapPage(0);
              setOverlapChoices(
                timelineHits(
                  glyphs,
                  overlays,
                  e.nativeEvent.offsetX,
                  e.nativeEvent.offsetY,
                ),
              );
            } else {
              const m = hit(e.nativeEvent.offsetX, e.nativeEvent.offsetY);
              if (m) selectMark(m);
            }
          }
          drag.current = null;
        }}
        onPointerCancel={() => (drag.current = null)}
        onPointerLeave={() => setHover(null)}
      />
      {overlapChoices.length > 0 && (
        <section
          className="overlap-choices"
          aria-label="Overlapping timeline records"
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              setOverlapChoices([]);
              canvas.current?.focus();
            }
          }}
        >
          <div className="source-remap-controls">
            <strong>{overlapChoices.length} records at this point</strong>
            <span>
              Frontmost first. Choose a record; aggregated groups zoom in.
            </span>
            <button
              onClick={() => {
                setOverlapChoices([]);
                canvas.current?.focus();
              }}
            >
              Dismiss overlaps
            </button>
          </div>
          <ol start={overlapPage * 20 + 1}>
            {overlapChoices
              .slice(overlapPage * 20, overlapPage * 20 + 20)
              .map((mark) => (
                <li key={mark.key}>
                  <button onClick={() => selectMark(mark)}>
                    {mark.label} · {mark.start}–{mark.end} {unit}
                    {mark.waveId !== undefined
                      ? ` · wave ${mark.waveId} · token ${mark.tokenIndex}`
                      : ""}
                  </button>
                </li>
              ))}
          </ol>
          {overlapChoices.length > 20 && (
            <div className="source-remap-controls">
              <button
                disabled={overlapPage === 0}
                onClick={() => setOverlapPage((p) => p - 1)}
              >
                Previous overlap page
              </button>
              <span>
                Page {overlapPage + 1} / {Math.ceil(overlapChoices.length / 20)}
              </span>
              <button
                disabled={(overlapPage + 1) * 20 >= overlapChoices.length}
                onClick={() => setOverlapPage((p) => p + 1)}
              >
                Next overlap page
              </button>
            </div>
          )}
        </section>
      )}
      <div className="plot-footer">
        <span>
          {hover
            ? `${hover.label} · ${fmt(hover.end - hover.start)} ${unit}`
            : measurement
              ? `Measured ${fmt(Math.abs(measurement[1] - measurement[0]))} ${unit}`
              : "Scroll to zoom · drag to pan · Shift-drag to measure · Alt-click / O for overlaps · A/D to pan"}
        </span>
        <span>
          <button aria-label="Previous timeline lane page" disabled={!safePage} onClick={() => setPage(safePage - 1)}>
            ‹
          </button>{" "}
          {laneNames.length
            ? `${safePage * perPage + 1}–${Math.min((safePage + 1) * perPage, laneNames.length)} / ${laneNames.length} lanes`
            : "No recorded lanes"}{" "}
          <button
            aria-label="Next timeline lane page"
            disabled={(safePage + 1) * perPage >= laneNames.length}
            onClick={() => setPage(safePage + 1)}
          >
            ›
          </button>
        </span>
      </div>
    </div>
  );
}
export function LineChart({
  points,
  range: workspaceRange,
  xBounds,
  color = "#7cc4a4",
  label,
  height: fixedHeight,
  step = false,
  axisKey,
}: {
  points: [number, number][];
  range: Range;
  xBounds: Range;
  color?: string;
  label: string;
  height?: number;
  cursor?: number;
  step?: boolean;
  axisKey: string;
}) {
  const display = useContext(DisplayScaleContext);
  const svg = useRef<SVGSVGElement>(null);
  const host = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(900);
  useLayoutEffect(() => {
    const element = host.current;
    if (!element) return;
    const resize = () => setWidth(Math.max(1, Math.round(element.getBoundingClientRect().width - 30)));
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const height = fixedHeight ?? Math.round(Math.max(180, Math.min(320, width / 6)));
  const [lodBias, setLodBias] = useAnalysisPreference<number>(
    "plot.lodBias",
    0,
  );
  const clipId = "plot-" + useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const axes = useContext(PlotAxesContext);
  const storedRange = axes?.ranges[axisKey];
  const [rangeError, setRangeError] = useState("");
  let range: Range = storedRange || workspaceRange;
  const clamped: Range = [
    Math.max(xBounds[0], range[0]),
    Math.min(xBounds[1], range[1]),
  ];
  const adjusted =
    !!storedRange && (clamped[0] !== range[0] || clamped[1] !== range[1]);
  if (storedRange) range = clamped[1] > clamped[0] ? clamped : xBounds;
  function changePlotRange(next: Range | null) {
    try {
      axes?.changeRange(axisKey, next);
      setRangeError("");
    } catch (error) {
      setRangeError(String(error));
    }
  }
  const manualAxis = axes?.axes[axisKey] || null;
  function setManualAxis(bounds: Range | null) {
    axes?.change(axisKey, bounds);
  }
  const [axisLow, setAxisLow] = useState("");
  const [axisHigh, setAxisHigh] = useState("");
  const [axisError, setAxisError] = useState("");
  useEffect(() => {
    setAxisLow(manualAxis ? String(manualAxis[0]) : "");
    setAxisHigh(manualAxis ? String(manualAxis[1]) : "");
    setAxisError("");
  }, [axisKey, manualAxis?.[0], manualAxis?.[1]]);
  const selected = points.filter((p) => p[0] >= range[0] && p[0] <= range[1]);
  const preceding = points.findLast((p) => p[0] < range[0]);
  if (step && preceding) selected.unshift([range[0], preceding[1]]);
  if (step && selected.length)
    selected.push([range[1], selected[selected.length - 1][1]]);
  const left = 48,
    right = 12,
    top = 14,
    bottom = 25;
  let max = 1;
  for (const p of selected)
    if (Number.isFinite(p[1])) max = Math.max(max, p[1]);
  let min = 0;
  for (const point of selected)
    if (Number.isFinite(point[1])) min = Math.min(min, point[1]);
  if (manualAxis) [min, max] = manualAxis;
  const x = (n: number) =>
      left + ((n - range[0]) / (range[1] - range[0])) * (width - left - right),
    y = (n: number) =>
      height - bottom - axisFraction(n, min, max) * (height - top - bottom);
  const { points: samples, limited: fragmented } = lineSamples(
    selected,
    lodBias,
  );
  const drawable = (p: [number, number]) =>
    Number.isFinite(p[1]) &&
    Number.isFinite(x(p[0])) &&
    Number.isFinite(y(p[1]));
  const unrepresentable = samples.some(
    (p) => Number.isFinite(p[1]) && !drawable(p),
  );
  const path = samples
    .map((p, i) =>
      !drawable(p)
        ? ""
        : i === 0 || !drawable(samples[i - 1])
          ? `M${x(p[0])},${y(p[1])}`
          : step
            ? `H${x(p[0])}V${y(p[1])}`
            : `L${x(p[0])},${y(p[1])}`,
    )
    .join(" ");
  return (
    <div className="line-plot" ref={host}>
      <details className="plot-lod">
        <summary>Line detail: LOD bias {lodBias}</summary>
        <label>
          LOD bias
          <input
            type="range"
            aria-label="LOD bias"
            min={-20}
            max={20}
            step={1}
            value={lodBias}
            onChange={(event) => setLodBias(Number(event.target.value))}
          />
          <span aria-hidden="true">{lodBias}</span>
        </label>
        <button onClick={() => setLodBias(0)}>Reset line detail</button>
        <p className="muted small">
          Applies to all line plots in this capture. Negative values show finer
          detail; positive values reduce detail. A target of{" "}
          {lineSampleBudget(lodBias).toLocaleString()} sample buckets retains
          endpoints and extrema, with at least one bucket per segment; gaps stay
          separate. Fine detail is capped at 8,000 buckets. Recorded data and
          axis bounds are unchanged.
        </p>
      </details>
      <details className="plot-x-range">
        <summary>
          X range: {storedRange ? "independent" : "workspace"} · {fmt(range[0])}
          –{fmt(range[1])}
        </summary>
        <div className="source-remap-controls">
          <label>
            <input
              type="checkbox"
              checked={!storedRange}
              onChange={(e) =>
                changePlotRange(e.target.checked ? null : [...range])
              }
            />{" "}
            Follow workspace range
          </label>
          {storedRange && (
            <>
              <RangeBound
                label={`Plot range for ${label}`}
                edge={0}
                range={range}
                bounds={xBounds}
                onCommit={changePlotRange}
              />
              <span>→</span>
              <RangeBound
                label={`Plot range for ${label}`}
                edge={1}
                range={range}
                bounds={xBounds}
                onCommit={changePlotRange}
              />
              <button onClick={() => changePlotRange(xBounds)}>
                Full plot range
              </button>
            </>
          )}
        </div>
        {adjusted && (
          <p className="muted small">
            Saved range adjusted to the available domain.
          </p>
        )}
        {rangeError && <p role="alert">{rangeError}</p>}
      </details>
      <div className="source-remap-controls">
        <details className="plot-y-axis">
          <summary>
            Y axis: {manualAxis ? "custom" : "automatic"} · {fmt(min)}–
            {fmt(max)}
          </summary>
          <form
            className="source-remap-controls"
            onSubmit={(event) => {
              event.preventDefault();
              try {
                const bounds = axisBounds(axisLow, axisHigh);
                if (
                  selected.some(
                    (p) =>
                      Number.isFinite(p[1]) &&
                      !Number.isFinite(
                        axisFraction(p[1], bounds[0], bounds[1]),
                      ),
                  )
                )
                  throw Error("Axis span is too small for these samples");
                setManualAxis(bounds);
                setAxisError("");
              } catch (e) {
                setAxisError(String(e));
              }
            }}
          >
            <label>
              Y minimum{" "}
              <input
                aria-label={`Y minimum for ${label}`}
                type="number"
                step="any"
                placeholder={String(min)}
                value={axisLow}
                onChange={(e) => setAxisLow(e.target.value)}
              />
            </label>
            <label>
              Y maximum{" "}
              <input
                aria-label={`Y maximum for ${label}`}
                type="number"
                step="any"
                placeholder={String(max)}
                value={axisHigh}
                onChange={(e) => setAxisHigh(e.target.value)}
              />
            </label>
            <button type="submit">Apply axis bounds</button>
            <button
              type="button"
              onClick={() => {
                setManualAxis(null);
                setAxisLow("");
                setAxisHigh("");
                setAxisError("");
              }}
            >
              Automatic axis
            </button>
            {axisError && <span role="alert">{axisError}</span>}
          </form>
        </details>
        <PlotExport
          format="svg"
          label={label}
          generate={() => {
            if (!svg.current) throw Error("Plot is unavailable");
            return svgPlot(
              svg.current,
              label,
              `Visible range ${range[0]}–${range[1]} (${storedRange ? "independent" : "workspace"}) · Y ${min}–${max} (${manualAxis ? "custom" : "automatic"}) · ${display.mode} strokes at DPR ${display.ratio} · LOD bias ${lodBias} · displayed samples; out-of-axis values clipped${unrepresentable ? "; samples with unrepresentable coordinates omitted" : ""}`,
            );
          }}
        />
      </div>
      {unrepresentable && (
        <p role="alert">
          Some samples cannot be represented at these axis bounds. Widen the
          bounds or select Automatic axis.
        </p>
      )}
      <svg
        ref={svg}
        className="line-chart"
        style={{ height: height + 5 }}
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label={label}
        preserveAspectRatio="none"
      >
        <defs>
          <clipPath id={clipId}>
            <rect
              x={left}
              y={top}
              width={width - left - right}
              height={height - top - bottom}
            />
          </clipPath>
        </defs>
        {[0, 0.5, 1].map((n) => (
          <g key={n}>
            <line
              x1={left}
              x2={width - right}
              y1={y(min * (1 - n) + max * n)}
              y2={y(min * (1 - n) + max * n)}
              stroke="var(--border)"
              strokeDasharray="3 5"
              strokeWidth={display.strokeScale}
              vectorEffect="non-scaling-stroke"
            />
            <text x={5} y={y(min * (1 - n) + max * n) + 4}>
              {fmt(min * (1 - n) + max * n)}
            </text>
          </g>
        ))}
        {samples.length > 0 && (
          <>
            <path
              d={path}
              stroke={color}
              strokeWidth={1.8 * display.strokeScale}
              vectorEffect="non-scaling-stroke"
              fill="none"
              clipPath={`url(#${clipId})`}
            />
          </>
        )}
        {[0, 0.25, 0.5, 0.75, 1].map((n) => (
          <text
            key={n}
            x={Math.min(width - 60, left + n * (width - left - right))}
            y={height - 5}
          >
            {fmt(range[0] + n * (range[1] - range[0]))}
          </text>
        ))}
        {fragmented && (
          <text x={left} y={height / 2}>
            More than 500 observed segments. Narrow the X range to draw samples.
          </text>
        )}
        {samples.map((point, index) =>
          drawable(point) &&
          (index === 0 || !drawable(samples[index - 1])) &&
          (index + 1 === samples.length || !drawable(samples[index + 1])) ? (
            <circle
              key={index}
              cx={x(point[0])}
              cy={y(point[1])}
              r="2.5"
              fill={color}
              clipPath={`url(#${clipId})`}
            />
          ) : null,
        )}
        {!selected.length && (
          <text x={width / 2 - 65} y={height / 2}>
            No samples in range
          </text>
        )}
      </svg>
    </div>
  );
}
