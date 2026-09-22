import { expect, it } from "vitest";
import { parseCodeFont } from "./codeFont";

it("accepts supported point sizes and defaults malformed stored preferences", () => {
  for (let size = 5; size <= 19; size++) expect(parseCodeFont(size)).toBe(size);
  for (const value of [
    null,
    undefined,
    "12",
    {},
    [],
    4,
    20,
    9.5,
    NaN,
    Infinity,
  ])
    expect(parseCodeFont(value)).toBe(9);
});
