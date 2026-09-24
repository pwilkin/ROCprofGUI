"""Single-shader optimization sessions for Vulkan: the counterpart of the HIP kernel
tuning worker. The application is run once under VK_LAYER_ROCPROF_capture, which
records one dispatch (SPIR-V, specialization and push constants, bound buffers before
and after). The shader's GLSL recipe is recovered by rebuilding the project's shaders
through a logging glslc and matching the captured SPIR-V, and edited variants are
compiled with that recipe and replayed in isolation with vkreplay."""
import glob
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import time

MAX_SOURCE = 8 * 1024 * 1024
# OpCapability, OpExtension, OpExecutionMode, OpExecutionModeId: instructions an
# application may add to or remove from a shader at run time without changing it.
PREAMBLE = {17, 10, 16, 331}
OP_ENTRY_POINT = 15


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


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


def descendants(pid):
    """Every process below pid, from /proc."""
    children = {}
    for stat in Path('/proc').glob('[0-9]*/stat'):
        try: text = stat.read_text()
        except OSError: continue
        fields = text[text.rindex(')') + 2:].split()
        children.setdefault(int(fields[1]), []).append(int(stat.parent.name))
    found, stack = [], [pid]
    while stack:
        for child in children.get(stack.pop(), []):
            found.append(child); stack.append(child)
    return found


def kill_tree(pid):
    """Kills a process and everything below it, stopping them first so none can
    start another process between the listing and the kill."""
    tree = [pid]
    try: os.kill(pid, signal.SIGSTOP)
    except OSError: pass
    tree += descendants(pid)
    for p in tree[1:]:
        try: os.kill(p, signal.SIGSTOP)
        except OSError: pass
    for p in tree:
        try: os.kill(p, signal.SIGKILL)
        except OSError: pass


def run_tree(argv, timeout, capture_output=False, **kwargs):
    """subprocess.run whose timeout kills the whole process tree. An application
    or a profiler can start children that killing it alone leaves running."""
    if capture_output: kwargs['stdout'] = kwargs['stderr'] = subprocess.PIPE
    with subprocess.Popen(argv, **kwargs) as process:
        try:
            out, err = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            kill_tree(process.pid)
            process.communicate()
            raise
        return subprocess.CompletedProcess(process.args, process.returncode, out, err)


def command(argv, cwd, log, env=None, timeout=540):
    if not argv or not all(isinstance(a, str) and '\0' not in a for a in argv):
        raise ValueError('Command must be a nonempty argument array')
    with Path(log).open('ab') as output:
        output.write(('\n$ ' + shlex.join(argv) + '\n').encode())
        output.flush()
        result = run_tree(argv, timeout, cwd=cwd, env=env, stdout=output, stderr=subprocess.STDOUT)
    if result.returncode:
        raise RuntimeError(f'Command exited {result.returncode}; see {log}')


def application_env(cfg):
    env = dict(os.environ)
    for key, value in (cfg.get('env') or {}).items():
        if not re.fullmatch(r'[A-Z_][A-Z0-9_]*', key) or key in {'LD_PRELOAD', 'TMPDIR'}:
            raise ValueError('Environment variable not allowed: ' + key)
        env[key] = str(value)
    # Tracing belongs to the profiler run, not to capture or replay.
    for key in ('MESA_VK_TRACE', 'MESA_VK_TRACE_PER_SUBMIT'):
        env.pop(key, None)
    return env


# ---- SPIR-V ----------------------------------------------------------------------

def spirv_words(data):
    if len(data) % 4 or len(data) < 20 or struct.unpack_from('<I', data)[0] != 0x07230203:
        raise ValueError('Not a SPIR-V module')
    return list(struct.unpack(f'<{len(data) // 4}I', data))


def instructions(words):
    out, i = [], 5
    while i < len(words):
        n = words[i] >> 16
        if n == 0 or i + n > len(words):
            raise ValueError('Malformed SPIR-V instruction stream')
        out.append(tuple(words[i:i + n]))
        i += n
    return out


def normalized(data):
    """The module without its capability/extension/execution-mode preamble."""
    words = spirv_words(data)
    body = [ins for ins in instructions(words) if ins[0] & 0xffff not in PREAMBLE]
    return digest(repr((words[:3], words[4], body)).encode())


def preamble(data):
    return [ins for ins in instructions(spirv_words(data)) if ins[0] & 0xffff in PREAMBLE]


def entry_point_id(ins_list):
    for ins in ins_list:
        if ins[0] & 0xffff == OP_ENTRY_POINT:
            return ins[2]
    raise ValueError('SPIR-V module has no entry point')


def reapply(data, patch):
    """Applies the application's run-time preamble edits to a freshly compiled module,
    remapping execution modes onto this module's entry point."""
    words = spirv_words(data)
    ins = instructions(words)
    entry = entry_point_id(ins)
    added = [tuple(x) for x in patch.get('added', [])]
    removed = [tuple(x) for x in patch.get('removed', [])]
    def remap(x):
        return (x[0], entry) + tuple(x[2:]) if x[0] & 0xffff in (16, 331) else x
    added = [remap(x) for x in added]
    removed = [remap(x) for x in removed]
    ins = [x for x in ins if x not in removed]
    for new in added:
        if new in ins:
            continue
        opcode = new[0] & 0xffff
        # SPIR-V layout order: capabilities, extensions, imports, memory model,
        # entry points, execution modes.
        order = {17: [17], 10: [17, 10], 16: [17, 10, 11, 14, 15, 16, 331], 331: [17, 10, 11, 14, 15, 16, 331]}[opcode]
        at = max((i + 1 for i, x in enumerate(ins) if x[0] & 0xffff in order), default=0)
        ins.insert(at, new)
    out = words[:5] + [w for x in ins for w in x]
    return struct.pack(f'<{len(out)}I', *out)


def declared_bindings(data):
    sets, bindings = {}, {}
    for ins in instructions(spirv_words(data)):
        if ins[0] & 0xffff == 71 and len(ins) >= 4:
            if ins[2] == 34:
                sets[ins[1]] = ins[3]
            if ins[2] == 33:
                bindings[ins[1]] = ins[3]
    return sorted({(sets.get(i, 0), b) for i, b in bindings.items()})


# ---- capture -----------------------------------------------------------------------

def read_capture(capture):
    if not (capture / 'capture_complete').exists():
        error = capture / 'capture_error'
        raise RuntimeError(error.read_text() if error.exists() else 'The capture layer never reached the selected dispatch; check the shader name, workgroup count and occurrence')
    info = {'labels': [], 'spec': [], 'pushRanges': [], 'setLayouts': [], 'buffers': [], 'bindings': [], 'extensions': [], 'features': [], 'unknownFeatures': []}
    for line in (capture / 'capture.txt').read_text().splitlines():
        key, _, rest = line.partition(' ')
        parts = rest.split()
        if key == 'device':
            info['device'] = {'vendorId': int(parts[0]), 'deviceId': int(parts[1]), 'driverVersion': int(parts[2]), 'name': ' '.join(parts[3:])}
        elif key == 'name': info['name'] = rest
        elif key == 'label': info['labels'].append(rest)
        elif key == 'entry': info['entry'] = rest
        elif key in {'stage_flags', 'required_subgroup_size', 'pipeline_flags', 'api_version'}: info[key] = int(rest)
        elif key == 'spec': info['spec'].append({'id': int(parts[0]), 'offset': int(parts[1]), 'size': int(parts[2])})
        elif key == 'push_range': info['pushRanges'].append({'stages': int(parts[0]), 'offset': int(parts[1]), 'size': int(parts[2])})
        elif key == 'set_layout': info['setLayouts'].append({'set': int(parts[0]), 'binding': int(parts[1]), 'type': int(parts[2]), 'count': int(parts[3])})
        elif key == 'groups': info['groups'] = [int(x) for x in parts]
        elif key == 'buffer': info['buffers'].append({'group': int(parts[0]), 'bytes': int(parts[1]), 'input': parts[2], 'changes': parts[3], 'writtenBytes': int(parts[4])})
        elif key == 'bind': info['bindings'].append(dict(zip(['set', 'binding', 'element', 'type', 'group', 'offset', 'range'], map(int, parts))))
        elif key == 'extension': info['extensions'].append(rest)
        elif key == 'feature': info['features'].append(int(parts[0]))
        elif key == 'unknown_feature': info['unknownFeatures'].append(int(parts[0]))
    spec = (capture / 'spec.bin').read_bytes()
    info['specValues'] = [{'id': s['id'], 'hex': spec[s['offset']:s['offset'] + s['size']].hex(),
        'value': int.from_bytes(spec[s['offset']:s['offset'] + s['size']], 'little')} for s in info['spec']]
    info['pushHex'] = (capture / 'push.bin').read_bytes().hex()
    return info


def expected_groups(trace_dispatch):
    grid = [int(x) for x in re.findall(r'\d+', str(trace_dispatch.get('grid', '')))]
    size = [int(x) for x in re.findall(r'\d+', str(trace_dispatch.get('workgroup', '')))]
    if len(grid) == 3 and len(size) == 3 and all(size) and all(g % s == 0 for g, s in zip(grid, size)):
        return [g // s for g, s in zip(grid, size)]
    return None


# ---- source recovery ------------------------------------------------------------------

def ggml_build(tree, cfg):
    """Locates a ggml Vulkan shader build: generated SPIR-V, generator and glslc."""
    tree = Path(tree)
    builds = [Path(cfg['buildDir'])] if cfg.get('buildDir') else sorted(p for p in tree.glob('build*') if p.is_dir())
    for build in builds:
        spv = build / 'ggml/src/ggml-vulkan/vulkan-shaders.spv'
        if not spv.is_dir():
            continue
        generators = [Path(p) for p in glob.glob(str(build / '*/vulkan-shaders-gen')) + glob.glob(str(build / 'bin/vulkan-shaders-gen')) if os.access(p, os.X_OK)]
        cache = build / 'CMakeCache.txt'
        glslc = cfg.get('glslc')
        if not glslc and cache.is_file():
            m = re.search(r'^Vulkan_GLSLC_EXECUTABLE:FILEPATH=(.+)$', cache.read_text(errors='replace'), re.M)
            glslc = m and m[1].strip()
        glslc = glslc or shutil.which('glslc')
        if generators and glslc:
            return {'build': str(build), 'spv': str(spv), 'generated': str(build / 'ggml/src/ggml-vulkan'),
                    'generator': str(generators[0]), 'glslc': glslc, 'shaders': str(tree / 'ggml/src/ggml-vulkan/vulkan-shaders')}
    raise ValueError('No ggml Vulkan shader build found under the source tree; set config.buildDir to the CMake build directory')


def search(state, cfg, root):
    captured = (Path(state['captured']) / 'shader.spv').read_bytes()
    target = normalized(captured)
    build = ggml_build(cfg['sourceTree'], cfg)
    candidates = []
    for path in sorted(Path(build['spv']).glob('*.spv')):
        data = path.read_bytes()
        if abs(len(data) - len(captured)) > 4096:
            continue
        try:
            if normalized(data) != target:
                continue
        except ValueError:
            continue
        variant = path.stem
        sources = [p.name.removesuffix('.cpp') for p in Path(build['generated']).glob('*.comp.cpp')
                   if re.search(r'\b' + re.escape(variant) + r'_data\b', p.read_text(errors='replace'))]
        mine, theirs = preamble(captured), preamble(data)
        candidates.append({'variant': variant, 'spirv': str(path), 'sources': sources,
            'runtimePatch': {'added': [list(x) for x in mine if x not in theirs], 'removed': [list(x) for x in theirs if x not in mine]}})
    if not candidates:
        raise ValueError('No shader in the build matches the captured SPIR-V; rebuild the application or check that it came from this source tree')
    state['build'] = build
    state['candidates'] = candidates


def recipe(state, candidate, root, log):
    """Reruns the generator for the candidate's source through a logging glslc and
    returns the command line that produced the candidate variant."""
    build = state['build']
    if len(candidate['sources']) != 1:
        raise ValueError('The variant is generated from more than one source; cannot choose a recipe')
    source = Path(build['shaders']) / candidate['sources'][0]
    work = root / ('recipe-' + str(time.time_ns()))
    work.mkdir()
    wrapper = work / 'glslc'
    wrapper.write_text('#!/usr/bin/env python3\nimport json, os, sys\n'
        "with open(os.environ['ROCPROF_GLSLC_LOG'], 'a') as f: f.write(json.dumps(sys.argv[1:]) + '\\n')\n"
        "os.execv(os.environ['ROCPROF_GLSLC'], [os.environ['ROCPROF_GLSLC']] + sys.argv[1:])\n")
    wrapper.chmod(0o755)
    env = dict(os.environ, ROCPROF_GLSLC_LOG=str(work / 'glslc.jsonl'), ROCPROF_GLSLC=build['glslc'])
    command([build['generator'], '--glslc', str(wrapper), '--source', str(source), '--output-dir', str(work / 'spv'),
             '--target-hpp', str(work / 'shaders.hpp'), '--target-cpp', str(work / 'shaders.cpp')], str(work), log, env)
    want = candidate['variant'] + '.spv'
    for line in (work / 'glslc.jsonl').read_text().splitlines():
        argv = json.loads(line)
        if '-o' in argv and Path(argv[argv.index('-o') + 1].strip('"')).name == want:
            produced = Path(argv[argv.index('-o') + 1].strip('"')).read_bytes()
            if produced != Path(candidate['spirv']).read_bytes():
                raise ValueError('Rebuilding the variant does not reproduce the build output; the build is stale')
            shutil.rmtree(work / 'spv', ignore_errors=True)
            return [a.strip('"') for a in argv], str(source)
    raise ValueError('The generator did not compile ' + want)


def compile_args(argv, source, overlay_source, output):
    """The recovered glslc arguments, reading the overlay copy and writing output."""
    result, skip = [], False
    for arg in argv:
        if skip:
            skip = False
            continue
        if arg in {'-o', '-MF'}:
            skip = True
            continue
        if arg in {'-MD', '-M', '-MM'}:
            continue
        result.append(str(overlay_source) if arg == source else arg)
    return result + ['-o', str(output)]


def dependencies(glslc, argv, source, root, log):
    """Files the variant's compile reads, from glslc's make-style dependency output."""
    argv = [glslc] + compile_args(argv, source, source, root / 'unused.spv')[:-2] + ['-M']
    with Path(log).open('ab') as output:
        output.write(('\n$ ' + shlex.join(argv) + '\n').encode())
        p = subprocess.run(argv, cwd=str(root), stdout=subprocess.PIPE, stderr=output, timeout=120)
    if p.returncode:
        raise RuntimeError('glslc could not list the shader dependencies; see job.log')
    text = p.stdout.decode(errors='replace').replace('\\\n', ' ')
    files = shlex.split(text.split(':', 1)[1]) if ':' in text else []
    return sorted({str(Path(f).resolve()) for f in files} | {str(Path(source).resolve())})


# ---- replay ------------------------------------------------------------------------

DEFAULT_CONDITIONS = {'mode': 'stateless', 'memory': 'device', 'flushMb': 0, 'repeats': 1}


def conditions_of(request):
    """The conditions a run's replay uses. The default is the isolated best case;
    sustained mode and system memory approximate an application that keeps the GPU
    busy and whose buffers do not fit in VRAM, and repeats replay with fresh
    allocations to show how much placement moves the timing."""
    c = {**DEFAULT_CONDITIONS, **(request.get('conditions') or {})}
    if c['mode'] not in ('stateless', 'sustained'):
        raise ValueError('conditions.mode must be stateless or sustained')
    if c['memory'] not in ('device', 'app', 'gtt', 'auto'):
        raise ValueError('conditions.memory must be device, app, gtt or auto')
    c['flushMb'], c['repeats'] = int(c['flushMb']), int(c['repeats'])
    if not 0 <= c['flushMb'] <= 1024:
        raise ValueError('conditions.flushMb must be between 0 and 1024')
    if not 1 <= c['repeats'] <= 10:
        raise ValueError('conditions.repeats must be between 1 and 10')
    return {k: c[k] for k in DEFAULT_CONDITIONS}


def replay_argv(cfg, capture, spirv, iterations, conditions, validate=True, counters=False, isa=None):
    argv = [cfg['replay'], str(capture), '--spirv', str(spirv), '--iterations', str(iterations),
            '--mode', conditions['mode'], '--memory', conditions['memory']]
    if conditions['flushMb']:
        argv += ['--flush-mb', str(conditions['flushMb'])]
    if isa:
        argv += ['--isa', str(isa)]
    if counters:
        argv.append('--counters')
    if not validate:
        argv.append('--no-validate')
    return argv


def driver_of(result):
    """Vulkan's standard version packing, which Mesa uses: major.minor.patch."""
    v = int(result.get('driverVersion') or 0)
    return {'version': f'{v >> 22}.{(v >> 12) & 0x3ff}.{v & 0xfff}', 'deviceType': result.get('deviceType'),
            'device': result.get('device'), 'memory': (result.get('execution') or {}).get('memory')}


def replay(cfg, argv, output, log):
    with output.open('wb') as stdout, Path(log).open('ab') as stderr:
        stderr.write(('\n$ ' + shlex.join(argv) + '\n').encode())
        stderr.flush()
        p = run_tree(argv, 540, stdout=stdout, stderr=stderr, env=application_env(cfg))
    if p.returncode:
        raise RuntimeError('Isolated replay failed; see job.log')
    return json.loads(output.read_text())


def trace_replay(cfg, capture, spirv, run, log):
    """Runs the replay once under RADV's RGP tracing and keeps the dispatch's submit."""
    env = application_env(cfg)
    env.update(MESA_VK_TRACE='rgp', MESA_VK_TRACE_PER_SUBMIT='1')
    started = time.time() - 1
    argv = [cfg['replay'], str(capture), '--spirv', str(spirv), '--iterations', '1', '--no-validate']
    with Path(log).open('ab') as output:
        output.write(('\n$ ' + shlex.join(argv) + '\n').encode())
        output.flush()
        p = run_tree(argv, 540, stdout=subprocess.DEVNULL, stderr=output, env=env)
    files = sorted((f for f in glob.glob('/tmp/vkreplay_*.rgp') if os.path.getmtime(f) >= started), key=os.path.getsize)
    if p.returncode or not files:
        for f in files:
            os.unlink(f)
        raise RuntimeError('Traced replay produced no RGP capture; see job.log')
    target = run / 'trace.rgp'
    shutil.move(files[-1], target)  # the largest submit holds the dispatch
    for f in files[:-1]:
        os.unlink(f)
    return str(target)


def metric(value=None, unit='', reason=None):
    return {'value': value, 'unit': unit, 'reason': reason}


COUNTER_METRICS = {'L0 hit rate': 'L0 cache hit ratio', 'L1 hit rate': 'L1 cache hit ratio', 'L2 hit rate': 'L2 cache hit ratio'}


def counter_metrics(results, requested):
    """Cycles, the clock they imply, and cache hit rates, from vkreplay --counters:
    medians over the counted dispatches, then over the replays."""
    counted = [r['counters'] for r in results if isinstance(r.get('counters'), dict)]
    if not counted:
        reason = 'Not supported by this driver' if requested else 'Counters not requested'
        return {name: metric(None, unit, reason) for name, unit in
                [('GPU cycles', 'cycles'), ('Clock', 'MHz'), *[(n, '%') for n in COUNTER_METRICS]]}
    def med(values):
        values = sorted(v for v in values if v is not None)
        return values[len(values) // 2] if values else None
    cycles = med(c['values'].get('GPU active cycles') for c in counted)
    clock = med(c['values']['GPU active cycles'] / c['durationNs'] * 1e3 for c in counted
                if c['values'].get('GPU active cycles') and c.get('durationNs'))
    out = {
        'GPU cycles': metric(cycles, 'cycles', 'Median over separately counted dispatches; each is drained around its counter query, which adds a small fixed cost'),
        'Clock': metric(clock, 'MHz', 'GPU active cycles over the counted dispatches\' duration: the clock the dispatch actually ran at'),
    }
    for name, counter in COUNTER_METRICS.items():
        value = med(c['values'].get(counter) for c in counted)
        out[name] = metric(value, '%', None if value is not None else 'Not reported by this driver')
    return out


def metrics_for(results, isa_text, work, counters=False):
    """Metrics of a run. results holds one vkreplay result per replay; with several,
    each had fresh allocations, and the duration is the median of their medians."""
    result = results[0]
    medians = [r['timing']['median'] / 1000 for r in results]
    us = sorted(medians)[len(medians) // 2]
    if not us > 0:
        raise ValueError('Replay returned no valid GPU timing')
    stats = result.get('statistics', {})
    def stat(name, unit, reason=None):
        return metric(stats.get(name), unit, None if name in stats else reason or 'Not reported by this driver')
    timing = result['timing']
    if len(results) == 1:
        duration_reason = f"Median of {len(timing['samples'])} timed dispatches; range {timing['min'] / 1000:.4g}–{timing['max'] / 1000:.4g} µs"
    else:
        duration_reason = f"Median of {len(results)} replays with fresh allocations; their medians range {min(medians):.4g}–{max(medians):.4g} µs"
    out = {
        'GPU duration': metric(us, 'µs', duration_reason),
        'Launch throughput': metric(1e6 / us, 'dispatches/s'),
        'VGPRs': stat('VGPRs', 'registers/lane'),
        'SGPRs': stat('SGPRs', 'registers'),
        'LDS': stat('LDS size', 'bytes/workgroup'),
        'Scratch': stat('Scratch size', 'bytes/lane'),
        'Spilled VGPRs': stat('Spilled VGPRs', 'registers'),
        'Max waves per SIMD': metric(stats.get('Subgroups per SIMD'), 'waves/SIMD', 'Compiler limit from register and LDS use; not measured occupancy' if 'Subgroups per SIMD' in stats else 'Not reported by this driver'),
        'Instructions': stat('Instructions', 'static instructions'),
        'WMMA instructions': metric(len(re.findall(r'\bv_wmma_[a-z0-9_]+', isa_text)) if isa_text else None, 'static instructions', None if isa_text else 'No ISA from this driver'),
        'Compute throughput': metric(None, 'TFLOP/s', 'Supply FLOPs per dispatch'),
        'Memory throughput': metric(None, 'GB/s', 'Supply bytes transferred per dispatch'),
    }
    flops, nbytes = float(work.get('flops') or 0), float(work.get('bytes') or 0)
    if flops > 0:
        out['Compute throughput'] = metric(flops / us / 1e6, 'TFLOP/s', 'User-supplied FLOPs per dispatch')
    if nbytes > 0:
        out['Memory throughput'] = metric(nbytes / us / 1e3, 'GB/s', 'User-supplied bytes per dispatch')
    if len(results) > 1:
        out['Replay spread'] = metric(100 * (max(medians) - min(medians)) / us, '%',
                                      'Range of the per-replay medians over their median. A large spread means the timing depends on where the buffers land')
    out.update(counter_metrics(results, counters))
    return out


# ---- operations -----------------------------------------------------------------------

def main(root, request):
    state_path = root / 'session.json'
    state = json.loads(state_path.read_text())
    op = request['op']
    cfg = {**state.get('config', {}), **request.get('config', {})}
    log = root / 'job.log'

    def phase(message):
        state['phase'] = message
        atomic(state_path, json.dumps(state, indent=2).encode())

    if op == 'capture':
        if state.get('captured'):
            raise ValueError('Inputs are immutable for this session; start a new session to recapture')
        if not cfg.get('argv'):
            raise ValueError('Provide the application command as an argument array')
        for name in ('layer', 'replay'):
            if not Path(cfg[name]).exists():
                raise ValueError(f'Shader tuning runtime is missing ({cfg[name]}); run scripts/setup-shader-tuning.py')
        name = cfg.get('name') or state['kernel']
        if name.startswith('pipeline ') or '(RADV internal)' in name:
            raise ValueError('This dispatch is identified only by its pipeline hash, which the capture layer cannot see. Profile with the application naming its pipelines (llama.cpp: GGML_VK_DEBUG_MARKERS=1), or set config.name to the pipeline debug name')
        capture = root / ('capture-' + str(time.time_ns()))
        capture.mkdir()
        env = application_env(cfg)
        layer_dir = str(Path(cfg['layer']).parent)
        env['VK_LAYER_PATH'] = layer_dir + (os.pathsep + env['VK_LAYER_PATH'] if env.get('VK_LAYER_PATH') else '')
        env.update(VK_ADD_LAYER_PATH=layer_dir, VK_INSTANCE_LAYERS='VK_LAYER_ROCPROF_capture',
                   ROCPROF_VKCAP_OUTPUT=str(capture), ROCPROF_VKCAP_NAME=name,
                   ROCPROF_VKCAP_OCCURRENCE=str(int(cfg.get('occurrence', 0))))
        groups = cfg.get('groups') or expected_groups(state.get('traceDispatch', {}))
        if groups:
            env['ROCPROF_VKCAP_GROUPS'] = ','.join(str(int(g)) for g in groups)
        phase('Running the application to the selected dispatch')
        try:
            command(cfg['argv'], cfg.get('cwd') or str(root), log, env)
            info = read_capture(capture)
            if groups and info['groups'] != [int(g) for g in groups]:
                raise ValueError('Captured workgroup count does not match the selected trace dispatch')
        except BaseException:
            # A capture holds every bound buffer (hundreds of MB for ggml); a failed
            # attempt must not stay behind, since each retry would add another.
            shutil.rmtree(capture, ignore_errors=True)
            raise
        state['captured'] = str(capture)
        state['dispatch'] = info
        state['snapshotHashes'] = {p.name: file_digest(p) for p in capture.iterdir() if p.is_file()}
        state['config'] = cfg
    elif op == 'search':
        if not state.get('captured'):
            raise ValueError('Capture the dispatch first: its SPIR-V identifies the shader source')
        state['config'] = cfg
        phase('Matching captured SPIR-V against the shader build')
        search(state, cfg, root)
    elif op == 'select':
        if state.get('recipe'):
            raise ValueError('Source selection is fixed for this session; open a new session to change it')
        candidate = state['candidates'][int(request['index'])]
        phase('Recovering the shader compile command')
        argv, source = recipe(state, candidate, root, log)
        files = dependencies(state['build']['glslc'], argv, source, root, log)
        originals = root / 'original-sources'
        originals.mkdir(exist_ok=True)
        entries = []
        for i, path in enumerate(files):
            data = Path(path).read_bytes()
            if len(data) > MAX_SOURCE:
                raise ValueError('Shader source exceeds 8 MiB: ' + path)
            (originals / f'{i}-{Path(path).name}').write_bytes(data)
            entries.append({'path': path, 'hash': digest(data), 'code': data.decode('utf-8')})
        # 'source' is the path exactly as it appears in argv; shaderDir is resolved.
        state['recipe'] = {'variant': candidate['variant'], 'source': source, 'glslc': state['build']['glslc'], 'argv': argv,
                           'runtimePatch': candidate['runtimePatch'], 'shaderDir': str(Path(source).resolve().parent)}
        state['files'] = entries
    elif op == 'calibrate':
        raise ValueError('GPU calibration is available for HIP kernel sessions only')
    elif op == 'run':
        if not state.get('captured'):
            raise ValueError('Capture the dispatch before running')
        capture = Path(state['captured'])
        for name, sha in state['snapshotHashes'].items():
            if file_digest(capture / name) != sha:
                raise ValueError('Captured input changed: ' + name)
        number = len(state.get('runs', [])) + 1
        if number > 1000:
            raise ValueError('Session history is limited to 1,000 runs')
        run = root / f'run-{number}-{time.time_ns()}'
        run.mkdir()
        variant = bool(request.get('variant'))
        edits = request.get('edits') or {}
        spirv = capture / 'shader.spv'
        if variant:
            r = state.get('recipe')
            if not r:
                raise ValueError('Find and select the shader source before building a variant')
            known = {f['path'] for f in state['files']}
            unknown = [p for p in edits if p not in known]
            if unknown:
                raise ValueError('Only files the shader compile reads can be edited: ' + ', '.join(unknown))
            phase('Compiling the edited shader')
            overlay = run / 'overlay'
            shutil.copytree(r['shaderDir'], overlay, ignore=shutil.ignore_patterns('*.cpp', '*.h', '*.txt', '*.py'))
            for path, text in edits.items():
                if not Path(path).resolve().is_relative_to(r['shaderDir']):
                    raise ValueError('Edited file is outside the shader directory: ' + path)
                atomic(overlay / Path(path).resolve().relative_to(r['shaderDir']), text.encode())
            compiled = run / 'variant.spv'
            overlay_source = overlay / Path(r['source']).resolve().relative_to(r['shaderDir'])
            command([r['glslc']] + compile_args(r['argv'], r['source'], overlay_source, compiled), str(run), log)
            spirv = run / 'variant-runtime.spv'
            spirv.write_bytes(reapply(compiled.read_bytes(), r['runtimePatch']))
            want = [(b['set'], b['binding']) for b in state['dispatch']['bindings']]
            have = declared_bindings(spirv.read_bytes())
            if sorted(want) != have:
                raise ValueError(f'The edited shader declares bindings {have}, but the capture binds {sorted(want)}; interface changes need a new capture')
        iterations = int(request.get('iterations', 20))
        if not 1 <= iterations <= 10000:
            raise ValueError('Iterations must be between 1 and 10,000')
        conditions = conditions_of(request)
        counters = request.get('autoCounters', True) is not False
        isa_path = run / 'isa.txt'
        results = []
        for i in range(conditions['repeats']):
            phase('Timing the isolated dispatch' + (f" (replay {i + 1} of {conditions['repeats']}, fresh allocations)" if conditions['repeats'] > 1 else ''))
            first = i == 0
            argv = replay_argv(cfg, capture, spirv, iterations, conditions, validate=first and request.get('validate', True),
                               counters=counters, isa=isa_path if first else None)
            results.append(replay(cfg, argv, run / ('replay.json' if first else f'replay-{i + 1}.json'), log))
        result = results[0]
        isa_text = isa_path.read_text(errors='replace') if isa_path.exists() else ''
        metrics = metrics_for(results, isa_text, request.get('work', {}), counters)
        v = result.get('validation')
        notes = []
        if isinstance(v, dict):
            differing = [g for g in v['groups'] if g['mismatched']]
            validation = {'status': 'bitwise match' if v['match'] else 'output differs', 'mismatchedRegions': [f"buffer {g['group']}" for g in differing], 'groups': v['groups']}
            if 'sustainedMatch' in v:
                validation['sustainedMatch'] = v['sustainedMatch']
                if not v['sustainedMatch']:
                    notes.append('A second back-to-back dispatch changed the outputs: this dispatch reads what it writes, so sustained timings measure different work than the application\'s dispatch.')
        else:
            validation = {'status': 'not checked'}
        trace = None
        if request.get('trace'):
            phase('Tracing the isolated dispatch')
            try:
                trace = trace_replay(cfg, capture, spirv, run, log)
            except Exception as error:
                trace = {'error': str(error)}
        # Deltas only against the latest run made under the same conditions and work.
        previous = next((r for r in reversed(state.get('runs', []))
                         if r.get('conditions', DEFAULT_CONDITIONS) == conditions and r.get('work', {}) == request.get('work', {})), None)
        for label, value in metrics.items():
            old = (previous or {}).get('metrics', {}).get(label, {})
            if value['value'] is not None and old.get('value') is not None and old.get('unit') == value['unit']:
                value['delta'] = value['value'] - old['value']
                value['deltaPercent'] = 100 * value['delta'] / abs(old['value']) if old['value'] else None
        notes = list(dict.fromkeys(n for r in results for n in r.get('notes', []))) + notes
        if 'sustainedMatch' in validation:
            # vkreplay's caveat about sustained timing; the two-dispatch check settled it.
            notes = [n for n in notes if not n.startswith('sustained mode:')]
        if variant and file_digest(spirv) == state['snapshotHashes'].get('shader.spv'):
            notes.insert(0, 'The edited source compiles to the same SPIR-V as the captured shader: the edit does not change this variant (it may be in a preprocessor branch the recipe does not compile).')
        record = {'number': number, 'variant': variant, 'iterations': iterations, 'work': request.get('work', {}),
                  'conditions': conditions, 'counters': counters, 'driver': driver_of(result),
                  'comparedWith': previous['number'] if previous else None,
                  'codeHash': digest(json.dumps(edits, sort_keys=True).encode()) if variant else 'captured',
                  'spirvHash': file_digest(spirv), 'metrics': metrics, 'validation': validation, 'statistics': result.get('statistics', {}),
                  'notes': notes, 'trace': trace, 'edits': sorted(edits), 'directory': str(run)}
        atomic(run / 'result.json', json.dumps(record, indent=2).encode())
        state.setdefault('runs', []).append(record)
        state['isa'] = isa_text[:4 * 1024 * 1024]
        state['config'] = cfg
    elif op == 'save':
        edits = request.get('edits') or {}
        files = {f['path']: f for f in state.get('files', [])}
        if not edits:
            raise ValueError('No edited files to save')
        for path in edits:
            if path not in files:
                raise ValueError('Only files the shader compile reads can be saved: ' + path)
            if digest(Path(path).read_bytes()) != files[path]['hash']:
                raise ValueError('Original source changed externally; save refused: ' + path)
        backups = []
        for path, text in edits.items():
            backup = root / (f'source-backup-{time.time_ns()}-' + Path(path).name)
            backup.write_bytes(Path(path).read_bytes())
            atomic(path, text.encode())
            files[path].update(hash=digest(text.encode()), code=text)
            backups.append({'path': path, 'backup': str(backup)})
        state['lastSave'] = {'path': ', '.join(b['path'] for b in backups), 'backup': ', '.join(b['backup'] for b in backups), 'files': backups}
    elif op != 'state':
        raise ValueError('Unknown tuning operation')
    state['phase'] = None
    atomic(state_path, json.dumps(state, indent=2).encode())
    return state


if __name__ == '__main__':
    try:
        root = Path(sys.argv[1]).resolve()
        main(root, json.loads((root / 'request.json').read_text()))
    except Exception as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
