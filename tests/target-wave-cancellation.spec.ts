import {test, expect} from '@playwright/test';
import {readFile} from 'node:fs/promises';

test('target changes and capture closure cancel their own pending reads and discard stale failures', async ({page}) => {
  const bundle=JSON.parse(await readFile('public/demo.json','utf8'));
  await page.addInitScript(bundle=>{
    const w=window as any;w.isTauri=true;w.requests=[];w.cancelled=[];
    const first={...bundle.capture.waves[0],id:'target-0',wid:0};
    const second={...first,id:'target-1',wid:1};
    w.__TAURI_INTERNALS__={invoke:async(command:string,args:any)=>{
      if(command==='plugin:dialog|open')return '/target';
      if(command==='open_capture')return {...bundle.capture,id:'native-target',name:'Target cancellation',waves:[first,second]};
      if(command==='begin_wave_query')return args.requestId;
      if(command==='cancel_wave_query'){w.cancelled.push(args.requestId);return null;}
      if(command==='close_capture')return null;
      if(command==='read_wave')return new Promise((resolve,reject)=>w.requests.push({args,resolve:()=>resolve({...bundle.waves[bundle.capture.waves[0].id],id:args.waveId}),reject}));
      throw Error(command);
    }};
  },bundle);
  await page.goto('/');
  await page.getByRole('button',{name:'Open capture file',exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>(window as any).requests.length)).toBe(1);
  await page.getByLabel('Wave ID',{exact:true}).selectOption('1');
  await expect.poll(()=>page.evaluate(()=>(window as any).requests.length)).toBe(2);
  await expect.poll(()=>page.evaluate(()=>{const w=window as any;return w.cancelled.includes(w.requests[0].args.requestId)&&!w.cancelled.includes(w.requests[1].args.requestId);})).toBe(true);
  await page.evaluate(()=>(window as any).requests[0].reject('obsolete target failure'));
  await expect(page.getByText('obsolete target failure',{exact:false})).toHaveCount(0);
  await page.evaluate(()=>(window as any).requests[1].resolve());
  await expect(page.getByLabel('Wave ID',{exact:true})).toHaveValue('1');
  await expect(page.locator('canvas')).toBeVisible();
  await page.getByLabel('Wave ID',{exact:true}).selectOption('0');
  await expect.poll(()=>page.evaluate(()=>(window as any).requests.length)).toBe(3);
  await page.getByRole('button',{name:'Close Target cancellation',exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>{const w=window as any;return w.cancelled.includes(w.requests[2].args.requestId);})).toBe(true);
  await page.evaluate(()=>(window as any).requests[2].reject('closed target failure'));
  await expect(page.locator('.capture-tab')).toHaveCount(1);
  await expect(page.getByText('closed target failure',{exact:false})).toHaveCount(0);
});
