import { useEffect, useRef, useState, type ReactNode } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
export type McpState = {
  active: null | { requestId: string | number; tool: string; cancelling: boolean; elapsedSeconds: number };
  revision: number;
  workspace: Record<string, unknown>;
  server: { url?: string; sse?: string; tokenFile?: string } | null;
};
export function McpControl({ children }: { children: ReactNode }) {
  const [state, setState] = useState<McpState | null>(null);
  const [error, setError] = useState("");
  const revision = useRef(-1);
  useEffect(() => {
    if (!isTauri()) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const next = await invoke<McpState>("mcp_status");
        if (stopped) return;
        setState(next);
        if (next.revision !== revision.current) {
          revision.current = next.revision;
          window.dispatchEvent(new CustomEvent("mcp-workspace", { detail: next.workspace || {} }));
        }
        window.dispatchEvent(new CustomEvent("mcp-control", { detail: next }));
      } catch (e) { if (!stopped) setError(String(e)); }
      if (!stopped) timer = setTimeout(poll, 150);
    }
    void poll();
    return () => { stopped = true; clearTimeout(timer); };
  }, []);
  const locked = !!state?.active;
  return <>
    <div className="mcp-application" inert={locked}>{children}</div>
    {state?.server?.url && !locked && <details className="mcp-connection"><summary>MCP server</summary><p>{state.server.url}</p><p>SSE: {state.server.sse}</p><p>Bearer token file: {state.server.tokenFile}</p></details>}
    {locked && <div className="mcp-lock" role="dialog" aria-modal="true" aria-label="Currently driven by MCP">
      <div className="mcp-lock-card"><strong>Currently driven by MCP</strong><p>{state.active!.tool} · {state.active!.elapsedSeconds}s</p>
      <button autoFocus disabled={state.active!.cancelling} onClick={() => { void invoke("mcp_cancel").catch(e => setError(String(e))); }}>{state.active!.cancelling ? "Stopping MCP execution…" : "Cancel MCP execution"}</button>
      <p>Cancellation stops the active operation and returns an error to the MCP client. Completed artifacts are retained.</p>{error && <p role="alert">{error}</p>}</div>
    </div>}
  </>;
}
