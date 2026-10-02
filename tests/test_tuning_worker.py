import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
from types import SimpleNamespace

spec = importlib.util.spec_from_file_location('worker', Path(__file__).resolve().parents[1] / 'src-tauri/src/tuning_worker.py')
w = importlib.util.module_from_spec(spec)
spec.loader.exec_module(w)


def assert_tree_killed(test, run_tree, root):
    """A timed-out command's own children die with it, as rocprofv3's profiled
    program must when a counter pass times out."""
    pidfile = root / 'grandchild.pid'
    script = 'import subprocess, sys, time; p = subprocess.Popen(["sleep", "30"]); open(sys.argv[1], "w").write(str(p.pid)); time.sleep(30)'
    with test.assertRaises(subprocess.TimeoutExpired):
        run_tree([sys.executable, '-c', script, str(pidfile)], 1)
    grandchild = Path(f'/proc/{int(pidfile.read_text())}/stat')
    deadline = time.time() + 5
    while grandchild.exists() and grandchild.read_text().split(') ')[-1][0] != 'Z' and time.time() < deadline:
        time.sleep(0.05)
    test.assertFalse(grandchild.exists() and grandchild.read_text().split(') ')[-1][0] != 'Z', 'grandchild survived')


class TuningTests(unittest.TestCase):
    def setUp(self):
        root = Path(__file__).resolve().parents[1] / '.codex/kernel-tuning/tests'
        root.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=root)
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def test_demangled_casts_and_function_pointers(self):
        self.assertEqual(w.kernel_base('void mul_mat_q<(ggml_type)8, 32, false>(char const*)'), 'mul_mat_q')
        self.assertEqual(w.kernel_base('void k_bin_bcast<&(op_add(float, float)), float>(float*)'), 'k_bin_bcast')
        self.assertEqual(w.kernel_base('space::kernel<int>(float*)'), 'kernel')
        self.assertEqual(w.kernel_base('void a::(anonymous namespace)::Gemm<64, true>(void const*)'), 'Gemm')
        self.assertEqual(w.kernel_base('a::(anonymous namespace)::Row(float const*)'), 'Row')

    def test_kernel_base_matches_shared_fixture_with_the_ts_classifier(self):
        # The same table src/kernelName.test.ts loads: parity between the Python
        # kernel_base and the TS kernelBase is enforced, not just claimed in a
        # comment. Includes the degenerate empty head that used to raise IndexError.
        cases = json.loads((Path(__file__).parent / 'kernel-name-bases.json').read_text())
        for case in cases:
            self.assertEqual(w.kernel_base(case['name']), case['base'], case['name'])

    def test_mangled_att_kernel_matches_kerncap_name(self):
        self.assertEqual(w.canonical_kernel('_Z5saxpyPKfS0_Pfi'), 'saxpy(float const*, float const*, float*, int)')
        self.assertEqual(w.canonical_kernel('saxpy(float const*, float const*, float*, int) [clone .kd]'), 'saxpy(float const*, float const*, float*, int)')

    def test_template_braces_comments_and_strings(self):
        text = 'int outside;\ntemplate<int N>\nstatic __global__ void kernel(float* p) {\n // }\n const char *a = "}";\n if (N) { p[0]=1; }\n}\nint after;'
        start, opening, end = w.source_regions(text, 'kernel')[0]
        self.assertTrue(text[start:].startswith('template<int N>'))
        self.assertEqual(text[end:], '\nint after;')
        self.assertEqual(text[opening], '{')

    def test_launch_bounds_preserves_template_context(self):
        text = 'template<int N>\n__launch_bounds__(N, 2)\nstatic __global__ void k(float* p) { p[0]=N; }'
        start, _, end = w.source_regions(text, 'k')[0]
        self.assertEqual(text[start:end], text)

    def test_exact_metadata_and_argument_layout(self):
        data = 'amdhsa.kernels:\n  - .args:\n      - .offset: 0\n        .size: 8\n        .value_kind: global_buffer\n    .kernarg_segment_size: 16\n    .name: _Z1kPf\n    .vgpr_count: 12\n  - .args:\n      - .offset: 4\n        .size: 4\n        .value_kind: by_value\n    .kernarg_segment_size: 8\n    .name: _Z1kf\n    .vgpr_count: 99\n'
        with patch.object(w.subprocess, 'run', return_value=SimpleNamespace(returncode=0,stdout=data)):
            result = w.notes('code.hsaco', 'readobj', '_Z1kPf')
        self.assertEqual(result['vgpr_count'], 12)
        self.assertEqual(result['arguments'], [{'offset': 0, 'size': 8, 'value_kind': 'global_buffer'}])

    def test_declaration_and_other_symbols_ignored(self):
        self.assertEqual(w.source_regions('__global__ void kernel();\n__global__ void other() {}', 'kernel'), [])

    def test_source_search_skips_hidden_history(self):
        (self.root / '.Codex').mkdir()
        (self.root / '.Codex/old.cu').write_text('__global__ void k() {}')
        (self.root / 'real.cu').write_text('__global__ void k() {}')
        self.assertEqual([Path(c['path']).name for c in w.search_sources(self.root, 'k()')], ['real.cu'])

    def test_compile_command_preserves_original_context(self):
        result = w.clean_compile(['clang', '-DTEST', '-Iinclude', '-c', 'kernel.cu', '-o', 'old.o', '--offload-arch=gfx942', '-MD', '-MF', 'old.d'], 'gfx1201', '/overlay', '/output')
        self.assertIn('-DTEST', result)
        self.assertIn('kernel.cu', result)
        self.assertIn('--offload-arch=gfx1201', result)
        self.assertNotIn('old.o', result)
        self.assertNotIn('old.d', result)

    def test_nonfinite_work_rejected(self):
        with self.assertRaises(ValueError):
            w.metrics_for({'timing': {'average': 1}}, {}, '', {'flops': 'nan'})

    def test_resources_are_not_measured_occupancy(self):
        m = w.metrics_for({'timing': {'average': 2}}, {'vgpr_count': 32}, 'v_wmma_f32_16x16x16_f16', {'flops': 2000000, 'peakTflops': 2})
        self.assertEqual(m['Compute throughput']['value'], 1)
        self.assertEqual(m['Compute speed of light']['value'], 50)
        self.assertIsNone(m['Occupancy']['value'])
        self.assertEqual(m['WMMA / MFMA instructions']['value'], 1)

    def test_replay_conditions_default_to_isolated_stateless(self):
        self.assertEqual(w.conditions_of({}), {'mode': 'stateless', 'repeats': 1})
        self.assertEqual(w.conditions_of({'conditions': None}), {'mode': 'stateless', 'repeats': 1})
        c = w.conditions_of({})
        argv = w.replay_argv({'replay': '/r/kerncap-replay'}, '/cap', '/k.hsaco', 5, c)
        self.assertEqual(argv, ['/r/kerncap-replay', '/cap', '--hsaco', '/k.hsaco', '--iterations', '5', '--json', '--hip-launch'])
        self.assertNotIn('--no-recopy', argv)
        self.assertEqual(w.expected_replay_mode(c), 'stateless')

    def test_sustained_condition_maps_to_kerncap_no_recopy(self):
        c = w.conditions_of({'conditions': {'mode': 'sustained', 'repeats': 3}})
        self.assertEqual(c, {'mode': 'sustained', 'repeats': 3})
        argv = w.replay_argv({'replay': '/r/kerncap-replay'}, '/cap', '/k.hsaco', 20, c)
        self.assertEqual(argv[-1], '--no-recopy')
        # kerncap reports its back-to-back path as 'stateful'; the run op verifies this.
        self.assertEqual(w.expected_replay_mode(c), 'stateful')

    def test_invalid_replay_conditions_rejected(self):
        for bad in [{'mode': 'turbo'}, {'repeats': 0}, {'repeats': 11}]:
            with self.assertRaises(ValueError):
                w.conditions_of({'conditions': bad})

    def test_vulkan_only_conditions_do_not_leak_into_hip(self):
        # Memory placement and cache flush are Vulkan-only; stray keys must be dropped,
        # never silently forwarded as if the HIP replay honored them.
        c = w.conditions_of({'conditions': {'mode': 'stateless', 'memory': 'gtt', 'flushMb': 64}})
        self.assertEqual(set(c), {'mode', 'repeats'})
        self.assertNotIn('--memory', w.replay_argv({'replay': '/r'}, '/c', '/k', 1, c))

    def session(self):
        original = 'int before;\n__global__ void k(float* p) { p[0]=1; }\nint after;\n'
        path = self.root / 'kernel.cu'; path.write_text(original)
        candidate = w.search_sources(self.root, 'k(float*)')[0]
        state = {'kernel': 'k(float*)', 'source': candidate, 'config': {}, 'code': candidate['code']}
        (self.root / 'session.json').write_text(json.dumps(state))
        return path, original

    def test_save_preserves_surroundings_and_backup(self):
        path, original = self.session()
        code = '__global__ void k(float* p) { p[0]=2; }'
        state = w.main(self.root, {'op': 'save', 'code': code})
        self.assertEqual(path.read_text(), original.replace('p[0]=1', 'p[0]=2'))
        self.assertEqual(Path(state['lastSave']['backup']).read_text(), original)
        w.main(self.root, {'op': 'save', 'code': code.replace('=2', '=3')})
        self.assertIn('p[0]=3', path.read_text())

    def test_save_rejects_external_edits(self):
        path, original = self.session(); path.write_text(original + '// external')
        with self.assertRaisesRegex(ValueError, 'changed externally'):
            w.main(self.root, {'op': 'save', 'code': '__global__ void k(float* p) {}'})
        self.assertTrue(path.read_text().endswith('// external'))

    def test_save_rejects_extra_definitions(self):
        path, original = self.session()
        with self.assertRaises(ValueError):
            w.main(self.root, {'op': 'save', 'code': '__global__ void k(float* p) {}\nvoid other() {}'})
        self.assertEqual(path.read_text(), original)

    def ready_to_capture(self, **config):
        self.session()
        state = json.loads((self.root / 'session.json').read_text())
        state['config'] = {'argv': ['app'], 'cwd': str(self.root), 'captureLibrary': str(self.root / 'libkerncap.so'), 'occurrence': 0, **config}
        (self.root / 'session.json').write_text(json.dumps(state))

    def test_failed_capture_leaves_no_snapshot_behind(self):
        self.ready_to_capture()
        def partial_snapshot(argv, cwd, log, env=None):
            memory = Path(env['KERNCAP_OUTPUT']) / 'memory'; memory.mkdir()
            (memory / 'region_1000.bin').write_bytes(b'x' * 4096)
            raise RuntimeError('Command exited 1')
        with patch.object(w, 'command', side_effect=partial_snapshot):
            with self.assertRaises(RuntimeError):
                w.main(self.root, {'op': 'capture'})
        self.assertEqual(list(self.root.glob('capture-*')), [])
        self.assertNotIn('captured', json.loads((self.root / 'session.json').read_text()))

    def test_capture_is_refused_without_room_for_a_snapshot(self):
        self.ready_to_capture()
        with patch.object(w.shutil, 'disk_usage', return_value=SimpleNamespace(total=0, used=0, free=1 << 30)), \
             patch.object(w, 'command') as command:
            with self.assertRaisesRegex(ValueError, 'at least 8 GiB free'):
                w.main(self.root, {'op': 'capture'})
        command.assert_not_called()
        self.assertEqual(list(self.root.glob('capture-*')), [])

    def test_capture_uses_the_recorded_environment_and_the_usual_model_cache(self):
        self.ready_to_capture(env={'HIP_VISIBLE_DEVICES': '1'})
        seen = {}
        def record(argv, cwd, log, env=None):
            seen.update(env); raise RuntimeError('stop')
        with patch.dict(os.environ, {'LLAMA_CACHE': '/models'}), patch.object(w, 'command', side_effect=record):
            with self.assertRaises(RuntimeError):
                w.main(self.root, {'op': 'capture'})
        # No second copy of the model under the session directory.
        self.assertEqual(seen['LLAMA_CACHE'], '/models')
        self.assertEqual(seen['HIP_VISIBLE_DEVICES'], '1')
        self.assertTrue(seen['LD_PRELOAD'].endswith('libkerncap.so'))
        for reserved in ('LD_PRELOAD', 'KERNCAP_OUTPUT', 'lower'):
            self.ready_to_capture(env={reserved: 'x'})
            with self.assertRaisesRegex(ValueError, 'not allowed'):
                w.main(self.root, {'op': 'capture'})

    def capture_with(self, regions, **config):
        self.ready_to_capture(**config)
        (self.root / 'original-source').write_text((self.root / 'kernel.cu').read_text())
        seen = {}
        def capture(argv, cwd, log, env=None):
            seen.update(env)
            (Path(env['KERNCAP_OUTPUT']) / 'memory_regions.json').write_text(json.dumps(regions))
        with patch.object(w, 'command', side_effect=capture), patch.object(w, 'finish_capture'):
            return w.main(self.root, {'op': 'capture'}), seen

    def test_capture_keeps_only_reachable_allocations_by_default(self):
        regions = {'scope': 'reachable', 'tracked_regions': 1266, 'tracked_bytes': 78 << 30,
                   'regions': [{'base': 4096, 'size': 100 << 20}, {'base': 1 << 40, 'size': 10 << 20}]}
        state, env = self.capture_with(regions)
        self.assertEqual(env['KERNCAP_SNAPSHOT_SCOPE'], 'reachable')
        self.assertEqual(state['captureScope'], 'reachable')
        self.assertEqual(state['captureRegions'], {'kept': 2, 'keptBytes': 110 << 20, 'tracked': 1266, 'trackedBytes': 78 << 30})
        self.assertNotIn('captureScopeNote', state)

    def test_capture_scope_all_and_old_runtimes(self):
        _, env = self.capture_with({'scope': 'all', 'regions': []}, captureScope='all')
        self.assertEqual(env['KERNCAP_SNAPSHOT_SCOPE'], 'all')
        # A runtime built before capture scopes ignores the variable and says nothing.
        self.temp.cleanup(); self.setUp()
        state, _ = self.capture_with({'regions': [{'base': 4096, 'size': 64}]})
        self.assertEqual(state['captureScope'], 'all')
        self.assertIn('setup-kernel-tuning', state['captureScopeNote'])
        self.temp.cleanup(); self.setUp()
        self.ready_to_capture(captureScope='some')
        with self.assertRaisesRegex(ValueError, 'reachable or all'):
            w.main(self.root, {'op': 'capture'})

    def test_validation_dumps_are_removed_whether_or_not_the_replay_succeeds(self):
        capture = self.root / 'capture'; (capture / 'memory').mkdir(parents=True)
        (capture / 'memory' / 'region_1000.bin').write_bytes(b'a' * 64)
        (capture / 'memory_regions.json').write_text(json.dumps({'regions': [{'base': 0x1000, 'size': 64}]}))
        def dump(size):
            def run(argv, cwd, log, env=None):
                out = Path(argv[1]) / 'output'; out.mkdir(); (out / 'region_1000.bin').write_bytes(b'b' * size)
            return run
        with patch.object(w, 'command', side_effect=dump(5)):
            with self.assertRaisesRegex(ValueError, 'complete output region'):
                w.replay_output_hashes(capture, capture / 'kernel.hsaco', {'replay': 'r'}, self.root / 'failed', self.root / 'job.log')
        with patch.object(w, 'command', side_effect=dump(64)):
            hashes = w.replay_output_hashes(capture, capture / 'kernel.hsaco', {'replay': 'r'}, self.root / 'passed', self.root / 'job.log')
        self.assertEqual(hashes, {'region_1000.bin': w.digest(b'b' * 64)})
        self.assertFalse((self.root / 'failed').exists() or (self.root / 'passed').exists())
        self.assertEqual((capture / 'memory' / 'region_1000.bin').read_bytes(), b'a' * 64)

    def test_parameters_split_only_at_top_level_commas(self):
        code = ('template <bool kHcDown>\n__launch_bounds__(256, 2)\n__global__ void gemm(const void* a, /* packed, */ int b,\n'
                '    std::conditional_t<kHcDown, __half, float>* out, unsigned long batch, void (*hook)(int, float)) { }')
        self.assertEqual(w.parameter_list(code, 'gemm'), ['const void* a', 'int b', 'std::conditional_t<kHcDown, __half, float>* out',
                                                          'unsigned long batch', 'void (*hook)(int, float)'])

    def test_timeout_kills_the_whole_process_tree(self):
        assert_tree_killed(self, w.calibration.run_tree, self.root)

    def test_stream_hash(self):
        p = self.root / 'data'; p.write_bytes(b'test' * 1024)
        self.assertEqual(w.file_digest(p), w.digest(p.read_bytes()))


if __name__ == '__main__':
    unittest.main()
