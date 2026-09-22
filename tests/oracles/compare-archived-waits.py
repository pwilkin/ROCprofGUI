import argparse
import json
import struct
import subprocess
from pathlib import Path
parser = argparse.ArgumentParser()
parser.add_argument("--root", type=Path, default=Path(".codex/archived-waits"))
parser.add_argument("--reports", default="metadata-fix")
parser.add_argument("--archive", default="initial/decoded.rocprof-capture")
parser.add_argument("--streams", action="store_true")
args = parser.parse_args()
out = args.root
reports = json.loads((out / args.reports / "waves.json").read_text())
code = {row["index"]:row["text"].strip() for row in json.loads((out / args.reports / "instructions.json").read_text())}
if args.streams:
    streams = json.loads((out / args.reports / "streams.json").read_text())
else:
    with (out / args.archive).open("rb") as archive:
        assert archive.read(8) in [b"RPCAP001", b"RPCAP002"]
        size = struct.unpack("<Q",archive.read(8))[0]
        archive.read(32)
        manifest = json.loads(archive.read(size))
        waves = {}
        for file in manifest["files"]:
            content = archive.read(file["size"])
            if file["path"].startswith("waves/"):
                waves[file["path"]] = json.loads(content)["wave"]
summary = []
assert len(reports) == (len(streams) if args.streams else len(manifest["capture"]["waves"]))
for ordinal, report in enumerate(reports):
    if args.streams:
        stream = streams[ordinal]
        assert stream["wave"] == report["wave"]
        identities = stream["instructions"]
    else:
        wave = waves[manifest["capture"]["waves"][ordinal]["id"]]
        identities = [token[4] for token in wave["instructions"]]
    assert report["tokens"] == len(identities)
    family = report["analysis"]["gfxip"]
    assert family in [9, 10, 11, 12]
    lines = [code[identity] for identity in identities]
    actual = {(entry["waitToken"],source["token"]) for entry in report["analysis"]["occurrences"] for source in entry["sources"]}
    checks = {}
    for name in ["upstream", "upstream-corrected"]:
        raw = f'{family} {len(lines)}\n' + "\n".join(lines) + "\n"
        rows = subprocess.check_output([f".codex/wait-differential/{name}"],input=raw,text=True).splitlines()
        assert int(rows[0]) == len(rows)-1
        expected = set()
        for row in rows[1:]:
            wait,count,*sources = map(int,row.split())
            assert count == len(sources)
            expected.update((wait,source) for source in sources)
        checks[name] = {"matching":actual == expected, "oracleEdges":len(expected),"rustOnly":len(actual-expected),"oracleOnly":len(expected-actual)}
        if actual != expected:
            (out / f'{Path(report["wave"]).stem}-{name}-differences.json').write_text(json.dumps({"rustOnly":sorted(actual-expected),"oracleOnly":sorted(expected-actual)}))
    assert report["analysis"]["unsupportedWaits"] == 0
    assert report["analysis"]["missingInstructionTokens"] == 0
    summary.append({"wave":report["wave"],"tokens":len(identities),"waitOccurrences":len(report["analysis"]["occurrences"]),"edges":len(actual),"checks":checks})
(out / "comparison.json").write_text(json.dumps(summary,indent=2))
print(json.dumps({"waves":len(summary),"tokens":sum(row["tokens"] for row in summary),"waitOccurrences":sum(row["waitOccurrences"] for row in summary),"edges":sum(row["edges"] for row in summary),"checks":summary}))
assert all(row["checks"]["upstream-corrected"]["matching"] for row in summary)
