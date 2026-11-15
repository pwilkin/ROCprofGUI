import { test, expect } from "@playwright/test";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";

// Real-hardware click-through of the hrx branch of the "Live profiling" panel.
// It does not run by default: set ROCPROF_HRX_PANEL_E2E to a JSON file
//   { "mcpPort": 8799, "tokenFile": "...", "command": "<as typed in the panel>",
//     "cwd": "...", "readyUrl": "http://127.0.0.1:8093/v1/models",
//     "workloadUrl": "http://127.0.0.1:8093/v1/chat/completions",
//     "workloadFiles": ["request.json"], "outputDir": "...",
//     "trace": { "bufferMib": "16", "maxTraces": "2", "targetCu": "0" } }
// naming a PRIVATE headless rocprof-studio and an HRX/Loom target. "trace" is
// optional: with it the panel's thread-trace option is ticked and filled, and
// the run must return thread-trace partitions (the target's driver needs the
// PM4 thread-trace producer).
//
// The panel runs in Chromium, where no Tauri IPC exists, so each live_profile_*
// command is forwarded to the profile_* MCP tool of that instance. Both entries
// run the same Rust core behind the same input schema (live_profile_start_core
// and the tools/call arm in src-tauri/src/mcp.rs), so the request the panel
// builds starts a real process and the result it renders is a real bundle. The
// webview-to-Rust IPC hop itself is the one part this does not exercise.
const configPath = process.env.ROCPROF_HRX_PANEL_E2E;

const TOOL: Record<string, string> = {
  live_profile_start: "profile_start",
  live_profile_trigger: "profile_trigger",
  live_profile_status: "profile_status",
  live_profile_stop: "profile_stop",
};

// Minimal Streamable-HTTP MCP client: one session per tool call.
async function callTool(port: number, token: string, name: string, args: unknown): Promise<{ text: string; isError: boolean }> {
  const url = `http://127.0.0.1:${port}/mcp`;
  const headers: Record<string, string> = {
    "content-type": "application/json",
    accept: "application/json, text/event-stream",
    authorization: `Bearer ${token}`,
  };
  const post = async (body: unknown, session?: string) => {
    const response = await fetch(url, {
      method: "POST",
      headers: session ? { ...headers, "mcp-session-id": session } : headers,
      body: JSON.stringify(body),
    });
    // The server answers a request with an event stream it keeps open, so read
    // until the reply arrives instead of waiting for the body to end.
    let message: any = null;
    if (response.body && (response.headers.get("content-type") ?? "").includes("text/event-stream")) {
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let pending = "";
      while (message === null) {
        const { value, done } = await reader.read();
        if (done) break;
        pending += decoder.decode(value, { stream: true });
        const lines = pending.split("\n");
        pending = lines.pop() ?? "";
        for (const line of lines)
          if (line.startsWith("data:")) {
            try {
              const parsed = JSON.parse(line.slice(5).trim());
              if ("result" in parsed || "error" in parsed) message = parsed;
            } catch {
              // Not a JSON event line.
            }
          }
      }
      await reader.cancel().catch(() => undefined);
    } else {
      const raw = await response.text();
      if (raw.trimStart().startsWith("{")) message = JSON.parse(raw);
    }
    return { session: response.headers.get("mcp-session-id") ?? session, message };
  };
  const init = await post({
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "hrx-panel-e2e", version: "0" } },
  });
  await post({ jsonrpc: "2.0", method: "notifications/initialized" }, init.session).catch(() => undefined);
  const reply = await post({ jsonrpc: "2.0", id: 2, method: "tools/call", params: { name, arguments: args } }, init.session);
  // The server caps live sessions, so close this one.
  await fetch(url, { method: "DELETE", headers: { ...headers, "mcp-session-id": init.session ?? "" } }).catch(() => undefined);
  if (!reply.message || reply.message.error) return { text: JSON.stringify(reply.message?.error ?? "no reply"), isError: true };
  const result = reply.message.result;
  return { text: (result.content ?? []).map((c: any) => c.text ?? "").join(""), isError: Boolean(result.isError) };
}

test("hrx panel click-through captures a real run through a private instance", async ({ page }) => {
  test.skip(!configPath, "set ROCPROF_HRX_PANEL_E2E to run against real hardware");
  test.setTimeout(20 * 60 * 1000);
  const config = JSON.parse(readFileSync(configPath!, "utf8"));
  const token = readFileSync(config.tokenFile, "utf8").trim();
  mkdirSync(config.outputDir, { recursive: true });
  const calls: { command: string; args: unknown; result?: unknown; error?: string }[] = [];

  await page.exposeFunction("hrxRealInvoke", async (command: string, args: unknown) => {
    const answer = await callTool(config.mcpPort, token, TOOL[command], args);
    if (answer.isError) {
      calls.push({ command, args, error: answer.text });
      return { error: answer.text };
    }
    const result = JSON.parse(answer.text);
    calls.push({ command, args, result });
    return { result };
  });
  await page.addInitScript(() => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      invoke: async (command: string, payload: any) => {
        if (command === "mcp_status") return { active: null, revision: 0, workspace: {}, server: null };
        if (!command.startsWith("live_profile_")) throw Error(command);
        const answer = await w.hrxRealInvoke(command, payload?.args ?? {});
        if (answer.error !== undefined) throw answer.error;
        return answer.result;
      },
    };
  });

  await page.goto("/");
  await page.getByRole("button", { name: "Live profiling", exact: true }).click();
  await page.getByLabel("Tracer").selectOption("hrx");
  await page.getByLabel("Command").fill(config.command);
  await page.getByLabel("Working directory").fill(config.cwd);
  await page.getByLabel("Allow partial families per device").check();
  if (config.trace) {
    await page.getByLabel("Thread trace (waves and ISA)").check();
    await expect(page.getByRole("list", { name: "What a thread trace covers" })).toContainText("A sample, not a census");
    if (config.trace.bufferMib) await page.getByLabel("Trace buffer per shader engine (MiB)").fill(String(config.trace.bufferMib));
    if (config.trace.maxTraces) await page.getByLabel("Traces to take").fill(String(config.trace.maxTraces));
    if (config.trace.targetCu) await page.getByLabel("Workgroup processor").fill(String(config.trace.targetCu));
  }
  await page.screenshot({ path: `${config.outputDir}/1-form.png`, fullPage: true });
  await page.getByRole("button", { name: "Start profiling", exact: true }).click();
  await expect(page.getByText(/Running [0-9a-f]{16,}/)).toBeVisible({ timeout: 60_000 });
  await expect(page.getByRole("button", { name: "Trigger window" })).toHaveCount(0);

  // The target is a server: wait until it is ready, then send the workload.
  await expect
    .poll(
      async () => {
        try {
          return (await (await fetch(config.readyUrl)).text()).includes('"registration":"ready"');
        } catch {
          return false;
        }
      },
      { timeout: 10 * 60 * 1000, intervals: [1000] },
    )
    .toBe(true);
  const responses: unknown[] = [];
  for (const file of config.workloadFiles as string[]) {
    const response = await fetch(config.workloadUrl, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: readFileSync(file, "utf8"),
    });
    responses.push((await response.json()).usage);
  }

  await page.getByRole("button", { name: "Refresh status", exact: true }).click();
  await expect(page.getByText(/Recording the whole run · bundle [\d,]+ bytes so far/)).toBeVisible({ timeout: 30_000 });
  await page.screenshot({ path: `${config.outputDir}/2-recording.png`, fullPage: true });

  await page.getByRole("button", { name: "Stop and collect", exact: true }).click();
  const summary = page.getByRole("list", { name: "Profile bundle summary" });
  await expect(summary).toBeVisible({ timeout: 3 * 60 * 1000 });
  await expect(summary).toContainText("· finalized");
  if (config.trace) {
    await expect(summary).toContainText(/Thread trace: \d+ partition\(s\)/);
    await expect(summary).toContainText("not a census");
  } else {
    await expect(summary).toContainText("Waves, ISA and hardware counters: none");
  }
  await page.screenshot({ path: `${config.outputDir}/3-collected.png`, fullPage: true });

  const lines = await summary.getByRole("listitem").allTextContents();
  const alerts = await page.getByRole("alert").allTextContents();
  writeFileSync(
    `${config.outputDir}/panel-run.json`,
    JSON.stringify({ calls, workloadUsage: responses, summaryLines: lines, alerts }, null, 2),
  );

  expect(calls.map((c) => c.command)).toEqual(["live_profile_start", "live_profile_status", "live_profile_stop"]);
  expect(calls.every((c) => c.error === undefined)).toBe(true);
  const stop = calls[2].result as any;
  expect(stop.bundle.finalized).toBe(true);
  expect(stop.bundle.dispatchEvents).toBeGreaterThan(0);
  if (config.trace) {
    const start = calls[0].args as any;
    expect(start.hrx.families).toContain("executable-traces");
    expect(stop.bundle.executableTraces).toBeGreaterThan(0);
    expect(stop.bundle.executableTraceBytes).toBeGreaterThan(0);
  }
});
