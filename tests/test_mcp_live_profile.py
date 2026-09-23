from pathlib import Path
import os
import socket
import subprocess
import tempfile
import threading
import time

from mcp_client import Client


repo = Path(__file__).resolve().parents[1]
binary = Path(os.environ.get("MCP_TEST_BINARY", repo / "target/debug/rocprof-studio"))
with tempfile.TemporaryDirectory(prefix="rocprof-live-test-") as temporary:
    root = Path(temporary)
    script = root / f"fake-radv-server-{os.getpid()}"
    script.write_text("""#!/usr/bin/env python3
import os, sys, time
from pathlib import Path
trigger = os.environ.get('MESA_VK_TRACE_TRIGGER')
if trigger:
    while not Path(trigger).exists(): time.sleep(.01)
    Path(trigger).unlink()
if len(sys.argv) > 2 and sys.argv[2] == 'no-capture':
    Path(sys.argv[1]).write_text('trigger consumed')
    while True: time.sleep(1)
output = Path('/tmp') / (Path(sys.argv[0]).name + '_2099.01.01_00.00.00.rgp')
output.write_bytes(b'fake RGP for MCP transport test')
for _ in range(2): print(f\"RGP capture saved to '{output}'\", file=sys.stderr, flush=True)
Path(sys.argv[1]).write_text('capture saved')
while True: time.sleep(1)
""")
    script.chmod(0o700)
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    token = root / "token"
    env = dict(os.environ, ROCPROFGUI_HOME=str(root / "app"))
    env.pop("DISPLAY", None)
    env.pop("WAYLAND_DISPLAY", None)
    with (root / "service.log").open("w") as log:
        process = subprocess.Popen([str(binary), "--headless", "--mcp-port", str(port), "--mcp-token-file", str(token)], cwd=repo, env=env, stdout=log, stderr=subprocess.STDOUT)
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
                time.sleep(.1)
            assert client is not None, "MCP service did not start"

            marker = root / "triggered"
            started = client.tool("profile_start", {"cwd": str(root), "argv": [str(script), str(marker)]})
            assert Path(started["triggerFile"]).parent == Path(started["directory"])
            assert "radv-compute-trigger.patch" in started["driverRequirement"]
            client.tool("profile_trigger", {"id": started["id"]})
            for _ in range(100):
                if marker.exists():
                    break
                time.sleep(.05)
            assert marker.exists(), "fake RADV capture was not triggered"
            status = client.tool("profile_status", {"id": started["id"]})
            assert status["running"] and status["savedMessages"] == 2, status
            stopped = client.tool("profile_stop", {"id": started["id"]})
            assert stopped["status"] == "stopped" and len(stopped["files"]) == 1, stopped
            assert stopped["savedMessages"] == 2 and stopped["uniqueSavePaths"] == 1, stopped
            assert stopped["warnings"] and Path(stopped["files"][0]).exists(), stopped

            empty_marker = root / "empty-trigger"
            empty = client.tool("profile_start", {"cwd": str(root), "argv": [str(script), str(empty_marker), "no-capture"]})
            client.tool("profile_trigger", {"id": empty["id"]})
            for _ in range(100):
                if empty_marker.exists():
                    break
                time.sleep(.05)
            assert empty_marker.exists(), "empty trigger was not consumed"
            no_capture = client.tool("profile_stop", {"id": empty["id"]})
            assert no_capture["triggerCount"] == 1 and not no_capture["files"] and no_capture["warnings"], no_capture

            marker.unlink()
            result = {}
            thread = threading.Thread(target=lambda: result.update(reply=client.tool("profile_run", {"cwd": str(root), "argv": [str(script), str(marker)], "tracer": "radv"}, raw=True)))
            thread.start()
            for _ in range(100):
                if marker.exists():
                    break
                time.sleep(.05)
            assert marker.exists(), "fake per-submit capture did not start"
            client.notify("notifications/cancelled", {"requestId": client.number})
            thread.join(15)
            assert not thread.is_alive(), "cancelled profile_run did not finish"
            reply = result["reply"]
            assert reply["error"]["code"] == -32800, reply
            assert "Artifacts:" in reply["error"]["message"] and ".rgp" in reply["error"]["message"], reply
            assert "repeated filenames overwrote earlier captures" in reply["error"]["message"], reply
            print("live trigger, duplicate warning, empty-capture warning, stop artifacts, and cancelled-run artifacts: passed")
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
