import { describe, expect, it } from "vitest";
import {
  defaultLibrary,
  editDefinition,
  importDefinitions,
  validateLibrary,
  definitionLimit,
} from "./definitionLibrary";
describe("definition files", () => {
  it("imports separate drafts, preserves names/text, and keeps collisions distinct", () => {
    const initial = defaultLibrary("Old := 1");
    const next = importDefinitions(initial, [
      { name: "derived_counters.txt", text: "# μ\r\nNew := 2\r\n" },
    ]);
    expect(next.files.map((f) => f.name)).toEqual([
      "derived_counters.txt",
      "derived_counters (2).txt",
    ]);
    expect(next.files[1].text).toBe("# μ\r\nNew := 2\r\n");
    expect(next.selected).toBe(next.files[1].id);
    expect(initial.files).toHaveLength(1);
    const changed = editDefinition(next, { text: "Changed := 3" });
    expect(changed.files[0].text).toBe("Old := 1");
    expect(validateLibrary(JSON.parse(JSON.stringify(changed)))).toEqual(
      changed,
    );
  });
  it("rejects malformed libraries, filename collisions and bounded UTF-8 data atomically", () => {
    const library = defaultLibrary();
    for (const value of [
      null,
      {},
      { ...library, version: 2 },
      { ...library, selected: "missing" },
      { ...library, files: [null] },
    ])
      expect(() => validateLibrary(value)).toThrow();
    expect(() =>
      importDefinitions(library, [{ name: "../outside.def", text: "" }]),
    ).toThrow();
    expect(() =>
      importDefinitions(library, [
        { name: "ok.def", text: "" },
        { name: "large.def", text: "μ".repeat(definitionLimit) },
      ]),
    ).toThrow("256 KiB");
    expect(() =>
      importDefinitions(library, [{ name: "binary.def", text: "x\0y" }]),
    ).toThrow();
    expect(library.files).toHaveLength(1);
    const next = importDefinitions(library, [{ name: "second.def", text: "" }]);
    expect(() => editDefinition(next, { name: library.files[0].name })).toThrow(
      "Duplicate",
    );
    expect(() =>
      importDefinitions(
        library,
        Array.from({ length: 32 }, () => ({ name: "a.def", text: "" })),
      ),
    ).toThrow("1–32");
  });
});
