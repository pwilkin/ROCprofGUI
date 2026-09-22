import { expect, it, vi } from "vitest";
import { waveQuery } from "./waveQuery";
it("cancels registration that finishes after the view is abandoned without starting a read", async () => {
  let ready!: (id: string) => void;
  const begin = new Promise<string>(resolve => { ready = resolve; });
  let identity = "";
  const transport = vi.fn(async (command: string, args: Record<string, unknown>): Promise<any> => {
    if (command === "begin_wave_query") { identity = String(args.requestId); return begin; }
    return null;
  });
  const controller = new AbortController();
  const pending = waveQuery("read_wave_stats", {captureId:"c"}, controller.signal, transport);
  const rejected = expect(pending).rejects.toMatchObject({name:"AbortError"});
  controller.abort();
  expect(transport.mock.calls.some(call => call[0] === "cancel_wave_query")).toBe(true);
  ready(identity);
  await rejected;
  expect(transport.mock.calls.filter(call => call[0] === "begin_wave_query")).toHaveLength(1);
  expect(transport.mock.calls.some(call => call[0] === "read_wave_stats")).toBe(false);
});
it("signals cancellation while a read is pending and never returns its stale result", async () => {
  let finish!: (value: unknown) => void;
  const read = new Promise(resolve => { finish = resolve; });
  const calls: string[] = [];
  const transport = vi.fn(async (command: string, args: Record<string, unknown>): Promise<any> => {
    calls.push(command);
    if (command === "begin_wave_query") return args.requestId;
    if (command === "read_wave_overview") return read;
    return null;
  });
  const controller = new AbortController();
  const pending = waveQuery("read_wave_overview", {captureId:"c"}, controller.signal, transport);
  const rejected = expect(pending).rejects.toMatchObject({name:"AbortError"});
  await vi.waitFor(() => expect(calls).toContain("read_wave_overview"));
  controller.abort();
  expect(calls).toContain("cancel_wave_query");
  finish({rows:[]}); await rejected;
});
it("cleans up failed reads while preserving their error", async () => {
  const transport = vi.fn(async (command: string, args: Record<string, unknown>): Promise<any> => {
    if (command === "begin_wave_query") return args.requestId;
    if (command === "read_wave_stats") throw Error("Invalid wave interval");
    return null;
  });
  await expect(waveQuery("read_wave_stats", {captureId:"c"}, new AbortController().signal, transport)).rejects.toThrow("Invalid wave interval");
  expect(transport.mock.calls.at(-1)?.[0]).toBe("cancel_wave_query");
});

it("cleans a caller-known registration after a lost acknowledgement", async () => {
  const calls: {command: string; args: Record<string, unknown>}[] = [];
  const transport = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
    calls.push({command,args});
    if (command === "begin_wave_query") throw Error("Registration reply lost");
    return null as T;
  };
  await expect(waveQuery("read_wave_stats",{captureId:"c"},new AbortController().signal,transport)).rejects.toThrow("Registration reply lost");
  expect(calls.map(c=>c.command)).toEqual(["begin_wave_query","cancel_wave_query"]);
  expect(calls[1].args.requestId).toBe(calls[0].args.requestId);
});
