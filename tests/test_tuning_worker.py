import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace

spec = importlib.util.spec_from_file_location('worker', Path(__file__).resolve().parents[1] / 'src-tauri/src/tuning_worker.py')
w = importlib.util.module_from_spec(spec)
spec.loader.exec_module(w)


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

    def test_stream_hash(self):
        p = self.root / 'data'; p.write_bytes(b'test' * 1024)
        self.assertEqual(w.file_digest(p), w.digest(p.read_bytes()))


if __name__ == '__main__':
    unittest.main()
