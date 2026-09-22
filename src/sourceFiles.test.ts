import { expect, it } from "vitest";
import {
  mapSource,
  mapSourceBatch,
  MAX_SOURCE_BYTES,
  type SourceMappings,
} from "./sourceFiles";
it("maps exact paths independently, supports empty files and restores snapshots", () => {
  const a = mapSource({}, "/build/a.hip", { name: "local.hip", text: "" });
  const b = mapSource(a, "/other/a.hip", { name: "local.hip", text: "other" });
  expect(b["/build/a.hip"].text).toBe("");
  expect(Object.keys(a)).toEqual(["/build/a.hip"]);
  expect(mapSource(b, "/build/a.hip", null)).toEqual({
    "/other/a.hip": { name: "local.hip", text: "other" },
  });
});
it("rejects invalid and oversized text atomically, including the combined budget", () => {
  const original = mapSource({}, "a", { name: "a", text: "valid" });
  expect(() => mapSource(original, "a", { name: "a", text: "a\0b" })).toThrow(
    "NUL",
  );
  expect(original.a.text).toBe("valid");
  expect(() =>
    mapSource({}, "a", { name: "a", text: "α".repeat(MAX_SOURCE_BYTES) }),
  ).toThrow("2 MiB");
  let mappings: SourceMappings = {};
  const file = { name: "large", text: "x".repeat(MAX_SOURCE_BYTES) };
  for (let i = 0; i < 8; i++) mappings = mapSource(mappings, String(i), file);
  expect(() => mapSource(mappings, "9", file)).toThrow("16 MiB");
  expect(Object.keys(mappings)).toHaveLength(8);
});

it("applies a folder batch atomically and budgets the final replacement set", () => {
  const original = { a: { name: "a", text: "old" } };
  expect(() =>
    mapSourceBatch(original, {
      b: { name: "b", text: "new" },
      c: { name: "c", text: "\0" },
    }),
  ).toThrow();
  expect(Object.keys(original)).toEqual(["a"]);
  const large = { name: "large", text: "x".repeat(MAX_SOURCE_BYTES) };
  const full = Object.fromEntries(
    Array.from({ length: 8 }, (_, i) => [String(i), large]),
  );
  const result = mapSourceBatch(full, {
    added: large,
    "7": { name: "empty", text: "" },
  });
  expect(result.added.text.length).toBe(MAX_SOURCE_BYTES);
  expect(result["7"].text).toBe("");
});

it("folder mappings replace legacy relative aliases without merging distinct registered paths", () => {
  const old = { "a.hip": { name: "old", text: "old" } };
  const files = { "/a.hip": { name: "new", text: "new" } };
  expect(Object.keys(mapSourceBatch(old, files, ["/a.hip"]))).toEqual([
    "/a.hip",
  ]);
  expect(
    Object.keys(mapSourceBatch(old, files, ["a.hip", "/a.hip"])),
  ).toHaveLength(2);
});

it("keeps large file references compact and rejects ambiguous or invalid references", () => {
  const file = {
    name: "large.hip",
    text: "",
    reference: {
      path: "/sources/large.hip",
      size: 400_000_000,
      sha256: "a".repeat(64),
    },
  };
  expect(mapSource({}, "original", file).original).toEqual(file);
  expect(() =>
    mapSource({}, "original", { ...file, text: "ambiguous" }),
  ).toThrow();
  for (const patch of [
    { path: "relative.hip" },
    { size: -1 },
    { size: 600_000_000 },
    { sha256: "unknown" },
  ]) {
    expect(() =>
      mapSource({}, "original", {
        ...file,
        reference: { ...file.reference, ...patch },
      }),
    ).toThrow();
  }
});
