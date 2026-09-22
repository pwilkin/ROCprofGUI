import csv
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('calibration', Path(__file__).resolve().parents[1] / 'src-tauri/src/tuning_calibration.py')
c = importlib.util.module_from_spec(spec); spec.loader.exec_module(c)


class CalibrationTests(unittest.TestCase):
    def setUp(self):
        path = Path(__file__).resolve().parents[1] / '.codex/calibration/tests'; path.mkdir(parents=True, exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(dir=path); self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_rates_and_variability(self):
        r = c.summarize({'outputCheck':'passed','samplesUs':[10]*9, 'operations':20e6, 'unit':'TFLOP/s'})
        self.assertEqual(r['rate'],2); self.assertEqual(r['quality'],'stable')
        r = c.summarize({'outputCheck':'passed','samplesUs':[10,10,10,10,100], 'operations':10000, 'unit':'GB/s'})
        self.assertEqual(r['rate'],1); self.assertEqual(r['quality'],'variable')

    def test_bad_calibration_samples_rejected(self):
        for samples in [[1,2], [0]*9, [float('nan')]*9, [-1]*9]:
            with self.assertRaises(ValueError): c.summarize({'outputCheck':'passed','samplesUs':samples,'operations':100,'unit':'GB/s'})

    def test_cache_invalidation(self):
        record={'version':c.VERSION,'fingerprint':'a','createdAt':100}
        self.assertTrue(c.fresh(record,'a',101))
        self.assertFalse(c.fresh(record,'b',101))
        self.assertFalse(c.fresh(record,'a',100+c.MAX_AGE))
        self.assertFalse(c.fresh(record,'a',99))
        self.assertFalse(c.fresh({**record,'version':-1},'a',101))

    def test_instruction_family_matching(self):
        isa=' v_wmma_f32_16x16x16_bf16 v0, v1\n v_mfma_f32_16x16x16f16 v0, v1\n v_fma_f32 v0, v1\n v_pk_fma_f16 v0, v1\n v_mul_f64 v0, v1\n'
        self.assertEqual(c.isa_families(isa),['fp16_packed','fp32','fp64','mfma_fp16','wmma_bf16'])

    def make_csv(self, rows):
        with (self.root/'counters.csv').open('w') as f:
            w=csv.DictWriter(f,fieldnames=['Kernel_Name','Counter_Name','Counter_Value']);w.writeheader();w.writerows(rows)

    def test_counter_scope_and_duplicate_rejection(self):
        rows=[{'Kernel_Name':'other','Counter_Name':'X','Counter_Value':'500'}, {'Kernel_Name':'k','Counter_Name':'X','Counter_Value':'10'}]
        self.make_csv(rows);self.assertEqual(c.parse_counters(self.root,'k',['X']),{'X':10})
        self.make_csv(rows+[rows[-1]])
        with self.assertRaises(ValueError):c.parse_counters(self.root,'k',['X'])

    def test_counter_nan_is_not_zero(self):
        self.make_csv([{'Kernel_Name':'k','Counter_Name':'X','Counter_Value':'nan'}])
        with self.assertRaises(ValueError):c.parse_counters(self.root,'k',['X'])

    def test_collects_derived_counters_in_separate_passes(self):
        with patch.object(c,'execute') as execute, patch.object(c,'parse_counters',side_effect=[{'A':1},{'B':2}]):
            result=c.profile({'profiler':'prof'},['replay'],'k',['A','B'],self.root/'profile',self.root/'log')
        self.assertEqual(result,{'A':1,'B':2});self.assertEqual(execute.call_count,2)
        for call in execute.call_args_list:
            args=call.args[0];self.assertEqual(args[args.index('--pmc')+2],'--kernel-include-regex')

    def test_memory_scope_uses_matching_denominator(self):
        cal={'fingerprint':'a','device':{'arch':'gfx1151'},'peaks':{'device_memory':{'rate':102.4},'l2':{'rate':256}}}
        state={'captured':'capture','kernel':'unknown','capturedSource':''}
        with patch.object(c,'profile',side_effect=[{'FETCH_SIZE':1,'WRITE_SIZE':1},{'GL2C_HIT_sum':10,'GL2C_MISS_sum':10}]):
            metrics, details=c.characterize(state,{'replay':'r'},self.root/'run','obj','',{'timing':{'average':1}},cal,{'FETCH_SIZE','WRITE_SIZE','GL2C_HIT_sum','GL2C_MISS_sum'})
        self.assertEqual(metrics['Memory throughput']['value'],2.048)
        self.assertEqual(metrics['Memory speed of light']['value'],2)
        self.assertEqual(metrics['L2 calibrated utilization']['value'],1)
        self.assertIsNone(metrics['Compute speed of light']['value'])
        self.assertEqual(metrics['Memory speed of light']['basis']['scope'],'device_memory')

    def test_unknown_kernel_does_not_invent_flops(self):
        cal={'fingerprint':'a','device':{'arch':'gfx1151'},'peaks':{'fp32':{'rate':20}}}
        metrics,details=c.characterize({'captured':'capture','kernel':'k'}, {'replay':'r'},self.root,'obj',' v_fma_f32 v0, v1',{'timing':{'average':10}},cal,set())
        self.assertIsNone(metrics['Compute throughput']['value'])
        self.assertEqual(details['families'],['fp32'])

    def test_useful_work_recipe_rejects_changed_algorithm(self):
        code='static __global__ void scale_f32() {ggml_cuda_pdl_lc();int64_t tid = (int64_t)blockIdx.x * (int64_t)blockDim.x + (int64_t)threadIdx.x;int64_t stride = (int64_t)blockDim.x * (int64_t)gridDim.x;ggml_cuda_pdl_sync();for (int64_t i=tid;i<nelements;i+=stride) {dst[i]=scale*x[i]+bias;}}'
        state={'kernel':'scale_f32(float*)','capturedSource':code,'parameters':[{'declaration':'const int64_t nelements','value':'100'}]}
        self.assertEqual(c.useful_work(state)['flops'],200)
        state['capturedSource']=code.replace('scale*x[i]+bias','scale*x[i]*bias')
        self.assertIsNone(c.useful_work(state))

    def test_no_wrong_family_peak_for_partial_compute_coverage(self):
        cal={'fingerprint':'a','device':{'arch':'gfx942'},'peaks':{'fp32':{'rate':20},'mfma_fp16':{'rate':100}}}
        with patch.object(c,'profile',return_value={'MfmaFlopsF16':1e6}):
            metrics,details=c.characterize({'kernel':'k','captured':'cap'},{'replay':'r'},self.root/'run','obj',' v_mfma_f32_16x16x16f16 v0, v1\n v_fma_f32 v0, v1',{'timing':{'average':10}},cal,{'MfmaFlopsF16'})
        self.assertEqual(metrics['mfma_fp16 calibrated utilization']['value'],.1)
        self.assertIsNone(metrics['Compute throughput']['value'])

    def test_recalibration_identity_changes_percentage_basis(self):
        cal={'id':'generation-2','fingerprint':'same-device','device':{'arch':'gfx1151'},'peaks':{'device_memory':{'rate':102.4}}}
        with patch.object(c,'profile',return_value={'FETCH_SIZE':1,'WRITE_SIZE':1}):
            metrics,_=c.characterize({'kernel':'k','captured':'cap'},{'replay':'r'},self.root/'run','obj','',{'timing':{'average':1}},cal,{'FETCH_SIZE','WRITE_SIZE'})
        self.assertEqual(metrics['Memory speed of light']['basis']['calibration'],'generation-2')

    def test_counter_discovery_is_scoped_to_replay_device(self):
        output='GPU:0\nName:gfx1151\nCounter_Name : A\nGPU:1\nName:gfx942\nCounter_Name : B\n'
        with patch.object(c,'execute',return_value=output):
            self.assertEqual(c.counter_names({'profiler':'prof'},self.root,self.root/'log'),{'A'})

    def test_gpu_lock_prevents_parallel_measurements(self):
        root=self.root/'sessions'/'one';root.mkdir(parents=True)
        with c.gpu_lock(root):
            with self.assertRaisesRegex(ValueError,'Another tuning'):
                with c.gpu_lock(root):pass


if __name__=='__main__': unittest.main()
