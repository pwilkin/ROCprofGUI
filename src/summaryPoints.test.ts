import {expect,it} from 'vitest';
import {PackedSummaryPoints} from './summaryPoints';
import {hardwareSummary} from './summary';
it('retains packed chunks without copying and supports repeated ordered traversal',()=>{
 const packed=new PackedSummaryPoints(), first=new Float64Array([0,2,32,4]), second=new Float64Array([64,6]);
 packed.append(first.buffer);packed.append(second.buffer);packed.append(new ArrayBuffer(0));
 expect(packed.length).toBe(3);first[1]=7;
 expect([...packed]).toEqual([[0,7],[32,4],[64,6]]);expect([...packed]).toEqual([...packed]);
 expect(()=>packed.append(new ArrayBuffer(8))).toThrow('Invalid');
 expect(()=>packed.append(new ArrayBuffer(65552))).toThrow('Invalid');
});
it('packed and tuple summaries agree across chunk boundaries, gaps, ties and invalid values',()=>{
 for(const bad of [false,true]){
  const input=Array.from({length:16},(_,cu)=>({name:'SQ_BUSY_CU_CYCLES',scope:`SE ${Math.floor(cu/8)} · CU ${cu%8} · bank 0`,unit:'raw count',points:Array.from({length:4101},(_,i)=>[i*32+(i>4095?128:0),bad&&cu===0&&i===4096?NaN:(i+cu)%35] as [number,number])}));
  const packed=input.map(s=>{
   const points=new PackedSummaryPoints();
   for(let start=0;start<s.points.length;start+=4096)points.append(new Float64Array(s.points.slice(start,start+4096).flat()).buffer);
   return {...s,points};
  });
  expect(hardwareSummary('gfx942',packed)).toEqual(hardwareSummary('gfx942',input));
 }
});
