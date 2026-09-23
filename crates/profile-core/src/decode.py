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

# The writer indents every value onto its own line, which makes a large
# occupancy.json several times its data and trips the importer's size limit.
def _write_compact_json(path, data):
    Path(path).write_text(json.dumps(data, separators=(',', ':')))
try:
    import rocprof_trace_decoder.rcv as rcv
    if hasattr(rcv, '_write_json'):
        rcv._write_json = _write_compact_json
except ImportError:
    pass

def warn(path, message):
    print(f'DECODER-WARNING: {Path(path).name}: {message}', flush=True)

source = Path(args.input).resolve()
root = source if source.is_dir() else source.parent
radv_manifest = source / 'radv-manifest.json' if source.is_dir() else None
if radv_manifest is not None and radv_manifest.is_file():
    # RADV captures carry no in-trace code-object load markers, so each code object
    # is placed at the base address its RGP loader event recorded.
    from rocprof_trace_decoder.att import generate_code_artifacts, RcvOutputWriter, write_code_json
    from rocprof_trace_decoder.bindings import Decoder
    from rocprof_trace_decoder.records import Pc
    manifest = json.loads(radv_manifest.read_text())
    objects = [CodeObject(path=Path(o['path']), code_object_id=int(o['id'])) for o in manifest['codeObjects']]
    ranges = tuple((int(o['base']), int(o['size']), int(o['id'])) for o in manifest['codeObjects'])

    class RadvIndex(CodeIndex):
        def _translate(self, pc):
            if pc.code_object_id == 0:
                for base, size, identity in ranges:
                    if base <= pc.address < base + size:
                        return Pc(pc.address - base, identity)
            return pc
        def isa_for_pc(self, pc): return super().isa_for_pc(self._translate(pc))
        def get_or_create(self, pc): return super().get_or_create(self._translate(pc))
        def line_number(self, pc): return super().line_number(self._translate(pc))

    index = RadvIndex.from_document(generate_code_artifacts(objects).code_index.document, load_counts=False) if objects else RadvIndex([])
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    writer = RcvOutputWriter(output, index, formats='json')
    # Dispatch entry points arrive untranslated, as (0, VA), so kernels are named there.
    placed = {int(o['id']): (int(o['base']), o.get('name')) for o in manifest['codeObjects']}
    for kernel in index.document.get('kernels', []):
        base, name = placed.get(int(kernel.get('codeobj', -1)), (None, None))
        if base is not None and name:
            writer.kernel_names[Pc(base + int(kernel['address']), 0)] = name
    with Decoder(args.lib) as decoder:
        for trace in manifest['traces']:
            records = decoder.parse(Path(trace['path']).read_bytes(), isa=index)
            for info in records.info:
                warn(trace['path'], decoder.info_string(info))
            writer.add_shader_records(int(trace['shaderEngine']), records)
    writer.finish()
    write_code_json(output, index)
    print(output)
    sys.exit(0)
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
    output_dir=args.output, lib_path=args.lib, formats='json', decode_markers=True, on_warning=warn)
for output in outputs:
    print(output)
