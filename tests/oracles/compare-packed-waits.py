import hashlib
import json
import random
import re
import subprocess
from pathlib import Path

out = Path(".codex/packed-waits")
out.mkdir(parents=True, exist_ok=True)
llvm = "/opt/rocm/llvm/bin/llvm-mc"
driver = ".codex/wait-differential/target/debug/wait-oracle-driver"
summary = {"llvm": subprocess.check_output([llvm, "--version"], text=True).splitlines()[0], "architectures": {}}
rng = random.Random(192027)
pairs = []
for arch in ["gfx942", "gfx1030", "gfx1100"]:
    lines = [f"s_waitcnt {value}" for value in range(65536)]
    result = subprocess.run([llvm, "-triple=amdgcn", f"-mcpu={arch}", "-show-encoding"], input="\n".join(lines), text=True, capture_output=True, check=True)
    (out / f"{arch}-llvm.s").write_text(result.stdout)
    rendered = [line.split(";")[0].strip() for line in result.stdout.splitlines() if line.strip().startswith("s_waitcnt")]
    assert len(rendered) == 65536
    actual = json.loads(subprocess.check_output([driver], input=json.dumps({"expand": True, "architecture": arch, "lines": lines})+"\n", text=True))
    def counts(text):
        result = {"vmcnt":63, "expcnt":7, "lgkmcnt":15 if arch == "gfx942" else 63}
        result.update((name, int(value)) for name, value in re.findall(r"(vmcnt|expcnt|lgkmcnt)\((\d+)\)", text))
        return result
    for value, (expected, decoded) in enumerate(zip(rendered, actual, strict=True)):
        assert counts(expected) == counts(decoded), (arch, value, expected, decoded)
    for value in [0,1,15,16,63,255,256,1023,16384,65535] + [rng.randrange(65536) for _ in range(90)]:
        prefix = ["ds_read_b32 v0, v1", "global_load_dword v0, v[1:2], off"] * 70
        pairs.append(({"architecture":arch,"lines":prefix+[lines[value]]}, {"architecture":arch,"lines":prefix+["s_waitcnt " + " ".join(f"{name}({count})" for name,count in counts(rendered[value]).items())]}))
    summary["architectures"][arch] = {"immediates": 65536, "llvmOutputSha256": hashlib.sha256(result.stdout.encode()).hexdigest()}
for operation in ["load", "store"]:
    for ds in range(0,64,7):
        for memory in range(0,64,7):
            prefix = ["ds_read_b32 v0, v1", f"global_{operation}_dword v0, v[1:2], off"] * 70
            packed = f"s_wait_{operation}cnt_dscnt {(memory << 8) | ds}"
            named = f"s_wait {operation}cnt({memory}) dscnt({ds})"
            pairs.append(({"architecture":"gfx1200","lines":prefix+[packed]}, {"architecture":"gfx1200","lines":prefix+[named]}))
raw = "".join(json.dumps(case)+"\n" for pair in pairs for case in pair)
results = [json.loads(line) for line in subprocess.check_output([driver], input=raw, text=True).splitlines()]
for index, (left, right) in enumerate(zip(results[::2], results[1::2], strict=True)):
    assert left["waits"] == right["waits"], (index, pairs[index],left,right)
    assert left["analysis"]["unsupportedWaits"] == right["analysis"]["unsupportedWaits"] == 0
    assert left["analysis"]["packedWaitTokens"] == 1
summary["equivalentDependencyPairs"] = len(pairs)
(out / "summary.json").write_text(json.dumps(summary, indent=2))
print(json.dumps(summary))
