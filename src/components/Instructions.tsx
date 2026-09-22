import { DisplayScaleContext } from "../displayScale";
import { sourceLocations } from "../sourceLocations";
import { useAnalysisPreference } from "../analysisPreferences";
import type { WaitIndex } from "../waitOccurrences";
import type { IsaState } from "../isaState";
import { isaColumns, isaWidths, isaHiddenColumns } from "../isaState";
import {
  useEffect,
  useContext,
  useRef,
  useState,
  useMemo,
  type Dispatch,
  type SetStateAction,
} from "react";
import { ArrowLeft, ArrowRight, Search } from "lucide-react";
import type { Capture, Instruction, Wave } from "../types";
import { fmt } from "../types";
import { savePreference } from "../session";
import SourcePane from "./SourcePane";
import type { SourceMappings, SourceFile } from "../sourceFiles";
import { Panel } from "./AnalysisViews";
const isLabel = (i: Instruction) =>
  i.text.trimStart().startsWith(";") || i.text.trimEnd().endsWith(":");
export default function Instructions({
  capture,
  state,
  onState,
  sourceMappings,
  onMapSource,
  onMapBatch,
  onDocumentation,
  instructions,
  sourceInstructions,
  hasHiddenCosts,
  wave,
  waitIndex,
  selected,
  onSelect,
  onTokenSelect,
  iteration,
  onIteration,
  highlighted = [],
}: {
  state: IsaState;
  onState: Dispatch<SetStateAction<IsaState>>;
  sourceMappings: SourceMappings;
  onMapSource: (path: string, file: SourceFile | null) => void;
  onMapBatch: (files: SourceMappings) => void;
  onDocumentation?: (text: string) => void;
  capture: Capture;
  instructions: Instruction[];
  sourceInstructions: Instruction[];
  hasHiddenCosts: boolean;
  wave: Wave | null;
  waitIndex: WaitIndex;
  selected: number;
  onSelect: (id: number) => void;
  onTokenSelect: (index: number) => void;
  iteration: number;
  onIteration: (i: number) => void;
  highlighted?: number[];
}) {
  const display = useContext(DisplayScaleContext);
  const [sourceFocus, setSourceFocus] = useAnalysisPreference<string>(
    "source.location",
    "",
  );
  const [dependencyPage, setDependencyPage] = useState(0);
  useEffect(() => setDependencyPage(0), [wave?.id, selected, iteration]);
  const { search, fold, collapsed, sort, page } = state;
  const widths = isaWidths(state.widths);
  const hiddenColumns = isaHiddenColumns(state.hiddenColumns);
  const visibleColumns = isaColumns
    .map((column, index) => ({ ...column, index }))
    .filter((column) => !hiddenColumns.includes(column.key));
  const update = <K extends keyof IsaState>(
    key: K,
    value: SetStateAction<IsaState[K]>,
  ) =>
    onState((previous) => ({
      ...previous,
      [key]:
        typeof value === "function"
          ? (value as (old: IsaState[K]) => IsaState[K])(previous[key])
          : value,
    }));
  const setSearch = (value: string) => update("search", value);
  const setFold = (value: boolean) => update("fold", value);
  const setCollapsed = (value: SetStateAction<number[]>) =>
    update("collapsed", value);
  const setSort = (value: boolean) => update("sort", value);
  const setPage = (value: number) => update("page", value);
  const setWidths = (value: SetStateAction<number[]>) => {
    const next = typeof value === "function" ? value(widths) : value;
    update("widths", next);
    savePreference("rocprof:isa-columns", next);
  };
  const table = useRef<HTMLDivElement>(null);
  const sections = useMemo(() => {
    const result = new Map<number, number>();
    let section = -1;
    for (const i of capture.instructions) {
      if (isLabel(i)) section = i.index;
      result.set(i.index, section);
    }
    return result;
  }, [capture.instructions]);
  const resizing = useRef<{
    column: number;
    start: number;
    width: number;
  } | null>(null);
  const rows = useMemo(() => {
    const hidden = new Set(collapsed);
    const query = search.toLowerCase();
    const result = instructions.filter(
      (i) =>
        (!fold ||
          sort ||
          !!search ||
          isLabel(i) ||
          !hidden.has(sections.get(i.index) ?? -1)) &&
        (!query || (i.text + i.source).toLowerCase().includes(query)),
    );
    if (sort) result.sort((a, b) => b.latency - a.latency);
    return result;
  }, [instructions, fold, sort, search, collapsed, sections]);
  const index = rows.findIndex((i) => i.index === selected),
    size = 150;
  const previousSelection = useRef(selected);
  const pendingSelection = useRef<number | null>(null);
  useEffect(() => {
    if (previousSelection.current !== selected) {
      previousSelection.current = selected;
      pendingSelection.current = selected;
      const parent = sections.get(selected);
      if (parent !== undefined)
        setCollapsed((old) =>
          old.includes(parent) ? old.filter((id) => id !== parent) : old,
        );
    }
    if (pendingSelection.current === selected && index >= 0) {
      setPage(Math.floor(index / size));
      pendingSelection.current = null;
    }
  }, [selected, index]);
  const safePage = Math.min(
    page,
    Math.max(0, Math.ceil(rows.length / size) - 1),
  );
  useEffect(() => {
    for (const [container, selector] of [
      [table.current, ".selected"],
    ] as const) {
      const item = container?.querySelector(selector);
      if (!container || !item) continue;
      const bounds = container.getBoundingClientRect(),
        rect = item.getBoundingClientRect();
      if (rect.top < bounds.top + 30)
        container.scrollTop += rect.top - bounds.top - 30;
      else if (rect.bottom > bounds.bottom)
        container.scrollTop += rect.bottom - bounds.bottom;
    }
  }, [selected, safePage]);
  const inst = capture.instructions.find((i) => i.index === selected);
  const sourceFrames = sourceLocations(inst?.source || "");
  const sourceFrame =
    sourceFrames.find((frame) => frame.reference === sourceFocus) ||
    sourceFrames.at(-1);
  const path = sourceFrame?.path || "",
    line = sourceFrame?.line || 0;
  const registeredPath =
    [...(capture.sourceFiles || []), ...Object.keys(capture.sources)].find(
      (name) => name === path,
    ) ||
    [...(capture.sourceFiles || []), ...Object.keys(capture.sources)].find(
      (name) => name === "/" + path,
    );
  const mappingPath = Object.hasOwn(sourceMappings, path)
    ? path
    : registeredPath && Object.hasOwn(sourceMappings, registeredPath)
      ? registeredPath
      : path;
  const sourceText = Object.hasOwn(capture.sources, path)
    ? capture.sources[path]
    : Object.hasOwn(capture.sources, "/" + path)
      ? capture.sources["/" + path]
      : undefined;
  const iterationCount = waitIndex.count(selected);
  const waits = (wave?.waits || []).filter((w) => w[0] === selected);
  const reconstructed = waitIndex.wait(selected, iteration);
  const dependencies = reconstructed
    ? [
        ...new Set(
          reconstructed.sources.map(
            (source) => wave!.tokens[source.token].instruction,
          ),
        ),
      ]
    : [...new Set(waits.flatMap((w) => w[1].map(([line]) => line)))];
  const nodes = reconstructed
    ? reconstructed.sources.map((source) => ({
        line: wave!.tokens[source.token].instruction,
        token: source.token,
        source,
      }))
    : dependencies.map((line) => ({
        line,
        token: undefined,
        source: undefined,
      }));
  const dependencyPages = Math.max(1, Math.ceil(nodes.length / 20));
  const currentDependencyPage = Math.min(dependencyPage, dependencyPages - 1);
  const visibleDependencies = nodes.slice(
    currentDependencyPage * 20,
    (currentDependencyPage + 1) * 20,
  );
  return (
    <div className="instruction-layout">
      <Panel
        title="ISA explorer"
        detail="ISA selection is linked to source and the target wave."
        tools={
          <div className="toolbar-inline">
            <label>
              <input
                type="checkbox"
                checked={sort}
                onChange={(e) => setSort(e.target.checked)}
              />{" "}
              Cost order
            </label>
            <select
              aria-label="ISA folding"
              value={fold ? "labels" : "none"}
              onChange={(e) => setFold(e.target.value === "labels")}
            >
              <option value="labels">Fold by labels</option>
              <option value="none">No folding</option>
            </select>
            <button
              aria-label="Expand all ISA sections"
              onClick={() => setCollapsed([])}
            >
              +
            </button>
          </div>
        }
      >
        <div className="isa-search search-box">
          <Search size={15} />
          <input
            aria-label="Search ISA"
            maxLength={8192}
            placeholder="Search instructions, operands or source…"
            value={search}
            onChange={(e) => {
              setSearch(e.target.value);
              setPage(0);
            }}
          />
          <span>{rows.length} lines</span>
        </div>
        <details className="isa-column-options">
          <summary>ISA columns</summary>
          <div className="toolbar-wrap">
            {isaColumns
              .filter((column) => column.key !== "instruction")
              .map((column) => (
                <label key={column.key}>
                  <input
                    type="checkbox"
                    aria-label={`Show ISA ${column.label}`}
                    checked={!hiddenColumns.includes(column.key)}
                    onChange={(event) => {
                      const next = event.target.checked
                        ? hiddenColumns.filter((key) => key !== column.key)
                        : [...hiddenColumns, column.key];
                      update("hiddenColumns", next);
                      savePreference("rocprof:isa-hidden-columns", next);
                    }}
                  />
                  {column.label}
                </label>
              ))}
          </div>
        </details>
        <div className="isa-table table-scroll" ref={table}>
          <table
            style={{
              tableLayout: "fixed",
              width: visibleColumns.reduce(
                (sum, column) => sum + widths[column.index],
                0,
              ),
            }}
          >
            <colgroup>
              {visibleColumns.map((column) => (
                <col key={column.key} style={{ width: widths[column.index] }} />
              ))}
            </colgroup>
            <thead>
              <tr>
                {visibleColumns.map(({ label, index: column }) => (
                  <th key={label}>
                    {label}
                    <span
                      className="column-resizer"
                      role="separator"
                      aria-label={`Resize ${label}`}
                      onDoubleClick={() =>
                        setWidths((old) =>
                          old.map((v, i) =>
                            i === column ? isaColumns[i].width : v,
                          ),
                        )
                      }
                      onPointerDown={(e) => {
                        resizing.current = {
                          column,
                          start: e.clientX,
                          width: widths[column],
                        };
                        e.currentTarget.setPointerCapture(e.pointerId);
                      }}
                      onPointerMove={(e) => {
                        const r = resizing.current;
                        if (r)
                          setWidths((old) =>
                            old.map((v, i) =>
                              i === r.column
                                ? Math.min(
                                    1000,
                                    Math.max(40, r.width + e.clientX - r.start),
                                  )
                                : v,
                            ),
                          );
                      }}
                      onPointerUp={() => (resizing.current = null)}
                      onPointerCancel={() => (resizing.current = null)}
                    />
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.slice(safePage * size, (safePage + 1) * size).map((i) => (
                <tr
                  key={i.index}
                  className={
                    i.index === selected
                      ? "selected"
                      : highlighted.includes(i.index)
                        ? "related"
                        : ""
                  }
                  onClick={() => onSelect(i.index)}
                  tabIndex={0}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") onSelect(i.index);
                  }}
                >
                  {!hiddenColumns.includes("line") && <td>{i.index}</td>}
                  {!hiddenColumns.includes("address") && (
                    <td className="mono dim">{i.address}</td>
                  )}
                  <td className="asm">
                    {fold && isLabel(i) && (
                      <button
                        aria-label={`${collapsed.includes(i.index) ? "Expand" : "Collapse"} ISA section ${i.index}`}
                        className="fold-toggle"
                        onClick={(e) => {
                          e.stopPropagation();
                          setCollapsed((old) =>
                            old.includes(i.index)
                              ? old.filter((id) => id !== i.index)
                              : [...old, i.index],
                          );
                        }}
                      >
                        {collapsed.includes(i.index) ? "▸" : "▾"}
                      </button>
                    )}
                    <code title={i.text}>
                      <span
                        className={
                          i.text.startsWith("ds_")
                            ? "lds"
                            : i.text.startsWith("v_")
                              ? "valu"
                              : i.text.startsWith("global")
                                ? "vmem"
                                : "salu"
                        }
                      >
                        {i.text.split(" ")[0]}
                      </span>
                      {" " + i.text.split(" ").slice(1).join(" ")}
                    </code>
                  </td>
                  {!hiddenColumns.includes("hits") && (
                    <td className="isa-numeric">{fmt(i.hits)}</td>
                  )}
                  {!hiddenColumns.includes("cycles") && (
                    <td className="isa-numeric">{fmt(i.latency)}</td>
                  )}
                  {!hiddenColumns.includes("stall") && (
                    <td className="isa-numeric">{fmt(i.stall)}</td>
                  )}
                  {!hiddenColumns.includes("samples") && (
                    <td className="isa-numeric">{fmt(i.pcSamples)}</td>
                  )}
                  {!hiddenColumns.includes("idle") && (
                    <td className="isa-numeric">{fmt(i.idle)}</td>
                  )}
                  {!hiddenColumns.includes("issued") && (
                    <td className="isa-numeric">
                      {i.pcIssued == null ? "—" : fmt(i.pcIssued)}
                    </td>
                  )}
                  {!hiddenColumns.includes("stalled") && (
                    <td className="isa-numeric">
                      {i.pcStalled == null ? "—" : fmt(i.pcStalled)}
                    </td>
                  )}
                  {!hiddenColumns.includes("codeObject") && (
                    <td title={i.codeObject}>{i.codeObject || "—"}</td>
                  )}
                  {!hiddenColumns.includes("source") && (
                    <td title={i.source}>{i.source || "—"}</td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="plot-footer">
          <span>
            <button
              disabled={!inst || isLabel(inst)}
              onClick={() => inst && onDocumentation?.(inst.text)}
            >
              Instruction reference
            </button>{" "}
            Code object {inst?.codeObject || "—"} ·{" "}
            {inst?.address || "No instruction selected"}
          </span>
          <span>
            <button
              aria-label="Previous ISA page"
              disabled={!safePage}
              onClick={() => setPage(safePage - 1)}
            >
              <ArrowLeft size={12} />
            </button>{" "}
            Page {safePage + 1} / {Math.max(1, Math.ceil(rows.length / size))}{" "}
            <button
              aria-label="Next ISA page"
              disabled={(safePage + 1) * size >= rows.length}
              onClick={() => setPage(safePage + 1)}
            >
              <ArrowRight size={12} />
            </button>
          </span>
        </div>
      </Panel>
      <Panel
        title="Source snapshot"
        detail={path || "Select an instruction with a source mapping"}
      >
        {sourceFrames.length > 1 && (
          <label className="toolbar-inline" style={{ padding: "10px 16px" }}>
            Inline source location
            <select
              aria-label="Inline source location"
              value={sourceFrame?.reference}
              onChange={(event) => setSourceFocus(event.target.value)}
            >
              {sourceFrames.map((frame, index) => (
                <option key={index} value={frame.reference}>
                  {index + 1}. {frame.reference}
                </option>
              ))}
            </select>
          </label>
        )}
        <SourcePane
          instructions={sourceInstructions}
          codeObject={inst?.codeObject || ""}
          unit={capture.unit}
          hasHiddenCosts={hasHiddenCosts}
          captureId={capture.id}
          snapshotPath={capture.sourceFiles?.find(
            (name) => name === path || name === "/" + path,
          )}
          path={path}
          registeredPath={registeredPath}
          line={line}
          snapshot={sourceText}
          mapping={
            Object.hasOwn(sourceMappings, mappingPath)
              ? sourceMappings[mappingPath]
              : undefined
          }
          onMap={(_, file) => onMapSource(mappingPath, file)}
          onMapBatch={onMapBatch}
          onLine={(number) => {
            const targets = capture.instructions.filter((ins) =>
              sourceLocations(ins.source).some(
                (frame) => frame.path === path && frame.line === number,
              ),
            );
            const target =
              targets.find((ins) => ins.index === selected) ||
              targets.find((ins) => ins.codeObject === inst?.codeObject) ||
              targets[0];
            if (target) {
              const frame = sourceLocations(target.source).find(
                (frame) => frame.path === path && frame.line === number,
              );
              if (frame) setSourceFocus(frame.reference);
              onSelect(target.index);
            }
          }}
        />
        <div className="iteration">
          <span>Loop occurrence</span>
          <button
            disabled={!iteration}
            onClick={() => onIteration(iteration - 1)}
          >
            −
          </button>
          <input
            aria-label="Loop occurrence"
            type="number"
            min="0"
            max={Math.max(0, iterationCount - 1)}
            value={iteration}
            onChange={(e) =>
              onIteration(
                Math.max(
                  0,
                  Math.min(iterationCount - 1, Number(e.target.value)),
                ),
              )
            }
          />
          <button
            disabled={iteration >= iterationCount - 1}
            onClick={() => onIteration(iteration + 1)}
          >
            +
          </button>
          <span>of {iterationCount}</span>
        </div>
        <div className="wait-links">
          <b>Wait dependencies</b>
          {!!reconstructed?.reason && <p>{reconstructed.reason}</p>}
          {!!reconstructed?.unsupported && (
            <p>
              Wait reconstruction unavailable: unsupported counter encodings,
              missing ISA text or unordered tokens.
            </p>
          )}
          {reconstructed?.exportCounterModeled && (
            <p className="muted">
              Export-counter links track export or register hazards;
              issue-to-wait intervals are not memory completion latency.
            </p>
          )}
          {reconstructed?.conservativeExportOrdering && (
            <p className="muted">
              Mixed export-counter event groups may complete out of order.
              Partial waits retain the pending group until a zero-count wait.
            </p>
          )}
          {!!reconstructed?.unmodeled && (
            <p>
              Counter dependencies not modeled: {reconstructed.unmodeled}.
              Displayed links cover the supported counters only.
            </p>
          )}
          {reconstructed?.conservativeImageOrdering && (
            <p className="muted">
              Image operations on this architecture are linked at zero-count
              waits only. Captured memory-ordering metadata is unavailable, so
              partial-wait completion order is not inferred.
            </p>
          )}
          {nodes.length ? (
            <>
              <p>
                {reconstructed
                  ? "Reconstructed from named ISA counters for the selected token occurrence. Issue-to-wait intervals do not measure memory completion latency."
                  : "Recorded ISA relationships for this wave. The format does not identify individual loop occurrences."}
              </p>
              <div className="wait-diagram-scroll">
                <svg
                  className="wait-diagram"
                  viewBox={`0 0 480 ${Math.max(70, visibleDependencies.length * 44)}`}
                  role="img"
                  aria-label="Memory instructions linked to selected wait instruction"
                >
                  {visibleDependencies.map(({ line, token }, n) => {
                    const y = n * 44 + 22,
                      end = Math.max(70, visibleDependencies.length * 44) / 2;
                    return (
                      <g key={token ?? line}>
                        <text x="8" y={y + 4}>
                          ISA {line}
                          {token !== undefined ? ` · #${token}` : ""}
                        </text>
                        <path
                          d={`M145,${y} C210,${y} 240,${end} 350,${end}`}
                          fill="none"
                          stroke="#eeb669"
                          strokeWidth={1.5 * display.strokeScale}
                          vectorEffect="non-scaling-stroke"
                        />
                        <path
                          d={`M342,${end - 4} L350,${end} L342,${end + 4}`}
                          strokeWidth={display.strokeScale}
                          vectorEffect="non-scaling-stroke"
                          fill="none"
                          stroke="#eeb669"
                        />
                      </g>
                    );
                  })}
                  <text
                    x="365"
                    y={Math.max(70, visibleDependencies.length * 44) / 2 + 4}
                  >
                    ISA {selected}
                  </text>
                </svg>
              </div>
              {visibleDependencies.map(({ line, token, source }) => {
                const instruction = capture.instructions.find(
                  (i) => i.index === line,
                );
                return (
                  <button
                    key={token ?? line}
                    disabled={!instruction}
                    onClick={() =>
                      token === undefined
                        ? onSelect(line)
                        : onTokenSelect(token)
                    }
                  >
                    ISA {line} → ISA {selected}
                    {token !== undefined ? ` · token ${token}` : ""}{" "}
                    {source && (
                      <span>
                        issue to wait {source.issueToWaitCycles} cycles · issue
                        to wait end {source.issueToWaitEndCycles} cycles ·{" "}
                      </span>
                    )}
                    <span className="dim">
                      {instruction?.text ?? "unmapped"}
                    </span>
                  </button>
                );
              })}
              <div className="toolbar-inline">
                <button
                  disabled={!currentDependencyPage}
                  onClick={() => setDependencyPage(currentDependencyPage - 1)}
                >
                  Previous dependencies
                </button>
                <span>
                  {currentDependencyPage * 20 + 1}–
                  {Math.min(nodes.length, (currentDependencyPage + 1) * 20)} of{" "}
                  {nodes.length} dependencies
                </span>
                <button
                  disabled={currentDependencyPage + 1 >= dependencyPages}
                  onClick={() => setDependencyPage(currentDependencyPage + 1)}
                >
                  Next dependencies
                </button>
              </div>
            </>
          ) : (
            <p>No wait dependency recorded for this ISA line.</p>
          )}
        </div>
      </Panel>
    </div>
  );
}
