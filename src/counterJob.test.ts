import { afterEach, describe, expect, it, vi } from "vitest";
import { startCounterJob } from "./counterJob";
class FakeWorker {
  static instances: FakeWorker[] = [];
  onmessage?: (e: any) => void;
  onerror?: (e: any) => void;
  onmessageerror?: () => void;
  terminate = vi.fn();
  postMessage = vi.fn();
  constructor() {
    FakeWorker.instances.push(this);
  }
}
const request = { series: [], scope: "", definitions: "A := 1" };
afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
  FakeWorker.instances = [];
});
describe("counter worker lifecycle", () => {
  it("terminates cancelled workers and ignores late success", async () => {
    vi.stubGlobal("Worker", FakeWorker);
    const job = startCounterJob(request),
      worker = FakeWorker.instances[0];
    const rejected = expect(job.result).rejects.toThrow("cancelled");
    job.cancel();
    worker.onmessage!({
      data: { result: { results: [], scope: "", times: [] } },
    });
    await rejected;
    expect(worker.terminate).toHaveBeenCalledTimes(1);
  });
  it("terminates timed-out workers and reports startup failures", async () => {
    vi.stubGlobal("Worker", FakeWorker);
    vi.useFakeTimers();
    const job = startCounterJob(request),
      rejected = expect(job.result).rejects.toThrow("30 seconds");
    await vi.advanceTimersByTimeAsync(30000);
    await rejected;
    expect(FakeWorker.instances[0].terminate).toHaveBeenCalledOnce();
    const next = startCounterJob(request),
      error = expect(next.result).rejects.toThrow("load failed");
    FakeWorker.instances[1].onerror!({
      message: "load failed",
      preventDefault: () => {},
    });
    await error;
    expect(FakeWorker.instances[1].terminate).toHaveBeenCalledOnce();
  });
});
