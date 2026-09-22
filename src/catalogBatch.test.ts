import { expect, it } from "vitest";
import bundle from "../public/demo.json";
import {
  assembleCatalogBatch,
  type CatalogBatchHeader,
  type CatalogInvoke,
} from "./catalogTransfer";
import type { Capture } from "./types";
const source = bundle.capture as unknown as Capture;
function setup() {
  const header: CatalogBatchHeader = {
    transport: "catalog-batch-v1",
    transferId: "project-9",
    captures: [5, 0, 3].map((count, index) => ({
      transport: "instruction-pages-v1",
      transferId: `project-9:${index}`,
      instructionCount: count,
      maxPageRows: 2,
      capture: { ...source, id: `capture-${index}`, instructions: [] },
    })),
  };
  const requests: Record<string, unknown>[] = [];
  const invoke: CatalogInvoke = async <T>(
    command: string,
    args: Record<string, unknown>,
  ) => {
    expect(command).toBe("read_project_catalog_page");
    requests.push(args);
    const child = header.captures[args.captureIndex as number];
    const offset = args.offset as number;
    return {
      transferId: child.transferId,
      captureId: child.capture.id,
      offset,
      total: child.instructionCount,
      instructions: source.instructions.slice(
        offset,
        Math.min(offset + 2, child.instructionCount),
      ),
    } as T;
  };
  return { header, requests, invoke, controller: new AbortController() };
}
it("assembles all project members in order without individual commits", async () => {
  const s = setup();
  const captures = await assembleCatalogBatch(
    s.header,
    s.invoke,
    s.controller.signal,
  );
  expect(captures.map((c) => c.id)).toEqual([
    "capture-0",
    "capture-1",
    "capture-2",
  ]);
  expect(captures.map((c) => c.instructions)).toEqual([
    source.instructions.slice(0, 5),
    [],
    source.instructions.slice(0, 3),
  ]);
  expect(s.requests.map((r) => r.captureIndex)).toEqual([0, 0, 0, 2, 2]);
  expect(
    s.header.captures.every((c) => c.capture.instructions.length === 0),
  ).toBe(true);
});
it("a later member failure returns no partial replacement", async () => {
  const s = setup();
  const existing = [source];
  let workspace = existing;
  const invoke: CatalogInvoke = async <T>(
    command: string,
    args: Record<string, unknown>,
  ) => {
    if (args.captureIndex === 2) throw Error("second nonempty catalog failed");
    return s.invoke<T>(command, args);
  };
  await expect(
    assembleCatalogBatch(s.header, invoke, s.controller.signal).then((c) => {
      workspace = c;
    }),
  ).rejects.toThrow("catalog failed");
  expect(workspace).toBe(existing);
  expect(s.requests).toHaveLength(3);
});
it("cancellation discards an assembled prefix before returning a workspace", async () => {
  const s = setup();
  const invoke: CatalogInvoke = async <T>(
    command: string,
    args: Record<string, unknown>,
  ) => {
    const page = await s.invoke<T>(command, args);
    if (args.offset === 4) s.controller.abort();
    return page;
  };
  await expect(
    assembleCatalogBatch(s.header, invoke, s.controller.signal),
  ).rejects.toThrow("cancelled");
  expect(s.requests.every((r) => r.captureIndex === 0)).toBe(true);
});
it("rejects duplicate capture IDs and cross-batch member identities before paging", async () => {
  for (const duplicate of [true, false]) {
    const s = setup();
    if (duplicate)
      s.header.captures[1].capture.id = s.header.captures[0].capture.id;
    else s.header.captures[1].transferId = "different-batch:1";
    await expect(
      assembleCatalogBatch(s.header, s.invoke, s.controller.signal),
    ).rejects.toThrow("batch header");
    expect(s.requests).toEqual([]);
  }
});

it("accepts six native captures plus the demo and rejects seven native captures", async () => {
  const s = setup();
  s.header.captures = Array.from({ length: 7 }, (_, index) => ({
    transport: "instruction-pages-v1",
    transferId: `project-9:${index}`,
    instructionCount: 0,
    maxPageRows: 2,
    capture: {
      ...source,
      id: index === 6 ? "demo" : `native-${index}`,
      instructions: [],
    },
  }));
  expect(
    await assembleCatalogBatch(s.header, s.invoke, s.controller.signal),
  ).toHaveLength(7);
  s.header.captures[6].capture.id = "seventh-native";
  await expect(
    assembleCatalogBatch(s.header, s.invoke, s.controller.signal),
  ).rejects.toThrow("batch header");
});
