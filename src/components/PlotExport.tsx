import { useState } from "react";
import { savePlot } from "../plotExport";
export function PlotExport({
  format,
  label,
  generate,
}: {
  format: "png" | "svg";
  label: string;
  generate: () => Blob | Promise<Blob>;
}) {
  const [busy, setBusy] = useState(false),
    [status, setStatus] = useState("");
  return (
    <span>
      <button
        disabled={busy}
        aria-label={`Export ${label} as ${format.toUpperCase()}`}
        onClick={async () => {
          setBusy(true);
          setStatus("");
          try {
            const saved = await savePlot(await generate(), label, format);
            setStatus(saved ? "Exported" : "Export cancelled");
          } catch (e) {
            setStatus(`Export failed: ${String(e)}`);
          } finally {
            setBusy(false);
          }
        }}
      >
        {busy ? "Exporting…" : `Export ${format.toUpperCase()}`}
      </button>
      {status && <span role="status"> {status}</span>}
    </span>
  );
}
