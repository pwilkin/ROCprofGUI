import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('shader_worker', Path(__file__).resolve().parents[1] / 'src-tauri/src/shader_worker.py')
w = importlib.util.module_from_spec(spec)
spec.loader.exec_module(w)

CAPABILITY, EXTENSION, MEMORY_MODEL, ENTRY_POINT, EXECUTION_MODE, DECORATE = 17, 10, 14, 15, 16, 71


def literal(text):
    data = text.encode() + b'\0'
    data += b'\0' * (-len(data) % 4)
    return list(struct.unpack(f'<{len(data) // 4}I', data))


def module(*instructions, entry=4):
    words = [0x07230203, 0x00010300, 0, 20, 0]
    for opcode, *operands in instructions:
        words += [(len(operands) + 1) << 16 | opcode, *operands]
    return struct.pack(f'<{len(words)}I', *words)


def shader(entry=4, extra=()):
    """A compute module with two buffers at bindings 0 and 1, the first declared
    through two aliased variables as ggml does."""
    return module(
        (CAPABILITY, 1), *[x for x in extra if x[0] in (CAPABILITY, EXTENSION)],
        (MEMORY_MODEL, 0, 1), (ENTRY_POINT, 5, entry, *literal('main')),
        (EXECUTION_MODE, entry, 17, 64, 1, 1), *[x for x in extra if x[0] == EXECUTION_MODE],
        (DECORATE, 7, 34, 0), (DECORATE, 7, 33, 0), (DECORATE, 8, 34, 0), (DECORATE, 8, 33, 0),
        (DECORATE, 9, 34, 0), (DECORATE, 9, 33, 1))


# What ggml adds at run time: fp16 float controls.
RUNTIME = [(CAPABILITY, 4464), (EXTENSION, *literal('SPV_KHR_float_controls')), (EXECUTION_MODE, 4, 4459, 16)]


class SpirvTests(unittest.TestCase):
    def test_matching_ignores_only_the_preamble(self):
        self.assertEqual(w.normalized(shader()), w.normalized(shader(extra=RUNTIME)))
        different = module((CAPABILITY, 1), (MEMORY_MODEL, 0, 1), (ENTRY_POINT, 5, 4, *literal('main')), (DECORATE, 7, 33, 2))
        self.assertNotEqual(w.normalized(shader()), w.normalized(different))

    def test_runtime_edits_are_reapplied_to_the_variant_entry_point(self):
        captured, generated = shader(extra=RUNTIME), shader()
        mine, theirs = w.preamble(captured), w.preamble(generated)
        patch = {'added': [list(x) for x in mine if x not in theirs], 'removed': [list(x) for x in theirs if x not in mine]}
        self.assertEqual(len(patch['added']), 3)
        self.assertEqual(w.reapply(generated, patch), captured)
        # A recompiled variant can number its entry point differently.
        variant = w.reapply(shader(entry=6), patch)
        ins = w.instructions(w.spirv_words(variant))
        self.assertIn((EXECUTION_MODE << 0 | 4 << 16, 6, 4459, 16), ins)
        opcodes = [x[0] & 0xffff for x in ins]
        self.assertLess(opcodes.index(EXTENSION), opcodes.index(MEMORY_MODEL))
        self.assertEqual(opcodes.count(CAPABILITY), 2)

    def test_aliased_bindings_count_once(self):
        self.assertEqual(w.declared_bindings(shader()), [(0, 0), (0, 1)])

    def test_malformed_modules_are_rejected(self):
        with self.assertRaises(ValueError):
            w.normalized(b'not spirv')
        with self.assertRaises(ValueError):
            w.instructions([0x07230203, 0, 0, 0, 0, 0x00050011])


class InputTests(unittest.TestCase):
    def test_recipe_is_rewritten_for_the_overlay(self):
        argv = ['-fshader-stage=compute', '/src/mul_mm.comp', '-o', '/build/x.spv', '-MD', '-MF', '/build/x.d', '-DA=1']
        self.assertEqual(w.compile_args(argv, '/src/mul_mm.comp', '/run/overlay/mul_mm.comp', '/run/v.spv'),
                         ['-fshader-stage=compute', '/run/overlay/mul_mm.comp', '-DA=1', '-o', '/run/v.spv'])

    def test_workgroup_counts_come_from_the_traced_grid(self):
        self.assertEqual(w.expected_groups({'grid': '16384 × 2 × 1', 'workgroup': '256 × 1 × 1'}), [64, 2, 1])
        self.assertIsNone(w.expected_groups({'grid': 'Not recorded', 'workgroup': '256 × 1 × 1'}))
        self.assertIsNone(w.expected_groups({'grid': '100 × 1 × 1', 'workgroup': '64 × 1 × 1'}))

    def test_conditions_default_to_the_isolated_best_case(self):
        self.assertEqual(w.conditions_of({}), w.DEFAULT_CONDITIONS)
        c = w.conditions_of({'conditions': {'mode': 'sustained', 'memory': 'auto', 'repeats': '3'}})
        self.assertEqual(c, {'mode': 'sustained', 'memory': 'auto', 'flushMb': 0, 'repeats': 3})
        for bad in ({'mode': 'burst'}, {'memory': 'vram'}, {'repeats': 0}, {'repeats': 11}, {'flushMb': 2048}):
            with self.assertRaises(ValueError):
                w.conditions_of({'conditions': bad})

    def test_replay_command_carries_conditions(self):
        cfg = {'replay': '/rt/vkreplay'}
        c = w.conditions_of({'conditions': {'mode': 'sustained', 'memory': 'gtt', 'flushMb': 64}})
        self.assertEqual(w.replay_argv(cfg, '/cap', '/v.spv', 50, c, validate=False, counters=True, isa='/run/isa.txt'),
                         ['/rt/vkreplay', '/cap', '--spirv', '/v.spv', '--iterations', '50', '--mode', 'sustained', '--memory', 'gtt',
                          '--flush-mb', '64', '--isa', '/run/isa.txt', '--counters', '--no-validate'])
        self.assertEqual(w.replay_argv(cfg, '/cap', '/v.spv', 20, w.DEFAULT_CONDITIONS)[5:],
                         ['20', '--mode', 'stateless', '--memory', 'device'])

    def test_counters_become_cycles_clock_and_hit_rates(self):
        def result(median_us, cycles, counted_ns, l2):
            return {'timing': {'median': median_us * 1e3, 'min': median_us * 1e3, 'max': median_us * 1e3, 'samples': [median_us * 1e3]},
                    'statistics': {}, 'counters': {'passes': 2, 'dispatches': 20, 'durationNs': counted_ns,
                                                   'values': {'GPU active cycles': cycles, 'L0 cache hit ratio': 87.5, 'L1 cache hit ratio': 25.0, 'L2 cache hit ratio': l2}}}
        m = w.metrics_for([result(1000, 5e6, 2e6, 50.0), result(1200, 6e6, 2.4e6, 48.0), result(1100, 5.5e6, 2.2e6, 49.0)], '', {}, True)
        self.assertEqual(m['GPU duration']['value'], 1100)
        self.assertIn('3 replays with fresh allocations', m['GPU duration']['reason'])
        self.assertAlmostEqual(m['Replay spread']['value'], 100 * 200 / 1100)
        self.assertEqual(m['GPU cycles']['value'], 5.5e6)
        self.assertAlmostEqual(m['Clock']['value'], 2500)
        self.assertEqual((m['L2 hit rate']['value'], m['L2 hit rate']['unit']), (49.0, '%'))
        single = w.metrics_for([{'timing': {'median': 1e6, 'min': 1e6, 'max': 1e6, 'samples': [1e6]}, 'statistics': {}, 'counters': None}], '', {}, False)
        self.assertNotIn('Replay spread', single)
        self.assertEqual((single['Clock']['value'], single['Clock']['reason']), (None, 'Counters not requested'))

    def test_environment_is_validated_and_tracing_removed(self):
        env = w.application_env({'env': {'VK_DRIVER_FILES': '/icd.json', 'MESA_VK_TRACE': 'rgp'}})
        self.assertEqual(env['VK_DRIVER_FILES'], '/icd.json')
        self.assertNotIn('MESA_VK_TRACE', env)
        for bad in ('LD_PRELOAD', 'lower', 'TMPDIR'):
            with self.assertRaises(ValueError):
                w.application_env({'env': {bad: 'x'}})


class SessionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def capture(self):
        c = self.root / 'capture'
        c.mkdir()
        (c / 'capture.txt').write_text('\n'.join([
            'version 1', 'device 4098 5510 1 AMD Test GPU', 'name matmul', 'label MUL_MAT', 'label matmul (64,2,1)', 'entry main',
            'stage_flags 2', 'required_subgroup_size 0', 'pipeline_flags 0', 'spirv shader.spv 20', 'spec_data spec.bin 8',
            'spec 0 0 4', 'spec 1 4 4', 'push push.bin 4', 'push_range 32 0 4', 'set_layout 0 0 7 1', 'groups 64 2 1',
            'buffer 0 16 in_0.bin out_0.changes 4', 'bind 0 0 0 7 0 0 16', 'feature 49 feature_0.bin 48', 'unknown_feature 7']) + '\n')
        (c / 'spec.bin').write_bytes(struct.pack('<II', 256, 2))
        (c / 'push.bin').write_bytes(b'\x01\x02\x03\x04')
        (c / 'shader.spv').write_bytes(shader())
        return c

    def state(self, **fields):
        base = {'mode': 'vulkan', 'kernel': 'matmul', 'traceDispatch': {}, 'runs': [], 'directory': str(self.root),
                'config': {'argv': ['app'], 'layer': str(self.root / 'layer.json'), 'replay': str(self.root / 'vkreplay'), 'env': {}}}
        base.update(fields)
        (self.root / 'session.json').write_text(json.dumps(base))

    def test_capture_manifest_is_read(self):
        c = self.capture()
        with self.assertRaises(RuntimeError):
            w.read_capture(c)
        (c / 'capture_complete').write_text('')
        info = w.read_capture(c)
        self.assertEqual(info['groups'], [64, 2, 1])
        self.assertEqual(info['labels'], ['MUL_MAT', 'matmul (64,2,1)'])
        self.assertEqual([s['value'] for s in info['specValues']], [256, 2])
        self.assertEqual(info['pushHex'], '01020304')
        self.assertEqual(info['buffers'][0]['writtenBytes'], 4)
        self.assertEqual(info['unknownFeatures'], [7])

    def test_capture_layer_error_is_reported(self):
        c = self.capture()
        (c / 'capture_error').write_text('binding 3 holds a descriptor type other than a buffer')
        with self.assertRaisesRegex(RuntimeError, 'descriptor type'):
            w.read_capture(c)

    def test_hash_only_names_cannot_be_captured(self):
        (self.root / 'layer.json').write_text('{}')
        (self.root / 'vkreplay').write_text('')
        self.state(kernel='pipeline 834527b9cb9b4ffb')
        with self.assertRaisesRegex(ValueError, 'GGML_VK_DEBUG_MARKERS'):
            w.main(self.root, {'op': 'capture'})

    def test_failed_capture_leaves_no_buffers_behind(self):
        (self.root / 'layer.json').write_text('{}')
        (self.root / 'vkreplay').write_text('')
        self.state()
        state = json.loads((self.root / 'session.json').read_text())
        # The application writes part of a capture but never reaches the dispatch.
        state['config']['argv'] = ['python3', '-c', 'import os, pathlib; pathlib.Path(os.environ["ROCPROF_VKCAP_OUTPUT"], "in_0.bin").write_bytes(b"x" * 4096)']
        (self.root / 'session.json').write_text(json.dumps(state))
        with self.assertRaisesRegex(RuntimeError, 'never reached the selected dispatch'):
            w.main(self.root, {'op': 'capture'})
        self.assertEqual(list(self.root.glob('capture-*')), [])

    def test_missing_runtime_points_at_the_setup_script(self):
        self.state()
        with self.assertRaisesRegex(ValueError, 'setup-shader-tuning'):
            w.main(self.root, {'op': 'capture'})

    def test_only_compiled_files_can_be_edited(self):
        c = self.capture()
        (c / 'capture_complete').write_text('')
        hashes = {p.name: w.file_digest(p) for p in c.iterdir() if p.is_file()}
        self.state(captured=str(c), snapshotHashes=hashes, recipe={'shaderDir': str(self.root)}, files=[{'path': str(self.root / 'a.comp'), 'hash': '', 'code': ''}])
        with self.assertRaisesRegex(ValueError, 'Only files the shader compile reads'):
            w.main(self.root, {'op': 'run', 'variant': True, 'edits': {'/etc/passwd': 'x'}})

    def fake_replay(self):
        """A vkreplay stand-in: slower in sustained mode and system memory, and a
        little slower on every call, like fresh allocations landing differently."""
        script = self.root / 'vkreplay'
        script.write_text(f'''#!/usr/bin/env python3
import json, sys
from pathlib import Path
a = sys.argv
calls = Path({str(self.root / 'calls')!r})
n = int(calls.read_text()) if calls.exists() else 0
calls.write_text(str(n + 1))
mode, memory = a[a.index('--mode') + 1], a[a.index('--memory') + 1]
memory = 'gtt' if memory == 'auto' else memory
us = (1200 if mode == 'sustained' else 1000) * (1.5 if memory == 'gtt' else 1) + 10 * n
if '--isa' in a:
    Path(a[a.index('--isa') + 1]).write_text('v_wmma_f16_16x16x16_f16 v[0:7]')
validation = 'not checked' if '--no-validate' in a else {{'match': True, 'groups': [{{'group': 0, 'bytes': 16, 'mismatched': 0}}]}}
if mode == 'sustained' and isinstance(validation, dict):
    validation['sustainedMatch'] = False
notes = ['sustained mode: inputs were restored once, so timings are valid only if the dispatch does not read what it writes'] if mode == 'sustained' else []
counters = {{'passes': 2, 'dispatches': 20, 'durationNs': us * 1100, 'values': {{'GPU active cycles': us * 2750, 'L2 cache hit ratio': 79.0}}}} if '--counters' in a else None
print(json.dumps({{'device': 'Test GPU', 'deviceType': 'integrated', 'driverVersion': (26 << 22) | (2 << 12),
                  'execution': {{'mode': mode, 'memory': memory}}, 'timing': {{'median': us * 1e3, 'min': us * 1e3, 'max': us * 1e3, 'samples': [us * 1e3]}},
                  'statistics': {{'VGPRs': 120}}, 'counters': counters, 'validation': validation, 'notes': notes}}))
''')
        script.chmod(0o755)
        return script

    def test_runs_report_conditions_counters_and_compare_like_with_like(self):
        c = self.capture()
        (c / 'capture_complete').write_text('')
        hashes = {p.name: w.file_digest(p) for p in c.iterdir() if p.is_file()}
        self.state(captured=str(c), snapshotHashes=hashes, dispatch=w.read_capture(c))
        state = json.loads((self.root / 'session.json').read_text())
        state['config']['replay'] = str(self.fake_replay())
        (self.root / 'session.json').write_text(json.dumps(state))

        w.main(self.root, {'op': 'run', 'variant': False})
        w.main(self.root, {'op': 'run', 'variant': False, 'autoCounters': False,
                           'conditions': {'mode': 'sustained', 'memory': 'auto', 'repeats': 3}})
        runs = w.main(self.root, {'op': 'run', 'variant': False})['runs']

        best, app, again = runs
        self.assertEqual(best['conditions'], w.DEFAULT_CONDITIONS)
        self.assertEqual(best['driver'], {'version': '26.2.0', 'deviceType': 'integrated', 'device': 'Test GPU', 'memory': 'device'})
        self.assertAlmostEqual(best['metrics']['Clock']['value'], 2500)
        self.assertEqual(best['metrics']['L2 hit rate']['value'], 79.0)
        # Three replays in system memory under sustained load, without counters.
        self.assertEqual(app['driver']['memory'], 'gtt')
        self.assertEqual(app['metrics']['GPU duration']['value'], 1800 + 20)
        self.assertIn('Replay spread', app['metrics'])
        self.assertEqual(app['metrics']['Clock']['reason'], 'Counters not requested')
        self.assertFalse(app['validation']['sustainedMatch'])
        # The failed two-dispatch check is reported once, in the worker's words.
        self.assertEqual([n for n in app['notes'] if 'reads what it writes' in n or 'restored once' in n][1:], [])
        self.assertIn('reads what it writes', app['notes'][-1])
        self.assertEqual(len(list(Path(app['directory']).glob('replay*.json'))), 3)
        # No deltas across conditions; the third run compares with the first.
        self.assertIsNone(app['comparedWith'])
        self.assertNotIn('delta', app['metrics']['GPU duration'])
        self.assertEqual(again['comparedWith'], 1)
        self.assertEqual(again['metrics']['GPU duration']['delta'], 40)

    def test_save_refuses_external_changes_and_keeps_a_backup(self):
        source = self.root / 'shader.comp'
        source.write_text('void main() {}\n')
        self.state(files=[{'path': str(source), 'hash': w.digest(source.read_bytes()), 'code': source.read_text()}])
        w.main(self.root, {'op': 'save', 'edits': {str(source): 'void main() { }\n'}})
        self.assertEqual(source.read_text(), 'void main() { }\n')
        saved = json.loads((self.root / 'session.json').read_text())['lastSave']
        self.assertEqual(Path(saved['files'][0]['backup']).read_text(), 'void main() {}\n')
        source.write_text('changed elsewhere\n')
        with self.assertRaisesRegex(ValueError, 'changed externally'):
            w.main(self.root, {'op': 'save', 'edits': {str(source): 'x'}})


if __name__ == '__main__':
    unittest.main()
