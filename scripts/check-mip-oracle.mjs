import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { buildSync } from "esbuild";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const out = path.join(root, ".codex", "mip-validation");
fs.mkdirSync(out, { recursive: true });
execFileSync("c++", ["-std=c++20", "-O2", "-I" + path.join(root, "third_party/rcv"), path.join(root, "tests/oracles/mip-main.cpp"), "-o", path.join(out, "mip-oracle")]);
buildSync({ entryPoints: [path.join(root, "src/timelineMip.ts")], bundle: true, platform: "node", format: "cjs", outfile: path.join(out, "mip.cjs") });
const { mipTokens } = createRequire(import.meta.url)(path.join(out, "mip.cjs"));
let seed = 91520;
const random = n => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed % n; };
const cases = [];
for (let i=0; i<600; i++) {
  const base = i%2 ? 4 : 1, count = [0,1,16,17,32,128][i%6];
  let start = i%7===0 ? 4500000000000 : 0;
  const tokens = Array.from({ length: count }, (_, source) => {
    start += random(i%3 ? 20 : 1);
    return { start, duration: random(200), category: random(18), slot: random(4), count: 1, source };
  });
  cases.push({ base, tokens });
}
const input = cases.map(c => `${c.tokens.length} ${c.base}\n` + c.tokens.map(t => `${t.start} ${t.duration} ${t.category} ${t.slot}`).join("\n")).join("\n") + "\n";
const result = execFileSync(path.join(out, "mip-oracle"), { input, encoding: "utf8", maxBuffer: 64*1024*1024 }).trim().split("\n");
let cursor=0;
for (const c of cases) for(let level=2; level<=10; level++) {
  const expected = result[cursor++].split(" ").map(Number), n = expected.shift();
  const actual = mipTokens(c.tokens, c.base, level).flatMap(t => [t.start,t.duration,t.category,t.slot]);
  if(n*4 !== actual.length || JSON.stringify(actual)!==JSON.stringify(expected)) {
    fs.writeFileSync(path.join(out, "failure.json"), JSON.stringify({c,level,expected,actual},null,2));
    throw Error(`Mip mismatch at case ${Math.floor((cursor-1)/9)}, level ${level}`);
  }
}
console.log(`${cases.length * 9} mip reductions exactly match unchanged upstream C++ across both architecture clock bases.`);
execFileSync("c++", ["-std=c++20", "-O2", "-I" + path.join(root,"third_party/rcv"), path.join(root,"tests/oracles/mip-geometry-main.cpp"), "-o", path.join(out,"mip-geometry")]);
buildSync({ entryPoints:[path.join(root,"src/timelineLayout.ts")], bundle:true, platform:"node", format:"cjs", outfile:path.join(out,"layout.cjs") });
const { mipPixel } = createRequire(import.meta.url)(path.join(out,"layout.cjs"));
const geometry = [];
for (let level=-4; level<=10; level++) for(const base of [1,4]) for(let n=0; n<100; n++) geometry.push({value:random(200000)-100000,level,base});
const geometryResults = execFileSync(path.join(out,"mip-geometry"), {input:geometry.map(g=>`${g.value} ${g.level} ${g.base}`).join("\n")+"\n",encoding:"utf8"}).trim().split("\n");
geometry.forEach((g,n) => {
  const [expected,reduction] = geometryResults[n].split(" ").map(Number);
  if(mipPixel(g.value,g)!==expected || reduction !== (g.level>=1 ? 2-g.level : 3)) throw Error(`Mip geometry mismatch ${JSON.stringify(g)}`);
});
console.log(`${geometry.length} integer position and slot-reduction cases exactly match upstream wave_utils.h.`);
