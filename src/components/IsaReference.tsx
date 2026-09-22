import { useEffect, useMemo, useState } from "react";
import { BookOpen, Search } from "lucide-react";
import { Panel } from "./AnalysisViews";
import {
  findIsaInstruction,
  instructionMnemonic,
  matrixTypes,
  mnemonicKey,
  type IsaManifest,
  type IsaDocument,
  type IsaInstruction,
} from "../isaReference";

export default function IsaReference({ lookup = "", architecture, revision }: { lookup?: string; architecture?: string; revision?: number }) {
  const [manifest, setManifest] = useState<IsaManifest>();
  const [arch, setArch] = useState("rdna3_5");
  const [document, setDocument] = useState<IsaDocument>();
  const [error, setError] = useState("");
  const [query, setQuery] = useState(instructionMnemonic(lookup));
  const [selected, setSelected] = useState(instructionMnemonic(lookup));
  useEffect(() => {
    if (revision === undefined) return;
    if (architecture && architecture !== "manifest") setArch(architecture);
    setQuery(instructionMnemonic(lookup));
    setSelected(instructionMnemonic(lookup));
  }, [lookup, architecture, revision]);
  const [group, setGroup] = useState("all");
  const [matrixOnly, setMatrixOnly] = useState(false);
  const [page, setPage] = useState(0);
  const [mode, setMode] = useState<"instructions" | "matrix">("instructions");
  const [encoding, setEncoding] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    fetch("/isa/manifest.json", { signal: controller.signal })
      .then((r) => {
        if (!r.ok) throw new Error(`ISA catalogue: ${r.status}`);
        return r.json();
      })
      .then(setManifest)
      .catch((e) => {
        if (!controller.signal.aborted) setError(String(e));
      });
    return () => controller.abort();
  }, []);
  useEffect(() => {
    const controller = new AbortController();
    setDocument(undefined);
    setError("");
    fetch(`/isa/${arch}.json`, { signal: controller.signal })
      .then((r) => {
        if (!r.ok) throw new Error(`ISA document: ${r.status}`);
        return r.json();
      })
      .then(setDocument)
      .catch((e) => {
        if (!controller.signal.aborted) setError(String(e));
      });
    return () => controller.abort();
  }, [arch]);
  useEffect(() => {
    setPage(0);
  }, [query, group, matrixOnly, arch]);
  useEffect(() => {
    setEncoding(0);
  }, [selected, arch]);
  const metadata = manifest?.architectures.find((a) => a.id === arch);
  const rows = useMemo(
    () =>
      (document?.instructions || []).filter(
        (i) =>
          (group === "all" || i.group === group) &&
          (!matrixOnly || !!i.matrix) &&
          (!query ||
            `${i.name} ${i.description} ${i.subgroups.join(" ")}`
              .toLowerCase()
              .includes(query.toLowerCase()) ||
            mnemonicKey(i.name).includes(mnemonicKey(query))),
      ),
    [document, query, group, matrixOnly],
  );
  const instruction = document && findIsaInstruction(document, selected);
  const choose = (i: IsaInstruction) => {
    setSelected(i.name);
    setEncoding(0);
  };
  const variant =
    instruction?.encodings[
      Math.min(encoding, instruction.encodings.length - 1)
    ];
  const encodingDoc = variant && document?.encodings[variant.name];
  const safePage = Math.min(page, Math.max(0, Math.ceil(rows.length / 60) - 1));
  const openMatrix = (id: string, name: string) => {
    setArch(id);
    setQuery(name);
    setSelected(name);
    setMatrixOnly(false);
    setGroup("all");
    setMode("instructions");
  };
  const manualNote =
    arch === "cdna4"
      ? {
          title: "CDNA 4: F8F6F4 format selection",
          text: "CBSZ selects A and BLGP selects B independently. Selectors 0–4 mean FP8 E4M3, BF8 E5M2, FP6 E2M3, BF6 E3M2, and FP4 E2M1. The accumulator is F32. Scaled variants add block exponents; their block size along K is 32. These selectors are instruction-specific, not the ordinary MFMA broadcast controls.",
          url: "https://www.amd.com/content/dam/amd/en/documents/instinct-tech-docs/instruction-set-architectures/amd-instinct-cdna4-instruction-set-architecture.pdf#page=58",
          section: "CDNA 4 manual, sections 7.1.4 and 7.2",
        }
      : arch === "rdna3_5"
        ? {
            title: "RDNA 3.5: WMMA operand layout",
            text: "WMMA accepts wave32 and wave64, but operand packing must match the wave size. A/B data in lanes 0–15 is repeated in lanes 16–31 and, for wave64, in the other two 16-lane groups. IU4/IU8 signedness is chosen separately for A and B using NEG[0] and NEG[1]; integer outputs are signed I32. Floating WMMA uses round-to-nearest-even.",
            url: "https://docs.amd.com/api/khub/documents/UVVZM22UN7tMUeiW_4ShTQ/content#page=84",
            section: "RDNA 3.5 manual, section 7.9",
          }
        : arch === "rdna4"
          ? {
              title: "RDNA 4: dense and sparse WMMA",
              text: "WMMA supports wave32 and wave64 with different register mappings. SWMMAC uses a structured 2:4 sparse A matrix, dense B, and explicit sparse indices. FP8 is E4M3 and BF8 is E5M2; floating-point mode affects bias and special values, so the names alone do not specify a byte-compatible format.",
              url: "https://docs.amd.com/api/khub/documents/uQpkEvk3pv~kfAb2x~j4uw/content#page=99",
              section: "RDNA 4 manual, sections 7.5 and 7.12",
            }
          : undefined;
  return (
    <div className="isa-reference">
      <Panel
        title="Architecture instruction reference"
        detail="AMD documentation · available offline · architecture-specific definitions"
      >
        <div className="isa-reference-toolbar">
          <BookOpen size={22} />
          <label>
            Reference architecture{" "}
            <select
              aria-label="Reference architecture"
              value={arch}
              onChange={(e) => setArch(e.target.value)}
            >
              {manifest?.architectures.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          </label>
          <button
            className={mode === "instructions" ? "active" : ""}
            onClick={() => setMode("instructions")}
          >
            Instruction catalogue
          </button>
          <button
            className={mode === "matrix" ? "active" : ""}
            onClick={() => setMode("matrix")}
          >
            Matrix data types
          </button>
        </div>
        <p className="isa-note">
          The selected reference architecture is independent of the capture.
          Availability in this specification does not establish compiler
          support, runtime device support, or measured throughput.
        </p>
        {error && <p role="alert">{error}</p>}
        {metadata && (
          <div className="isa-provenance">
            {metadata.count.toLocaleString()} instructions · XML release{" "}
            {metadata.release} · schema {metadata.schema} · {metadata.license}
            {" · "}
            <a href={manifest?.source} target="_blank" rel="noreferrer">
              AMD XML specifications
            </a>
            {" · "}
            <a href={manifest?.manuals} target="_blank" rel="noreferrer">
              Architecture manuals
            </a>
            <details>
              <summary>Source provenance and license</summary>
              <p>{metadata.copyright}</p>
              <p>{metadata.file}</p>
              <code>SHA-256 {metadata.sha256}</code>
              <p>
                <a href="/isa/LICENSE.txt" target="_blank" rel="noreferrer">
                  MIT license
                </a>
              </p>
            </details>
          </div>
        )}
      </Panel>
      {manualNote && (
        <Panel title={manualNote.title}>
          <p className="isa-note">
            {manualNote.text}{" "}
            <a href={manualNote.url} target="_blank" rel="noreferrer">
              {manualNote.section}
            </a>
          </p>
        </Panel>
      )}
      {mode === "matrix" ? (
        <Panel
          title="Matrix instruction and data-type compatibility"
          detail="Exact variants in the bundled AMD specifications"
        >
          <p className="isa-note">
            Pairs below show multiplicand type(s) → result/accumulator type from
            instruction names. IU4/IU8 denote integer formats with signedness
            controls; mixed FP8/BF8 pairs preserve A/B order. F8F6F4 names
            require format selectors: inspect the encoding fields before use.
            Sparse instructions and scaled variants are listed separately.
            Register sizes in the operand tables are not element precision.
          </p>
          <div className="table-scroll">
            <table className="isa-matrix">
              <thead>
                <tr>
                  <th>Architecture</th>
                  <th>Instruction family</th>
                  <th>Input → accumulator types</th>
                  <th>Exact variants / M×N×K</th>
                </tr>
              </thead>
              <tbody>
                {manifest?.architectures.flatMap((a) => {
                  const families = [
                    ...new Set(
                      a.matrix.map(
                        (m) => `${m.family}${m.scaled ? " SCALE" : ""}`,
                      ),
                    ),
                  ];
                  if (!families.length)
                    return [
                      <tr key={a.id}>
                        <td>{a.name}</td>
                        <td colSpan={3}>
                          No WMMA / MFMA matrix instructions listed
                        </td>
                      </tr>,
                    ];
                  return families.map((f) => {
                    const entries = a.matrix.filter(
                      (m) => `${m.family}${m.scaled ? " SCALE" : ""}` === f,
                    );
                    return (
                      <tr key={a.id + f}>
                        <td>{a.name}</td>
                        <td>
                          {f}
                          <small>
                            {f.startsWith("S") ? "Sparse" : "Dense"}
                          </small>
                        </td>
                        <td>
                          {matrixTypes(entries).map((t) => (
                            <div key={t}>
                              <code>{t}</code>
                            </div>
                          ))}
                        </td>
                        <td>
                          <details>
                            <summary>
                              {entries.length} instruction variants
                            </summary>
                            {entries.map((m) => (
                              <button
                                className="isa-variant-link"
                                key={m.name}
                                onClick={() => openMatrix(a.id, m.name)}
                              >
                                {m.name}
                                <small>
                                  {m.shape}
                                  {m.blocks && ` · ${m.blocks} block suffix`}
                                </small>
                              </button>
                            ))}
                          </details>
                        </td>
                      </tr>
                    );
                  });
                })}
              </tbody>
            </table>
          </div>
        </Panel>
      ) : (
        <div className="isa-reference-grid">
          <Panel
            title="Instruction catalogue"
            detail={`${rows.length.toLocaleString()} matching instructions`}
          >
            <div className="isa-search">
              <Search size={16} />
              <input
                aria-label="Search instruction reference"
                placeholder="Mnemonic, operation, or subgroup…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </div>
            <div className="isa-reference-toolbar">
              <select
                aria-label="Instruction group"
                value={group}
                onChange={(e) => setGroup(e.target.value)}
              >
                <option value="all">All instruction groups</option>
                {[...new Set(document?.instructions.map((i) => i.group))]
                  .sort()
                  .map((g) => (
                    <option key={g}>{g}</option>
                  ))}
              </select>
              <label>
                <input
                  type="checkbox"
                  checked={matrixOnly}
                  onChange={(e) => setMatrixOnly(e.target.checked)}
                />{" "}
                Matrix only
              </label>
            </div>
            {!document && !error && (
              <p className="isa-note">Loading local reference…</p>
            )}
            <div className="isa-list">
              {rows.slice(safePage * 60, (safePage + 1) * 60).map((i) => (
                <button
                  key={i.name}
                  className={instruction?.name === i.name ? "selected" : ""}
                  onClick={() => choose(i)}
                >
                  <code>{i.name}</code>
                  <small>
                    {i.group}{" "}
                    {i.subgroups
                      .filter((s) => s !== "NOT_ASSIGNED")
                      .join(" · ")}
                  </small>
                </button>
              ))}
            </div>
            {document && !rows.length && (
              <p className="isa-note">
                No matching instruction in {metadata?.name}. Try another
                architecture or a shorter mnemonic.
              </p>
            )}
            <div className="plot-footer">
              <button
                disabled={!safePage}
                onClick={() => setPage(safePage - 1)}
              >
                Previous
              </button>
              <span>
                {safePage + 1} / {Math.max(1, Math.ceil(rows.length / 60))}
              </span>
              <button
                disabled={(safePage + 1) * 60 >= rows.length}
                onClick={() => setPage(safePage + 1)}
              >
                Next
              </button>
            </div>
          </Panel>
          <Panel
            title={instruction?.name || "Instruction details"}
            detail={metadata?.name || "Choose an architecture"}
          >
            {!instruction ? (
              <div className="isa-note">
                <p>
                  {selected
                    ? `${selected} is not listed in this architecture's specification.`
                    : "Select an instruction to inspect its operation, encodings and operands."}
                </p>
                {selected && manifest && (
                  <p>
                    Matching specifications:{" "}
                    {manifest.architectures
                      .filter((a) =>
                        a.mnemonics.some(
                          (n) => mnemonicKey(n) === mnemonicKey(selected),
                        ),
                      )
                      .map((a) => (
                        <button key={a.id} onClick={() => setArch(a.id)}>
                          {a.name}
                        </button>
                      ))}
                  </p>
                )}
              </div>
            ) : (
              <div className="isa-detail">
                {selected &&
                  instructionMnemonic(selected) !== instruction.name && (
                    <p className="isa-alias">
                      Lookup spelling: <code>{selected}</code>. AMD
                      specification name: <code>{instruction.name}</code>.
                    </p>
                  )}
                <p className="isa-description">
                  {instruction.description ||
                    "No operation description supplied in this specification."}
                </p>
                {instruction.matrix && (
                  <div className="isa-matrix-summary">
                    <strong>
                      {instruction.matrix.family}
                      {instruction.matrix.scaled && " SCALE"}
                    </strong>
                    <span>
                      {instruction.matrix.shape} · {instruction.matrix.inputs} →{" "}
                      {instruction.matrix.accumulator}
                    </span>
                  </div>
                )}
                {!!instruction.flags.length && (
                  <p>{instruction.flags.join(" · ")}</p>
                )}
                <label>
                  Encoding{" "}
                  <select
                    aria-label="Instruction encoding"
                    value={Math.min(encoding, instruction.encodings.length - 1)}
                    onChange={(e) => setEncoding(Number(e.target.value))}
                  >
                    {instruction.encodings.map((e, n) => (
                      <option key={n} value={n}>
                        {e.name} · {e.condition} · opcode {e.opcode} (base{" "}
                        {e.radix})
                      </option>
                    ))}
                  </select>
                </label>
                <p className="isa-description">
                  {encodingDoc?.description}{" "}
                  {encodingDoc?.bits && `(${encodingDoc.bits} encoding bits)`}
                </p>
                <div className="table-scroll">
                  <table>
                    <thead>
                      <tr>
                        <th>Operand</th>
                        <th>Direction</th>
                        <th>Register class / size</th>
                        <th>Data format</th>
                      </tr>
                    </thead>
                    <tbody>
                      {variant?.operands.map((o, n) => (
                        <tr key={n}>
                          <td>
                            {o.field || "(implicit)"}
                            {o.implicit && <small>Implicit</small>}
                          </td>
                          <td>
                            {[o.input && "Input", o.output && "Output"]
                              .filter(Boolean)
                              .join(" / ")}
                          </td>
                          <td>
                            <code>{o.type}</code>
                            <small>{o.bits} bits</small>
                          </td>
                          <td>
                            <code>{o.format}</code>
                            <small>
                              {document?.formats[o.format]?.type} ·{" "}
                              {document?.formats[o.format]?.components || "?"}{" "}
                              component(s)
                            </small>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                {[...new Set(variant?.operands.map((o) => o.format))].map(
                  (f) => (
                    <details key={f}>
                      <summary>{f}</summary>
                      <p className="isa-description">
                        {document?.formats[f]?.description ||
                          "No format description supplied."}
                      </p>
                    </details>
                  ),
                )}
                <details>
                  <summary>Encoding fields and modifiers</summary>
                  {encodingDoc?.fields.map((f, n) => (
                    <div key={n}>
                      <strong>{f.name}</strong>
                      <small>
                        {f.ranges
                          .map((r) => `offset ${r.offset}, ${r.bits} bits`)
                          .join(" · ")}
                      </small>
                      <p className="isa-description">
                        {f.description || "No field description supplied."}
                      </p>
                    </div>
                  ))}
                </details>
                <details>
                  <summary>
                    Also listed in these architecture specifications
                  </summary>
                  <div className="isa-reference-toolbar">
                    {manifest?.architectures
                      .filter((a) =>
                        a.mnemonics.some(
                          (n) =>
                            mnemonicKey(n) === mnemonicKey(instruction.name),
                        ),
                      )
                      .map((a) => (
                        <button key={a.id} onClick={() => setArch(a.id)}>
                          {a.name}
                        </button>
                      ))}
                  </div>
                  <p>
                    Matching names do not imply identical operand layouts or
                    semantics. Select the architecture to inspect its
                    definition.
                  </p>
                </details>
              </div>
            )}
          </Panel>
        </div>
      )}
    </div>
  );
}
