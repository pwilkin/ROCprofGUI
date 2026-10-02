"""CPU-only verification of the rocprofv3 attach-window path (feedback 1/2/3).

No GPU and no real rocprofv3 are used: profile_start launches a fake long-lived
target, and the `profiler` parameter points profile_trigger at a fake rocprofv3
that records its argv, honours (or omits) --attach-duration-msec, blocks on stdin
for a triggered window, and can simulate the set-agent attach failure. This
exercises kernel-filter passthrough, timed vs triggered windows, the
same-collection-per-process guard, and the bounded attach retry + diagnostics.
"""

from pathlib import Path
import json
import os
import socket
import subprocess
import tempfile
import time

from mcp_client import Client

repo = Path(__file__).resolve().parents[1]
binary = Path(os.environ.get("MCP_TEST_BINARY", repo / "target/debug/rocprof-studio"))

FAKE_TARGET = """#!/usr/bin/env python3
import time
while True:
    time.sleep(1)
"""

FAKE_ROCPROFV3 = """#!/usr/bin/env python3
import sys, os, json, time
argv = sys.argv[1:]
def opt(name):
    return argv[argv.index(name) + 1] if name in argv else None
outdir = opt('--output-directory')
with open(os.path.join(outdir, 'argv.txt'), 'w') as f:
    f.write('\\n'.join(argv))
if opt('--kernel-include-regex') == 'FAILATTACH':
    print('rocprofv3: queue.cpp:907: Could not set agent to be profiled', file=sys.stderr, flush=True)
    sys.exit(1)
duration = opt('--attach-duration-msec')
if duration is not None:
    time.sleep(int(duration) / 1000)
else:
    try:
        sys.stdin.readline()
    except Exception:
        pass
with open(os.path.join(outdir, 'results_kernel_trace.csv'), 'w') as f:
    f.write('Kernel_Name,Start_Timestamp,End_Timestamp\\nk,0,10\\n')
sys.exit(0)
"""


def wait_window_closed(client, pid):
    for _ in range(100):
        if not client.tool("profile_status", {"id": pid})["windowRunning"]:
            return
        time.sleep(0.05)
    raise RuntimeError("window did not close")


with tempfile.TemporaryDirectory(prefix="rocprof-attach-test-") as temporary:
    root = Path(temporary)
    target = root / "fake-target"
    target.write_text(FAKE_TARGET)
    target.chmod(0o700)
    profiler = root / "fake-rocprofv3"
    profiler.write_text(FAKE_ROCPROFV3)
    profiler.chmod(0o700)
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    token = root / "token"
    env = dict(os.environ, ROCPROFGUI_HOME=str(root / "app"))
    env.pop("DISPLAY", None)
    env.pop("WAYLAND_DISPLAY", None)
    with (root / "service.log").open("w") as log:
        process = subprocess.Popen(
            [str(binary), "--headless", "--mcp-port", str(port), "--mcp-token-file", str(token)],
            cwd=repo, env=env, stdout=log, stderr=subprocess.STDOUT,
        )
        client = None
        try:
            for _ in range(100):
                if process.poll() is not None:
                    raise RuntimeError((root / "service.log").read_text())
                if token.exists():
                    try:
                        client = Client(port, token.read_text())
                        break
                    except OSError:
                        pass
                time.sleep(0.1)
            assert client is not None, "MCP service did not start"

            def start():
                return client.tool("profile_start", {"cwd": str(root), "argv": [str(target)], "tracer": "rocprofv3", "profiler": str(profiler)})

            # --- Session A: timed window, kernel filter + iteration range, same-collection guard. ---
            a = start()
            assert "pid" in a and a["pid"], a
            win = client.tool("profile_trigger", {"id": a["id"], "windowMs": 300, "traces": ["kernel"], "kernelIncludeRegex": "mul_mat", "kernelIterationRange": "64"})
            assert win["mode"] == "timed" and win["window"] == 1, win
            argv = (Path(win["directory"]) / "argv.txt").read_text().splitlines()
            assert "--kernel-include-regex" in argv and argv[argv.index("--kernel-include-regex") + 1] == "mul_mat", argv
            # kernelIterationRange passes through verbatim to --kernel-iteration-range (campaign: iteration 64 selected dispatch 7139).
            assert "--kernel-iteration-range" in argv and argv[argv.index("--kernel-iteration-range") + 1] == "64", argv
            assert "--kernel-trace" in argv and "--attach-sync-output" in argv, argv
            assert "--attach-duration-msec" in argv and "--kernel-exclude-regex" not in argv, argv
            wait_window_closed(client, a["id"])
            # An invalid iteration range is rejected up front, before any attach.
            try:
                client.tool("profile_trigger", {"id": a["id"], "windowMs": 300, "traces": ["kernel"], "kernelIncludeRegex": "mul_mat", "kernelIterationRange": "0..3"})
                raise AssertionError("invalid kernelIterationRange should have been rejected")
            except RuntimeError as e:
                assert "kernelIterationRange" in str(e), e
            # A different iteration range is a collection mismatch, like any other filter (rocprofv3's reattach set).
            try:
                client.tool("profile_trigger", {"id": a["id"], "windowMs": 300, "traces": ["kernel"], "kernelIncludeRegex": "mul_mat", "kernelIterationRange": "0-3"})
                raise AssertionError("mismatched iteration range should have been rejected")
            except RuntimeError as e:
                assert "same" in str(e) and "--kernel-iteration-range" in str(e), e
            try:
                client.tool("profile_trigger", {"id": a["id"], "windowMs": 300, "traces": ["hip"]})
                raise AssertionError("mismatched streams should have been rejected")
            except RuntimeError as e:
                assert "same" in str(e) and "streams" in str(e), e
            stopped = client.tool("profile_stop", {"id": a["id"]})
            assert any(str(f).endswith("results_kernel_trace.csv") for f in stopped["files"]), stopped

            # --- Session B: triggered window ended on demand (no fixed duration). ---
            b = start()
            win = client.tool("profile_trigger", {"id": b["id"], "triggered": True, "traces": ["kernel"]})
            assert win["mode"] == "triggered" and win["windowMs"] is None, win
            status = client.tool("profile_status", {"id": b["id"]})
            assert status["windowRunning"] and status["windowTriggered"], status
            argv = (Path(win["directory"]) / "argv.txt").read_text().splitlines()
            assert "--attach-duration-msec" not in argv, argv
            ended = client.tool("profile_trigger", {"id": b["id"], "end": True})
            assert ended["detached"], ended
            stopped = client.tool("profile_stop", {"id": b["id"]})
            assert any(str(f).endswith("results_kernel_trace.csv") for f in stopped["files"]), stopped

            # --- Session C: bounded retry and diagnostic on the set-agent failure. ---
            c = start()
            win = client.tool("profile_trigger", {"id": c["id"], "windowMs": 300, "kernelIncludeRegex": "FAILATTACH"})
            assert win["attachAttempts"] == 3, win
            assert win["warnings"] and "set the agent" in win["warnings"][0], win
            stopped = client.tool("profile_stop", {"id": c["id"]})
            assert any("could not set the agent" in w for w in stopped["warnings"]), stopped

            print("rocprofv3 attach windows: kernel filter, timed/triggered, same-collection guard, attach retry: passed")
        finally:
            if client is not None:
                try:
                    client.close()
                except OSError:
                    pass
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
