import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import {
  findIsaInstruction,
  instructionMnemonic,
  matrixTypes,
  type IsaDocument,
  type IsaManifest,
} from "../src/isaReference";
const manifest: IsaManifest = JSON.parse(
  readFileSync("public/isa/manifest.json", "utf8"),
);
const load = (id: string): IsaDocument =>
  JSON.parse(readFileSync(`public/isa/${id}.json`, "utf8"));
describe("AMD offline ISA reference", () => {
  it("covers all nine architectures and preserves resolvable definitions", () => {
    expect(manifest.architectures.map((a) => a.id)).toEqual([
      "cdna1",
      "cdna2",
      "cdna3",
      "cdna4",
      "rdna1",
      "rdna2",
      "rdna3",
      "rdna3_5",
      "rdna4",
    ]);
    expect(manifest.architectures.reduce((n, a) => n + a.count, 0)).toBe(10628);
    for (const a of manifest.architectures) {
      const d = load(a.id);
      expect(d.instructions).toHaveLength(a.count);
      expect(a.sha256).toMatch(/^[a-f0-9]{64}$/);
      expect(a.license).toBe("MIT");
      expect(
        d.instructions
          .filter((i) => /V_(S?MFMA|S?WMMA)/.test(i.name))
          .every((i) => i.matrix),
      ).toBe(true);
      for (const i of d.instructions)
        for (const e of i.encodings) {
          expect(d.encodings[e.name], `${a.id}: ${e.name}`).toBeDefined();
          for (const o of e.operands)
            expect(
              d.formats[o.format],
              `${a.id}: ${i.name}: ${o.format}`,
            ).toBeDefined();
        }
    }
  });
  it("distinguishes native matrix families and architecture type additions", () => {
    const a = (id: string) => manifest.architectures.find((a) => a.id === id)!;
    expect(a("rdna1").matrix).toHaveLength(0);
    expect(a("rdna2").matrix).toHaveLength(0);
    expect(a("rdna3").matrix).toHaveLength(6);
    expect(a("rdna3_5").matrix).toHaveLength(6);
    expect(matrixTypes(a("rdna3").matrix)).toEqual([
      "BF16 → BF16",
      "BF16 → F32",
      "F16 → F16",
      "F16 → F32",
      "IU4 → I32",
      "IU8 → I32",
    ]);
    expect(a("rdna4").matrix.filter((i) => i.family === "WMMA")).toHaveLength(
      11,
    );
    expect(a("rdna4").matrix.filter((i) => i.family === "SWMMAC")).toHaveLength(
      11,
    );
    expect(matrixTypes(a("rdna4").matrix)).toContain("FP8_BF8 → F32");
    expect(a("cdna1").matrix.every((i) => i.family === "MFMA")).toBe(true);
    expect(matrixTypes(a("cdna1").matrix)).not.toContain("F64 → F64");
    expect(matrixTypes(a("cdna2").matrix)).toContain("F64 → F64");
    expect(a("cdna4").matrix.filter((i) => i.scaled)).toHaveLength(2);
    expect(matrixTypes(a("cdna4").matrix)).toContain("F8F6F4 → F32");
  });
  it("resolves disassembly suffixes and old CDNA naming without changing source names", () => {
    expect(instructionMnemonic("  v_add_f32_e32 v1, v2, v3")).toBe("V_ADD_F32");
    expect(
      findIsaInstruction(
        load("cdna1"),
        "v_mfma_f32_16x16x16_f16 a[0:3], v1, v2, a[0:3]",
      )?.name,
    ).toBe("V_MFMA_F32_16X16X16F16");
    expect(
      findIsaInstruction(load("rdna3_5"), "ds_read_b32 v5, v1")?.name,
    ).toBe("DS_LOAD_B32");
    expect(
      findIsaInstruction(load("rdna2"), "v_wmma_f32_16x16x16_f16"),
    ).toBeUndefined();
  });
});
