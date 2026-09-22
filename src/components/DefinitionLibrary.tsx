import { useRef, useState } from "react";
import { desktop } from "../bridge";
import { invoke } from "@tauri-apps/api/core";
import {
  definitionLimit,
  libraryLimit,
  libraryKey,
  defaultLibrary,
  editDefinition,
  importDefinitions,
  validateLibrary,
  type DefinitionLibrary as Library,
  type DefinitionFile,
} from "../definitionLibrary";
export function loadDefinitionLibrary(): { library: Library; error: string } {
  try {
    const stored = localStorage.getItem(libraryKey);
    if (stored)
      return { library: validateLibrary(JSON.parse(stored)), error: "" };
    const legacy = JSON.parse(
      localStorage.getItem("rocprof:tensor-definitions") || "null",
    );
    return {
      library: validateLibrary(
        defaultLibrary(typeof legacy === "string" ? legacy : undefined),
      ),
      error: "",
    };
  } catch (e) {
    return {
      library: defaultLibrary(),
      error: `Stored definitions could not be restored: ${String(e)}`,
    };
  }
}
export default function DefinitionLibrary({
  library,
  onChange,
  storageError,
  onBusy,
}: {
  library: Library;
  onChange: (library: Library) => void;
  storageError: string;
  onBusy: (busy: boolean) => void;
}) {
  const upload = useRef<HTMLInputElement>(null);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const [rename, setRename] = useState("");
  const file = library.files.find((f) => f.id === library.selected)!;
  function apply(next: () => Library) {
    try {
      onChange(next());
      setError("");
      setStatus("");
    } catch (e) {
      setError(String(e));
    }
  }
  async function importFiles(files?: FileList | null) {
    setBusy(true);
    onBusy(true);
    setError("");
    setStatus("");
    try {
      let imported: DefinitionFile[];
      if (desktop) imported = await invoke("import_definition_files");
      else {
        const selected = [...(files || [])];
        if (
          selected.length > 32 ||
          selected.reduce((n, f) => n + f.size, 0) > libraryLimit
        )
          throw Error("Import at most 32 files and 2 MiB at once");
        imported = await Promise.all(
          selected.map(async (f) => {
            if (f.size > definitionLimit)
              throw Error(`${f.name} exceeds 256 KiB`);
            return {
              name: f.name,
              text: new TextDecoder("utf-8", { fatal: true }).decode(
                await f.arrayBuffer(),
              ),
            };
          }),
        );
      }
      if (imported.length) {
        onChange(importDefinitions(library, imported));
        setStatus(
          `Imported ${imported.length} definition file(s). Existing drafts were retained.`,
        );
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      onBusy(false);
      if (upload.current) upload.current.value = "";
    }
  }
  async function exportFile() {
    setBusy(true);
    onBusy(true);
    setError("");
    setStatus("");
    try {
      const name = /\.(txt|def)$/i.test(file.name)
        ? file.name
        : `${file.name}.txt`;
      if (desktop) {
        if (
          await invoke<boolean>("export_definition_file", {
            name,
            text: file.text,
          })
        )
          setStatus(`Exported ${file.name}`);
      } else {
        const url = URL.createObjectURL(
          new Blob([file.text], { type: "text/plain;charset=utf-8" }),
        );
        const a = document.createElement("a");
        a.href = url;
        a.download = name;
        a.click();
        setTimeout(() => URL.revokeObjectURL(url), 1000);
        setStatus(`Download requested for ${name}`);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      onBusy(false);
    }
  }
  return (
    <div className="definition-library">
      <div className="toolbar-inline">
        <label>
          Definition file{" "}
          <select
            aria-label="Definition file"
            disabled={busy}
            value={library.selected}
            onChange={(e) => {
              onChange({ ...library, selected: e.target.value });
              setRename("");
              setStatus("");
            }}
          >
            {library.files.map((f) => (
              <option key={f.id} value={f.id}>
                {f.name}
              </option>
            ))}
          </select>
        </label>
        <button
          disabled={busy || library.files.length >= 32}
          onClick={() =>
            apply(() =>
              importDefinitions(library, [{ name: "untitled.def", text: "" }]),
            )
          }
        >
          New definition file
        </button>
        <button
          disabled={busy}
          onClick={() =>
            desktop ? void importFiles() : upload.current?.click()
          }
        >
          Import definitions
        </button>
        <button disabled={busy} onClick={() => void exportFile()}>
          Export definitions
        </button>
        <button
          disabled={busy || library.files.length === 1}
          onClick={() =>
            apply(() => {
              const files = library.files.filter(
                (f) => f.id !== library.selected,
              );
              return { ...library, files, selected: files[0].id };
            })
          }
        >
          Remove draft
        </button>
        <input
          type="file"
          ref={upload}
          hidden
          multiple
          accept=".txt,.def,text/plain"
          aria-label="Import definition files"
          onChange={(e) => void importFiles(e.target.files)}
        />
      </div>
      <div className="toolbar-inline">
        <input
          aria-label="Rename definition file"
          placeholder={file.name}
          value={rename}
          disabled={busy}
          onChange={(e) => setRename(e.target.value)}
        />
        <button
          disabled={busy || !rename.trim()}
          onClick={() => {
            apply(() => editDefinition(library, { name: rename.trim() }));
            setRename("");
          }}
        >
          Rename file
        </button>
      </div>
      <p className="small muted">
        Drafts are stored locally as you edit. Export writes an
        upstream-compatible .txt or .def file. Import adds drafts; it does not
        evaluate them or overwrite existing files.
      </p>
      {busy && <p role="status">Waiting for file operation…</p>}
      {status && <p role="status">{status}</p>}
      {(error || storageError) && (
        <p role="alert" className="error-text">
          {error || storageError}
        </p>
      )}
    </div>
  );
}
