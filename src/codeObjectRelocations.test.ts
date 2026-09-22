import { expect, it } from "vitest";
import { readCodeObjectRelocations as read } from "./codeObjectRelocations";
import { validAnalysisPreference } from "./analysisPreferences";

it("preserves scoped relocation identities and validates project mappings", () => {
  const rows = [
    ["a", "file:///host", "/new/host"],
    ["b", "file:///host", "/other/host"],
    ["a", "file:///changed", "/third/host"],
  ];
  expect(read(JSON.stringify(rows))).toEqual(rows);
  expect(
    validAnalysisPreference("objects.relocations", JSON.stringify(rows)),
  ).toBe(true);
  for (const value of [
    null,
    "bad",
    "{}",
    JSON.stringify([rows[0], rows[0]]),
    JSON.stringify([["a", "memory://1", "/host"]]),
    JSON.stringify([["a", "file:///host", "relative"]]),
    JSON.stringify([["a", "file:///host", "//remote/host"]]),
    JSON.stringify([["a", "file:///host", "/x\0"]]),
    JSON.stringify([["a", "file:///host", "/host", "extra"]]),
    JSON.stringify(
      Array.from({ length: 257 }, (_, i) => [
        String(i),
        "file:///host",
        "/host",
      ]),
    ),
  ]) {
    expect(read(value), String(value)).toBeNull();
    expect(validAnalysisPreference("objects.relocations", value)).toBe(false);
  }
});
