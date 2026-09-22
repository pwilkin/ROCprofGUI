import { test, expect } from "@playwright/test";

test("desktop cancellation releases staging while a chunk is pending and never commits", async ({page}) => {
  await page.addInitScript(() => {
    const w=window as any;
    w.isTauri=true;w.exportCalls=[];w.pendingChunks=[];
    w.__TAURI_INTERNALS__={invoke:async(command:string,args:any)=>{
      w.exportCalls.push({command,args});
      if(command==='begin_timeline_export')return true;
      if(command==='append_timeline_export')return new Promise(resolve=>w.pendingChunks.push(resolve));
      if(command==='abort_timeline_export')return null;
      if(command==='finish_timeline_export')throw Error('Must not commit cancelled output');
      throw Error(command);
    }};
  });
  await page.goto('/');
  await page.getByRole('button',{name:'CU waves',exact:true}).click();
  await page.getByText('Export timeline pages',{exact:true}).click();
  await page.getByRole('button',{name:'Export printable HTML',exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>(window as any).pendingChunks.length)).toBe(1);
  await page.getByRole('button',{name:'Cancel timeline export',exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>(window as any).exportCalls.filter((c:any)=>c.command==='abort_timeline_export').length)).toBe(1);
  await page.evaluate(()=>(window as any).pendingChunks[0](null));
  await expect(page.locator('.timeline-export [role=status]')).toHaveText('Export cancelled');
  await expect(page.getByRole('button',{name:'Export printable HTML',exact:true})).toBeEnabled();
  const calls=await page.evaluate(()=>(window as any).exportCalls);
  expect(calls.some((c:any)=>c.command==='finish_timeline_export')).toBe(false);
  expect(new Set(calls.filter((c:any)=>c.command.endsWith('_timeline_export')).map((c:any)=>c.args.id)).size).toBe(1);
});

for (const scenario of ['lost reply', 'late cancellation']) {
  test(`desktop export confirms publication after ${scenario}`, async ({page}) => {
    await page.addInitScript(scenario => {
      const w=window as any;w.isTauri=true;w.finishes=[];
      w.__TAURI_INTERNALS__={invoke:async(command:string,args:any)=>{
        if(command==='begin_timeline_export')return true;
        if(command==='append_timeline_export'||command==='abort_timeline_export')return null;
        if(command==='finish_timeline_export'){
          w.finishes.push(args);
          if(scenario==='lost reply' && w.finishes.length===1)throw Error('Lost commit reply');
          if(scenario==='late cancellation')return new Promise(resolve=>{w.confirmPublication=resolve;});
          return 1000;
        }
        throw Error(command);
      }};
    },scenario);
    await page.goto('/');
    await page.getByText('Export timeline pages',{exact:true}).click();
    await page.getByRole('button',{name:'Export printable HTML',exact:true}).click();
    if(scenario==='late cancellation'){
      await expect.poll(()=>page.evaluate(()=>typeof (window as any).confirmPublication)).toBe('function');
      await page.getByRole('button',{name:'Cancel timeline export',exact:true}).click();
      await page.evaluate(()=>(window as any).confirmPublication(1000));
    }
    await expect(page.locator('.timeline-export [role=status]')).toHaveText('Timeline pages exported');
    const commits=await page.evaluate(()=>(window as any).finishes);
    expect(commits).toHaveLength(scenario==='lost reply'?2:1);
    if(commits.length===2)expect(commits[0]).toEqual(commits[1]);
  });
}

test("cancelling an export aborts its pending wave query without cancelling the visible view", async ({page}) => {
  const {readFile} = await import("node:fs/promises");
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript(bundle => {
    const w = window as any;
    w.isTauri = true; w.calls = []; w.reads = []; w.exporting = false;
    w.__TAURI_INTERNALS__ = {invoke: async (command: string, args: any) => {
      w.calls.push({command, args});
      if(command === "plugin:dialog|open") return "/export-wave";
      if(command === "open_capture") return {...bundle.capture, id:"export-wave"};
      if(command === "read_wave") return bundle.waves[args.waveId];
      if(command === "begin_wave_query") return args.requestId;
      if(command === "cancel_wave_query") return null;
      if(command === "read_wave_window") {
        w.reads.push(args.requestId);
        if(w.exporting) return new Promise(resolve => {w.finishRead = () => resolve(Object.values(bundle.waves));});
        return Object.values(bundle.waves);
      }
      if(command === "begin_timeline_export") {w.exporting = true; return true;}
      if(command === "abort_timeline_export") return null;
      throw Error(command);
    }};
  },bundle);
  await page.goto("/");
  await page.getByRole("button", {name:"Open capture file",exact:true}).click();
  await expect(page.locator(".capture-tab")).toHaveCount(2);
  await page.getByRole("button", {name:"CU waves",exact:true}).click();
  await expect.poll(() => page.evaluate(() => (window as any).reads.length)).toBe(1);
  await expect(page.getByText("Loading detailed waves for the selected range…",{exact:true})).toHaveCount(0);
  await page.getByText("Export timeline pages",{exact:true}).click();
  await page.getByRole("button",{name:"Export printable HTML",exact:true}).click();
  await expect.poll(() => page.evaluate(() => (window as any).reads.length)).toBe(2);
  await page.getByRole("button",{name:"Cancel timeline export",exact:true}).click();
  await expect.poll(() => page.evaluate(() => {
    const w=window as any;
    return w.calls.some((c:any)=>c.command === "cancel_wave_query" && c.args.requestId === w.reads[1]) && w.calls.some((c:any)=>c.command === "abort_timeline_export");
  })).toBe(true);
  await page.evaluate(() => (window as any).finishRead());
  await expect(page.locator(".timeline-export [role=status]")).toHaveText("Export cancelled");
  await expect(page.getByRole("button",{name:"Export printable HTML",exact:true})).toBeEnabled();
  expect(await page.evaluate(() => (window as any).calls.some((c:any)=>["append_timeline_export","finish_timeline_export"].includes(c.command)))).toBe(false);
});

test("desktop export subdivides wave-count failures into exact pages", async ({page}) => {
  const {readFile} = await import("node:fs/promises");
  const bundle = JSON.parse(await readFile("public/demo.json", "utf8"));
  await page.addInitScript(bundle => {
    const w=window as any; w.isTauri=true; w.exporting=false; w.ranges=[]; w.chunks=[];
    w.__TAURI_INTERNALS__={invoke:async(command:string,args:any)=>{
      if(command==='plugin:dialog|open')return '/export-budget';
      if(command==='open_capture')return {...bundle.capture,id:'export-budget'};
      if(command==='read_wave')return bundle.waves[args.waveId];
      if(command==='begin_wave_query')return args.requestId;
      if(command==='cancel_wave_query')return null;
      if(command==='read_wave_window'){
        if(w.exporting){
          w.ranges.push([args.start,args.end]);
          if(args.end-args.start>(bundle.capture.end-bundle.capture.start)/2)
            throw Error('This window contains more than 128 waves. Narrow the range; no waves were silently dropped.');
        }
        return Object.values(bundle.waves);
      }
      if(command==='begin_timeline_export'){w.exporting=true;return true;}
      if(command==='append_timeline_export'){w.chunks.push(args.text);return null;}
      if(command==='finish_timeline_export'){w.committed=args.chunks;return 1000;}
      if(command==='abort_timeline_export')return null;
      throw Error(command);
    }};
  },bundle);
  await page.goto('/');
  await page.getByRole('button',{name:'Open capture file',exact:true}).click();
  await expect(page.locator('.capture-tab')).toHaveCount(2);
  await page.getByRole('button',{name:'CU waves',exact:true}).click();
  await expect(page.getByText('Loading detailed waves for the selected range…',{exact:true})).toHaveCount(0);
  await page.getByText('Export timeline pages',{exact:true}).click();
  await page.getByRole('button',{name:'Export printable HTML',exact:true}).click();
  await expect(page.locator('.timeline-export [role=status]')).toHaveText('Timeline pages exported');
  const result=await page.evaluate(()=>{const w=window as any;return {ranges:w.ranges,html:w.chunks.join(''),committed:w.committed,chunks:w.chunks.length};});
  const {start,end}=bundle.capture,mid=start+(end-start)/2;
  expect(result.ranges).toEqual([[start,end],[start,mid],[mid,end]]);
  expect(result.html.match(/<section /g)).toHaveLength(2);
  expect(result.html).toMatch(/<\/body><\/html>$/);
  expect(result.committed).toBe(result.chunks);
});
