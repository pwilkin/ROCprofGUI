import { desktop } from "../bridge";
import { streamTimeline, TimelineCommitUnknown } from "../timelineStream";
import { useEffect, useRef, useState } from "react";
import type { Mark } from "./Plots";
import type { Range } from "../types";
import { timelineDocument } from "../timelineExport";
import { savePlot } from "../plotExport";
export default function TimelineExport({
  label,
  unit,
  bounds,
  range,
  stacked,
  load,
}: {
  label: string;
  unit: string;
  bounds: Range;
  range: Range;
  stacked: boolean;
  load: (range: Range, signal: AbortSignal) => Promise<Mark[]>;
}) {
  const [timePages, setTimePages] = useState(1),
    [lanes, setLanes] = useState(12),
    [scope, setScope] = useState("whole"),
    [status, setStatus] = useState(""),
    [busy, setBusy] = useState(false);
  const request = useRef<AbortController | null>(null);
  useEffect(() => () => request.current?.abort(), []);
  return (
    <details className="timeline-export">
      <summary>Export timeline pages</summary>
      <div className="source-remap-controls">
        <label>
          Scope{" "}
          <select
            aria-label="Timeline export scope"
            disabled={busy}
            value={scope}
            onChange={(e) => setScope(e.target.value)}
          >
            <option value="whole">Whole timeline</option>
            <option value="visible">Visible time range, all lanes</option>
          </select>
        </label>
        <label>
          Time segments{" "}
          <input
            aria-label="Timeline export time segments"
            type="number"
            min="1"
            max="64"
            value={timePages}
            disabled={busy}
            onChange={(e) => setTimePages(Number(e.target.value))}
          />
        </label>
        <label>
          Lanes per page{" "}
          <input
            aria-label="Timeline export lanes per page"
            type="number"
            min="1"
            max="24"
            value={lanes}
            disabled={busy}
            onChange={(e) => setLanes(Number(e.target.value))}
          />
        </label>
        <button
          disabled={busy}
          onClick={async () => {
            const controller = new AbortController();
            request.current = controller;
            setBusy(true);
            setStatus("");
            try {
              const options = {
                label,
                unit,
                bounds: scope === "whole" ? bounds : range,
                timePages,
                lanesPerPage: lanes,
                stacked,
                load,
                signal: controller.signal,
                progress: setStatus,
              };
              if (desktop) {
                const saved = await streamTimeline(options);
                setStatus(saved ? "Timeline pages exported" : "Export cancelled");
                return;
              }
              const blob = await timelineDocument(options);
              if (controller.signal.aborted) return;
              const saved = await savePlot(blob, `${label}-pages`, "html");
              setStatus(saved ? "Timeline pages exported" : "Export cancelled");
            } catch (error) {
              if (!controller.signal.aborted || error instanceof TimelineCommitUnknown)
                setStatus(`Export failed: ${String(error)}`);
              else setStatus("Export cancelled");
            } finally {
              request.current = null;
              setBusy(false);
            }
          }}
        >
          Export printable HTML
        </button>
        {busy && (
          <button onClick={() => request.current?.abort()}>
            Cancel timeline export
          </button>
        )}
        {status && <span role="status">{status}</span>}
      </div>
      <p>
        All lanes in the selected scope, paginated over time. Print the
        self-contained HTML to paper or PDF. Desktop exports write incrementally and
        subdivide dense CU/utilization ranges automatically. Limits are 512 MiB,
        two million drawn records and 512 pages; larger exports fail explicitly.
        Browser downloads retain the smaller 16 MiB limit.
      </p>
    </details>
  );
}
