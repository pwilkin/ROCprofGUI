import { expect, it } from "vitest";
import { streamTimeline } from "./timelineStream";
const options = () => ({ label:"Trace", unit:"cycles", bounds:[0,100] as [number,number], timePages:1, lanesPerPage:1, stacked:false, load: async () => [], signal:new AbortController().signal });
it("sends ordered chunks and commits only after the final HTML footer", async () => {
  const commands: string[] = [], chunks: string[] = [];
  const result = await streamTimeline(options(), async <T>(command: string, args: Record<string,unknown>) => {
    commands.push(command);
    if(command === "append_timeline_export") {expect(args.sequence).toBe(chunks.length);chunks.push(String(args.text));}
    if(command === "finish_timeline_export") {expect(args.chunks).toBe(chunks.length);expect(chunks.join("")).toMatch(/<\/body><\/html>$/);}
    return true as T;
  });
  expect(result).toBe(true);
  expect(commands[0]).toBe("begin_timeline_export");
  expect(commands).toContain("finish_timeline_export");
  expect(commands.at(-1)).toBe("finish_timeline_export");
});
it("aborts staging on renderer failure and does not commit", async () => {
  const commands: string[] = [];
  await expect(streamTimeline({...options(), load: async () => {throw Error("failed query");}}, async <T>(command: string) => {commands.push(command);return true as T;})).rejects.toThrow("failed query");
  expect(commands).toEqual(["begin_timeline_export","abort_timeline_export"]);
});
it("cancellation during the picker cleans the caller-owned session without rendering", async () => {
  const controller = new AbortController();
  const commands: string[] = [];
  expect(await streamTimeline({...options(), signal:controller.signal, load:async()=>{throw Error("must not load");}}, async <T>(command:string) => {commands.push(command);controller.abort();return true as T;})).toBe(false);
  expect(commands).toEqual(["begin_timeline_export","abort_timeline_export","abort_timeline_export"]);
});

it("releases native staging while an exact query is still pending", async () => {
  const controller = new AbortController();
  const commands: string[] = [];
  let release!: (value: []) => void;
  let loaded!: () => void;
  const started = new Promise<void>(resolve => {loaded = resolve;});
  const pending = streamTimeline({...options(), signal:controller.signal, load:async()=> {
    loaded();
    return new Promise<[]>(resolve=>{release=resolve;});
  }},async <T>(command:string)=>{commands.push(command);return true as T;});
  await started;
  controller.abort();
  expect(commands).toEqual(["begin_timeline_export","abort_timeline_export"]);
  release([]);
  await expect(pending).rejects.toThrow("cancelled");
  expect(commands).not.toContain("finish_timeline_export");
  expect(commands.at(-1)).toBe("abort_timeline_export");
});

it("retries a lost commit reply with the same identity and does not abort confirmed output", async () => {
  const calls: {command:string;args:Record<string,unknown>}[]=[];
  let finishes=0;
  expect(await streamTimeline(options(),async <T>(command:string,args:Record<string,unknown>)=>{
    calls.push({command,args});
    if(command==='finish_timeline_export' && ++finishes===1)throw Error('reply lost');
    return true as T;
  })).toBe(true);
  const commits=calls.filter(c=>c.command==='finish_timeline_export');
  expect(commits).toHaveLength(2);
  expect(commits[0].args).toEqual(commits[1].args);
  expect(calls.some(c=>c.command==='abort_timeline_export')).toBe(false);
});
it("reports uncertainty when neither commit response arrives", async () => {
  await expect(streamTimeline(options(),async <T>(command:string)=>{
    if(command==='finish_timeline_export')throw Error('transport unavailable');
    return true as T;
  })).rejects.toThrow('completed file may already exist');
});
it("keeps a confirmed publication successful when cancellation arrives during commit", async () => {
  const controller=new AbortController();
  expect(await streamTimeline({...options(),signal:controller.signal},async <T>(command:string)=>{
    if(command==='finish_timeline_export')controller.abort();
    return true as T;
  })).toBe(true);
});

it("preserves commit uncertainty when cleanup also loses its reply", async () => {
  await expect(streamTimeline(options(),async <T>(command:string)=>{
    if(['finish_timeline_export','abort_timeline_export'].includes(command))throw Error('transport unavailable');
    return true as T;
  })).rejects.toThrow('completed file may already exist');
});
