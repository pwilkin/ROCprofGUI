import { useAnalysisPreference } from "../analysisPreferences";
import { useEffect, useMemo, useRef, useState } from "react";
import { cancelImport, desktop, extractCodeObject } from "../bridge";
import type { CodeObjectRecord } from "../types";
import { readCodeObjectRelocations } from "../codeObjectRelocations";
export default function CodeObjects({
  records,
  captureId,
}: {
  records: CodeObjectRecord[];
  captureId: string;
}) {
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const identity = useMemo(
    () => ({ captureId, records }),
    [captureId, records],
  );
  const currentIdentity = useRef(identity);
  currentIdentity.current = identity;
  const mounted = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    setBusy(false);
    setStatus("");
  }, [identity]);
  const [relocations, setRelocations] = useAnalysisPreference<string>(
    "objects.relocations",
    "[]",
  );
  const mappings = readCodeObjectRelocations(relocations) ?? [];
  async function extract(
    record: CodeObjectRecord,
    chooseSource = false,
    sourcePath?: string,
  ) {
    const current = () =>
      mounted.current && currentIdentity.current === identity;
    setBusy(true);
    setStatus("");
    try {
      const result = await extractCodeObject(
        captureId,
        record.id,
        chooseSource,
        sourcePath,
      );
      if (!current()) return;
      let retentionNote = "";
      if (result?.remapped && result.recordedUri === record.uri) {
        const next = JSON.stringify([
          ...mappings.filter(
            ([id, uri]) => id !== record.id || uri !== record.uri,
          ),
          [record.id, record.uri, result.source],
        ]);
        if (readCodeObjectRelocations(next)) setRelocations(next);
        else
          retentionNote =
            " Container path was not retained: project mapping limits were exceeded or the path is invalid.";
      }
      setStatus(
        result
          ? `Saved ${result.bytes} bytes to ${result.path}. ${result.remapped ? "User-selected replacement container" : "Source"}: ${result.source}, offset ${result.offset}. Recorded URI: ${result.recordedUri ?? record.uri}. SHA-256: ${result.sha256}. Identity at profiling time remains unverified.${retentionNote}`
          : "Extraction cancelled.",
      );
    } catch (error) {
      if (current()) setStatus(String(error));
    } finally {
      if (current()) setBusy(false);
    }
  }
  const [search, setSearch] = useAnalysisPreference<string>(
    "objects.search",
    "",
  );
  const [page, setPage] = useAnalysisPreference<number>("objects.page", 0);
  if (!records.length) return null;
  const filtered = records.filter((record) =>
    [record.objectId, record.scope, record.uri, record.agent]
      .join(" ")
      .toLowerCase()
      .includes(search.toLowerCase()),
  );
  const pages = Math.max(1, Math.ceil(filtered.length / 50));
  const current = Math.min(page, pages - 1);
  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h3>Recorded code objects</h3>
          <p>
            Database provenance only. A URI does not establish that a binary is
            available or matches an ATT capture.
          </p>
        </div>
      </div>
      <label>
        Search code objects{" "}
        <input
          aria-label="Search code objects"
          value={search}
          onChange={(event) => {
            setSearch(event.target.value);
            setPage(0);
          }}
        />
      </label>
      <p>
        Extract a recorded file range in the desktop app (maximum 256 MiB).
        Choose a new output file; existing files are preserved.
      </p>
      {status && (
        <p role="status" className="summary-note">
          {status}
        </p>
      )}
      {busy && (
        <button
          onClick={() =>
            void cancelImport().catch((error) => setStatus(String(error)))
          }
        >
          Cancel code-object extraction
        </button>
      )}
      <p>
        {filtered.length} matching records · page {current + 1}/{pages}
      </p>
      {filtered.slice(current * 50, (current + 1) * 50).map((record) => {
        const retained = mappings.find(
          ([id, uri]) => id === record.id && uri === record.uri,
        )?.[2];
        return (
          <details key={record.id} className="counter-definition">
            <summary>
              Object {record.objectId} · {record.uri ?? "URI not recorded"}
            </summary>
            <dl>
              <dt>Scope</dt>
              <dd>{record.scope}</dd>
              <dt>Agent</dt>
              <dd>{record.agent ?? "Not recorded"}</dd>
              <dt>Storage</dt>
              <dd>{record.storage ?? "Not recorded"}</dd>
              <dt>Load base</dt>
              <dd>{record.loadBase ?? "Not recorded"}</dd>
              <dt>Load size</dt>
              <dd>{record.loadSize ?? "Not recorded"}</dd>
              <dt>Load delta</dt>
              <dd>{record.loadDelta ?? "Not recorded"}</dd>
            </dl>
            {record.uri?.startsWith("file://") ? (
              <div>
                <button
                  disabled={!desktop || busy}
                  onClick={() => void extract(record)}
                >
                  {busy ? "Extracting…" : "Extract recorded file range…"}
                </button>
                <button
                  disabled={!desktop || busy}
                  onClick={() => void extract(record, true)}
                >
                  Select relocated container…
                </button>
                <p>
                  A relocated container uses the recorded offset and size.
                  Select the original host binary or container, not an already
                  extracted object. The recorded URI remains unchanged.
                </p>
                {retained && (
                  <div>
                    <p>
                      Retained container: {retained}. Saved with this capture in
                      the project. Each extraction revalidates the range and
                      reports its SHA-256; profiling-time identity remains
                      unverified.
                    </p>
                    <button
                      disabled={!desktop || busy}
                      onClick={() => void extract(record, false, retained)}
                    >
                      Extract from retained container…
                    </button>
                    <button
                      disabled={busy}
                      onClick={() =>
                        setRelocations((previous) =>
                          JSON.stringify(
                            (readCodeObjectRelocations(previous) ?? []).filter(
                              ([id, uri]) =>
                                id !== record.id || uri !== record.uri,
                            ),
                          ),
                        )
                      }
                    >
                      Forget retained container
                    </button>
                  </div>
                )}
              </div>
            ) : (
              <p>
                {record.uri?.startsWith("memory://")
                  ? "Memory-only URI: bytes must be captured while the process is available."
                  : "No supported file-backed URI was recorded."}
              </p>
            )}
          </details>
        );
      })}
      <button disabled={current === 0} onClick={() => setPage(current - 1)}>
        Previous code objects
      </button>
      <button
        disabled={current + 1 >= pages}
        onClick={() => setPage(current + 1)}
      >
        Next code objects
      </button>
    </section>
  );
}
