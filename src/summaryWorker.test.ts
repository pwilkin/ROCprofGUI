import {afterEach, expect, it, vi} from 'vitest';
import {requestHardwareSummary} from './summaryWorker';
import type {Series} from './types';
const sample = (count:number):Series => ({name:'BUSY_CU_CYCLES',scope:'SE 0 · CU 0 · bank 0',unit:'raw count',points:Array.from({length:count},(_,i)=>[i*32,1])});
afterEach(()=>vi.unstubAllGlobals());
it('sends bounded transferable chunks and terminates after completion',async()=>{
 const calls:any[]=[];let worker:any;
 vi.stubGlobal('Worker',class{onmessage:any;onerror:any;onmessageerror:any;terminate=vi.fn();constructor(){worker=this;}postMessage(message:any,transfer:any){calls.push({message,transfer});}});
 const pending=requestHardwareSummary('gfx942',[sample(4100)],new AbortController().signal);
 await vi.waitFor(()=>expect(calls.at(-1).message.kind).toBe('finish'));
 const chunks=calls.filter(c=>c.message.kind==='points');
 expect(chunks.map(c=>c.message.buffer.byteLength)).toEqual([65536,64]);
 for(const c of chunks)expect(c.transfer).toEqual([c.message.buffer]);
 const result={columns:[],rows:[]};worker.onmessage({data:{result}});
 expect(await pending).toEqual(result);expect(worker.terminate).toHaveBeenCalledTimes(1);
});
it('aborting transfer stops later chunks, terminates the worker and rejects stale completion',async()=>{
 const controller=new AbortController();const calls:any[]=[];let worker:any;
 vi.stubGlobal('Worker',class{onmessage:any;onerror:any;onmessageerror:any;terminate=vi.fn();constructor(){worker=this;}postMessage(message:any){calls.push(message);if(message.kind==='points')controller.abort();}});
 await expect(requestHardwareSummary('gfx942',[sample(10000)],controller.signal)).rejects.toMatchObject({name:'AbortError'});
 await new Promise(r=>setTimeout(r,10));
 expect(calls.filter(c=>c.kind==='points')).toHaveLength(1);
 expect(calls.some(c=>c.kind==='finish')).toBe(false);
 worker.onmessage({data:{result:{}}});expect(worker.terminate).toHaveBeenCalledTimes(1);
});
it('worker failures reject and release the worker',async()=>{
 let worker:any;vi.stubGlobal('Worker',class{onmessage:any;onerror:any;onmessageerror:any;terminate=vi.fn();constructor(){worker=this;}postMessage(){}});
 const pending=requestHardwareSummary('gfx942',[sample(1)],new AbortController().signal);
 worker.onerror({message:'worker unavailable'});
 await expect(pending).rejects.toThrow('worker unavailable');expect(worker.terminate).toHaveBeenCalledTimes(1);
});

it('rejects oversized summary inputs before creating or transferring to a worker',async()=>{
 const worker=vi.fn();vi.stubGlobal('Worker',worker);
 const result=await requestHardwareSummary('gfx942',[sample(2000001)],new AbortController().signal);
 expect(result.unavailable).toContain('2,000,000-sample');expect(worker).not.toHaveBeenCalled();
});
