import json
from pathlib import Path
root=Path('fixtures/transpose-ui');root.mkdir(parents=True,exist_ok=True)
def write(name,obj): (root/name).write_text(json.dumps(obj))
code=[['; transpose_tiled<float, 32>',0,0,'transpose.hip:7',1,4096,0,0,0,0]]
isa=[('s_load_dwordx4 s[8:11], s[0:1], 0x00',1,9),('v_mad_u32_u24 v2, v1, 32, v0',6,10),('global_load_dword v4, v[2:3], off',3,11),('s_waitcnt vmcnt(0)',9,11),('ds_write_b32 v0, v4',5,12),('s_barrier',9,13),('ds_read_b32 v5, v1',5,14),('s_waitcnt lgkmcnt(0)',9,14),('v_add_u32 v6, vcc, s8, v2',6,15),('global_store_dword v[6:7], v5, off',3,15),('s_endpgm',2,16)]
for i,(ins,cat,line) in enumerate(isa,1):code.append([ins,cat,i,f'transpose.hip:{line}',1,4096+i*4,0,0,0,0])
names={'gfxip':11,'gfxv':'gfx1151','demo':True,'thread_trace':True,'pc_sampling':False,'counter_names':['SQ_INSTS_VALU','SQ_INSTS_LDS','SQ_INSTS_VMEM','SQ_WAVES'],'wave_filenames':{}}
occ={'version':'3.1.0','dispatches':{'0':'transpose_tiled<float, 32>','1':'reduce_partials<float>'},'events':{}}
for se in range(2):
 events=[]
 for cu in range(8):
  for simd in range(4):
   for slot in range(3):
    start=500+cu*65+simd*180+slot*850+se*70;end=start+3400+((cu+slot)%3)*320
    events.extend([[start,cu,simd,slot,1,0,0,0,0,cu*12+slot,0],[end,cu,simd,slot,0,0,0,0,0,cu*12+slot,0]])
    if cu!=2:continue
    name=f'se{se}_simd{simd}_slot{slot}_wave0.json'
    names['wave_filenames'].setdefault(str(se),{}).setdefault(str(simd),{}).setdefault(str(slot),{})['0']=[name,start,end]
    tokens=[];clock=start
    for iteration in range(3):
     for i,(ins,cat,line) in enumerate(isa,1):
      duration=125 if cat==5 else 90 if cat==3 else 100 if cat==9 else 18
      stall=duration-12 if cat==9 else 10 if cat==5 else 0
      tokens.append([clock,cat,stall,duration,i]);clock+=duration+5
      code[i][6]+=1;code[i][7]+=duration;code[i][8]+=stall
    write(name,{'wave':{'id':0,'cu':cu,'begin':start,'end':end,'instructions':tokens,'timeline':[[2,1000],[3,900],[4,end-start-1900]],'waitcnt':[[4,[[3,0]]],[8,[[7,0]]]],'info':{'valu':54,'lds':750,'vmem':540}}})
 occ[str(se)]=sorted(events)
 occ['events'][str(se)]=[{'kind':'dispatch','time':400,'kernel_id':0,'kernel_name':'transpose_tiled<float, 32>','me_id':0,'pipe_id':0,'vgprs':32,'sgprs':48,'lds_size':4224,'thread_dim_x':32,'thread_dim_y':8,'thread_dim_z':1}]
 write(f'se{se}_perfcounter.json',{'data':[[t,80+(t//200%7)*18,12+(t//300%5)*6,25+(t//500%4)*8,4+(t//400%4),2,0] for t in range(500,6500,120)]})
write('filenames.json',names);write('occupancy.json',occ)
write('code.json',{'sqtt_funcmap':[[1,1,'F','transpose_tiled<float, 32>','transpose.hip:7'],[1,2,'U','Load tile','transpose.hip:11'],[1,3,'U','Transpose tile','transpose.hip:14'],[1,4,'P','Barrier','transpose.hip:13']], 'version':'3.0.0','header':['ISA','_','LineNumber','Source','Codeobj','Vaddr','Hit','Latency','Stall','Idle'],'code':code})
write('snapshots.json',{'transpose.hip':'source_transpose.hip'})
(root/'source_transpose.hip').write_text('''// Synthetic fixture. Values are illustrative, not benchmark evidence.
#include <hip/hip_runtime.h>

template <typename T, int TILE>
__global__ void transpose_tiled(const T* input, T* output,
                                const int width) {
    __shared__ T tile[TILE][TILE + 1];
    const int x = blockIdx.x * TILE + threadIdx.x;
    const int y = blockIdx.y * TILE + threadIdx.y;
    const int offset = y * width + x;
    T value = input[offset];
    tile[threadIdx.y][threadIdx.x] = value;
    __syncthreads();
    T transposed = tile[threadIdx.x][threadIdx.y];
    output[x * width + y] = transposed;
}
''')
names['shaderdata_filenames']={'0':[['shaderdata_demo.json',650,2800]]}
write('filenames.json',names)
write('shaderdata_demo.json',{'records_schema':['time','value','cu','simd','wave_id','flags'],'records':[[650,6,2,0,0,0],[700,10,2,0,0,0],[1250,1,2,0,0,0],[1300,16,2,0,0,0],[1400,14,2,0,0,0],[2500,1,2,0,0,0],[2800,1,2,0,0,0]]})
