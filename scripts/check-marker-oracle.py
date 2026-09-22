import json,random,subprocess,tempfile
from pathlib import Path
subprocess.run(['cargo','build','-q','-p','profile-core'],check=True)
with tempfile.TemporaryDirectory(prefix='rocprof-marker-oracle-') as temp:
    root=Path(temp);binary=root/'oracle'
    subprocess.run(['c++','-std=c++17','-O2','-Itests/oracles/include','-Ithird_party/rcv','tests/oracles/marker-main.cpp','third_party/rcv/data/marker_walker.cpp','-o',str(binary)],check=True)
    capture=root/'capture';capture.mkdir()
    def write(name,value): (capture/name).write_text(json.dumps(value))
    write('filenames.json',{'wave_filenames':{'0':{'0':{'0':{'0':['wave.json',0,100000]}}}},'shaderdata_filenames':{'0':[['markers.json',0,100000]]}})
    write('wave.json',{'wave':{'cu':0,'begin':0,'end':100000,'instructions':[[0,6,0,1,1]]}})
    write('code.json',{'code':[['v_add',6,1,'',7,4096,0,0,0,0]],'sqtt_funcmap':[[7,i,'U','name'] for i in range(1,6)]})
    random.seed(192026)
    for trial in range(60):
        records=[[n*10+1,(random.randrange(7)<<2)|random.randrange(4)] for n in range(150)]
        write('markers.json',{'records_schema':['time','value','cu','simd','wave_id','flags'],'records':[[*r,0,0,0,0] for r in records]})
        source=str(len(records))+'\n'+'\n'.join(f'{t} {v}' for t,v in records)+'\n'
        expected=sorted(tuple(map(int,line.split())) for line in subprocess.check_output([str(binary)],input=source,text=True,timeout=10).splitlines())
        actual=json.loads(subprocess.check_output(['target/debug/profile-core',str(capture)],text=True,timeout=10))['markers']
        observed=sorted((m['markerId'],int(m['start']),-1 if m['end'] is None else int(m['end']),m['depth'],int(m['point'])) for m in actual)
        if observed!=expected:raise AssertionError(f'Marker walker mismatch in trial {trial}')
print('60 randomized marker streams matched unchanged upstream C++ (span IDs, bounds, nesting depth, points and open scopes).')
