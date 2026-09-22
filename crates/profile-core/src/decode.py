import json
import argparse
import re
import sys
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('input')
parser.add_argument('output')
parser.add_argument('--lib')
parser.add_argument('--python-package')
args = parser.parse_args()
if args.python_package:
    import zipfile
    package = Path(args.python_package).resolve(strict=True)
    with zipfile.ZipFile(package) as archive:
        for name in ('rocprof_trace_decoder/att.py', 'elftools/elf/elffile.py'):
            if name not in archive.namelist():
                raise ValueError(f'Decoder Python package is incomplete: missing {name}')
    sys.path.insert(0, str(package))
from rocprof_trace_decoder.att import AttTrace, generate_att_outputs
from rocprof_trace_decoder.codegen import CodeObject
from rocprof_trace_decoder.code_index import CodeIndex

source = Path(args.input).resolve()
root = source if source.is_dir() else source.parent
if source.suffix.lower() == '.rocprof-att':
    manifest = json.loads(source.read_text())
    traces = [AttTrace(path=Path(t['path']), shader_engine=t['shaderEngine'], run=manifest['run']) for t in manifest['traces']]
    objects = [CodeObject(path=Path(o['path']), code_object_id=int(o['id'])) for o in manifest['codeObjects']]
    index = None
else:
    # Mirrors att_runs::SE_MARKER / split_stem; keep the two in step.
    pattern = re.compile(r'^(.*)_shader_engine_(\d+)_(\d+)\.att$', re.I)
    selected = pattern.match(source.name) if source.is_file() else None
    if source.is_file() and not selected:
        raise ValueError("Selected ATT filename has no SE/run identity")
    traces = []
    identities = set()
    for path in sorted(p for p in root.iterdir() if p.suffix.lower() == '.att'):
        match = pattern.match(path.name)
        if not match and selected:
            continue
        if not match:
            raise ValueError(f'Cannot infer SE/run from {path.name}; expected *_shader_engine_SE_RUN.att')
        prefix, se, run = match.groups()
        se, run = int(se), int(run)
        if selected and (prefix, run) != (selected[1], int(selected[3])):
            continue
        if (se, run) in identities:
            raise ValueError('Ambiguous SE/run identity; select a trace file from one process')
        identities.add((se, run))
        traces.append(AttTrace(path=path, shader_engine=int(se), run=int(run)))
    if not traces:
        raise ValueError('No raw ATT traces found')
    if len({trace.run for trace in traces}) > 1:
        raise ValueError('Multiple runs found; open a specific .att file to select its run')
    objects = []
    ids = set()
    for path in sorted(root.iterdir()):
        if path.suffix.lower() not in ('.out', '.co', '.hsaco'):
            continue
        match = re.search(r'(?:^|_)code_object_id_(\d+)(?:_|$)', path.stem, re.I)
        if not match:
            raise ValueError(f'Cannot infer code object ID from {path.name}')
        identity = int(match[1])
        if identity in ids:
            raise ValueError(f'Duplicate code object ID {identity}')
        ids.add(identity)
        objects.append(CodeObject(path=path.resolve(), code_object_id=identity))
    index = CodeIndex.from_code_json(root / 'code.json', load_counts=False) if not objects and (root / 'code.json').exists() else None
outputs = generate_att_outputs(traces, code_objects=objects or None, code_index=index,
    output_dir=args.output, lib_path=args.lib, formats='json', decode_markers=True)
for output in outputs:
    print(output)
