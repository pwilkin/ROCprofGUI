import { expect, test } from "vitest";
import { activityCost, requestActivityCost } from "./summary";
import type { Instruction } from "./types";
const instruction = (idle: number, stall: number, latency: number) =>
  ({ idle, stall, latency }) as Instruction;

test("batched activity preserves original summation across chunk boundaries", async () => {
  const rows = Array.from({ length: 20001 }, (_, i) =>
    instruction(i % 7 / 10, i % 13 / 10, i % 13 / 10 + i % 17 / 10));
  let yields = 0;
  const result = await requestActivityCost(rows, new AbortController().signal, async () => { yields++; });
  expect(result).toEqual(activityCost(rows));
  expect(yields).toBe(5);
  for (const invalid of [instruction(NaN, 1, 2), instruction(0, 3, 2), instruction(-1, 0, 1)]) {
    const input = [...rows, invalid];
    expect(await requestActivityCost(input, new AbortController().signal, async () => {})).toBeNull();
  }
  expect(await requestActivityCost([], new AbortController().signal)).toBeNull();
});

test("activity cancellation stops before reading the next batch", async () => {
  const controller = new AbortController();
  const rows = Array.from({ length: 8193 }, () => instruction(1, 2, 3));
  let reads = 0, yields = 0;
  Object.defineProperty(rows, 4096, { get: () => { reads++; throw Error("Cancelled batch was read"); } });
  await expect(requestActivityCost(rows, controller.signal, async () => {
    if (++yields === 2) controller.abort();
  })).rejects.toMatchObject({ name: "AbortError" });
  expect(reads).toBe(0);
  const aborted = new AbortController(); aborted.abort();
  await expect(requestActivityCost(rows, aborted.signal, async () => { throw Error("Should not yield"); }))
    .rejects.toMatchObject({ name: "AbortError" });
});
