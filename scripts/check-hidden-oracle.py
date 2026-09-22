"""Compare Rust with the unchanged upstream C++ hidden-latency algorithm."""
import json
import random
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
subprocess.run(['cargo', 'build', '-q', '-p', 'profile-core'], cwd=ROOT, check=True)
with tempfile.TemporaryDirectory(prefix='rocprof-hidden-oracle-') as temporary:
    temporary = Path(temporary)
    binary = temporary / 'hidden-oracle'
    subprocess.run(['c++', '-std=c++20', '-O2', '-I'+str(ROOT/'tests/oracles/include'), '-I'+str(ROOT/'third_party/rcv'), str(ROOT/'tests/oracles/hidden-main.cpp'), str(ROOT/'third_party/rcv/hidden_latency.cpp'), '-o', str(binary)], check=True)
    rng = random.Random(91519)
    for case in range(40):
        root = temporary / str(case)
        root.mkdir()
        waves = []
        names = {'gfxip':11, 'gfxv':'gfx1151', 'wave_filenames':{}, 'other_simd_filenames':{}}
        code = [[('v_wmma_f32' if line==13 else f'instruction_{line}'),0,line,None,1,4096+4*line,0,0,0,0] for line in range(14)]
        for se in range(2):
            for simd in range(2):
                for slot in range(3):
                    begin = rng.randrange(0,20)
                    clock = begin
                    tokens = []
                    for index in range(20):
                        duration = rng.randrange(1,40)
                        stall = rng.randrange(0,duration+1)
                        category = rng.randrange(14)
                        clock += rng.randrange(0,25)
                        tokens.append([clock,category,stall,duration,category])
                    end = max(t[0]+t[3] for t in tokens)
                    filename=f'{se}-{simd}-{slot}.json'
                    names['wave_filenames'].setdefault(str(se),{}).setdefault(str(simd),{}).setdefault(str(slot),{})['0']=[filename,begin,end]
                    (root/filename).write_text(json.dumps({'wave':{'begin':begin,'end':end,'instructions':tokens,'timeline':[],'waitcnt':[],'info':{}}}))
                    waves.append((se,simd,begin,tokens))
        others=[]
        for se in range(2):
            records=[[rng.randrange(0,250),rng.randrange(1,40)] for _ in range(case%7)]
            (root/f'other{se}.json').write_text(json.dumps({'instructions_schema':['time','duration'],'instructions':records}))
            names['other_simd_filenames'][str(se)]=[[f'other{se}.json',0,1000]]
            others.extend((se,*r) for r in records)
        (root/'filenames.json').write_text(json.dumps(names))
        (root/'code.json').write_text(json.dumps({'header':[],'code':code}))
        lines=[str(len(waves))]
        for se,simd,begin,tokens in waves:
            lines.append(f'{se} {simd} {begin} {len(tokens)}')
            lines.extend(f'{clock} {duration} {stall} {category} {line}' for clock,category,stall,duration,line in tokens)
        lines.append(str(len(others)))
        lines.extend(' '.join(map(str,row)) for row in others)
        result=subprocess.check_output([str(binary)],input='\n'.join(lines),text=True)
        expected={}
        for line in result.splitlines():
            index,idle,stall,issue=map(int,line.split());expected[str(index)]={'idle':idle,'stall':stall,'issue':issue}
        actual=json.loads(subprocess.check_output([str(ROOT/'target/debug/profile-core'),str(root),'--hidden'],text=True))['byInstruction']
        if actual != expected:
            raise AssertionError(f'Case {case}: Rust {actual} != upstream {expected}')
print('40 randomized multi-SE/SIMD/wave captures exactly match upstream C++ hidden-latency totals.')
