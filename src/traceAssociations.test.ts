import { expect, it } from "vitest";
import { readTraceAssociations as read } from "./traceAssociations";
import { validAnalysisPreference } from "./analysisPreferences";
it("retains explicit trace ownership and rejects invalid or ambiguous serialized claims", () => {
  for (const mode of ["unknown", "single-dispatch", "region"]) {
    const rows = [
      {
        path: "/data/trace.rocprof-att",
        mode,
        dispatch: mode === "single-dispatch" ? "partition:guid:42" : "",
      },
    ];
    expect(read(JSON.stringify(rows))).toEqual(rows);
    expect(
      validAnalysisPreference("att.associations", JSON.stringify(rows)),
    ).toBe(true);
  }
  const row = { path: "/trace.rocprof-att", mode: "unknown", dispatch: "" };
  for (const value of [
    null,
    "invalid",
    "{}",
    JSON.stringify([row, row]),
    JSON.stringify([{ ...row, mode: "automatic" }]),
    JSON.stringify([{ ...row, dispatch: "42" }]),
    JSON.stringify([{ ...row, mode: "single-dispatch" }]),
    JSON.stringify([{ ...row, path: "relative.rocprof-att" }]),
    JSON.stringify([{ ...row, path: "/trace.att" }]),
    JSON.stringify([{ ...row, path: "/trace\0.rocprof-att" }]),
    JSON.stringify([{ ...row, extra: true }]),
    JSON.stringify(
      Array.from({ length: 65 }, (_, i) => ({
        ...row,
        path: `/${i}.rocprof-att`,
      })),
    ),
  ]) {
    expect(read(value)).toBeNull();
    expect(validAnalysisPreference("att.associations", value)).toBe(false);
  }
});

it("preserves recorded provenance with a validated optional replacement", () => {
  const original = {path:"/original.rocprof-att",mode:"single-dispatch",dispatch:"scope:42"};
  for (const relocation of ["", "/moved/trace.rocprof-att"]) {
    const row = {...original, relocation};
    expect(read(JSON.stringify([row]))).toEqual([row]);
  }
  for (const relocation of [null, 1, "relative.rocprof-att", "//host/trace.rocprof-att", "/trace.att", "/bad\0.rocprof-att", "/" + "x".repeat(8192) + ".rocprof-att"]) {
    expect(read(JSON.stringify([{...original, relocation}]))).toBeNull();
  }
});
