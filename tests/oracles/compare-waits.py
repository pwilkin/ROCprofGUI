import argparse
import json
import random
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--oracle", default="upstream")
parser.add_argument("--label", default="original")
parser.add_argument("--expected-differences", type=int, required=True)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
rng = random.Random(192026)
cases = []
for family in [9, 10, 11, 12]:
    ops = ["s_load_dword s0, s[2:3], 0", "s_store_dword s0, s[2:3], 0", "s_sendmsg sendmsg(MSG_INTERRUPT)",
           "ds_read_b32 v0, v1", "ds_write_b32 v0, v1", "global_load_dword v0, v[1:2], off",
           "global_store_dword v0, v[1:2], off", "flat_load_dword v0, v[1:2]",
           "flat_store_dword v0, v[1:2]", "buffer_load_dword v0, off, s[0:3], 0", "v_add_f32 v0, v1, v2"]
    names = ["lgkmcnt", "vmcnt"]
    if family >= 10:
        ops += ["s_sendmsg_rtn_b32 s0, sendmsg(MSG_GET_DOORBELL)"]
        names += ["vscnt"]
    if family == 12:
        ops += ["ds_direct_load v0", "image_bvh_intersect_ray v0", "tensor_load v0",
                "global_load_async_b32 v0", "global_store_async_b32 v0", "global_prefetch_b8 v0"]
        names = ["dscnt", "loadcnt", "storecnt", "kmcnt", "expcnt", "bvhcnt", "samplecnt", "tensorcnt", "asynccnt"]
    for case in range(300):
        lines = []
        for _ in range(rng.randint(10, 100)):
            if rng.random() < .35:
                name = rng.choice(names)
                value = rng.randrange(4)
                if family == 12:
                    lines.append(f"s_wait_{name} {value}")
                elif name == "vscnt":
                    lines.append(f"s_waitcnt_vscnt null, {value}")
                else:
                    lines.append(f"s_waitcnt {name}({value})")
            else:
                lines.append(rng.choice(ops))
        for name in names:
            lines.append(f"s_wait_{name} 0" if family == 12 else f"s_waitcnt_vscnt null, 0" if name == "vscnt" else f"s_waitcnt {name}(0)")
        cases.append({"family": family, "architecture": f"gfx{family}00", "lines": lines})
raw = "".join(f'{case["family"]} {len(case["lines"])}\n' + "\n".join(case["lines"]) + "\n" for case in cases)
upstream = subprocess.run([str(args.output / args.oracle)], input=raw, text=True, capture_output=True, check=True).stdout.splitlines()
rust = subprocess.run([str(args.output / "target/debug/wait-oracle-driver")], input="".join(json.dumps(case)+"\n" for case in cases), text=True, capture_output=True, check=True).stdout.splitlines()
position = 0
differences = []
for index, (case, result) in enumerate(zip(cases, rust, strict=True)):
    count = int(upstream[position])
    position += 1
    expected = set()
    for row in upstream[position:position+count]:
        wait, size, *sources = map(int, row.split())
        assert len(sources) == size
        expected.update((wait, source) for source in sources)
    position += count
    result = json.loads(result)
    actual = {(wait, source[0]) for wait, sources in result["waits"] for source in sources}
    assert result["analysis"]["unsupportedWaits"] == 0
    for occurrence in result["analysis"]["occurrences"]:
        for source in occurrence["sources"]:
            assert source["token"] < occurrence["waitToken"]
            assert source["issueToWaitCycles"] == (occurrence["waitToken"] - source["token"]) * 10
    if expected != actual:
        differences.append({"case": index, **case, "upstreamOnly": sorted(expected-actual), "rustOnly": sorted(actual-expected)})
assert position == len(upstream)
(args.output / f"{args.label}-differences.json").write_text(json.dumps(differences, indent=2))
summary = {"cases":len(cases), "matched":len(cases)-len(differences), "different":len(differences),
           "perFamily":{str(f):sum(case["family"]==f for case in differences) for f in [9,10,11,12]}}
(args.output / f"{args.label}-summary.json").write_text(json.dumps(summary, indent=2))
print(json.dumps(summary))
assert len(differences) == args.expected_differences, "Unexpected upstream comparison result"
