import csv
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import struct
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
import tuning_calibration as calibration

MAX_SOURCE = 8 * 1024 * 1024
# A capture snapshots every allocation the kernel's arguments point into; for
# llama.cpp that includes the whole model's weight pool.
MIN_FREE_FOR_CAPTURE = 8 << 30
VALIDATION_MARGIN = 2 << 30
RESERVED_ENV = {'LD_PRELOAD', 'HSA_TOOLS_LIB', 'HSA_TOOLS_REPORT_LOAD_FAILURE', 'TMPDIR'}
# "reachable" snapshots only the allocations the dispatch's arguments lead to
# (following pointer tables); "all" snapshots every allocation in the process,
# for kernels that reach memory some other way.
CAPTURE_SCOPES = ('reachable', 'all')
EXTENSIONS = {'.hip', '.cu', '.cpp', '.cc', '.cxx', '.h', '.hpp', '.cuh'}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def tree_bytes(path):
    """Bytes held by the files under path, counting hard-linked files once."""
    seen, total = set(), 0
    for directory, _, files in os.walk(path):
        for name in files:
            st = os.lstat(os.path.join(directory, name))
            if (st.st_dev, st.st_ino) not in seen:
                seen.add((st.st_dev, st.st_ino)); total += st.st_size
    return total


def application_env(cfg):
    """Extra environment for the application, as recorded by profile_run."""
    env = {}
    for key, value in (cfg.get('env') or {}).items():
        if not re.fullmatch(r'[A-Z_][A-Z0-9_]*', key) or key in RESERVED_ENV or key.startswith('KERNCAP_'):
            raise ValueError('Environment variable not allowed: ' + key)
        env[key] = str(value)
    return env


def atomic(path, data):
    path = Path(path)
    fd, tmp = tempfile.mkstemp(prefix='.' + path.name, dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        if path.exists():
            os.chmod(tmp, path.stat().st_mode & 0o777)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def read_source(path):
    data = Path(path).read_bytes()
    if len(data) > MAX_SOURCE:
        raise ValueError('Source file exceeds 8 MiB')
    return data.decode('utf-8')


def mask_cpp(text):
    pattern = r'R"([^ ()\\\t\r\n]{0,16})\([\s\S]*?\)\1"|//[^\n]*|/\*[\s\S]*?\*/|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\''
    return re.sub(pattern, lambda m: ''.join('\n' if c == '\n' else ' ' for c in m.group()), text)


def canonical_kernel(name):
    if name.startswith('_Z'):
        name = subprocess.check_output(['c++filt', name], text=True).strip()
    return name.removesuffix(' [clone .kd]')


def kernel_base(name):
    canonical = canonical_kernel(name)
    cleaned = canonical.replace('(anonymous namespace)', '')
    depth, head = 0, []
    for char in cleaned:
        if char == '(' and depth == 0: break
        if char == '<': depth += 1
        elif char == '>': depth = max(0, depth - 1)
        elif depth == 0: head.append(char)
    tokens = ''.join(head).split('::')[-1].split()
    # Mirror the TS kernelBase `token || canonical`: an empty head (a degenerate
    # or blank name) falls back to the canonical name instead of raising
    # IndexError on `[][-1]`, so a blank Kernel_Name yields a clean no-match
    # rather than crashing the worker.
    return tokens[-1] if tokens else canonical


def parameter_list(code, name):
    """The parameter declarations of name's definition in code, without comments,
    split at top-level commas only, so std::conditional_t<A, __half, float>* stays
    one parameter."""
    masked = mask_cpp(code)
    match = re.search(r'\b' + re.escape(name) + r'\s*\(', masked)
    if not match: return []
    parts, depth, start = [], 0, match.end()
    for i in range(match.end(), len(masked)):
        char = masked[i]
        if char in '(<[{':
            depth += 1
        elif char in ')>]}':
            if depth == 0:
                parts.append(masked[start:i]); break
            depth -= 1
        elif char == ',' and depth == 0:
            parts.append(masked[start:i]); start = i + 1
    return [' '.join(part.split()) for part in parts if part.strip()]


def source_regions(text, name):
    masked = mask_cpp(text)
    result = []
    for match in re.finditer(r'\b__global__\b', masked):
        opening = masked.find('{', match.end())
        semicolon = masked.find(';', match.end())
        if opening < 0 or (semicolon >= 0 and semicolon < opening):
            continue
        signature = masked[match.start():opening]
        if not re.search(r'\b' + re.escape(name) + r'\s*(?:<[^;{}]*>)?\s*\(', signature):
            continue
        depth, end = 1, opening + 1
        while end < len(masked) and depth:
            depth += (masked[end] == '{') - (masked[end] == '}')
            end += 1
        if depth:
            continue
        start = masked.rfind('\n', 0, match.start()) + 1
        preceding = masked[:start].rstrip()
        if preceding.endswith(('>', ')')):
            template = preceding.rfind('template')
            if template >= 0 and not any(c in preceding[template:] for c in '{};'):
                start = masked.rfind('\n', 0, template) + 1
        result.append((start, opening, end))
    return result


def search_sources(tree, kernel):
    root = Path(tree).resolve(strict=True)
    name = kernel_base(kernel)
    candidates, visited = [], 0
    for directory, dirs, files in os.walk(root, followlinks=False):
        dirs[:] = sorted(d for d in dirs if not d.startswith('.') and d not in {'node_modules', 'target'} and not Path(directory, d).is_symlink())
        for filename in sorted(files):
            visited += 1
            if visited > 100000:
                raise ValueError('Source tree exceeds 100,000 files; choose a narrower root')
            path = Path(directory, filename)
            if path.suffix not in EXTENSIONS or path.is_symlink() or path.stat().st_size > MAX_SOURCE:
                continue
            try:
                text = read_source(path)
            except UnicodeError:
                continue
            for start, opening, end in source_regions(text, name):
                candidates.append({'path': str(path), 'start': start, 'opening': opening, 'end': end,
                    'line': text.count('\n', 0, start) + 1, 'hash': digest(text.encode()), 'code': text[start:end]})
                if len(candidates) > 100 or sum(len(c['code']) for c in candidates) > MAX_SOURCE:
                    raise ValueError('More than 100 kernel matches; choose a narrower source tree')
    return sorted(candidates, key=lambda c: (len(Path(c['path']).parts), c['path'], c['line']))


def command(argv, cwd, log, env=None):
    if not argv or not all(isinstance(a, str) and '\0' not in a for a in argv):
        raise ValueError('Command must be a nonempty argument array')
    with Path(log).open('ab') as output:
        output.write(('\n$ ' + shlex.join(argv) + '\n').encode()); output.flush()
        result = calibration.run_tree(argv, 540, cwd=cwd, env=env, stdout=output, stderr=subprocess.STDOUT)
    if result.returncode:
        raise RuntimeError(f'Command exited {result.returncode}; see {log}')


def compile_context(tree, source):
    databases = []
    dependency_cache = {}
    for directory, dirs, files in os.walk(tree):
        dirs[:] = [d for d in dirs if not d.startswith('.') and d not in {'node_modules'} and not Path(directory, d).is_symlink()]
        if 'compile_commands.json' in files:
            databases.append(Path(directory, 'compile_commands.json'))
        if len(databases) > 20:
            break
    source = Path(source).resolve()
    contexts = []
    for database in sorted(databases, key=lambda p: (p.parent != Path(tree) / 'build', len(p.parts), str(p))):
        if database.stat().st_size > 64 * 1024 * 1024:
            continue
        for entry in json.loads(database.read_text()):
            directory = Path(entry['directory'])
            unit = (directory / entry['file']).resolve()
            argv = entry.get('arguments') or shlex.split(entry['command'])
            if not argv or not shutil.which(argv[0]) or not any('hip' in a for a in argv):
                continue
            context = {'argv': argv, 'cwd': str(directory), 'translationUnit': str(unit)}
            if unit == source:
                return context
            output = None
            if '-o' in argv:
                output = directory / argv[argv.index('-o') + 1]
            depfile = Path(str(output) + '.d') if output else None
            if depfile and depfile.is_file() and depfile.stat().st_size < MAX_SOURCE:
                dependencies = shlex.split(depfile.read_text().replace('\\\n', ' '))
                if str(source) in dependencies:
                    contexts.append(context)
            elif output:
                for parent in output.parents:
                    makefile = parent / 'depend.make'
                    if makefile.is_file() and makefile.stat().st_size < MAX_SOURCE:
                        if makefile not in dependency_cache:
                            entries = {}
                            for line in makefile.read_text().replace('\\\n', ' ').splitlines():
                                if ':' not in line or line.startswith('#'): continue
                                target, dependencies = line.split(':', 1)
                                entries[(database.parent / target).resolve()] = shlex.split(dependencies)
                            dependency_cache[makefile] = entries
                        if str(source) in dependency_cache[makefile].get(output.resolve(), []): contexts.append(context)
                        break
                    if parent == directory: break
    if contexts:
        # Prefer the matching translation unit among header consumers.
        contexts.sort(key=lambda c: (Path(c['translationUnit']).stem != source.stem, len(c['argv']), c['translationUnit']))
        unique = {json.dumps([c['argv'], c['cwd']]): c for c in contexts}
        choices = list(unique.values())[:100]
        return {**choices[0], 'alternatives': choices}
    return None


def clean_compile(argv, arch, overlay, output):
    result, skip = [], False
    for arg in argv:
        if skip:
            skip = False; continue
        if arg in {'-o', '-MF', '-MT', '-MQ', '--offload-arch'}:
            skip = True; continue
        if arg in {'-c', '-MD', '-MMD', '-MP', '--save-temps', '-save-temps'} or arg.startswith(('--offload-arch=', '-MF', '-MT', '-MQ', '-save-temps=', '--save-temps=')):
            continue
        result.append(arg)
    if not result:
        raise ValueError('Provide the original compiler command, including its translation unit')
    return result + [f'--offload-arch={arch}', '-ivfsoverlay', str(overlay), '--cuda-device-only', '--no-gpu-bundle-output', '-o', str(output)]


def disassemble(hsaco, tool, symbol):
    p = subprocess.run([tool, '-d', '--demangle', str(hsaco)], capture_output=True, text=True, timeout=60)
    if p.returncode:
        raise RuntimeError(p.stderr[-4000:])
    sections = re.split(r'(?=^[0-9a-f]+ <)', p.stdout, flags=re.M)
    base = kernel_base(symbol)
    canonical = symbol.removesuffix(' [clone .kd]')
    matching = [s for s in sections if s.split('\n', 1)[0].endswith('<' + canonical + '>:')]
    if not matching:
        matching = [s for s in sections if base in s.split('\n', 1)[0]]
        if len(matching) > 1: raise ValueError('Ambiguous kernel disassembly; exact symbol required')
    if not matching:
        raise RuntimeError('Compiled code object has no matching kernel disassembly')
    return '\n'.join(matching)[:4 * 1024 * 1024]


def notes(hsaco, tool, symbol):
    p = subprocess.run([tool, '--notes', str(hsaco)], capture_output=True, text=True, timeout=60)
    if p.returncode:
        raise RuntimeError(p.stderr[-4000:])
    blocks = re.split(r'(?=^\s*- \.args:)', p.stdout, flags=re.M)
    base = kernel_base(symbol)
    for block in blocks:
        name = re.search(r'^\s*\.name:\s*(.*)', block, re.M)
        if name and name[1].strip(' \"\'') == symbol.removesuffix('.kd'):
            values = {}
            for key in ['kernarg_segment_size', 'private_segment_fixed_size', 'group_segment_fixed_size', 'vgpr_count', 'sgpr_count', 'wavefront_size']:
                m = re.search(r'^\s*\.' + key + r':\s*(\d+)', block, re.M)
                if m: values[key] = int(m[1])
            argument_lines = []
            for line in block.splitlines()[1:]:
                if line.startswith('    .'): break
                argument_lines.append(line)
            arguments = []
            for entry in re.split(r'(?=^      - \.)', '\n'.join(argument_lines), flags=re.M):
                fields = dict(re.findall(r'\.(offset|size|value_kind|address_space):\s*(\S+)', entry))
                if 'offset' in fields and 'size' in fields:
                    fields['offset'] = int(fields['offset']); fields['size'] = int(fields['size']); arguments.append(fields)
            values['arguments'] = arguments
            return values
    return {}


def metric(value=None, unit='', reason=None):
    if value is not None and not math.isfinite(value): raise ValueError('Computed metric is not finite')
    return {'value': value, 'unit': unit, 'reason': reason}


DEFAULT_CONDITIONS = {'mode': 'stateless', 'repeats': 1}


def conditions_of(request):
    """The conditions a HIP run's replay uses. 'stateless' (the default) restores the
    captured inputs before every iteration and times one dispatch in isolation;
    'sustained' restores once and dispatches back-to-back, approximating a kernel kept
    resident on a busy queue -- valid only when the dispatch does not read what it
    writes (kerncap calls this 'stateful', its --no-recopy path). 'repeats' replays the
    whole capture with fresh allocations to show how much placement moves the timing.
    Memory-pool placement (vkreplay --memory) and cache flushing (--flush-mb) are not
    offered on the HIP path; see docs/kernel-tuning-parity.md for why."""
    c = {**DEFAULT_CONDITIONS, **(request.get('conditions') or {})}
    if c['mode'] not in ('stateless', 'sustained'):
        raise ValueError('conditions.mode must be stateless or sustained')
    c['repeats'] = int(c['repeats'])
    if not 1 <= c['repeats'] <= 10:
        raise ValueError('conditions.repeats must be between 1 and 10')
    return {k: c[k] for k in DEFAULT_CONDITIONS}


def replay_argv(cfg, capture, hsaco, iterations, conditions):
    """The kerncap-replay command for one timed replay under the given conditions.
    kerncap restores captured inputs on each iteration unless --no-recopy is given, so
    'stateless' needs no flag and 'sustained' maps to --no-recopy."""
    argv = [cfg['replay'], str(capture), '--hsaco', str(hsaco),
            '--iterations', str(iterations), '--json', '--hip-launch']
    if conditions['mode'] == 'sustained':
        argv.append('--no-recopy')
    return argv


def expected_replay_mode(conditions):
    """kerncap-replay echoes 'stateless' for its default (recopy) path and 'stateful'
    for --no-recopy; map the condition name onto the string the replay reports."""
    return 'stateful' if conditions['mode'] == 'sustained' else 'stateless'


def metrics_for(replay, resources, isa, options):
    us = replay.get('timing', {}).get('average')
    if not isinstance(us, (float, int)) or not math.isfinite(us) or us <= 0:
        raise ValueError('Replay returned no valid GPU timing')
    result = {'GPU duration': metric(us, 'µs'), 'Launch throughput': metric(1e6 / us, 'kernels/s'),
        'Occupancy': metric(None, '%', 'Configure a measured occupancy counter'),
        'LDS bank conflicts': metric(None, '', 'Configure a supported LDS conflict counter'),
        'Scratch': metric(resources.get('private_segment_fixed_size'), 'bytes/work-item', 'Compiler private segment; not measured traffic'),
        'LDS': metric(resources.get('group_segment_fixed_size'), 'bytes/workgroup'),
        'VGPRs': metric(resources.get('vgpr_count'), 'registers/work-item'),
        'WMMA / MFMA instructions': metric(len(re.findall(r'\bv_(?:wmma|mfma)_[a-z0-9_]+', isa)), 'static instructions'),
        'Compute throughput': metric(None, 'TFLOP/s', 'Supply FLOPs per dispatch'),
        'Memory throughput': metric(None, 'GB/s', 'Supply bytes transferred per dispatch'),
        'Compute speed of light': metric(None, '%', 'Supply FLOPs per dispatch and peak TFLOP/s'),
        'Memory speed of light': metric(None, '%', 'Supply bytes per dispatch and peak GB/s')}
    for work, peak, throughput, sol, scale in [('flops', 'peakTflops', 'Compute throughput', 'Compute speed of light', 1e6), ('bytes', 'peakGbps', 'Memory throughput', 'Memory speed of light', 1e3)]:
        amount = float(options.get(work) or 0); ceiling = float(options.get(peak) or 0)
        if not math.isfinite(amount) or not math.isfinite(ceiling) or amount < 0 or ceiling < 0:
            raise ValueError('Work estimates and peaks must be finite, nonnegative numbers')
        if amount > 0:
            value = amount / us / scale
            result[throughput] = metric(value, result[throughput]['unit'], 'User-supplied work per dispatch')
            if ceiling > 0: result[sol] = metric(value / ceiling * 100, '%', 'User-supplied work and hardware peak')
    return result


def finish_capture(state, capture, cfg):
    if not (capture / 'capture_complete').exists(): raise RuntimeError('Capture did not complete; partial inputs were not accepted')
    dispatch = json.loads((capture / 'dispatch.json').read_text())
    if canonical_kernel(dispatch['demangled_name']) != canonical_kernel(state['kernel']): raise ValueError('Captured symbol does not exactly match the selected kernel: ' + dispatch['demangled_name'])
    if len((capture / 'kernarg.bin').read_bytes()) != dispatch['kernarg_size']: raise ValueError('Kernel argument snapshot size mismatch')
    for name in ['kernel.hsaco', 'memory_regions.json']:
        if not (capture / name).is_file(): raise ValueError('Missing capture artifact: ' + name)
    state['captured'] = str(capture); state['dispatch'] = dispatch
    state['argumentsHex'] = (capture / 'kernarg.bin').read_bytes().hex()
    resource = notes(capture / 'kernel.hsaco', cfg['readobj'], dispatch['mangled_name'])
    state['capturedResources'] = resource
    state['capturedSource'] = state.get('capturedSource', state.get('code', ''))
    blob = bytes.fromhex(state['argumentsHex'])
    declarations = parameter_list(state.get('code', ''), kernel_base(state['kernel']))
    parameters = []
    for index, arg in enumerate(a for a in resource.get('arguments', []) if not a.get('value_kind', '').startswith('hidden_')):
        raw = blob[arg['offset']:arg['offset'] + arg['size']]
        declaration = declarations[index].strip() if index < len(declarations) else f'argument {index}'
        value = '0x' + raw.hex()
        if '*' in declaration: value = hex(int.from_bytes(raw, 'little'))
        elif re.search(r'\bfloat\b', declaration) and len(raw) == 4: value = repr(struct.unpack('<f', raw)[0])
        elif re.search(r'\bdouble\b', declaration) and len(raw) == 8: value = repr(struct.unpack('<d', raw)[0])
        elif re.search(r'\b(?:u?int(?:8|16|32|64)_t|int|long|size_t)\b', declaration): value = str(int.from_bytes(raw, 'little', signed=not re.search(r'unsigned|uint|size_t', declaration)))
        parameters.append({**arg, 'declaration': declaration, 'value': value, 'hex': raw.hex()})
    state['parameters'] = parameters
    state['allocations'] = json.loads((capture / 'memory_regions.json').read_text())
    for region in state['allocations']['regions']:
        file = capture / 'memory' / f"region_{region['base']:x}.bin"
        if not file.is_file() or file.stat().st_size != region['size']: raise ValueError('Incomplete memory snapshot: ' + str(file))
    for key, target in [('grid', 'grid'), ('workgroup', 'block')]:
        recorded = re.findall(r'\d+', str(state.get('traceDispatch', {}).get(key, '')))
        if len(recorded) == 3 and [int(x) for x in recorded] != dispatch[target]: raise ValueError('Replayed launch dimensions do not match selected trace dispatch')
    state['snapshotHashes'] = {str(p.relative_to(capture)): file_digest(p) for p in capture.rglob('*') if p.is_file()}
    state['isa'] = disassemble(capture / 'kernel.hsaco', cfg['objdump'], state['kernel'])


def replay_output_hashes(capture, hsaco, cfg, folder, log):
    """Hashes every region after one replay. The replay dumps all of them, as large
    as the capture itself, so the folder is removed whether or not this succeeds."""
    folder.mkdir()
    try:
        for source in capture.rglob('*'):
            relative = source.relative_to(capture)
            if relative.parts[0] == 'output': continue
            target = folder / relative
            if source.is_dir(): target.mkdir(exist_ok=True)
            elif source.is_file(): os.link(source, target)
        command([cfg['replay'], str(folder), '--hsaco', str(hsaco), '--iterations', '1', '--hip-launch', '--dump-output'], str(folder), log)
        regions = json.loads((capture / 'memory_regions.json').read_text())['regions']
        hashes = {}
        for region in regions:
            name = f"region_{region['base']:x}.bin"
            path = folder / 'output' / name
            if not path.is_file() or path.stat().st_size != region['size']:
                raise ValueError('Replay did not dump a complete output region: ' + name)
            hashes[name] = file_digest(path)
            path.unlink()
        return hashes
    finally:
        shutil.rmtree(folder, ignore_errors=True)


def main(root, request):
    state_path = root / 'session.json'
    state = json.loads(state_path.read_text())
    op = request['op']; cfg = request.get('config', state.get('config', {}))
    log = root / 'job.log'
    def phase(message):
        state['phase'] = message
        atomic(state_path, json.dumps(state, indent=2).encode())
    if op in {'search', 'select'} and state.get('captured'): raise ValueError('Source selection is fixed after capture; open a new session to change it')
    if op == 'search':
        state['config'] = cfg
        state['candidates'] = search_sources(cfg['sourceTree'], state['kernel'])
        if not state['candidates']: raise ValueError('No matching __global__ definition found. Select a narrower source tree or check the traced kernel name.')
    elif op == 'select':
        candidate = state['candidates'][int(request['index'])]
        text = read_source(candidate['path'])
        if digest(text.encode()) != candidate['hash']: raise ValueError('Source changed since search; search again')
        state['source'] = candidate
        state['config']['compile'] = compile_context(state['config']['sourceTree'], candidate['path']) or {'argv': [], 'cwd': state['config']['sourceTree']}
        atomic(root / 'original-source', text.encode())
        state['code'] = candidate['code']
        state['kernelSignature'] = mask_cpp(text[candidate['start']:candidate['opening']]).strip()
    elif op == 'calibrate':
        phase('Calibrating GPU')
        expected = re.search(r'gfx[0-9a-f]+', state.get('dispatch', {}).get('isa_name', ''))
        state['calibration'] = calibration.calibrate(root, cfg, expected[0] if expected else None, force=True)
        state['config'] = cfg
    elif op == 'capture':
        if state.get('captured'): raise ValueError('Inputs are immutable for this session; start a new session to recapture')
        if not state.get('source'): raise ValueError('Select the kernel source first')
        state['config'] = cfg
        if not cfg.get('argv'): raise ValueError('Provide the original application command as an argument array')
        scope = cfg.get('captureScope') or 'reachable'
        if scope not in CAPTURE_SCOPES: raise ValueError('config.captureScope must be reachable or all')
        free = shutil.disk_usage(root).free
        if free < MIN_FREE_FOR_CAPTURE:
            raise ValueError(f'Capture needs at least {MIN_FREE_FOR_CAPTURE >> 30} GiB free in {root} ({free >> 30} GiB free): it snapshots every allocation the kernel reads, which for llama.cpp includes the model')
        env = dict(os.environ)
        env.update(application_env(cfg))
        if cfg.get('modelCache'): env['LLAMA_CACHE'] = cfg['modelCache']
        env.pop('HSA_TOOLS_LIB', None); env.pop('HSA_TOOLS_REPORT_LOAD_FAILURE', None)
        capture = root / ('capture-' + str(time.time_ns())); capture.mkdir()
        env.update(LD_PRELOAD=str(Path(cfg['captureLibrary']).resolve()), KERNCAP_KERNEL=canonical_kernel(state['kernel']), KERNCAP_OUTPUT=str(capture), KERNCAP_CAPTURE_CHILD='1', KERNCAP_DISPATCH=str(int(cfg.get('occurrence', 0))), KERNCAP_SNAPSHOT_SCOPE=scope)
        try:
            command(cfg['argv'], cfg['cwd'], log, env)
            finish_capture(state, capture, cfg)
        except BaseException:
            # A failed attempt must not leave its snapshot behind: the session stays
            # uncaptured, so every retry would add another one.
            shutil.rmtree(capture, ignore_errors=True)
            for key in ('captured', 'dispatch', 'snapshotHashes', 'allocations'):
                state.pop(key, None)
            raise
        state['captureBytes'] = tree_bytes(capture)
        regions = json.loads((capture / 'memory_regions.json').read_text())
        kept = regions['regions']
        state['captureScope'] = regions.get('scope', 'all')
        state['captureRegions'] = {'kept': len(kept), 'keptBytes': sum(r['size'] for r in kept),
                                   'tracked': regions.get('tracked_regions', len(kept)), 'trackedBytes': regions.get('tracked_bytes', sum(r['size'] for r in kept))}
        if scope == 'reachable' and 'scope' not in regions:
            state['captureScopeNote'] = 'This Kerncap runtime predates capture scopes and snapshotted every allocation; rerun scripts/setup-kernel-tuning.py to capture only what the kernel can reach'
        original_text = (root / 'original-source').read_text()
        state['capturedSource'] = original_text[state['source']['start']:state['source']['end']]
    elif op == 'run':
        if not state.get('captured'): raise ValueError('Capture inputs before running')
        capture = Path(state['captured']); cfg = {**state['config'], **cfg}
        for name, sha in state['snapshotHashes'].items():
            if file_digest(capture / name) != sha: raise ValueError('Captured input changed: ' + name)
        number = len(state.get('runs', [])) + 1
        if number > 1000: raise ValueError('Session history is limited to 1,000 runs')
        run = root / f'run-{number}-{time.time_ns()}'; run.mkdir()
        variant = bool(request.get('variant'))
        code = request.get('code', state['code'])
        hsaco = capture / 'kernel.hsaco'
        if variant:
            source = state['source']; original = (root / 'original-source').read_text()
            original_signature = original[source['start']:source['opening']]
            regions = source_regions(code, kernel_base(state['kernel']))
            if len(regions) != 1 or mask_cpp(code[:regions[0][1]]).strip() != state.get('kernelSignature', mask_cpp(original_signature).strip()):
                raise ValueError('Keep the captured kernel signature unchanged; argument-layout changes require a new capture')
            full = original[:source['start']] + code + original[source['end']:]
            path = run / 'variant.cpp'; atomic(path, full.encode())
            overlay = run / 'overlay.json'; overlay.write_text(json.dumps({'version': 0, 'roots': [{'type': 'file', 'name': source['path'], 'external-contents': str(path)}]}))
            arch_match = re.search(r'gfx[0-9a-f]+', state['dispatch'].get('isa_name', '') + state['dispatch'].get('agent_name', ''))
            if not arch_match: raise ValueError('Captured GPU architecture is missing')
            hsaco = run / 'variant.hsaco'
            build = cfg['compile']; command(clean_compile(build['argv'], arch_match[0], overlay, hsaco), build['cwd'], log)
        resources = notes(hsaco, cfg['readobj'], state['dispatch']['mangled_name'])
        if resources.get('kernarg_segment_size') != state['dispatch']['kernarg_size']: raise ValueError('Variant changed the captured kernel argument layout')
        captured_resources = state.get('capturedResources') or notes(capture / 'kernel.hsaco', cfg['readobj'], state['dispatch']['mangled_name'])
        if resources.get('arguments') != captured_resources.get('arguments'): raise ValueError('Variant changed captured argument offsets, sizes or address spaces')
        isa = disassemble(hsaco, cfg['objdump'], state['kernel'])
        iterations = int(request.get('iterations', 20))
        if not 1 <= iterations <= 10000: raise ValueError('Iterations must be between 1 and 10,000')
        calibrated = None
        if request.get('autoCalibrate', True):
            try:
                expected = re.search(r'gfx[0-9a-f]+', state['dispatch']['isa_name'])
                phase('Calibrating GPU')
                calibrated = calibration.calibrate(root, cfg, expected[0] if expected else None)
                state['calibration'] = calibrated
                state.pop('calibrationError', None)
            except calibration.DeviceMismatch:
                raise
            except Exception as error:
                state['calibrationError'] = str(error)
        conditions = conditions_of(request)
        expected_mode = expected_replay_mode(conditions)
        replays = []
        for attempt in range(conditions['repeats']):
            phase('Timing isolated kernel' + (f" (replay {attempt + 1} of {conditions['repeats']}, fresh allocations)" if conditions['repeats'] > 1 else ''))
            argv = replay_argv(cfg, capture, hsaco, iterations, conditions)
            output = run / ('replay.json' if attempt == 0 else f'replay-{attempt + 1}.json')
            with output.open('wb') as stdout, log.open('ab') as stderr:
                p = calibration.run_tree(argv, 540, stdout=stdout, stderr=stderr)
            if p.returncode:
                hint = ''
                if state.get('captureScope') == 'reachable':
                    hint = '. The capture kept only the allocations reachable from the kernel\'s arguments; if the kernel reaches memory another way, open a new session and capture with config.captureScope "all"'
                raise RuntimeError('Isolated replay failed; see job.log' + hint)
            attempt_replay = json.loads(output.read_text())
            # A runtime that predates replay conditions omits the whole 'execution'
            # block, so index it defensively: a missing block is treated as the
            # mismatch this very message describes, rather than raising a bare
            # KeyError('execution') that hides the real cause.
            actual_mode = attempt_replay.get('execution', {}).get('mode')
            if actual_mode != expected_mode:
                raise ValueError(f"Replay ran in {actual_mode!r} mode, not the requested {conditions['mode']!r}; the kernel-tuning runtime may predate replay conditions (rerun scripts/setup-kernel-tuning.py)")
            replays.append(attempt_replay)
        # The representative replay is the fastest average across fresh-allocation
        # repeats -- the isolated best case; the spread below shows placement sensitivity.
        replay = min(replays, key=lambda r: r.get('timing', {}).get('average', math.inf))
        metrics = metrics_for(replay, resources, isa, request.get('work', {}))
        if conditions['repeats'] > 1:
            averages = [r['timing']['average'] for r in replays]
            metrics['Replay spread'] = metric(max(averages) - min(averages), 'µs', f"Range of per-replay average GPU duration across {conditions['repeats']} fresh-allocation replays")
        characterization = None
        if calibrated:
            try:
                names = calibration.counter_names(cfg, run, log) if request.get('autoCounters', True) else set()
                phase('Characterizing kernel work and memory traffic')
                automatic, characterization = calibration.characterize(state, cfg, run, hsaco, isa, replay, calibrated, names)
                for label, entry in automatic.items():
                    if label.startswith('Compute ') and request.get('work', {}).get('flops'): continue
                    if label.startswith('Memory ') and request.get('work', {}).get('bytes'): continue
                    metrics[label] = entry
                work = request.get('work', {})
                if work.get('flops'):
                    metrics['Compute throughput'].update(method='manual', basis={'flops': work['flops']})
                    family = work.get('family') or (characterization['families'][0] if len(characterization['families']) == 1 else None)
                    if not work.get('peakTflops') and family in calibrated['peaks']:
                        metrics['Compute speed of light'] = calibration.value(metrics['Compute throughput']['value'] / calibrated['peaks'][family]['rate'] * 100, '%', 'Manual FLOP count / calibrated ' + family + ' peak', 'derived', {'calibration': calibrated.get('id', calibrated['fingerprint']), 'family': family, 'flops': work['flops']})
                if work.get('bytes'):
                    metrics['Memory throughput'].update(method='manual', basis={'bytes': work['bytes'], 'scope': 'logical'})
                    if not work.get('peakGbps'):
                        metrics['Memory speed of light'] = calibration.value(metrics['Memory throughput']['value'] / calibrated['peaks']['stream_copy']['rate'] * 100, '%', 'Manual logical byte count / calibrated streaming-copy rate', 'derived', {'calibration': calibrated.get('id', calibrated['fingerprint']), 'scope': 'logical', 'bytes': work['bytes']})
            except Exception as error:
                characterization = {'error': str(error)}
        phase('Collecting supported counters')
        profiles = []
        mappings = list(request.get('counters', []))
        if len(mappings) > 16: raise ValueError('At most 16 counter mappings per run')
        if request.get('autoCounters', True):
            try:
                available = calibration.run_tree([cfg['profiler'], '--list-avail'], 30, capture_output=True, text=True)
                if available.returncode: raise ValueError('Counter discovery failed')
                names = set(re.findall(r'Counter_Name\s*:\s*(\w+)', available.stdout))
                defaults = [('MeanOccupancyPerCU', 'Occupancy', 'waves/CU'), ('LDSBankConflict', 'LDS bank conflicts', '%'), ('SQ_INSTS_VEC32_VALU_WMMA', 'WMMA issued', 'instructions')]
                mappings = [{'counter': c, 'label': label, 'unit': unit} for c, label, unit in defaults if c in names and label not in {m['label'] for m in mappings}] + mappings
            except Exception as error:
                profiles.append({'discoveryError': str(error)})
        for mapping_index, mapping in enumerate(mappings):
            counter = mapping['counter']; label = mapping['label']
            if label in {'GPU duration', 'Launch throughput', 'Scratch', 'LDS', 'VGPRs', 'WMMA / MFMA instructions'}: raise ValueError('Counter mapping cannot replace a timing or compiler metric')
            if not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', counter): raise ValueError('Invalid profiler counter name')
            folder = run / ('counter-' + str(mapping_index)); folder.mkdir()
            try:
                command([cfg['profiler'], '--pmc', counter, '--kernel-include-regex', '^' + re.escape(canonical_kernel(state['kernel'])) + '$', '--output-format', 'csv', '--output-directory', str(folder), '--'] + [cfg['replay'], str(capture), '--hsaco', str(hsaco), '--iterations', '1', '--hip-launch'], str(root), log)
                values = []
                for path in folder.rglob('*.csv'):
                    with path.open() as f:
                        for row in csv.DictReader(f):
                            if canonical_kernel(row.get('Kernel_Name', '')) == canonical_kernel(state['kernel']):
                                raw = row.get(counter) or (row.get('Counter_Value') if row.get('Counter_Name') == counter else None)
                                if raw is not None and raw.strip(): values.append(float(raw))
                if len(values) != 1 or not math.isfinite(values[0]): raise ValueError('Expected exactly one measured dispatch counter value')
                metrics[label] = metric(values[0], mapping.get('unit', ''), 'Measured counter: ' + counter)
                profiles.append({'counter': counter, 'directory': str(folder)})
            except Exception as error:
                metrics[label] = metric(None, mapping.get('unit', ''), str(error))
                profiles.append({'counter': counter, 'directory': str(folder), 'error': str(error)})
        validation = {'status': 'not checked'}
        dump_bytes = sum(r['size'] for r in json.loads((capture / 'memory_regions.json').read_text())['regions'])
        free = shutil.disk_usage(root).free
        if request.get('validate', True) and free < dump_bytes + VALIDATION_MARGIN:
            validation = {'status': 'not checked', 'reason': f'Output validation dumps every captured region ({dump_bytes / 2**30:.1f} GiB) and needs {(dump_bytes + VALIDATION_MARGIN) / 2**30:.1f} GiB free; {free / 2**30:.1f} GiB is free'}
        elif request.get('validate', True):
            phase('Comparing outputs with captured baseline')
            reference = state.get('referenceOutputHashes')
            if reference is None:
                reference = replay_output_hashes(capture, capture / 'kernel.hsaco', cfg, run / 'reference-validation', log)
                state['referenceOutputHashes'] = reference
            actual = replay_output_hashes(capture, hsaco, cfg, run / 'variant-validation', log)
            mismatches = [name for name, sha in reference.items() if actual.get(name) != sha]
            validation = {'status': 'bitwise match' if not mismatches else 'output differs', 'mismatchedRegions': mismatches, 'outputHashes': actual, 'referenceHashes': reference}
        # Deltas only against the latest earlier run made under the same replay conditions.
        previous = next((r for r in reversed(state.get('runs', [])) if r.get('conditions', DEFAULT_CONDITIONS) == conditions), None)
        for label, value in metrics.items():
            old = (previous or {}).get('metrics', {}).get(label, {})
            if value['value'] is not None and old.get('value') is not None and old.get('unit') == value['unit'] and old.get('reason') == value.get('reason') and old.get('basis') == value.get('basis') and (previous or {}).get('work', {}) == request.get('work', {}) and ((previous or {}).get('calibration') or {}).get('fingerprint') == (calibrated or {}).get('fingerprint'):
                value['delta'] = value['value'] - old['value']
                value['deltaPercent'] = 100 * value['delta'] / abs(old['value']) if old['value'] else None
        record = {'calibration': calibrated, 'characterization': characterization, 'validation': validation, 'work': request.get('work', {}), 'number': number, 'variant': variant, 'codeHash': digest(code.encode()) if variant else 'captured', 'hsacoHash': digest(hsaco.read_bytes()), 'iterations': iterations, 'conditions': conditions, 'replay': replay, 'replays': replays, 'metrics': metrics, 'profiles': profiles, 'directory': str(run)}
        atomic(run / 'result.json', json.dumps(record, indent=2).encode())
        state.setdefault('runs', []).append(record); state['isa'] = isa; state['code'] = code; state['config'] = cfg
    elif op == 'save':
        source = state['source']; path = Path(source['path']); current = path.read_bytes()
        if digest(current) != source['hash']: raise ValueError('Original source changed externally; save refused. Start a fresh source selection.')
        code = request['code']
        regions = source_regions(code, kernel_base(state['kernel']))
        if len(regions) != 1 or code[regions[0][2]:].strip(): raise ValueError('Save must contain exactly the selected kernel definition')
        if state.get('kernelSignature') and mask_cpp(code[:regions[0][1]]).strip() != state['kernelSignature']: raise ValueError('Keep the captured kernel signature unchanged')
        old = current.decode(); replaced = old[:source['start']] + code + old[source['end']:]
        backup = root / ('source-backup-' + str(time.time_ns())); backup.write_bytes(current)
        atomic(path, replaced.encode())
        source['end'] = source['start'] + len(code); source['hash'] = digest(replaced.encode()); source['code'] = code
        regions = source_regions(code, kernel_base(state['kernel']))
        if regions: source['opening'] = source['start'] + regions[0][1]
        atomic(root / 'original-source', replaced.encode()); state['code'] = code; state['lastSave'] = {'path': str(path), 'backup': str(backup)}
    elif op != 'state':
        raise ValueError('Unknown tuning operation')
    state['phase'] = None
    atomic(state_path, json.dumps(state, indent=2).encode())
    return state


if __name__ == '__main__':
    try:
        root = Path(sys.argv[1]).resolve()
        request = json.loads((root / 'request.json').read_text())
        if request['op'] in {'run', 'capture', 'calibrate'}:
            with calibration.gpu_lock(root): main(root, request)
        else: main(root, request)
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
