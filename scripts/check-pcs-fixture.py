"""Compare imported PC sample fields with an upstream decoded fixture."""
import json
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]
fixture = Path(sys.argv[1] if len(sys.argv) > 1 else '/tmp/rocprofgui-upstream/tests/pcsampling')
original = json.loads((fixture / 'code.json').read_text())
actual = json.loads(subprocess.check_output(['cargo', 'run', '-q', '-p', 'profile-core', '--offline', '--', str(fixture)], cwd=root))
header = original['header']
rows = {r[2]: r for r in original['code']}
assert len(actual['instructions']) == len(original['code'])
for instruction in actual['instructions']:
    row = rows[instruction['index']]
    for field, column in [('pcIssued', 'PC_Issued'), ('pcStalled', 'PC_Stalled'), ('stallReasons', 'Stall_Reasons')]:
        assert instruction[field] == row[header.index(column)], (instruction['index'], field)
print(f"Matched all PC fields for {len(rows)} instructions; issued={sum(i['pcIssued'] for i in actual['instructions'])}, stalled={sum(i['pcStalled'] for i in actual['instructions'])}")
