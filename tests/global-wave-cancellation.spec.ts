import {test,expect} from '@playwright/test';
import {readFile} from 'node:fs/promises';
test('Global candidate searches cancel replaced work and ignore stale failures',async({page})=>{
 const bundle=JSON.parse(await readFile('public/demo.json','utf8'));
 await page.addInitScript(bundle=>{
  const w=window as any;w.isTauri=true;w.pending=[];w.cancelled=[];w.hold=false;
  const first={...bundle.capture.waves[0],se:0,simd:0,slot:0,start:100,end:900};
  const wave={...bundle.waves[first.id],cu:2,start:100,end:900,tokens:[{start:200,duration:200,stall:0,category:6,instruction:1}]};
  w.__TAURI_INTERNALS__={invoke:async(command:string,args:any)=>{
   if(command==='plugin:dialog|open')return '/global';
   if(command==='open_capture')return {...bundle.capture,id:'global-native',name:'Global cancellation',start:0,end:1000,waves:[first],intervals:[{start:100,end:900,se:0,cu:2,simd:0,slot:0,kernel:'navigation'}]};
   if(command==='begin_wave_query')return args.requestId;
   if(command==='cancel_wave_query'){w.cancelled.push(args.requestId);return null;}
   if(command==='read_wave'){
    if(w.hold)return new Promise((resolve,reject)=>w.pending.push({args,resolve:()=>resolve(wave),reject}));
    return wave;
   }
   throw Error(command);
  }};
 },bundle);
 await page.goto('/');
 await page.getByRole('button',{name:'Open capture file',exact:true}).click();
 await expect(page.locator('.capture-tab')).toHaveCount(2);
 await expect(page.locator('canvas')).toBeVisible();
 await page.getByRole('button',{name:'Global',exact:true}).click();
 await page.evaluate(()=>(window as any).hold=true);
 const canvas=page.locator('canvas'),box=await canvas.boundingBox();if(!box)throw Error('Missing canvas');
 const position={x:155+.5*(box.width-169),y:40};
 await canvas.click({position});
 await expect.poll(()=>page.evaluate(()=>(window as any).pending.length)).toBe(1);
 await canvas.click({position});
 await expect.poll(()=>page.evaluate(()=>(window as any).pending.length)).toBe(2);
 await expect.poll(()=>page.evaluate(()=>{const w=window as any;return w.cancelled.includes(w.pending[0].args.requestId)&&!w.cancelled.includes(w.pending[1].args.requestId);})).toBe(true);
 await page.evaluate(()=>(window as any).pending[0].reject('obsolete global failure'));
 await expect(page.getByText('obsolete global failure',{exact:false})).toHaveCount(0);
 await page.getByRole('button',{name:'Target wave',exact:true}).click();
 await expect.poll(()=>page.evaluate(()=>{const w=window as any;return w.cancelled.includes(w.pending[1].args.requestId);})).toBe(true);
 await page.evaluate(()=>(window as any).pending[1].resolve());
 await expect(page.getByRole('button',{name:'Target wave',exact:true})).toHaveClass('active');
 await page.evaluate(()=>(window as any).hold=false);
 await page.getByRole('button',{name:'Global',exact:true}).click();
 await canvas.click({position});
 await expect(page.getByRole('button',{name:'Target wave',exact:true})).toHaveClass('active');
 await expect(page.getByLabel('Range start')).toHaveValue('100');
 await expect(page.getByLabel('Range end')).toHaveValue('900');
});
