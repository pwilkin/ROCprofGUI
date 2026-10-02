import CodeObjects from "./CodeObjects";
import { defaultWaveStateDisplay, isWaveStateSeries } from "../waveStateDisplay";
import { topHotspots } from "../hotspots";
import TraceAssociations from "./TraceAssociations";
import { counterGaps } from "../counterGaps";
import { counterDetail } from "../counterMetadata";
import {
  sourceHierarchy,
  clipSourceHierarchy,
  codeObjectLabel,
  type FlameGrouping,
} from "../sourceHierarchy";
import { useAnalysisPreference } from "../analysisPreferences";
import HostCalls from "./HostCalls";
import MonitorIntervals from "./MonitorIntervals";
import { clipWeightedInterval, fitWeightedRange } from "../weightedViewport";
import EventCounters from "./EventCounters";
import {
  scalarDefinition,
  scalarSeries,
  type ScalarDefinition,
} from "../scalarCounters";
import RangeBound from "./RangeBound";
import { PlotAxesContext, plotAxisKey } from "../plotAxes";
import { PlotExport } from "./PlotExport";
import { boxPlot, type PlotBox } from "../plotExport";
import { useContext, useMemo, useState } from "react";
import {
  BarChart3,
  ChevronRight,
  Cpu,
  Database,
  Info,
  Layers,
  Search,
} from "lucide-react";
import type { Capture, Instruction, Range, Wave } from "../types";
import { fmt, colors } from "../types";
import { kernelStats, occupancy, union } from "../analysis";
import { isInternalKernel } from "../kernelName";
import { LineChart } from "./Plots";
import TensorCounters from "./TensorCounters";
export function Empty({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="empty">
      <Layers size={30} />
      <h3>{title}</h3>
      <p>{detail}</p>
    </div>
  );
}
export function Panel({
  title,
  detail,
  children,
  tools,
}: {
  title: string;
  detail?: string;
  children: React.ReactNode;
  tools?: React.ReactNode;
}) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h3>{title}</h3>
          {detail && <p>{detail}</p>}
        </div>
        {tools}
      </div>
      {children}
    </section>
  );
}
export function OccupancyView({
  capture,
  range,
}: {
  capture: Capture;
  range: Range;
}) {
  const ses = [...new Set(capture.intervals.map((i) => i.se))];
  const kernels = [...new Set(capture.intervals.map((i) => i.kernel))];
  return (
    <>
      {ses.length ? (
        ses.map((se, i) => (
          <Panel
            key={se}
            title={`Shader engine ${se}`}
            detail="Concurrent recorded waves · not occupancy as a percentage of hardware capacity"
          >
            <LineChart
              xBounds={[capture.start, capture.end]}
              axisKey={plotAxisKey("occupancy-se", se)}
              points={occupancy(capture, se)}
              range={range}
              label={`Shader engine ${se} occupancy`}
              color={colors[i + 3]}
              step
            />
          </Panel>
        ))
      ) : (
        <Empty
          title="Occupancy was not collected"
          detail="Open a decoded SQTT capture containing occupancy.json. Kernel dispatch durations do not measure wave occupancy."
        />
      )}
      {kernels.map((kernel, i) => (
        <Panel
          key={kernel}
          title={kernel}
          detail="Kernel dispatch occupancy · concurrent waves"
        >
          <LineChart
            xBounds={[capture.start, capture.end]}
            axisKey={plotAxisKey("occupancy-kernel", kernel)}
            points={occupancy(capture, undefined, kernel)}
            range={range}
            label={kernel}
            color={colors[(i + 5) % colors.length]}
            step
          />
        </Panel>
      ))}
    </>
  );
}
export function HardwareView({
  capture,
  range,
}: {
  capture: Capture;
  range: Range;
}) {
  const [cursor, setCursor] = useAnalysisPreference<number>(
      "hardware.cursor",
      0.45,
    ),
    [selected, setSelected] = useAnalysisPreference<string>(
      "hardware.selected",
      "0:2",
    );
  const time = range[0] + cursor * (range[1] - range[0]);
  const coords = [
    ...new Set(capture.intervals.map((i) => `${i.se}:${i.cu}`)),
  ].sort(
    (a, b) =>
      Number(a.split(":")[0]) - Number(b.split(":")[0]) ||
      Number(a.split(":")[1]) - Number(b.split(":")[1]),
  );
  const counts = new Map<string, number>();
  for (const i of capture.intervals)
    if (i.start <= time && i.end > time) {
      const key = `${i.se}:${i.cu}`;
      counts.set(key, (counts.get(key) || 0) + 1);
    }
  const selectedCoord = coords.includes(selected) ? selected : coords[0];
  const [se, cu] = (selectedCoord || "0:0").split(":").map(Number);
  const simds = [
    ...new Set(
      capture.intervals
        .filter((i) => i.se === se && i.cu === cu)
        .map((i) => i.simd),
    ),
  ].sort();
  const activeKernels = [
    ...new Set(
      capture.intervals
        .filter(
          (i) => i.se === se && i.cu === cu && i.start <= time && i.end > time,
        )
        .map((i) => i.kernel),
    ),
  ];
  const kernel = activeKernels.length === 1 ? activeKernels[0] : undefined;
  const resourceMatches = kernel
    ? capture.dispatches.filter(
        (d) =>
          d.kernel === kernel &&
          d.agent === `SE ${se}` &&
          d.start <= time &&
          d.end > time,
      )
    : [];
  const resource =
    resourceMatches.length === 1 ? resourceMatches[0] : undefined;
  const resourceContext =
    activeKernels.length > 1
      ? "Multiple active kernels; resource identity is ambiguous"
      : kernel
        ? `${kernel}${resourceMatches.length > 1 ? " · multiple matching dispatches; resources unavailable" : ""}`
        : "No active kernel at this cycle";
  return (
    <>
      <Panel
        title="Compute fabric"
        detail="Only CUs represented in the capture are shown. Color indicates recorded active waves, not percent utilization."
        tools={
          <>
            <PlotExport
              format="svg"
              label="Compute fabric"
              generate={() => {
                const boxes = coords.map((key, n) => ({
                  x: 10 + (n % 8) * 122,
                  y: 10 + Math.floor(n / 8) * 48,
                  width: 116,
                  height: 40,
                  color: key === selectedCoord ? "#e4ad7c" : "#7cc4a4",
                  label: `${key}: ${counts.get(key) || 0}`,
                  detail: `SE:CU ${key}: ${counts.get(key) || 0} recorded active waves`,
                }));
                return boxPlot(
                  boxes,
                  1000,
                  Math.max(60, Math.ceil(coords.length / 8) * 48 + 20),
                  `${capture.name} · Compute fabric`,
                  `${capture.architecture} · ${time} ${capture.unit} · SE:CU and active waves; untraced CUs unknown, not percent utilization`,
                );
              }}
            />
            <span className="badge">{capture.architecture}</span>
          </>
        }
      >
        <div className="scrubber">
          <span>
            Inspect at{" "}
            <b>
              {fmt(time)} {capture.unit}
            </b>
          </span>
          <input
            aria-label="Hardware inspection cycle"
            type="range"
            min="0"
            max="1"
            step=".001"
            value={cursor}
            onChange={(e) => setCursor(Number(e.target.value))}
          />
        </div>
        {coords.length ? (
          <div className="fabric">
            {[...new Set(coords.map((k) => k.split(":")[0]))].map((engine) => (
              <div className="engine" key={engine}>
                <div className="eyebrow">SHADER ENGINE {engine}</div>
                <div className="cu-grid">
                  {coords
                    .filter((k) => k.startsWith(engine + ":"))
                    .map((key) => (
                      <button
                        key={key}
                        className={`cu ${selectedCoord === key ? "selected" : ""}`}
                        style={{
                          background: `rgba(117,197,158,${0.04 + (counts.get(key) || 0) / 32})`,
                        }}
                        onClick={() => setSelected(key)}
                      >
                        <Cpu size={17} />
                        <b>CU {key.split(":")[1]}</b>
                        <span>{counts.get(key) || 0} waves</span>
                      </button>
                    ))}
                </div>
              </div>
            ))}
          </div>
        ) : (
          <Empty
            title="No CU mapping"
            detail="This capture has no global wave residency events. Dispatch traces alone cannot place work on individual CUs."
          />
        )}
      </Panel>
      <div className="two-column">
        <Panel
          title={`CU ${cu} · SIMD residency`}
          detail="Recorded active slots at the selected cycle"
          tools={
            <PlotExport
              format="svg"
              label="SIMD residency"
              generate={() => {
                const boxes: PlotBox[] = [];
                let y = 10;
                for (const simd of simds) {
                  const slots = capture.intervals.filter(
                    (i) =>
                      i.se === se &&
                      i.cu === cu &&
                      i.simd === simd &&
                      i.start <= time &&
                      i.end > time,
                  );
                  boxes.push({
                    x: 10,
                    y,
                    width: 160,
                    height: 28,
                    color: "#adb8c7",
                    label: `SIMD ${simd}`,
                  });
                  if (!slots.length)
                    boxes.push({
                      x: 180,
                      y,
                      width: 760,
                      height: 28,
                      color: "#adb8c7",
                      label: "No active waves recorded",
                    });
                  slots.forEach((i, n) =>
                    boxes.push({
                      x: 180 + (n % 8) * 94,
                      y: y + Math.floor(n / 8) * 32,
                      width: 88,
                      height: 28,
                      color: "#7cc4a4",
                      label: `slot ${i.slot}`,
                      detail: `SIMD ${simd} slot ${i.slot} · ${i.kernel} · interval ${i.start}–${i.end} ${capture.unit}`,
                    }),
                  );
                  y += Math.max(1, Math.ceil(slots.length / 8)) * 32 + 8;
                }
                if (!simds.length)
                  boxes.push({
                    x: 10,
                    y,
                    width: 940,
                    height: 28,
                    color: "#adb8c7",
                    label: "No recorded SIMD mapping",
                  });
                return boxPlot(
                  boxes,
                  960,
                  y + 40,
                  `${capture.name} · SE ${se} CU ${cu} SIMD residency`,
                  `${time} ${capture.unit} · recorded active slots only; untraced slots and missing boundaries unknown; hardware capacity not inferred`,
                );
              }}
            />
          }
        >
          {simds.map((simd) => {
            const active = capture.intervals.filter(
              (i) =>
                i.se === se &&
                i.cu === cu &&
                i.simd === simd &&
                i.start <= time &&
                i.end > time,
            );
            return (
              <div className="simd" key={simd}>
                <span>SIMD {simd}</span>
                <div>
                  {active.length ? (
                    active.map((i, n) => (
                      <span className="wave-slot" key={n}>
                        slot {i.slot}
                      </span>
                    ))
                  ) : (
                    <span className="muted">No active waves recorded</span>
                  )}
                </div>
              </div>
            );
          })}
          <div className="notice">
            <Info size={14} />
            Untraced slots and missing boundaries are unknown. This diagram does
            not infer hardware capacity.
          </div>
        </Panel>
        <Panel
          title="Kernel resources"
          detail={resourceContext}
          tools={
            <PlotExport
              format="svg"
              label="Kernel resources"
              generate={() => {
                const rows = [
                  ["VGPR metadata", resource?.vgpr, "registers"],
                  ["SGPR metadata", resource?.sgpr, "registers"],
                  ["LDS allocation", resource?.lds, "bytes / workgroup"],
                  ["Private scratch", resource?.scratch, "bytes"],
                ] as const;
                return boxPlot(
                  rows.map(([label, value, unit], n) => ({
                    x: 10,
                    y: 10 + n * 36,
                    width: 940,
                    height: 28,
                    color: value == null ? "#adb8c7" : "#7cc4a4",
                    label: `${label}: ${value == null ? "Not collected" : `${value} ${unit}`}`,
                  })),
                  960,
                  165,
                  `${capture.name} · SE ${se} CU ${cu} kernel resources`,
                  `${time} ${capture.unit} · ${resourceContext} · static metadata; VGPR count is not spill count; scratch can include private objects; bank conflicts and occupancy limits not collected`,
                );
              }}
            />
          }
        >
          <div className="resource-list">
            <Resource
              label="VGPR metadata"
              value={resource?.vgpr}
              suffix=" registers"
            />
            <Resource
              label="SGPR metadata"
              value={resource?.sgpr}
              suffix=" registers"
            />
            <Resource
              label="LDS allocation"
              value={resource?.lds}
              suffix=" bytes / workgroup"
            />
            <Resource
              label="Private scratch"
              value={resource?.scratch}
              suffix=" bytes"
            />
          </div>
          <div className="notice">
            VGPR count is not a spill count. Scratch allocation may contain
            private objects as well as spills.
          </div>
        </Panel>
      </div>
      <Panel
        title="Local data share"
        detail="Allocation, accesses, and bank conflicts are separate measurements"
      >
        <div className="lds-layout">
          <div className="lds-icon">
            <Database size={37} />
            <strong>
              {resource?.lds == null
                ? "Not collected"
                : `${fmt(resource.lds)} B`}
            </strong>
            <span>static allocation / workgroup</span>
          </div>
          <div className="lds-explanation">
            <h4>Bank conflicts</h4>
            <p>
              Not collected. A bank-level heatmap requires address
              instrumentation or a validated, architecture-specific counter.
              SQ_INSTS_LDS and in-flight LDS instructions do not identify
              conflicting banks.
            </p>
            <h4>Occupancy limits</h4>
            <p>
              Hardware allocation granularity, wave size, register-file capacity
              and LDS capacity are required to calculate theoretical residency.
              Those limits are not guessed from the GPU name.
            </p>
          </div>
        </div>
      </Panel>
    </>
  );
}
function Resource({
  label,
  value,
  suffix,
}: {
  label: string;
  value?: number | null;
  suffix: string;
}) {
  return (
    <div>
      <span>{label}</span>
      <strong>
        {value == null ? "Not collected" : `${fmt(value)}${suffix}`}
      </strong>
    </div>
  );
}
export function CountersView({
  capture,
  range,
  definitions,
  onDefinitions,
  onDispatch,
}: {
  capture: Capture;
  range: Range;
  definitions: ScalarDefinition[];
  onDefinitions: (definitions: ScalarDefinition[]) => void;
  onDispatch: (dispatch: import("../types").Dispatch) => void;
}) {
  const [showWaveStates, setShowWaveStates] = useAnalysisPreference<boolean>(
    "counters.waveStates", defaultWaveStateDisplay(capture),
  );
  const hasWaveStates = capture.counters.some(isWaveStateSeries);
  const [search, setSearch] = useAnalysisPreference<string>(
      "counters.search",
      "",
    ),
    [formula, setFormula] = useAnalysisPreference<string>(
      "counters.formula",
      "C0 / max(C1, 1)",
    ),
    [name, setName] = useAnalysisPreference<string>(
      "counters.name",
      "Derived ratio",
    ),
    [error, setError] = useState("");
  const { gaps, gapError } = useMemo(() => {
    try {
      return { gaps: counterGaps(capture.counters), gapError: "" };
    } catch (error) {
      return {
        gaps: new Map(),
        gapError: `SPM gap projection unavailable: ${String(error)}`,
      };
    }
  }, [capture]);
  const results = useMemo(() => {
    const budget = { remaining: 2_000_000 };
    return definitions.map((definition) => {
      try {
        return {
          series: scalarSeries(definition, capture.counters, budget),
          error: "",
        };
      } catch (error) {
        return { series: null, error: String(error) };
      }
    });
  }, [definitions, capture]);
  function add() {
    try {
      if (definitions.length >= 64)
        throw Error("Limit of 64 scalar definitions per capture reached");
      const definition = scalarDefinition(name, formula, capture.counters);
      scalarSeries(definition, capture.counters);
      onDefinitions([...definitions, definition]);
      setError("");
    } catch (error) {
      setError(String(error));
    }
  }
  return (
    <>
      {gapError && <p role="alert">{gapError}</p>}
      <Panel
        title="Counter expressions"
        detail="Arithmetic + − × ÷, min(), max(), abs(). Exact timestamps and identical scopes only; no evaluation of code."
      >
        <div className="formula-row">
          <input
            aria-label="Derived counter name"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <input
            className="mono"
            aria-label="Counter expression"
            value={formula}
            onChange={(e) => setFormula(e.target.value)}
          />
          <button className="primary" onClick={add}>
            Add counter
          </button>
        </div>
        {error && (
          <p role="alert" className="error-text">
            {error}
          </p>
        )}
        <p className="muted small">
          Counter units are preserved as supplied. Convert cycles and quadcycles
          explicitly; a wave-cycle ratio is not wall-time utilization.
        </p>
      </Panel>
      <EventCounters capture={capture} onDispatch={onDispatch} />
      <MonitorIntervals capture={capture} range={range} />
      <TensorCounters key={capture.id} capture={capture} range={range} />
      {hasWaveStates && (
        <div className="summary-note">
          <label>
            <input type="checkbox" checked={showWaveStates}
              onChange={(event) => setShowWaveStates(event.target.checked)} />
            Show precomputed wave states
          </label>
          <p>Defaults off for RDNA traces and on otherwise. This display preference
            does not change imported samples or counter-expression identifiers.
            Wave-state availability depends on the recorded data.</p>
        </div>
      )}
      <div className="search-box">
        <Search size={15} />
        <input
          aria-label="Filter counters"
          placeholder="Filter counters by name or scope…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <span>{capture.counters.length} raw series</span>
      </div>
      {results.map(
        (result, index) =>
          result.error && (
            <Panel
              key={`error-${index}`}
              title={definitions[index].name}
              detail={result.error}
              tools={
                <button
                  onClick={() =>
                    onDefinitions(definitions.filter((_, n) => n !== index))
                  }
                >
                  Remove
                </button>
              }
            >
              <p role="alert">
                Saved definition could not be evaluated against this capture.
              </p>
            </Panel>
          ),
      )}
      {[
        ...capture.counters.map((s, i) => ({ s, i, definition: -1 })),
        ...results.flatMap((result, definition) =>
          result.series
            ? [
                {
                  s: result.series,
                  i: capture.counters.length + definition,
                  definition,
                },
              ]
            : [],
        ),
      ]
        .filter(({ s, definition }) => showWaveStates || definition >= 0 || !isWaveStateSeries(s))
        .filter(({ s }) =>
          (s.name + s.scope).toLowerCase().includes(search.toLowerCase()),
        )
        .map(({ s, i, definition }) => (
          <Panel
            key={i}
            title={`${i < capture.counters.length ? `C${i} · ` : ""}${s.name}`}
            detail={`${counterDetail(s)}${gaps.has(s) ? ` · ${gaps.get(s)!.missing} unobserved peer timestamps between samples; line breaks do not establish data loss` : ""}`}
            tools={
              i >= capture.counters.length ? (
                <button
                  onClick={() =>
                    onDefinitions(
                      definitions.filter((_, j) => j !== definition),
                    )
                  }
                >
                  Remove
                </button>
              ) : undefined
            }
          >
            {s.definition && (
              <details className="counter-definition">
                <summary>Recorded counter definition</summary>
                <dl>
                  <dt>Counter ID</dt>
                  <dd>{s.definition.id}</dd>
                  <dt>Type</dt>
                  <dd>
                    {s.definition.isDerived === null
                      ? "Unspecified"
                      : s.definition.isDerived
                        ? "Derived"
                        : "Hardware"}
                  </dd>
                  <dt>Constant</dt>
                  <dd>
                    {s.definition.isConstant === null
                      ? "Unspecified"
                      : s.definition.isConstant
                        ? "Yes"
                        : "No"}
                  </dd>
                  <dt>Hardware block</dt>
                  <dd>{s.definition.block || "Unspecified"}</dd>
                  <dt>Description</dt>
                  <dd>{s.definition.description || "Not supplied"}</dd>
                  <dt>Expression</dt>
                  <dd>
                    <code>{s.definition.expression || "Not supplied"}</code>
                  </dd>
                </dl>
              </details>
            )}
            <LineChart
              xBounds={[capture.start, capture.end]}
              axisKey={plotAxisKey(
                i < capture.counters.length ? "raw-counter" : "derived-counter",
                s.name,
                s.scope,
                s.unit,
              )}
              points={gaps.get(s)?.points ?? s.points}
              range={range}
              label={s.name}
              color={colors[(i + 3) % colors.length]}
            />
          </Panel>
        ))}
      {!capture.counters.length && (
        <Empty
          title="No time-series counters in this capture"
          detail="Import ATT perfcounter sidecars or open a rocprofv3 SPM JSON. Missing counters are not zero."
        />
      )}
    </>
  );
}
export function HotspotView({
  bins,
  onBins,
  instructions,
  onSelect,
  onSelectGroup,
  exportContext = "Selected instruction weights",
}: {
  bins: number;
  onBins: (bins: number) => void;
  instructions: Instruction[];
  onSelect: (id: number) => void;
  onSelectGroup: (ids: number[]) => void;
  exportContext?: string;
}) {
  const plots = useContext(PlotAxesContext);
  const rangeKey = plotAxisKey("histogram", "mean-latency");
  const stored = plots?.ranges[rangeKey];
  const rows = useMemo(() => instructions.filter((i) => i.hits > 0), [instructions]);
  const expensive = useMemo(() => topHotspots(instructions), [instructions]);
  const maximumLatency = Math.max(1, expensive[0]?.latency ?? 0);
  let automaticMaximum = 1;
  for (const i of rows)
    automaticMaximum = Math.max(automaticMaximum, i.latency / i.hits);
  const domain: Range = [0, Number.MAX_SAFE_INTEGER];
  const candidate: Range = stored
    ? [Math.max(0, stored[0]), Math.min(domain[1], stored[1])]
    : [0, automaticMaximum];
  const histogramRange: Range =
    candidate[0] < candidate[1] ? candidate : [0, automaticMaximum];
  const [minimum, max] = histogramRange;
  const width = max - minimum;
  const hist = Array.from({ length: bins }, (_, i) => ({
    low: minimum + (i * width) / bins,
    high: minimum + ((i + 1) * width) / bins,
    total: 0,
    rows: [] as Instruction[],
  }));
  for (const i of rows) {
    if (i.latency / i.hits < minimum || i.latency / i.hits > max) continue;
    const b =
      hist[
        Math.min(
          bins - 1,
          Math.floor(((i.latency / i.hits - minimum) / width) * bins),
        )
      ];
    b.total += i.latency;
    b.rows.push(i);
  }
  const peak = Math.max(1, ...hist.map((b) => b.total));
  return (
    <>
      <Panel
        title="Instruction cost distribution"
        detail="Accumulated latency grouped by mean instruction cost. Idle excluded; concurrent waits can be overrepresented."
        tools={
          <div className="histogram-options">
            <PlotExport
              format="svg"
              label="Instruction cost distribution"
              generate={() =>
                boxPlot(
                  hist.flatMap((b, n) => {
                    const h = (200 * b.total) / peak,
                      x = 40 + (n * 880) / bins;
                    return [
                      {
                        x,
                        y: 210 - h,
                        width: 880 / bins - 2,
                        height: h,
                        color: "#7cc4a4",
                        label: "",
                        detail: `${b.low}–${b.high} cycles/issue: ${b.total} accumulated cycles; ${b.rows.length} ISA rows`,
                      },
                      {
                        x,
                        y: 215,
                        width: 880 / bins - 2,
                        height: 24,
                        color: "#adb8c7",
                        label: fmt(b.low),
                      },
                    ];
                  }),
                  960,
                  250,
                  "Instruction cost distribution",
                  `${exportContext} · ${stored ? "custom" : "automatic"} histogram range · mean ${minimum}–${max} cycles/issue · peak bin ${Math.max(0, ...hist.map((b) => b.total))} accumulated cycles · idle excluded`,
                )
              }
            />
            <label>
              Min{" "}
              <RangeBound
                label="Histogram range"
                edge={0}
                range={histogramRange}
                bounds={domain}
                onCommit={(r) => plots?.changeRange(rangeKey, r)}
              />
            </label>
            <label>
              Max{" "}
              <RangeBound
                label="Histogram range"
                edge={1}
                range={histogramRange}
                bounds={domain}
                onCommit={(r) => plots?.changeRange(rangeKey, r)}
              />
            </label>
            <button onClick={() => plots?.changeRange(rangeKey, null)}>
              Automatic histogram range
            </button>
            <label>
              Bins{" "}
              <select
                aria-label="Histogram bins"
                value={bins}
                onChange={(e) => onBins(Number(e.target.value))}
              >
                {[8, 16, 32, 64].map((n) => (
                  <option key={n}>{n}</option>
                ))}
              </select>
            </label>
          </div>
        }
      >
        {stored && (stored[0] !== minimum || stored[1] !== max) && (
          <p className="summary-note">
            Saved histogram bounds exceed the supported nonnegative range;
            adjusted bounds are shown.
          </p>
        )}
        <div className="histogram">
          {hist.map((b, i) => (
            <button
              title={`${fmt(b.low)}–${fmt(b.high)} cycles · ${fmt(b.total)} accumulated`}
              key={i}
              style={{ height: `${Math.max(2, (b.total / peak) * 100)}%` }}
              disabled={!b.rows.length}
              onClick={() => onSelectGroup(b.rows.map((i) => i.index))}
            >
              <span>{fmt(b.low)}</span>
            </button>
          ))}
        </div>
        <div className="axis-label">
          Mean latency (cycles per issue) → · {stored ? "Custom" : "Automatic"}{" "}
          range · independent of trace time · click a bin to highlight its ISA
          lines
        </div>
      </Panel>
      <Panel
        title="Most expensive instructions"
        detail="Click an instruction to jump to the linked ISA and source view"
      >
        <div className="hotspot-rows">
          {expensive.map((i) => (
              <button key={i.index} onClick={() => onSelect(i.index)}>
                <code>{i.text}</code>
                <span>{fmt(i.latency)} cycles</span>
                <div
                  style={{
                    width: `${(i.latency / maximumLatency) * 100}%`,
                  }}
                />
              </button>
            ))}
        </div>
      </Panel>
    </>
  );
}
export function FlameView({
  instructions,
  onSelect,
  metric = "latency",
  exportContext = "Whole capture",
}: {
  metric?: "latency" | "pcSamples" | "pcIssued" | "pcStalled";
  exportContext?: string;
  instructions: Instruction[];
  onSelect: (id: number) => void;
}) {
  const weight = (i: Instruction) => i[metric] ?? 0;
  const unit = metric === "latency" ? "cycles" : "samples";
  const [focus, setFocus] = useAnalysisPreference<string | null>(
    `flame.${metric}`,
    null,
  );
  const [hierarchy, setHierarchy] = useAnalysisPreference<boolean>(
    "flame.hierarchy",
    false,
  );
  const [grouping, setGrouping] = useAnalysisPreference<FlameGrouping>(
    "flame.groupBy",
    "source",
  );
  const plots = useContext(PlotAxesContext);
  const viewportKey = plotAxisKey(
    "source-flamegraph",
    metric,
    grouping,
    focus === null ? "all" : "source",
    focus || "",
    ...(hierarchy ? ["hierarchy"] : []),
  );
  const savedViewport = plots?.ranges[viewportKey];
  const groups = useMemo(() => {
    const map = new Map<string, Instruction[]>();
    for (const i of instructions)
      if (weight(i) > 0) {
        const name =
          grouping === "codeObject"
            ? codeObjectLabel(i.codeObject)
            : i.source || "Unmapped ISA";
        const group = map.get(name);
        if (group) group.push(i);
        else map.set(name, [i]);
      }
    return [...map]
      .map(([name, rows]) => ({
        name,
        rows,
        total: rows.reduce((s, i) => s + weight(i), 0),
      }))
      .sort((a, b) => b.total - a.total);
  }, [instructions, metric, grouping]);
  // A saved focus from one grouping (e.g. a source line) is not a group name in
  // the other (code objects); ignore it until it matches a current group.
  const focused = focus && groups.some((g) => g.name === focus) ? focus : null;
  const allLabel =
    grouping === "codeObject" ? "All code objects" : "All mapped source lines";
  const shown = focused ? groups.filter((g) => g.name === focused) : groups;
  const total = shown.reduce((s, g) => s + g.total, 0);
  const domain: Range = [0, Math.max(1, total)];
  const viewport = savedViewport
    ? fitWeightedRange(savedViewport, total)
    : domain;
  const setViewport = (range: Range | null) =>
    plots?.changeRange(viewportKey, range);
  let offset = 0;
  const layout = shown.flatMap((group, index) => {
    const start = offset;
    offset += group.total;
    const clipped = clipWeightedInterval(start, offset, viewport);
    if (!clipped) return [];
    let leafOffset = start;
    const leaves = group.rows.flatMap((instruction) => {
      const low = leafOffset;
      leafOffset += weight(instruction);
      const leaf = clipWeightedInterval(low, leafOffset, viewport);
      return leaf ? [{ instruction, ...leaf }] : [];
    });
    return [{ group, index, ...clipped, leaves }];
  });
  const frames = useMemo(
    () =>
      hierarchy
        ? sourceHierarchy(
            (focused
              ? groups.filter((group) => group.name === focused)
              : groups
            ).flatMap((group) => group.rows),
            metric,
            grouping,
          )
        : [],
    [groups, focused, metric, hierarchy, grouping],
  );
  const visibleFrames = clipSourceHierarchy(frames, viewport);
  const hierarchyHeight =
    (frames.reduce((depth, frame) => Math.max(depth, frame.depth), 0) + 1) * 30;
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
  if (
    !Number.isFinite(total) ||
    (metric !== "latency" && !Number.isSafeInteger(total))
  )
    return (
      <Empty
        title={
          metric === "latency"
            ? "Latency total is not finite"
            : "Sample total exceeds exact integer range"
        }
        detail="The source flamegraph cannot represent this aggregate count exactly."
      />
    );
  return (
    <Panel
      title={
        metric === "latency"
          ? "Source latency flamegraph"
          : metric === "pcSamples"
            ? "Attributed source samples"
          : metric === "pcIssued"
            ? "Issued source samples"
            : "Stalled source samples"
      }
      detail={
        (metric === "latency"
          ? "Width is accumulated instruction latency."
          : "Width is the recorded sample count. Missing counts are excluded.") +
        (grouping === "codeObject"
          ? " Grouped by the recorded code object (the compiled kernel unit: a pipeline/kernel label for RADV, an ELF id for HIP), then source and ISA."
          : " Grouped by source location and ISA.") +
        " This is source-to-ISA aggregation, not a sampled call stack, cycle estimate or per-dispatch cost split."
      }
      tools={
        <>
          <PlotExport
            format="svg"
            label="Source flamegraph"
            generate={() => {
              const boxes: PlotBox[] = [
                {
                  x: 0,
                  y: 0,
                  width: 1000,
                  height: 28,
                  color: "#adb8c7",
                  label: focused || allLabel,
                  detail: `${focused || allLabel}: ${total} ${unit}`,
                },
              ];
              if (hierarchy)
                visibleFrames.forEach((frame) =>
                  boxes.push({
                    x: frame.x * 1000,
                    y: 30 + frame.depth * 30,
                    width: frame.width * 1000,
                    height: 28,
                    color: frame.instruction
                      ? "#e4ad7c"
                      : colors[(frame.depth + 4) % colors.length],
                    label: frame.label,
                    detail: `${frame.instruction ? "ISA " + frame.instruction.index + ": " : ""}${frame.label}: ${frame.total} ${unit}`,
                  }),
                );
              else
                layout.forEach((g) => {
                  boxes.push({
                    x: g.x * 1000,
                    y: 30,
                    width: g.width * 1000,
                    height: 28,
                    color: colors[(g.index + 4) % colors.length],
                    label: g.group.name,
                    detail: `${g.group.name}: ${g.group.total} ${unit} · visible weight interval ${g.start}–${g.end}`,
                  });
                  g.leaves.forEach((leaf) =>
                    boxes.push({
                      x: leaf.x * 1000,
                      y: 60,
                      width: leaf.width * 1000,
                      height: 28,
                      color: "#e4ad7c",
                      label: leaf.instruction.text,
                      detail: `ISA ${leaf.instruction.index}: ${leaf.instruction.text}: ${weight(leaf.instruction)} ${unit} · visible weight interval ${leaf.start}–${leaf.end}`,
                    }),
                  );
                });
              return boxPlot(
                boxes,
                1000,
                hierarchy ? hierarchyHeight + 30 : 90,
                `Source flamegraph · ${metric} · ${grouping === "codeObject" ? "by code object" : "by source location"}`,
                `${exportContext} · ${total} ${unit} · visible cumulative weight ${viewport[0]}–${viewport[1]} ${unit} · ${grouping === "codeObject" ? "grouped by recorded code object · " : ""}source-to-ISA aggregation, not a call stack`,
              );
            }}
          />
          <button disabled={!focused} onClick={() => setFocus(null)}>
            Reset zoom
          </button>
        </>
      }
    >
      <label>
        Group by{" "}
        <select
          aria-label="Flamegraph grouping"
          value={grouping}
          onChange={(event) =>
            setGrouping(event.target.value as FlameGrouping)
          }
        >
          <option value="source">Source location</option>
          <option value="codeObject">Code object (kernel/pipeline)</option>
        </select>
      </label>
      <label>
        <input
          type="checkbox"
          checked={hierarchy}
          onChange={(event) => setHierarchy(event.target.checked)}
        />{" "}
        File and inline source hierarchy
      </label>
      <div className="flame-viewport toolbar-inline">
        <span>Cumulative weight ({unit})</span>
        <RangeBound
          label="Flamegraph range"
          edge={0}
          range={viewport}
          bounds={domain}
          onCommit={setViewport}
        />
        <RangeBound
          label="Flamegraph range"
          edge={1}
          range={viewport}
          bounds={domain}
          onCommit={setViewport}
        />
        <button
          aria-label="Pan flamegraph left"
          onClick={() => navigate(1, -0.5)}
        >
          ←
        </button>
        <button aria-label="Zoom flamegraph in" onClick={() => navigate(0.5)}>
          +
        </button>
        <button aria-label="Zoom flamegraph out" onClick={() => navigate(2)}>
          −
        </button>
        <button
          aria-label="Pan flamegraph right"
          onClick={() => navigate(1, 0.5)}
        >
          →
        </button>
        <button onClick={() => setViewport(null)}>Full flamegraph range</button>
        <span>
          {savedViewport ? "Independent viewport" : "Full weight range"} · not
          trace time
        </span>
      </div>
      <div className="flame-root">
        {focused || allLabel}{" "}
        <span>
          {fmt(total)} {unit}
        </span>
      </div>
      {hierarchy ? (
        <div
          className="source-hierarchy"
          style={{ position: "relative", height: hierarchyHeight }}
          aria-label="Recorded source hierarchy frames"
        >
          {visibleFrames.map((frame) => (
            <button
              key={frame.key}
              style={{
                position: "absolute",
                left: `${frame.x * 100}%`,
                width: `${frame.width * 100}%`,
                top: frame.depth * 30,
                height: 28,
                overflow: "hidden",
                whiteSpace: "nowrap",
                background: frame.instruction
                  ? "#e4ad7c"
                  : colors[(frame.depth + 4) % colors.length],
                color: "#161b23",
              }}
              title={`${frame.label} · ${fmt(frame.total)} ${unit}`}
              onClick={() =>
                frame.instruction
                  ? onSelect(frame.instruction.index)
                  : setViewport([frame.start, frame.end])
              }
            >
              {frame.label}
            </button>
          ))}
        </div>
      ) : (
        <div className="flames">
          {layout.map((g) => (
            <div style={{ width: `${g.width * 100}%` }} key={g.group.name}>
              <button
                style={{ background: colors[(g.index + 4) % colors.length] }}
                title={`${g.group.name} · ${fmt(g.group.total)} ${unit}`}
                onClick={() => setFocus(g.group.name)}
              >
                {g.group.name}
              </button>
              <div className="flame-leaves">
                {g.leaves.map((leaf) => (
                  <button
                    key={leaf.instruction.index}
                    style={{ width: `${(leaf.width / g.width) * 100}%` }}
                    title={`${leaf.instruction.text} · ${fmt(weight(leaf.instruction))} ${unit}`}
                    onClick={() => onSelect(leaf.instruction.index)}
                  >
                    {leaf.instruction.text}
                  </button>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
      {!groups.length && (
        <Empty
          title={
            metric === "latency"
              ? "No instruction latency"
              : "No positive samples"
          }
          detail="This view requires positive instruction weights; missing values do not supply weights."
        />
      )}
    </Panel>
  );
}
export function DispatchView({
  capture,
  range,
  onRange,
  onTune,
  hostPreferences,
  onHostPreferences,
}: {
  capture: Capture;
  onTune?: (dispatch: import("../types").Dispatch) => void;
  range: Range;
  onRange: (r: Range) => void;
  hostPreferences?: import("../timelinePreferences").TimelinePreferences;
  onHostPreferences?: (
    preferences: import("../timelinePreferences").TimelinePreferences,
  ) => void;
}) {
  const [search, setSearch] = useAnalysisPreference<string>(
      "dispatch.search",
      "",
    ),
    [sort, setSort] = useAnalysisPreference<"total" | "count" | "name">(
      "dispatch.sort",
      "total",
    );
  const stats = kernelStats(capture)
    .filter((k) => k.name.toLowerCase().includes(search.toLowerCase()))
    .sort((a, b) =>
      sort === "name" ? a.name.localeCompare(b.name) : b[sort] - a[sort],
    );
  const total = stats.reduce((s, k) => s + k.total, 0);
  const clipped = capture.dispatches
    .filter((d) => d.end > range[0] && d.start < range[1])
    .map(
      (d) => [Math.max(d.start, range[0]), Math.min(d.end, range[1])] as Range,
    );
  return (
    <>
      <HostCalls
        capture={capture}
        range={range}
        onRange={onRange}
        preferences={hostPreferences}
        onPreferences={onHostPreferences}
      />
      <div className="metrics">
        <Metric
          label="Recorded dispatches"
          value={fmt(capture.dispatches.length)}
          detail="Capture-wide count"
        />
        <Metric
          label="Summed durations"
          value={`${fmt(total)} ${capture.unit}`}
          detail="Filtered kernels · overlaps counted"
        />
        <Metric
          label="Activity interval union"
          value={`${fmt(union(clipped))} ${capture.unit}`}
          detail="Visible range · overlaps counted once"
        />
      </div>
      <Panel
        title="Kernel dispatches"
        detail={
          capture.unit === "cycles"
            ? "SQTT dispatch events have no dispatch end timestamp; duration is unavailable. Wave-active is the union of the kernel's wave lifetimes from occupancy."
            : "Kernel shares use summed recorded durations; replay coverage is unknown."
        }
        tools={
          <div className="search-box">
            <Search size={14} />
            <input
              aria-label="Filter kernels"
              placeholder="Filter kernels…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
        }
      >
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>
                  <button onClick={() => setSort("name")}>Kernel</button>
                </th>
                <th>
                  <button onClick={() => setSort("count")}>Calls</button>
                </th>
                <th>
                  <button onClick={() => setSort("total")}>Total ↓</button>
                </th>
                <th>Mean</th>
                <th>Share</th>
                <th title="Waves recorded in occupancy for this kernel">Waves</th>
                <th title="Union of this kernel's wave lifetimes: the time it had any wave resident">
                  Wave-active
                </th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {stats.map((k) => (
                <tr key={k.name}>
                  <td>
                    <code title={k.name}>{k.name}</code>
                  </td>
                  <td>{fmt(k.count)}</td>
                  <td>{k.total ? fmt(k.total) : "—"}</td>
                  <td>{k.total ? fmt(k.total / k.count) : "—"}</td>
                  <td>
                    {total ? `${((k.total / total) * 100).toFixed(1)}%` : "—"}
                  </td>
                  <td>{k.waves ? fmt(k.waves) : "—"}</td>
                  <td>{k.activeTime ? fmt(k.activeTime) : "—"}</td>
                  <td>
                    {onTune && !isInternalKernel(k.name) && (
                      <button onClick={() => onTune(capture.dispatches.find((d) => d.kernel === k.name)!)}>
                        {capture.format.startsWith("RADV RGP") ? "Tune shader" : "Tune kernel"}
                      </button>
                    )}
                    <button
                      title="Focus first dispatch"
                      onClick={() => {
                        const d = capture.dispatches.find(
                          (d) => d.kernel === k.name,
                        )!;
                        if (d.end > d.start) onRange([d.start, d.end]);
                        else
                          onRange([
                            Math.max(capture.start, d.start),
                            Math.min(
                              capture.end,
                              Math.max(capture.start, d.start) + 1000,
                            ),
                          ]);
                      }}
                    >
                      <ChevronRight size={14} />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {!stats.length && (
          <Empty
            title="No matching dispatches"
            detail="Global SQTT occupancy may be available even when explicit dispatch events were not emitted."
          />
        )}
      </Panel>
    </>
  );
}
export function Metric({
  label,
  value,
  detail,
}: {
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{detail}</small>
    </div>
  );
}
export function EvidenceView({
  capture,
  onOpenTrace,
  onExport,
  exporting,
  exportStatus,
}: {
  capture: Capture;
  onOpenTrace: (path: string) => void;
  onExport: () => void;
  exporting: boolean;
  exportStatus: string;
}) {
  return (
    <>
      <Panel
        title="Capture evidence"
        detail="Keep attribution, correctness, and native performance qualification separate."
        tools={
          <button onClick={onExport} disabled={exporting}>
            {exporting
              ? "Generating diagnostics…"
              : "Export diagnostic manifest"}
          </button>
        }
      >
        <p className="summary-note">
          Exports collection counts, measurement availability, capture metadata,
          local paths, analysis settings and definition text. Source replacement
          text is represented by hashes. Raw trace data is not embedded.
        </p>
        {exportStatus && (
          <p role="status" className="summary-note">
            {exportStatus}
          </p>
        )}
        <div className="evidence-grid">
          {[
            ["Input format", capture.format],
            ["Architecture", capture.architecture],
            ["Clock domain", capture.unit],
            ["Original clock origin", capture.origin],
            [
              "Cross-SE clock alignment",
              Object.keys(capture.seTimeOffsets ?? {}).length
                ? `Applied (${Object.keys(capture.seTimeOffsets ?? {}).length} engines)`
                : "Not applied",
            ],
            ["REALTIME anchors", String(capture.realtime?.anchors.length ?? 0)],
            [
              "Recorded realtime frequency (Hz)",
              capture.realtime?.frequencyHz ?? "Not recorded",
            ],
            ["Replay coverage", capture.metadata.coverage || "Unknown"],
            ["Native timing control", "Not attached"],
            ["Output correctness gate", "Not attached"],
            ["Temperature / contention", "Not attached"],
            ["Runtime / library identity", "Not attached"],
          ].map(([k, v]) => (
            <div key={k}>
              <span>{k}</span>
              <strong>{v}</strong>
            </div>
          ))}
        </div>
      </Panel>
      <CodeObjects
        key={capture.id}
        captureId={capture.id}
        records={capture.codeObjects ?? []}
      />
      <TraceAssociations
        key={capture.id}
        capture={capture}
        onOpen={onOpenTrace}
      />
      <Panel
        title="Collection diagnostics"
        detail="Warnings travel with the capture."
      >
        {capture.warnings.map((w, i) => (
          <div className="warning-row" key={i}>
            <Info size={17} />
            <p>{w}</p>
          </div>
        ))}
      </Panel>
      <Panel
        title="What comes next"
        detail="Extension boundaries are defined in docs/architecture.md."
      >
        <div className="roadmap">
          <div>
            <Cpu />
            <h4>Isolate & capture</h4>
            <p>
              Dispatch identity, code-object hash, launch dimensions, argument
              schema, buffer snapshots and a reproducible replay harness.
            </p>
          </div>
          <div>
            <BarChart3 />
            <h4>Tune & validate</h4>
            <p>
              Bounded parameter sweeps, reference outputs, thermal controls and
              matched native A/B measurements.
            </p>
          </div>
          <div>
            <Layers />
            <h4>Iterate with an LLM</h4>
            <p>
              Reviewable source patches, isolated builds, correctness gates and
              an auditable experiment history.
            </p>
          </div>
        </div>
      </Panel>
    </>
  );
}
