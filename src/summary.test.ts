import { expect, it } from "vitest";
import { activityCost, hardwareSummary } from "./summary";
import type { Instruction, Series } from "./types";
const sample = (
  name: string,
  points: [number, number][],
  cu = 0,
  bank = 0,
): Series => ({
  name: `SQ_${name}`,
  points,
  unit: "raw count",
  scope: `SE 0 · CU ${cu} · bank ${bank}`,
});
const fixture = () => [
  sample("BUSY_CU_CYCLES", [
    [0, 10],
    [64, 30],
  ]),
  sample("ACTIVE_INST_VALU", [
    [0, 20],
    [64, 60],
  ]),
  sample("ACTIVE_INST_LDS", [
    [0, 5],
    [64, 15],
  ]),
  sample(
    "VALU_MFMA_BUSY_CYCLES",
    [
      [0, 20],
      [64, 60],
    ],
    0,
    1,
  ),
];
it("uses quadcycle scaling, preserves 200% VALU and original samples", () => {
  const input = fixture(),
    original = JSON.stringify(input);
  const summary = hardwareSummary("gfx942", input);
  expect(summary.columns.map((c) => [c.name, c.average, c.peak])).toEqual([
    ["LDS", 100, 100],
    ["VALU", 200, 200],
    ["MFMA", 50, 50],
  ]);
  expect(summary.rows[0].accumulated).toEqual([40, 80, 40, 80]);
  expect(JSON.stringify(input)).toBe(original);
});
it("computes aggregate peaks over sampled CU state, with gap reset", () => {
  const result = hardwareSummary("gfx90a", [
    sample("BUSY_CU_CYCLES", [
      [0, 10],
      [32, 10],
      [128, 5],
    ]),
    sample(
      "BUSY_CU_CYCLES",
      [
        [0, 0],
        [96, 20],
        [128, 20],
      ],
      1,
    ),
  ]);
  expect(result.delta).toBe(32);
  expect(result.peakCounts).toEqual([25]);
});
it("does not invent missing measurements or divide by zero", () => {
  const zero = hardwareSummary("gfx941", [
    sample("BUSY_CU_CYCLES", [[0, 0]]),
    sample("ACTIVE_INST_VALU", [[0, 10]]),
  ]);
  expect(zero.columns[0].average).toBeNull();
  expect(zero.rows[0].utilization[0]).toBeNull();
  const missing = hardwareSummary("gfx942", fixture().slice(1));
  expect(
    missing.columns.every((c) => c.average === null && c.peak === null),
  ).toBe(true);
  const mismatch = hardwareSummary("gfx942", [
    ...fixture(),
    sample("ACTIVE_INST_VALU", [[0, 100]], 2),
  ]);
  expect(mismatch.columns.find((c) => c.name === "VALU")?.reason).toContain(
    "different CUs",
  );
  expect(mismatch.rows[1].utilization.every((n) => n === null)).toBe(true);
});
it("rejects ambiguous, malformed and unsupported inputs without fabricated zeros", () => {
  for (const bad of [NaN, Infinity, -1]) {
    const s = hardwareSummary("gfx942", [
      sample("BUSY_CU_CYCLES", [[0, 10]]),
      sample("ACTIVE_INST_VALU", [[0, bad]]),
    ]);
    expect(s.columns[0].average).toBeNull();
    expect(s.peakCounts[0]).toBeNull();
  }
  const duplicated = hardwareSummary("gfx942", [...fixture(), fixture()[1]]);
  expect(duplicated.columns.find((c) => c.name === "VALU")?.reason).toContain(
    "Ambiguous",
  );
  expect(hardwareSummary("gfx1151", fixture()).unavailable).toContain(
    "no validated",
  );
  expect(hardwareSummary("gfx942", []).unavailable).toContain("not collected");
  expect(hardwareSummary("gfx950", fixture()).unavailable).toContain(
    "no validated",
  );
});
it("reports instruction cost shares and rejects inconsistent latency", () => {
  const instruction = { idle: 10, stall: 30, latency: 90 } as Instruction;
  expect(activityCost([instruction])).toEqual({
    idle: 10,
    stall: 30,
    issue: 60,
    total: 100,
  });
  expect(activityCost([{ ...instruction, latency: 10 }])).toBeNull();
  expect(activityCost([])).toBeNull();
  expect(activityCost([{ ...instruction, idle: NaN }])).toBeNull();
});

it("matches the full event-sort oracle across CU ties, gaps and irregular cadence", () => {
  let seed = 72915;
  const random = () => ((seed = Math.imul(seed, 1664525) + 1013904223 | 0) >>> 0);
  for (let trial = 0; trial < 200; trial++) {
    const series = Array.from({length: 1 + random()%16}, (_, cu) => {
      let time = random()%5;
      const points: [number,number][] = Array.from({length: 1+random()%80}, () => {
        time += [1,4,32,64,128][random()%5];
        return [time,random()%300];
      });
      const stream = sample("BUSY_CU_CYCLES",points,cu%8);
      stream.scope = `SE ${Math.floor(cu/8)} · CU ${cu%8} · bank 0`;
      return stream;
    }).reverse();
    const result = hardwareSummary("gfx942",series);
    const events: {time:number;cu:number;value:number}[] = [];
    for (const s of series) {
      const cu = Number(s.scope.match(/SE (\d+)/)![1])*16 + Number(s.scope.match(/CU (\d+)/)![1]);
      let next = s.points[0][0];
      for (const [time,value] of s.points) {
        if (time > next) events.push({time:next,cu,value:0});
        events.push({time,cu,value});
        next=time+result.delta;
      }
    }
    events.sort((a,b)=>a.time-b.time||a.cu-b.cu);
    const last=new Map<number,number>();let sum=0,time=0,peak=0;
    for(const e of events){
      if(time+4<e.time)peak=Math.max(peak,sum);
      sum+=e.value-(last.get(e.cu)??0);last.set(e.cu,e.value);time=e.time;
    }
    expect(result.peakCounts).toEqual([Math.max(peak,sum)]);
  }
});

it("retains exact totals at the two-million sample boundary", () => {
  const series=Array.from({length:16},(_,cu)=>sample("BUSY_CU_CYCLES",Array.from({length:125000},(_,i)=>[i*32,cu+1]),cu));
  const result=hardwareSummary("gfx942",series);
  expect(result.unavailable).toBeUndefined();
  expect(result.peakCounts).toEqual([136]);
  expect(result.rows.map(r=>r.accumulated[0])).toEqual(Array.from({length:16},(_,cu)=>125000*(cu+1)));
  series[0].points.push([125000*32,1]);
  expect(hardwareSummary("gfx942",series).unavailable).toContain("2,000,000-sample");
});
