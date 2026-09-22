import {test,expect} from '@playwright/test';
import {readFile} from 'node:fs/promises';
test('closing a capture cancels hidden analysis and late failures do not affect a reopened capture',async({page})=>{
 const bundle=JSON.parse(await readFile('public/demo.json','utf8'));
 await page.addInitScript(bundle=>{
  const w=window as any;w.isTauri=true;w.hidden=[];w.cancelled=[];
  w.__TAURI_INTERNALS__={invoke:async(command:string,args:any)=>{
   if(command==='plugin:dialog|open')return '/hidden';
   if(command==='open_capture')return {...bundle.capture,id:'hidden-native',name:'Hidden cancellation',architecture:'gfx1100'};
   if(command==='begin_wave_query')return args.requestId;
   if(command==='cancel_wave_query'){w.cancelled.push(args.requestId);return null;}
   if(command==='read_wave')return bundle.waves[args.waveId];
   if(command==='analyze_hidden')return new Promise((resolve,reject)=>w.hidden.push({args,resolve,reject}));
   if(command==='close_capture')return null;
   throw Error(command);
  }};
 },bundle);
 await page.goto('/');
 await page.getByRole('button',{name:'Open capture file',exact:true}).click();
 await expect.poll(()=>page.evaluate(()=>(window as any).hidden.length)).toBe(1);
 await page.getByRole('button',{name:'Close Hidden cancellation',exact:true}).click();
 await expect.poll(()=>page.evaluate(()=>{const w=window as any;return w.cancelled.includes(w.hidden[0].args.requestId);})).toBe(true);
 await page.getByRole('button',{name:'Open capture file',exact:true}).click();
 await expect.poll(()=>page.evaluate(()=>(window as any).hidden.length)).toBe(2);
 await page.evaluate(()=>(window as any).hidden[0].reject('obsolete hidden failure'));
 await expect(page.getByText('obsolete hidden failure',{exact:false})).toHaveCount(0);
 expect(await page.evaluate(()=>{const w=window as any;return w.cancelled.includes(w.hidden[1].args.requestId);})).toBe(false);
 await page.evaluate(()=>(window as any).hidden[1].resolve({model:'fixture',byInstruction:{},byWave:{},warnings:[]}));
 await expect(page.getByRole('button',{name:'Recompute hidden latency',exact:true})).toBeEnabled();
});
