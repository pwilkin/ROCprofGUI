import { expect, it } from "vitest";
import {
  OBSERVATION_SEARCH_BATCH,
  searchObservationRows,
} from "./observationSearch";

it("matches all fields case-insensitively while preserving duplicate row identities", async () => {
  const rows = Array.from({ length: OBSERVATION_SEARCH_BATCH + 2 }, (_, i) => [
    String(i),
    i % 2 ? "µ Repeat" : "other",
  ]);
  let yields = 0;
  const matches = await searchObservationRows(
    rows,
    "µ REPEAT",
    new AbortController().signal,
    async () => {
      yields++;
    },
  );
  expect(matches).toEqual(rows.flatMap((_, i) => (i % 2 ? [i] : [])));
  expect(yields).toBe(2);
  expect(
    await searchObservationRows(rows, "", new AbortController().signal),
  ).toBeNull();
  expect(
    await searchObservationRows(
      rows,
      "absent",
      new AbortController().signal,
      async () => {},
    ),
  ).toEqual([]);
});

it("yields before scanning and stops before another batch after cancellation", async () => {
  const controller = new AbortController();
  let accesses = 0,
    yields = 0;
  const row = {
    some: () => {
      accesses++;
      return true;
    },
  } as unknown as string[];
  const rows = Array.from({ length: OBSERVATION_SEARCH_BATCH * 3 }, () => row);
  await expect(
    searchObservationRows(rows, "x", controller.signal, async () => {
      yields++;
      if (yields === 1) expect(accesses).toBe(0);
      if (yields === 2) controller.abort();
    }),
  ).rejects.toMatchObject({ name: "AbortError" });
  expect(accesses).toBe(OBSERVATION_SEARCH_BATCH);
  expect(yields).toBe(2);
  await expect(
    searchObservationRows(rows, "", controller.signal),
  ).rejects.toMatchObject({ name: "AbortError" });
});
