import {test,expect} from '@playwright/test';
test('real Activity worker preserves a two million samples while UI frames continue',async({page})=>{
 await page.goto('/');
 const result=await page.evaluate(async()=>{
  const {requestHardwareSummary}=await import('/src/summaryWorker.ts');
  const series=Array.from({length:16},(_,cu)=>({name:'SQ_BUSY_CU_CYCLES',scope:`SE 0 · CU ${cu} · bank 0`,unit:'raw count',points:Array.from({length:125000},(_,i)=>[i*32,cu+1])}));
  let frames=0,done=false;const tick=()=>{if(!done){frames++;requestAnimationFrame(tick);}};requestAnimationFrame(tick);
  const result=await requestHardwareSummary('gfx942',series,new AbortController().signal);done=true;
  return {frames,peak:result.peakCounts,totals:result.rows.map(r=>r.accumulated[0])};
 });
 expect(result.frames).toBeGreaterThan(2);expect(result.peak).toEqual([136]);
 expect(result.totals).toEqual(Array.from({length:16},(_,cu)=>125000*(cu+1)));
});

test('million-instruction activity calculation yields and cancels without changing totals', async ({page}) => {
 await page.goto('/');
 const result = await page.evaluate(async () => {
  const {requestActivityCost} = await import('/src/summary.ts');
  const instructions = Array.from({length: 1000000}, () => ({idle: 1, stall: 2, latency: 5}));
  let frames = 0, done = false;
  const tick = () => { if (!done) { frames++; requestAnimationFrame(tick); } };
  requestAnimationFrame(tick);
  const result = await requestActivityCost(instructions, new AbortController().signal);
  done = true;
  const controller = new AbortController();
  const cancelled = requestActivityCost(instructions, controller.signal);
  setTimeout(() => controller.abort(), 0);
  let cancellation = '';
  try { await cancelled; } catch (error) { cancellation = error.name; }
  return {result, frames, cancellation};
 });
 expect(result.result).toEqual({idle:1000000,stall:2000000,issue:3000000,total:6000000});
 expect(result.frames).toBeGreaterThan(2);
 expect(result.cancellation).toBe('AbortError');
});
