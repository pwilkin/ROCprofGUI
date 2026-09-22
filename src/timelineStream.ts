import { invoke } from "@tauri-apps/api/core";
import { timelineDocument } from "./timelineExport";

type Options = Parameters<typeof timelineDocument>[0];
type Invoke = <T>(command: string, args: Record<string, unknown>) => Promise<T>;
export class TimelineCommitUnknown extends Error {}
export async function streamTimeline(options: Options, call: Invoke = invoke): Promise<boolean> {
  const id = crypto.randomUUID();
  const name = options.label.replace(/[^a-z0-9_.-]+/gi, "-").slice(0, 120) + "-pages.html";
  let sequence = 0;
  let published = false;
  let failure: unknown;
  const abort = () => {
    void call("abort_timeline_export", {id}).catch(() => {});
  };
  options.signal.addEventListener("abort", abort, {once: true});
  try {
    const selected = await call<boolean>("begin_timeline_export", {id, name});
    if (!selected) return false;
    if (options.signal.aborted) return false;
    await timelineDocument({...options, writeChunk: async text => {
      if (options.signal.aborted) throw Error("Timeline export cancelled");
      await call("append_timeline_export", {id, sequence, text});
      sequence++;
    }});
    if (options.signal.aborted) return false;
    try {
      await call("finish_timeline_export", {id, chunks: sequence});
    } catch {
      try { await call("finish_timeline_export", {id, chunks: sequence}); }
      catch (error) { throw new TimelineCommitUnknown(`Export outcome could not be confirmed. The completed file may already exist at the chosen destination. ${String(error)}`); }
    }
    published = true;
    return true;
  } catch (error) {
    failure = error;
    throw error;
  } finally {
    options.signal.removeEventListener("abort", abort);
    if (!published) {
      try { await call("abort_timeline_export", {id}); }
      catch (error) {
        if (failure instanceof TimelineCommitUnknown) throw failure;
        throw Error(`${failure ? `${String(failure)}. ` : ""}Export cleanup could not be confirmed: ${String(error)}`);
      }
    }
  }
}
