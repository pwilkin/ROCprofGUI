import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { desktop } from "../bridge";
import { useAnalysisPreference } from "../analysisPreferences";
import { sourceLineCosts, type SourceCostMetric } from "../sourceCosts";
import { fmt, type Instruction } from "../types";
import {
  MAX_SOURCE_BYTES,
  type SourceFile,
  type SourceMappings,
  type SourcePage,
} from "../sourceFiles";
export default function SourcePane({
  instructions,
  codeObject,
  unit,
  hasHiddenCosts,
  path,
  captureId,
  snapshotPath,
  registeredPath,
  line,
  snapshot,
  mapping,
  onMap,
  onMapBatch,
  onLine,
}: {
  instructions: Instruction[];
  codeObject: string;
  unit: string;
  hasHiddenCosts: boolean;
  path: string;
  captureId: string;
  snapshotPath?: string;
  registeredPath?: string;
  line: number;
  snapshot?: string;
  mapping?: SourceFile;
  onMap: (path: string, file: SourceFile | null) => void;
  onMapBatch: (files: SourceMappings) => void;
  onLine: (line: number) => void;
}) {
  const [lineNumbers, setLineNumbers] = useAnalysisPreference<boolean>(
    "source.lineNumbers",
    true,
  );
  const [costWidth, setCostWidth] = useAnalysisPreference<number>(
    "source.costWidth",
    100,
  );
  const [costMetric, setCostMetric] = useAnalysisPreference<SourceCostMetric>(
    "source.costMetric",
    "latency",
  );
  const [includeHidden, setIncludeHidden] = useAnalysisPreference<boolean>(
    "source.includeHidden",
    true,
  );
  const costs = useMemo(
    () => sourceLineCosts(instructions, path, codeObject, costMetric),
    [instructions, path, codeObject, costMetric],
  );
  const maxCost = useMemo(() => {
    let maximum = 0;
    for (const cost of costs.values())
      if (Number.isFinite(cost.value)) maximum = Math.max(maximum, cost.value);
    return maximum;
  }, [costs]);
  const [page, setPage] = useState(0),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null),
    container = useRef<HTMLDivElement>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const prefixPath = registeredPath || path;
  const [prefix, setPrefix] = useState(
    prefixPath.includes("/")
      ? prefixPath.slice(0, prefixPath.lastIndexOf("/")) || "/"
      : ".",
  );
  const [folderStatus, setFolderStatus] = useState("");
  async function chooseDirectory() {
    setBusy(true);
    setError("");
    setFolderStatus("");
    try {
      const batch = await invoke<{
        files: SourceMappings;
        missing: string[];
      } | null>("import_source_directory", { captureId, prefix });
      if (!alive.current) return;
      if (!batch) {
        setFolderStatus("Source folder selection cancelled");
        return;
      }
      onMapBatch(batch.files);
      setFolderStatus(
        `Mapped ${Object.keys(batch.files).length} files; ${batch.missing.length} matching paths missing from the selected folder.`,
      );
    } catch (error) {
      if (alive.current) setError(String(error));
    } finally {
      if (alive.current) setBusy(false);
    }
  }
  const [retry, setRetry] = useState(0);
  const reference = mapping?.reference;
  const [loaded, setLoaded] = useState<{
    key: string;
    page?: SourcePage;
    error?: string;
  } | null>(null);
  const size = 200;
  const loadKey = `${captureId}\0${snapshotPath || ""}\0${reference?.path || ""}\0${reference?.sha256 || ""}\0${page}`;
  useEffect(() => {
    let active = true;
    if (!reference && (snapshot !== undefined || mapping || !snapshotPath))
      return;
    setLoaded(null);
    invoke<SourcePage>(
      reference ? "read_source_reference_page" : "read_source_page",
      {
        captureId,
        original: snapshotPath,
        startLine: page * size + 1,
        count: size,
        ...(reference ? { reference } : {}),
      },
    ).then(
      (file) => {
        if (active) {
          const last = Math.max(0, Math.ceil(file.totalLines / size) - 1);
          if (page > last) setPage(last);
          else setLoaded({ key: loadKey, page: file });
        }
      },
      (error) => {
        if (active) setLoaded({ key: loadKey, error: String(error) });
      },
    );
    return () => {
      active = false;
    };
  }, [captureId, snapshotPath, snapshot, mapping, loadKey, retry, reference]);
  const currentLoad = loaded?.key === loadKey ? loaded : null;
  const text = reference ? undefined : (mapping?.text ?? snapshot);
  const loading =
    (!!reference || (!mapping && snapshot === undefined && !!snapshotPath)) &&
    !currentLoad;

  const lines = useMemo(
    () => (text === undefined ? [] : text.split(/\r?\n/)),
    [text],
  );
  const totalLines =
    text !== undefined ? lines.length : (currentLoad?.page?.totalLines ?? 0);
  const lastPage = Math.max(0, Math.ceil(totalLines / size) - 1);
  const current = text !== undefined ? Math.min(page, lastPage) : page;
  const visibleLines =
    text !== undefined
      ? lines.slice(current * size, (current + 1) * size)
      : currentLoad?.page?.lines;
  useEffect(() => {
    setPage(
      Number.isFinite(line) ? Math.max(0, Math.floor((line - 1) / size)) : 0,
    );
    setError("");
  }, [path, line, text]);
  useEffect(() => {
    const element =
      container.current?.querySelector<HTMLElement>(".source-selected");
    if (element && container.current)
      container.current.scrollTop =
        element.offsetTop - container.current.offsetTop - 70;
  }, [line, current, text, currentLoad]);
  async function choose() {
    if (!desktop) {
      input.current?.click();
      return;
    }
    const target = path;
    setBusy(true);
    setError("");
    try {
      const file = await invoke<SourceFile | null>("import_source_file");
      if (file && alive.current) onMap(target, file);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <div className="source-remap-controls">
        <button disabled={!path || busy} onClick={() => void choose()}>
          {busy ? "Opening source…" : "Map source file"}
        </button>
        {mapping && (
          <button onClick={() => onMap(path, null)}>
            Restore captured source
          </button>
        )}
        <span>
          {mapping
            ? reference
              ? `Referenced ${mapping.name} · checked against saved SHA-256 when read · kernel source revision not verified`
              : `Mapped to ${mapping.name} · saved with projects · source revision not verified`
            : "Captured source, when available"}
        </span>
        <input
          ref={input}
          hidden
          type="file"
          aria-label="Source replacement file"
          onChange={async (event) => {
            const file = event.target.files?.[0],
              target = path;
            event.target.value = "";
            if (!file) return;
            setError("");
            setBusy(true);
            try {
              if (file.size > MAX_SOURCE_BYTES)
                throw Error(
                  "Browser source imports are limited to 2 MiB. Use the desktop app to reference larger files.",
                );
              const text = new TextDecoder("utf-8", { fatal: true }).decode(
                await file.arrayBuffer(),
              );
              if (alive.current) onMap(target, { name: file.name, text });
            } catch (e) {
              setError(String(e));
            } finally {
              setBusy(false);
            }
          }}
        />
      </div>
      {desktop && (
        <details className="source-remap-controls">
          <summary>Map source directory</summary>
          <label>
            Recorded directory prefix{" "}
            <input
              aria-label="Recorded source directory prefix"
              value={prefix}
              onChange={(e) => setPrefix(e.target.value)}
            />
          </label>
          <button
            disabled={busy || !prefix}
            onClick={() => void chooseDirectory()}
          >
            Choose replacement source folder
          </button>
          <p className="muted small">
            Matches registered source paths beneath this prefix. Replaces
            matching files together; missing files keep existing mappings. Use .
            for relative paths. Files up to 2 MiB are embedded in projects;
            larger files are referenced by path and fingerprint and must remain
            available.
          </p>
          {folderStatus && <p role="status">{folderStatus}</p>}
        </details>
      )}
      {currentLoad?.error && (!mapping || reference) && (
        <p className="summary-note" role="alert">
          {currentLoad.error}{" "}
          <button onClick={() => setRetry((n) => n + 1)}>
            {reference ? "Retry source reference" : "Retry captured source"}
          </button>
        </p>
      )}
      {loading && (
        <p className="summary-note" role="status">
          {reference
            ? "Loading referenced source…"
            : "Loading captured source…"}
        </p>
      )}
      {error && (
        <p className="summary-note" role="alert">
          {error}
        </p>
      )}
      <div className="source-remap-controls">
        <label>
          <input
            type="checkbox"
            checked={lineNumbers}
            onChange={(event) => setLineNumbers(event.target.checked)}
          />{" "}
          Source line numbers
        </label>
        <label>
          Source cost{" "}
          <select
            aria-label="Source cost metric"
            value={costMetric}
            onChange={(event) =>
              setCostMetric(event.target.value as SourceCostMetric)
            }
          >
            <option value="latency">ISA cost</option>
            <option value="pcSamples">PC samples</option>
            <option value="pcIssued">Issued samples</option>
            <option value="pcStalled">Stalled samples</option>
          </select>
        </label>
        <label>
          Bar width{" "}
          <input
            aria-label="Source cost bar width"
            type="range"
            min="0"
            max="400"
            step="20"
            value={costWidth}
            onChange={(event) => setCostWidth(Number(event.target.value))}
          />
        </label>
        <label
          title={
            hasHiddenCosts
              ? "Uses the reconstructed hidden-cost analysis"
              : "Hidden-cost separation is unavailable for this capture"
          }
        >
          <input
            type="checkbox"
            disabled={!hasHiddenCosts}
            checked={includeHidden}
            onChange={(event) => setIncludeHidden(event.target.checked)}
          />{" "}
          Include hidden source cost
        </label>
      </div>
      <p className="muted small source-cost-note">
        ISA cost follows the current scope; samples use recorded instruction
        totals. Bars compare lines in this file and code object. Inline
        locations are inclusive; one instruction can contribute to multiple
        lines. An em dash means no recorded value; ≥ marks a partial sum.
      </p>
      <div className="source" ref={container}>
        {visibleLines !== undefined ? (
          visibleLines.map((text, n) => {
            const number = current * size + n + 1;
            const cost = costs.get(number);
            const known = !!cost?.available && Number.isFinite(cost.value);
            const label = known
              ? `${cost.missing ? "≥ " : ""}${fmt(cost.value)}`
              : "—";
            const fraction = known && maxCost > 0 ? cost.value / maxCost : 0;
            return (
              <button
                key={number}
                className={number === line ? "source-selected" : ""}
                onClick={() => onLine(number)}
                aria-label={`Source line ${number}: ${text || "blank"}`}
              >
                {lineNumbers && (
                  <span
                    className="source-line-number"
                    style={{
                      width: `${Math.max(2, String(totalLines).length)}ch`,
                      flexShrink: 0,
                    }}
                  >
                    {number}
                  </span>
                )}
                {costWidth > 0 && (
                  <span
                    className="source-cost"
                    style={{ width: costWidth }}
                    title={`Line ${number}: ${label} ${costMetric === "latency" ? unit : "samples"}`}
                  >
                    <span
                      className="source-cost-fill"
                      style={{ width: `${Math.min(100, fraction * 100)}%` }}
                    />
                    <span className="source-cost-value">{label}</span>
                  </span>
                )}
                <code>{text || " "}</code>
              </button>
            );
          })
        ) : (
          <p className="muted">
            {reference
              ? "The referenced file must be available and match its saved fingerprint. Map a file again to replace this reference."
              : snapshotPath
                ? loading
                  ? "Reading the selected captured source."
                  : "Captured source could not be loaded. Choose a local replacement or retry."
                : "No captured source for this instruction. Choose a local file to map this recorded path. Original host paths are never opened automatically."}
          </p>
        )}
      </div>
      {visibleLines !== undefined && (
        <div className="source-remap-controls">
          <button disabled={!current} onClick={() => setPage(current - 1)}>
            Previous source lines
          </button>
          <span>
            Lines {current * size + 1}–
            {Math.min(totalLines, (current + 1) * size)} of {totalLines}
          </span>
          <button
            disabled={current === lastPage}
            onClick={() => setPage(current + 1)}
          >
            Next source lines
          </button>
          <button
            disabled={!Number.isFinite(line) || line < 1 || line > totalLines}
            onClick={() => setPage(Math.floor((line - 1) / size))}
          >
            Selected source line
          </button>
          {line > totalLines && (
            <span>Recorded line {line} is outside this file.</span>
          )}
        </div>
      )}
    </>
  );
}
