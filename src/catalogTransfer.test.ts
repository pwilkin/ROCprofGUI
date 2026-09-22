import { describe, expect, it } from "vitest";
import bundle from "../public/demo.json";
import {
  receiveCatalog,
  requestCatalog,
  requestCatalogCommand,
  type CatalogHeader,
  type CatalogInvoke,
} from "./catalogTransfer";
import type { Capture } from "./types";
const capture = bundle.capture as unknown as Capture;
function setup(count = 5) {
  const controller = new AbortController();
  const calls: string[] = [];
  const header: CatalogHeader = {
    transport: "instruction-pages-v1",
    transferId: "transfer-1",
    instructionCount: count,
    maxPageRows: 2,
    capture: { ...capture, instructions: [] },
  };
  const invoke: CatalogInvoke = async <T>(
    command: string,
    args: Record<string, unknown>,
  ) => {
    calls.push(command);
    if (command === "read_catalog_page") {
      const offset = args.offset as number;
      return {
        transferId: header.transferId,
        captureId: capture.id,
        offset,
        total: count,
        instructions: capture.instructions.slice(
          offset,
          Math.min(count, offset + 2),
        ),
      } as T;
    }
    if (command === "finish_catalog_transfer") return capture.id as T;
    return undefined as T;
  };
  return { controller, calls, header, invoke };
}
describe("catalog transfer transaction", () => {
  it("assembles ordered pages and commits without changing the header", async () => {
    const s = setup();
    const result = await receiveCatalog(
      s.header,
      s.invoke,
      s.controller.signal,
    );
    expect(result.instructions).toEqual(capture.instructions.slice(0, 5));
    expect(s.header.capture.instructions).toEqual([]);
    expect(s.calls).toEqual([
      "read_catalog_page",
      "read_catalog_page",
      "read_catalog_page",
      "finish_catalog_transfer",
    ]);
  });
  it("commits an empty catalog without requesting a page", async () => {
    const s = setup(0);
    expect(
      (await receiveCatalog(s.header, s.invoke, s.controller.signal))
        .instructions,
    ).toEqual([]);
    expect(s.calls).toEqual(["finish_catalog_transfer"]);
  });
  it("aborts a malformed or failed page before workspace commit", async () => {
    for (const malformed of [true, false]) {
      const s = setup();
      const invoke: CatalogInvoke = async <T>(
        command: string,
        args: Record<string, unknown>,
      ) => {
        if (command === "read_catalog_page") {
          if (!malformed) throw Error("page unavailable");
          return { transferId: "other" } as T;
        }
        return s.invoke<T>(command, args);
      };
      await expect(
        receiveCatalog(s.header, invoke, s.controller.signal),
      ).rejects.toThrow();
      expect(s.calls).toEqual(["abort_catalog_transfer"]);
    }
  });
  it("discards a late page after cancellation and aborts native staging", async () => {
    const s = setup();
    const invoke: CatalogInvoke = async <T>(
      command: string,
      args: Record<string, unknown>,
    ) => {
      const result = await s.invoke<T>(command, args);
      if (command === "read_catalog_page") s.controller.abort();
      return result;
    };
    await expect(
      receiveCatalog(s.header, invoke, s.controller.signal),
    ).rejects.toThrow("cancelled");
    expect(s.calls).toEqual(["read_catalog_page", "abort_catalog_transfer"]);
  });
  it("closes a just-committed capture when cancellation wins the response race", async () => {
    const s = setup(0);
    const invoke: CatalogInvoke = async <T>(
      command: string,
      args: Record<string, unknown>,
    ) => {
      const result = await s.invoke<T>(command, args);
      if (command === "finish_catalog_transfer") s.controller.abort();
      return result;
    };
    await expect(
      receiveCatalog(s.header, invoke, s.controller.signal),
    ).rejects.toThrow("cancelled");
    expect(s.calls).toEqual(["finish_catalog_transfer", "close_capture"]);
  });
  it("aborts invalid headers and reports cleanup failures", async () => {
    const s = setup(-1);
    await expect(
      receiveCatalog(s.header, s.invoke, s.controller.signal),
    ).rejects.toThrow("header");
    expect(s.calls).toEqual(["abort_catalog_transfer"]);
    const fail: CatalogInvoke = async () => {
      throw Error("cleanup unavailable");
    };
    await expect(
      receiveCatalog(s.header, fail, s.controller.signal),
    ).rejects.toThrow("cleanup failed");
  });
  it("accepts the legacy complete response and cleans it up if cancelled", async () => {
    const s = setup();
    expect(await receiveCatalog(capture, s.invoke, s.controller.signal)).toBe(
      capture,
    );
    expect(s.calls).toEqual([]);
    s.controller.abort();
    await expect(
      receiveCatalog(capture, s.invoke, s.controller.signal),
    ).rejects.toThrow("cancelled");
    expect(s.calls).toEqual(["close_capture"]);
  });
});

describe("catalog request recovery", () => {
  it("uses a caller-known identity and cleans up a lost initial response", async () => {
    const calls: [string, Record<string, unknown>][] = [];
    const invoke: CatalogInvoke = async <T>(
      command: string,
      args: Record<string, unknown>,
    ) => {
      calls.push([command, args]);
      if (command === "open_capture") throw Error("response lost");
      return undefined as T;
    };
    await expect(
      requestCatalog(
        "/capture",
        "known-id",
        invoke,
        new AbortController().signal,
      ),
    ).rejects.toThrow("response lost");
    expect(calls).toEqual([
      [
        "open_capture",
        {
          path: "/capture",
          instructionPages: true,
          transferId: "known-id",
          loadWaveStates: true,
        },
      ],
      ["abort_catalog_transfer", { transferId: "known-id" }],
    ]);
  });
  it("rejects an unexpected transfer without aborting that unrelated identity", async () => {
    const s = setup();
    const invoke: CatalogInvoke = async <T>(
      command: string,
      args: Record<string, unknown>,
    ) => {
      if (command === "open_capture") return s.header as T;
      expect(args.transferId).toBe("expected");
      return s.invoke<T>(command, args);
    };
    await expect(
      requestCatalog("/capture", "expected", invoke, s.controller.signal),
    ).rejects.toThrow("identity mismatch");
    expect(s.calls).toEqual(["abort_catalog_transfer"]);
  });
  it("does not start an already-cancelled request", async () => {
    const s = setup();
    s.controller.abort();
    await expect(
      requestCatalog("/capture", "known", s.invoke, s.controller.signal),
    ).rejects.toThrow("cancelled");
    expect(s.calls).toEqual([]);
  });
});

it("cleans up a native commit whose acknowledgement was lost", async () => {
  const s = setup(0);
  let nativeCommitted = false;
  const invoke: CatalogInvoke = async <T>(
    command: string,
    args: Record<string, unknown>,
  ) => {
    if (command === "finish_catalog_transfer") {
      nativeCommitted = true;
      throw Error("commit acknowledgement lost");
    }
    if (command === "close_capture") {
      expect(args.captureId).toBe(s.header.capture.id);
      nativeCommitted = false;
    }
    return s.invoke<T>(command, args);
  };
  await expect(
    receiveCatalog(s.header, invoke, s.controller.signal),
  ).rejects.toThrow("acknowledgement lost");
  expect(nativeCommitted).toBe(false);
  expect(s.calls).toEqual(["abort_catalog_transfer", "close_capture"]);
});

describe("SPM attachment catalog transport", () => {
  it("handles a cancelled native picker without a transfer or commit", async () => {
    const calls: string[] = [];
    const invoke: CatalogInvoke = async <T>(command: string) => {
      calls.push(command);
      return null as T;
    };
    expect(
      await requestCatalogCommand(
        "attach_spm",
        { captureId: "source", se: 0 },
        "attach",
        invoke,
        new AbortController().signal,
      ),
    ).toBeNull();
    expect(calls).toEqual(["attach_spm"]);
  });
  it("assembles a combined capture through the same transaction", async () => {
    const s = setup();
    const invoke: CatalogInvoke = async <T>(
      command: string,
      args: Record<string, unknown>,
    ) => {
      if (command === "attach_spm") {
        expect(args).toEqual({
          captureId: "source",
          se: 2,
          instructionPages: true,
          transferId: s.header.transferId,
        });
        return s.header as T;
      }
      return s.invoke<T>(command, args);
    };
    const result = await requestCatalogCommand(
      "attach_spm",
      { captureId: "source", se: 2 },
      s.header.transferId,
      invoke,
      s.controller.signal,
    );
    expect(result?.instructions).toEqual(capture.instructions.slice(0, 5));
    expect(s.calls.at(-1)).toBe("finish_catalog_transfer");
  });
});

it("assembles CSV observation pages before commit without mutating the header", async () => {
  const s = setup(0);
  s.header.transport = "instruction-pages-v2";
  s.header.observationCount = 3;
  s.header.capture = {
    ...s.header.capture,
    pcSamplingObservations: { columns: ["Exact_Id"], rows: [] },
  };
  const expected = [["18446744073709551615"], ["duplicate"], ["duplicate"]];
  const calls: string[] = [];
  const invoke: CatalogInvoke = async <T>(
    command: string,
    args: Record<string, unknown>,
  ) => {
    calls.push(command);
    if (command === "read_observation_page")
      return {
        transferId: s.header.transferId,
        captureId: capture.id,
        offset: args.offset,
        total: 3,
        observations: expected.slice(
          Number(args.offset),
          Number(args.offset) + 2,
        ),
      } as T;
    return capture.id as T;
  };
  const result = await receiveCatalog(s.header, invoke, s.controller.signal);
  expect(result.pcSamplingObservations?.rows).toEqual(expected);
  expect(s.header.capture.pcSamplingObservations?.rows).toEqual([]);
  expect(calls).toEqual([
    "read_observation_page",
    "read_observation_page",
    "finish_catalog_transfer",
  ]);
});

it("aborts malformed or cancelled observation pages before exposing a capture", async () => {
  for (const mode of ["wrong-offset", "wrong-width", "cancel"] as const) {
    const s = setup(0);
    s.header.transport = "instruction-pages-v2";
    s.header.observationCount = 1;
    s.header.capture = {
      ...s.header.capture,
      pcSamplingObservations: { columns: ["Id"], rows: [] },
    };
    const calls: string[] = [];
    const invoke: CatalogInvoke = async <T>(command: string) => {
      calls.push(command);
      if (mode === "cancel") s.controller.abort();
      return {
        transferId: s.header.transferId,
        captureId: capture.id,
        offset: mode === "wrong-offset" ? 1 : 0,
        total: 1,
        observations: mode === "wrong-width" ? [["a", "b"]] : [["a"]],
      } as T;
    };
    await expect(
      receiveCatalog(s.header, invoke, s.controller.signal),
    ).rejects.toThrow();
    expect(calls).toEqual(["read_observation_page", "abort_catalog_transfer"]);
    expect(s.header.capture.pcSamplingObservations?.rows).toEqual([]);
  }
});
