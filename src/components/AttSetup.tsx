import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
export default function AttSetup({
  initialPath,
  onReady,
  onCancel,
}: {
  initialPath: string;
  onReady: (path: string) => void;
  onCancel: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [traces, setTraces] = useState(
    initialPath
      ? [{ path: initialPath, shaderEngine: "" }]
      : ([] as { path: string; shaderEngine: string }[]),
  );
  const [objects, setObjects] = useState<{ path: string; id: string }[]>([]);
  const [run, setRun] = useState("1"),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  useEffect(() => {
    const element = dialog.current!;
    element.showModal();
    return () => element.close();
  }, []);
  async function choose(kind: "traces" | "objects") {
    try {
      const paths = await open({ multiple: true, directory: false });
      if (!paths) return;
      const files = typeof paths === "string" ? [paths] : paths;
      if (kind === "traces")
        setTraces((previous) => [
          ...previous,
          ...files
            .filter((p) => !previous.some((t) => t.path === p))
            .map((path) => ({ path, shaderEngine: "" })),
        ]);
      else
        setObjects((previous) => [
          ...previous,
          ...files
            .filter((p) => !previous.some((t) => t.path === p))
            .map((path) => ({ path, id: "" })),
        ]);
    } catch (e) {
      setError(String(e));
    }
  }
  async function save() {
    setError("");
    try {
      if (!/^\d+$/.test(run) || Number(run) > 4294967295)
        throw Error("Run identifier must be an unsigned 32-bit integer.");
      if (!traces.length || traces.length > 256 || objects.length > 256)
        throw Error("Choose 1–256 trace files and at most 256 code objects.");
      const engines = traces.map((t) => Number(t.shaderEngine));
      if (
        traces.some((t) => !/^\d+$/.test(t.shaderEngine)) ||
        engines.some((n) => n > 255) ||
        new Set(engines).size !== engines.length
      )
        throw Error(
          "Enter a unique shader engine from 0 to 255 for each trace.",
        );
      if (
        objects.some(
          (o) =>
            !/^\d{1,20}$/.test(o.id) || BigInt(o.id) > 18446744073709551615n,
        )
      )
        throw Error(
          "Code object IDs must be unsigned decimal 64-bit integers.",
        );
      if (
        new Set(objects.map((o) => BigInt(o.id).toString())).size !==
        objects.length
      )
        throw Error("Code object IDs must be unique.");
      setBusy(true);
      const path = await invoke<string | null>("save_att_manifest", {
        manifest: {
          version: 1,
          run: Number(run),
          traces: traces.map((t) => ({
            path: t.path,
            shaderEngine: Number(t.shaderEngine),
          })),
          codeObjects: objects,
        },
      });
      if (path) onReady(path);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <dialog
      ref={dialog}
      className="modal run-dialog att-setup"
      aria-labelledby="att-setup-title"
      onCancel={(e) => {
        e.preventDefault();
        if (!busy) onCancel();
      }}
    >
      <h2 id="att-setup-title">Raw ATT input setup</h2>
      <p>
        Map each trace to its recorded shader engine and each code object to its
        recorded ID. Files can have any name. Save a reusable manifest, then
        decode this run. Original inputs are read without renaming.
      </p>
      <label>
        Run identifier{" "}
        <input
          aria-label="ATT run identifier"
          value={run}
          onChange={(e) => setRun(e.target.value)}
          disabled={busy}
        />
      </label>
      <fieldset disabled={busy}>
        <legend>Trace files</legend>
        <button onClick={() => choose("traces")}>Add ATT files</button>
        {traces.map((t, n) => (
          <div className="att-input-row" key={t.path}>
            <code title={t.path}>{t.path}</code>
            <label>
              SE{" "}
              <input
                aria-label={`Shader engine ${n + 1}`}
                value={t.shaderEngine}
                onChange={(e) =>
                  setTraces((rows) =>
                    rows.map((r, i) =>
                      i === n ? { ...r, shaderEngine: e.target.value } : r,
                    ),
                  )
                }
              />
            </label>
            <button
              aria-label={`Remove trace ${n + 1}`}
              onClick={() =>
                setTraces((rows) => rows.filter((_, i) => i !== n))
              }
            >
              Remove
            </button>
          </div>
        ))}
      </fieldset>
      <fieldset disabled={busy}>
        <legend>Code objects</legend>
        <button onClick={() => choose("objects")}>Add code objects</button>
        {objects.map((o, n) => (
          <div className="att-input-row" key={o.path}>
            <code title={o.path}>{o.path}</code>
            <label>
              ID{" "}
              <input
                aria-label={`Code object ID ${n + 1}`}
                value={o.id}
                onChange={(e) =>
                  setObjects((rows) =>
                    rows.map((r, i) =>
                      i === n ? { ...r, id: e.target.value } : r,
                    ),
                  )
                }
              />
            </label>
            <button
              aria-label={`Remove code object ${n + 1}`}
              onClick={() =>
                setObjects((rows) => rows.filter((_, i) => i !== n))
              }
            >
              Remove
            </button>
          </div>
        ))}
      </fieldset>
      {!objects.length && (
        <p>
          No code objects selected. ISA/source resolution may be unavailable;
          unrelated files will not be inferred from the directory.
        </p>
      )}
      {error && <p role="alert">{error}</p>}
      <div className="toolbar-inline">
        <button disabled={busy} onClick={onCancel}>
          Cancel ATT setup
        </button>
        <button disabled={busy || !traces.length} onClick={save}>
          {busy ? "Saving manifest…" : "Save manifest and decode"}
        </button>
      </div>
    </dialog>
  );
}
