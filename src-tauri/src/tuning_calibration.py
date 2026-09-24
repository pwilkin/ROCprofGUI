import contextlib
import csv
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import statistics
import subprocess
import tempfile
import time

class DeviceMismatch(ValueError):
    pass


VERSION = 1
MAX_AGE = 7 * 24 * 3600


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def save(path, value):
    path = Path(path)
    with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, delete=False) as stream:
        json.dump(value, stream, indent=2, allow_nan=False)
        temporary = stream.name
    os.replace(temporary, path)


@contextlib.contextmanager
def gpu_lock(root):
    directory = root.parent.parent / 'calibration'; directory.mkdir(parents=True, exist_ok=True)
    with (directory / 'gpu.lock').open('a') as lock:
        try: fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError: raise ValueError('Another tuning window is using the GPU; retry after it finishes')
        yield


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
    """subprocess.run whose timeout kills the whole process tree. rocprofv3 runs
    the profiled program as its own child, which killing rocprofv3 alone leaves
    running."""
    if capture_output: kwargs['stdout'] = kwargs['stderr'] = subprocess.PIPE
    with subprocess.Popen(argv, **kwargs) as process:
        try:
            out, err = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            kill_tree(process.pid)
            process.communicate()
            raise
        return subprocess.CompletedProcess(process.args, process.returncode, out, err)


def execute(argv, cwd, log, timeout=120, output=None):
    env = dict(os.environ, TMPDIR=str(cwd))
    for key in ['LD_PRELOAD', 'HSA_TOOLS_LIB', 'HSA_TOOLS_REPORT_LOAD_FAILURE']: env.pop(key, None)
    with Path(log).open('ab') as err:
        err.write(('\n' + json.dumps([str(x) for x in argv]) + '\n').encode()); err.flush()
        p = run_tree([str(x) for x in argv], timeout, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=err)
    if output: Path(output).write_bytes(p.stdout)
    if p.returncode: raise ValueError(f'{Path(argv[0]).name} exited {p.returncode}; see {log}')
    return p.stdout.decode()


def hardware_settings(device):
    pci = device['pci'].lower()
    if not re.fullmatch(r'[0-9a-f]{4,8}:[0-9a-f]{2}:[0-9a-f]{2}\.[0-7]', pci): return {}
    root = Path('/sys/bus/pci/devices') / pci
    settings = {}
    for name in ['power_dpm_force_performance_level', 'pp_power_profile_mode', 'pp_od_clk_voltage']:
        try: settings[name] = (root / name).read_text().strip()
        except OSError: pass
    if settings.get('power_dpm_force_performance_level') == 'manual':
        for name in ['pp_dpm_sclk', 'pp_dpm_mclk']:
            try: settings[name] = (root / name).read_text().strip()
            except OSError: pass
    for path in root.glob('hwmon/hwmon*/power1_cap'):
        try: settings['power_cap'] = path.read_text().strip()
        except OSError: pass
    return settings


def telemetry(device):
    root = Path('/sys/bus/pci/devices') / device['pci'].lower()
    values = {}
    for name in ['temp1_input', 'power1_average', 'freq1_input', 'freq2_input']:
        for path in root.glob('hwmon/hwmon*/' + name):
            try: values[name] = path.read_text().strip()
            except OSError: pass
    return values


def summarize(result):
    if result.get('outputCheck') != 'passed': raise ValueError('Calibration output check did not pass')
    samples = result['samplesUs']; operations = result['operations']
    if len(samples) < 5 or any(not isinstance(x, (int, float)) or not math.isfinite(x) or x <= 0 for x in samples): raise ValueError('Calibration requires at least five valid GPU timing samples')
    if not math.isfinite(operations) or operations <= 0: raise ValueError('Invalid calibration work count')
    median = statistics.median(samples)
    cv = statistics.stdev(samples) / statistics.mean(samples)
    rate = operations / median / (1e3 if result['unit'] == 'GB/s' else 1e6)
    if not math.isfinite(rate) or rate <= 0: raise ValueError('Invalid calibrated rate')
    return {**result, 'medianUs': median, 'rate': rate, 'coefficientOfVariation': cv, 'quality': 'variable' if cv > .1 else 'stable', 'method': 'measured', 'samples': len(samples)}


def parse_counters(folder, kernel, counters):
    rows = []
    for path in sorted(Path(folder).rglob('*.csv')):
        with path.open() as stream:
            rows += [r for r in csv.DictReader(stream) if r.get('Kernel_Name', '').removesuffix(' [clone .kd]') == kernel.removesuffix(' [clone .kd]')]
    values = {}
    for counter in counters:
        samples = []
        for row in rows:
            raw = row.get(counter)
            if raw is None and row.get('Counter_Name') == counter: raw = row.get('Counter_Value')
            if raw is not None and raw.strip(): samples.append(float(raw))
        if len(samples) != 1 or not math.isfinite(samples[0]) or samples[0] < 0: raise ValueError(f'{counter}: expected one finite, nonnegative dispatch value')
        values[counter] = samples[0]
    return values


def profile(cfg, argv, kernel, counters, folder, log):
    folder.mkdir(parents=True)
    values = {}
    for index, counter in enumerate(counters):
        part = folder / str(index); part.mkdir()
        execute([cfg['profiler'], '--pmc', counter, '--kernel-include-regex', '^' + re.escape(kernel.removesuffix(' [clone .kd]')) + '$', '--output-format', 'csv', '--output-directory', part, '--', *argv], part, log, timeout=180, output=part / 'stdout.log')
        values.update(parse_counters(part, kernel, [counter]))
    return values


def counter_names(cfg, root, log):
    output = execute([cfg['profiler'], '--list-avail'], root, log, timeout=30)
    sections = re.split(r'(?m)^GPU:\s*(\d+)\s*$', output)
    if len(sections) > 1:
        output = '\n'.join(sections[i+1] for i in range(1,len(sections),2) if sections[i] == '0')
    return set(re.findall(r'Counter_Name\s*:\s*(\w+)', output))


def fresh(record, fingerprint, now=None):
    return record.get('version') == VERSION and record.get('fingerprint') == fingerprint and 0 <= (time.time() if now is None else now) - record.get('createdAt', 0) < MAX_AGE


def calibrate(root, cfg, expected_arch=None, force=False):
    directory = root.parent.parent / 'calibration'; directory.mkdir(parents=True, exist_ok=True)
    log = root / 'job.log'
    source = Path(__file__).with_name('calibration.hip')
    compiler = cfg.get('hipcc') or '/opt/rocm/bin/hipcc'
    version = execute([compiler, '--version'], directory, log, timeout=20)
    compiler_key = hashlib.sha256((version + str(Path(compiler).resolve()) + sha(source)).encode()).hexdigest()[:20]
    probe = directory / ('probe-' + compiler_key)
    if not probe.exists():
        execute([compiler, '-O3', '-std=c++17', '--offload-arch=native', source, '-o', probe], directory, log, timeout=120)
    device = json.loads(execute([probe, 'probe', '0'], directory, log))
    arch = device['arch'].split(':')[0]
    if expected_arch and arch != expected_arch: raise DeviceMismatch(f'Captured GPU {expected_arch} differs from replay device {arch}')
    settings = hardware_settings(device)
    fingerprint = hashlib.sha256(json.dumps({'device': device, 'settings': settings, 'compiler': compiler_key, 'kernel': platform.release(), 'implementation': sha(__file__), 'version': VERSION}, sort_keys=True).encode()).hexdigest()
    cache = directory / (fingerprint + '.json')
    if cache.exists() and not force:
        record = json.loads(cache.read_text())
        if fresh(record, fingerprint): return {**record, 'reused': True, 'path': str(cache)}
    work = directory / ('run-' + str(time.time_ns())); work.mkdir()
    executable = work / 'calibrate'
    flags = []
    matrix = []
    if arch.startswith('gfx11'): flags = ['-DCALIB_WMMA11']; matrix = ['wmma_fp16', 'wmma_bf16', 'wmma_int8']
    elif arch.startswith('gfx12'): flags = ['-DCALIB_WMMA12']; matrix = ['wmma_fp16', 'wmma_bf16', 'wmma_int8']
    elif arch in {'gfx908', 'gfx90a', 'gfx940', 'gfx941', 'gfx942', 'gfx950'}: flags = ['-DCALIB_MFMA']; matrix = ['mfma_fp16']
    errors = {}
    try: execute([compiler, '-O3', '-std=c++17', '--offload-arch=' + arch, *flags, source, '-o', executable], work, log)
    except Exception as error:
        if not flags: raise
        errors.update({family: str(error) for family in matrix}); matrix = []
        execute([compiler, '-O3', '-std=c++17', '--offload-arch=' + arch, source, '-o', executable], work, log)
    peaks = {}
    before = telemetry(device)
    for family in ['fp32', 'fp64', 'fp16_packed', *matrix, 'stream_copy', 'cache_copy', 'lds']:
        try:
            output = execute([executable, family, '0'], work, log, output=work / (family + '.json'))
            peaks[family] = summarize(json.loads(output))
        except Exception as error: errors[family] = str(error)
    if 'fp32' not in peaks or 'stream_copy' not in peaks: raise ValueError('Core compute or memory calibration failed; see job.log')
    try:
        names = counter_names(cfg, work, log)
        for family, counters, destination, factor in [('stream_copy', ['FETCH_SIZE', 'WRITE_SIZE'], 'device_memory', 1024), ('cache_copy', ['GL2C_HIT_sum', 'GL2C_MISS_sum'], 'l2', 128)]:
            # L2 request size is only used on the reviewed RDNA gfx11/gfx12 definitions.
            if destination == 'l2' and not arch.startswith(('gfx11', 'gfx12')): continue
            if family not in peaks or not set(counters) <= names: continue
            try:
                values = profile(cfg, [str(executable), family, '0', 'profile'], 'calibrate_copy' if family == 'stream_copy' else 'calibrate_cache', counters, work / (family + '-counters'), log)
                amount = sum(values.values()) * factor
                if amount <= 0: raise ValueError('Calibration counters reported no memory traffic')
                peaks[destination] = {**peaks[family], 'family': destination, 'operations': amount, 'rate': amount / peaks[family]['medianUs'] / 1e3, 'counterValues': values, 'counterFactor': factor, 'basis': counters, 'method': 'derived', 'scope': destination}
            except Exception as error: errors[destination] = str(error)
    except Exception as error: errors['counterDiscovery'] = str(error)
    record = {'version': VERSION, 'createdAt': time.time(), 'fingerprint': fingerprint, 'device': device, 'settings': settings, 'compiler': version, 'sourceHash': sha(source), 'peaks': peaks, 'errors': errors, 'telemetryBefore': before, 'telemetryAfter': telemetry(device), 'directory': str(work), 'reused': False, 'path': str(cache)}
    record['id'] = hashlib.sha256((fingerprint + str(record['createdAt'])).encode()).hexdigest()
    if hardware_settings(device) != settings: raise ValueError('GPU configuration changed during calibration; rerun calibration')
    save(cache, record)
    return record


def isa_families(isa):
    result = set()
    for op in re.findall(r'^\s*(v_[a-zA-Z0-9_]+)\s', isa, re.M):
        if 'wmma' in op or 'mfma' in op:
            family = 'wmma' if 'wmma' in op else 'mfma'
            if 'bf16' in op: result.add(family + '_bf16')
            elif 'f16' in op: result.add(family + '_fp16')
            elif 'iu8' in op or 'i8' in op: result.add(family + '_int8')
            elif 'f64' in op: result.add(family + '_fp64')
            elif 'f32' in op: result.add(family + '_fp32')
        elif re.search(r'_(?:fma|fmac|mac|mul|add|sub|div|rcp|sqrt|rsq|exp|log|sin|cos|cvt)_', op):
            if 'f64' in op: result.add('fp64')
            elif 'f16' in op: result.add('fp16_packed' if 'pk_' in op else 'fp16_scalar')
            elif 'f32' in op: result.add('fp32')
    return sorted(result)


def useful_work(state):
    source = state.get('capturedSource', '')
    if not source: return None
    # This recipe is restricted to the captured llama.cpp scale loop and its launch arguments.
    compact = re.sub(r'\s+', '', re.sub(r'//[^\n]*|/\*[\s\S]*?\*/', '', source))
    expected = ['int64_ttid=(int64_t)blockIdx.x*(int64_t)blockDim.x+(int64_t)threadIdx.x;', 'int64_tstride=(int64_t)blockDim.x*(int64_t)gridDim.x;', 'for(int64_ti=tid;i<nelements;i+=stride){dst[i]=scale*x[i]+bias;}']
    if not state['kernel'].startswith('scale_f32(') or not all(x in compact for x in expected): return None
    body = compact[compact.index('{')+1:-1]
    for part in [*expected, 'ggml_cuda_pdl_lc();', 'ggml_cuda_pdl_sync();']: body = body.replace(part, '')
    if body: return None
    args = {p['declaration'].split()[-1]: p for p in state.get('parameters', [])}
    try: n = int(args['nelements']['value'])
    except (KeyError, ValueError): return None
    if n <= 0: return None
    return {'family': 'fp32', 'flops': 2*n, 'bytes': 8*n, 'recipe': 'llama.cpp scale: 2 FLOPs and 8 logical bytes per element', 'method': 'derived'}


def value(number, unit, reason, method, basis=None):
    if number is not None and not math.isfinite(number): raise ValueError('Computed characterization metric is not finite')
    return {'value': number, 'unit': unit, 'reason': reason, 'method': method, 'basis': basis}


def characterize(state, cfg, run, hsaco, isa, replay, calibration, names):
    log = run.parent / 'job.log'
    argv = [cfg['replay'], state['captured'], '--hsaco', str(hsaco), '--iterations', '1', '--hip-launch']
    groups = []
    if {'FETCH_SIZE', 'WRITE_SIZE'} <= names: groups.append(('device_memory', ['FETCH_SIZE', 'WRITE_SIZE']))
    if calibration['device']['arch'].startswith(('gfx11', 'gfx12')) and {'GL2C_HIT_sum', 'GL2C_MISS_sum'} <= names: groups.append(('l2', ['GL2C_HIT_sum', 'GL2C_MISS_sum']))
    counter_by_family = {'mfma_fp16': 'MfmaFlopsF16', 'mfma_bf16': 'MfmaFlopsBF16', 'mfma_fp32': 'MfmaFlopsF32'}
    families = isa_families(isa)
    for family in families:
        if counter_by_family.get(family) in names: groups.append((family, [counter_by_family[family]]))
    counts, errors = {}, {}
    for family, counters in groups:
        try: counts[family] = profile(cfg, argv, state['kernel'], counters, run / ('characterize-' + family), log)
        except Exception as error: errors[family] = str(error)
    metrics = {}; us = replay['timing']['average']; peaks = calibration['peaks']; identity = calibration.get('id', calibration['fingerprint'])
    for family, label, factor in [('device_memory', 'Device-memory', 1024), ('l2', 'L2', 128)]:
        if family not in counts: continue
        amount = sum(counts[family].values()) * factor
        rate = amount / us / 1e3
        basis = {'scope': family, 'counters': list(counts[family]), 'factor': factor}
        metrics[label + ' throughput'] = value(rate, 'GB/s', f'{family} traffic from counters / separate GPU timing', 'derived', basis)
        if family in peaks:
            metrics[label + ' calibrated utilization'] = value(rate / peaks[family]['rate'] * 100, '%', f'Of calibrated {family} peak; same counter definition', 'derived', {**basis, 'calibration': identity})
        if family == 'device_memory':
            metrics['Memory throughput'] = metrics[label + ' throughput']
            if label + ' calibrated utilization' in metrics: metrics['Memory speed of light'] = metrics[label + ' calibrated utilization']
    covered = []
    for family in families:
        if family not in counts: continue
        rate = sum(counts[family].values()) / us / 1e6
        metrics[family + ' throughput'] = value(rate, 'TFLOP/s', 'Per-type executed FLOPs from profiler counters', 'derived', {'family': family, 'counters': list(counts[family])})
        if family in peaks:
            utilization = rate / peaks[family]['rate'] * 100; covered.append(utilization)
            metrics[family + ' calibrated utilization'] = value(utilization, '%', 'Of calibrated ' + family + ' peak', 'derived', {'family': family, 'calibration': identity})
    if families and all(f in counts and f in peaks for f in families):
        metrics['Compute throughput'] = value(sum(sum(counts[f].values()) for f in families) / us / 1e6, 'TFLOP/s', 'Executed floating-point work from all detected families', 'derived', {'families': families})
        metrics['Compute speed of light'] = value(max(covered), '%', 'Largest per-family percentage of calibrated peak; parallel pipelines are not summed', 'derived', {'families': families, 'calibration': identity})
    useful = useful_work(state)
    if useful:
        metrics['Useful compute throughput'] = value(useful['flops'] / us / 1e6, 'TFLOP/s', useful['recipe'], 'derived', {'recipe': useful['recipe']})
        metrics['Effective memory throughput'] = value(useful['bytes'] / us / 1e3, 'GB/s', 'Logical source reads plus writes; not physical memory traffic', 'derived', {'recipe': useful['recipe']})
        if 'Memory throughput' not in metrics:
            metrics['Memory throughput'] = metrics['Effective memory throughput']
            if 'stream_copy' in peaks:
                metrics['Memory speed of light'] = value(useful['bytes'] / us / 1e3 / peaks['stream_copy']['rate'] * 100, '%', 'Logical byte rate / calibrated streaming-copy rate; physical traffic unavailable', 'derived', {'recipe': useful['recipe'], 'scope': 'logical', 'calibration': identity})
        if 'Compute throughput' not in metrics:
            metrics['Compute throughput'] = metrics['Useful compute throughput']
            metrics['Compute throughput']['reason'] = 'Useful-work rate: ' + useful['recipe']
            if useful['family'] in peaks:
                metrics['Compute speed of light'] = value(useful['flops'] / us / 1e6 / peaks[useful['family']]['rate'] * 100, '%', 'Useful-work equivalent / calibrated ' + useful['family'] + ' peak', 'derived', {'recipe': useful['recipe'], 'calibration': identity})
    for label, unit in [('Compute throughput', 'TFLOP/s'), ('Compute speed of light', '%'), ('Memory throughput', 'GB/s'), ('Memory speed of light', '%')]:
        if label not in metrics: metrics[label] = value(None, unit, 'Calibration available, but matching per-kernel counters/work model are unavailable', 'unavailable')
    return metrics, {'families': families, 'counterValues': counts, 'errors': errors, 'usefulWork': useful, 'calibration': identity}
