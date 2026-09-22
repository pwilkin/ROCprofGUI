import { expect, it } from "vitest";
import bundle from "../public/demo.json";
import { requestProject, acknowledgeProject } from "./projectTransfer";
import type { CatalogInvoke } from "./catalogTransfer";
import type { Capture } from "./types";
const source = bundle.capture as unknown as Capture;
function setup() {
  const controller = new AbortController();
  let current = ["old"],
    acknowledged = false;
  const calls: string[] = [];
  const header = {
    transport: "catalog-batch-v1",
    transferId: "batch",
    captures: [0, 1].map((n) => ({
      transport: "instruction-pages-v1",
      transferId: `batch:${n}`,
      instructionCount: 2,
      maxPageRows: 1,
      capture: { ...source, id: `new-${n}`, instructions: [] },
    })),
    project: {
      schemaVersion: 1,
      active: 1,
      entries: [
        { input: "/a", state: {}, sources: {} },
        { input: "/b", state: {}, sources: {} },
      ],
    },
  };
  const invoke: CatalogInvoke = async <T>(
    command: string,
    args: Record<string, unknown>,
  ) => {
    calls.push(command);
    if (command === "open_project") return header as T;
    if (command === "read_project_catalog_page") {
      const child = header.captures[args.captureIndex as number],
        offset = args.offset as number;
      return {
        transferId: child.transferId,
        captureId: child.capture.id,
        total: 2,
        offset,
        instructions: source.instructions.slice(offset, offset + 1),
      } as T;
    }
    if (command === "finish_project_transfer") {
      current = ["new-0", "new-1"];
      return [...current] as T;
    }
    if (command === "abort_catalog_transfer" && !acknowledged)
      current = ["old"];
    if (command === "acknowledge_project_transfer") acknowledged = true;
    return undefined as T;
  };
  return { controller, calls, header, invoke, current: () => current };
}
it("assembles all captures then commits once and acknowledges only after caller installation", async () => {
  const s = setup();
  const result = await requestProject("batch", s.invoke, s.controller.signal);
  expect(result?.captures.map((c) => c.id)).toEqual(["new-0", "new-1"]);
  expect(result?.pendingTransfer).toBe("batch");
  expect(s.calls).toEqual([
    "open_project",
    ...Array(4).fill("read_project_catalog_page"),
    "finish_project_transfer",
  ]);
  await acknowledgeProject(result?.pendingTransfer, s.invoke);
  await s.invoke("abort_catalog_transfer", { transferId: "batch" });
  expect(s.current()).toEqual(["new-0", "new-1"]);
});
it("rolls back a commit whose reply was lost", async () => {
  const s = setup();
  const invoke: CatalogInvoke = async <T>(
    command: string,
    args: Record<string, unknown>,
  ) => {
    const value = await s.invoke<T>(command, args);
    if (command === "finish_project_transfer") throw Error("lost commit reply");
    return value;
  };
  await expect(
    requestProject("batch", invoke, s.controller.signal),
  ).rejects.toThrow("lost commit reply");
  expect(s.current()).toEqual(["old"]);
  expect(s.calls.at(-1)).toBe("abort_catalog_transfer");
});
it("preserves the previous workspace when the second catalog fails", async () => {
  const s = setup();
  const invoke: CatalogInvoke = async <T>(
    command: string,
    args: Record<string, unknown>,
  ) => {
    if (command === "read_project_catalog_page" && args.captureIndex === 1)
      throw Error("missing page");
    return s.invoke<T>(command, args);
  };
  await expect(
    requestProject("batch", invoke, s.controller.signal),
  ).rejects.toThrow("missing page");
  expect(s.current()).toEqual(["old"]);
  expect(s.calls).not.toContain("finish_project_transfer");
});
it("rolls back cancellation after native commit but before UI installation", async () => {
  const s = setup();
  const invoke: CatalogInvoke = async <T>(
    command: string,
    args: Record<string, unknown>,
  ) => {
    const result = await s.invoke<T>(command, args);
    if (command === "finish_project_transfer") s.controller.abort();
    return result;
  };
  await expect(
    requestProject("batch", invoke, s.controller.signal),
  ).rejects.toThrow("cancelled");
  expect(s.current()).toEqual(["old"]);
});
it("retries an acknowledgement idempotently after its reply is lost", async () => {
  const s = setup();
  await requestProject("batch", s.invoke, s.controller.signal);
  let lost = true;
  const invoke: CatalogInvoke = async <T>(
    command: string,
    args: Record<string, unknown>,
  ) => {
    const value = await s.invoke<T>(command, args);
    if (lost) {
      lost = false;
      throw Error("lost ack");
    }
    return value;
  };
  await acknowledgeProject("batch", invoke);
  expect(
    s.calls.filter((c) => c === "acknowledge_project_transfer"),
  ).toHaveLength(2);
  await s.invoke("abort_catalog_transfer", { transferId: "batch" });
  expect(s.current()).toEqual(["new-0", "new-1"]);
});
it("cleans a known request after initial failure and accepts picker cancellation", async () => {
  const s = setup();
  const failing: CatalogInvoke = async <T>(
    command: string,
    args: Record<string, unknown>,
  ) => {
    if (command === "open_project") throw Error("load failed");
    return s.invoke<T>(command, args);
  };
  await expect(
    requestProject("batch", failing, s.controller.signal),
  ).rejects.toThrow("load failed");
  expect(s.calls).toEqual(["abort_catalog_transfer"]);
  const cancelled: CatalogInvoke = async <T>() => null as T;
  expect(
    await requestProject("batch", cancelled, s.controller.signal),
  ).toBeNull();
});
