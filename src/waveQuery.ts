import { invoke } from "@tauri-apps/api/core";
type Transport = <T>(command: string, args: Record<string, unknown>) => Promise<T>;
export async function waveQuery<T>(command: string, args: Record<string, unknown>, signal?: AbortSignal, transport: Transport = invoke): Promise<T> {
  if (!signal) return transport<T>(command, args);
  const check = () => { if (signal.aborted) throw new DOMException("Wave query cancelled", "AbortError"); };
  check();
  const requestId = crypto.randomUUID();
  const cancel = () => { void transport("cancel_wave_query", { requestId }).catch(() => {}); };
  signal.addEventListener("abort", cancel, { once: true });
  try {
    const confirmed = await transport<string>("begin_wave_query", { captureId: args.captureId, requestId });
    if (confirmed !== requestId) throw Error("Invalid wave request identity");
    check();
    const value = await transport<T>(command, { ...args, requestId });
    check();
    return value;
  } finally {
    signal.removeEventListener("abort", cancel);
    await transport("cancel_wave_query", { requestId }).catch(() => {});
  }
}
