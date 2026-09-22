import {execFileSync} from 'node:child_process';
import {mkdtempSync,readFileSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
import ts from 'typescript';
const temp=mkdtempSync(join(tmpdir(),'rocprof-tensor-'));
try {
 execFileSync('c++',['-std=c++17','-O2','-Ithird_party/rcv','tests/oracles/tensor-main.cpp','third_party/rcv/derived_counter.cpp','-o',join(temp,'oracle')],{stdio:'inherit'});
 writeFileSync(join(temp,'tensor.mjs'),ts.transpileModule(readFileSync('src/tensor.ts','utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText);
 const {Tensor,tensorExpression}=await import(pathToFileURL(join(temp,'tensor.mjs')).href);
 const expressions=['A + 2','A * 3 - A / 2','sum[A]','mean[A]','min[A]','max[A]','sum[A,axis=[XCC,CU]]','mean[A,axis=SE]','max[A,axis=Time]','min[A,axis=CU]','select[A,-1,axis=SE]','select[A,0:3:2,axis=Time]','remove[A,-1,axis=Time]','delta[A,axis=Time]','delta[A,axis=CU]','A / max(mean[A,axis=Time],1)','(A > 4) & (A <= 13)','(A == 3) | (A < 0)','linear(3,axis=Time) + A'];
 let seed=192026,checks=0;
 const random=()=>{seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed/2**32};
 for(let trial=0;trial<30;trial++) {
  const shape=[1+Math.floor(random()*2),2,2,3];
  const data=Array.from({length:shape.reduce((a,b)=>a*b)},()=>Math.floor(random()*31)-10);
  const valid=data.map(()=>trial%2===0||random()>.25);
  const input=shape.join(' ')+'\n'+data.map((v,n)=>`${v} ${+valid[n]}`).join('\n')+'\n'+expressions.length+'\n'+expressions.join('\n')+'\n';
  const rows=execFileSync(join(temp,'oracle'),{input,encoding:'utf8',timeout:10000}).trim().split('\n');
  const raw=new Tensor(shape,data.map((v,n)=>valid[n]?v:NaN));
  expressions.forEach((expression,n)=>{
   const parts=rows[n].trim().split(/\s+/),expectedShape=parts.slice(0,4).map(Number),expected=parts.slice(4).map(v=>v==='null'?NaN:Number(v));
   const actual=tensorExpression(expression,name=>{if(name==='A')return raw;throw Error(name)});
   if(JSON.stringify(actual.shape)!==JSON.stringify(expectedShape))throw Error(`Shape mismatch ${expression}`);
   expected.forEach((value,i)=>{const observed=actual.values[i];if(Number.isFinite(value)!==Number.isFinite(observed)||(Number.isFinite(value)&&Math.abs(value-observed)>1e-5*Math.max(1,Math.abs(value))))throw Error(`Trial ${trial}: ${expression} at ${i}: ${observed} != ${value}`)});
   checks++;
  });
 }
 console.log(`${checks} tensor expressions matched unchanged upstream C++ across 30 captures, including validity gaps.`);
} finally {rmSync(temp,{recursive:true,force:true})}
