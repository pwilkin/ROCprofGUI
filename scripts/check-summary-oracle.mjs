import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import ts from "typescript";
const temp = mkdtempSync(join(tmpdir(), "rocprof-summary-"));
try {
  execFileSync(
    "c++",
    [
      "-std=c++17",
      "-O2",
      "-Itests/oracles/include",
      "-Ithird_party/rcv",
      "tests/oracles/summary-main.cpp",
      "third_party/rcv/datanode.cpp",
      "-o",
      join(temp, "oracle"),
    ],
    { stdio: "inherit" },
  );
  writeFileSync(
    join(temp, "summary.mjs"),
    ts.transpileModule(readFileSync("src/summary.ts", "utf8"), {
      compilerOptions: {
        target: ts.ScriptTarget.ES2022,
        module: ts.ModuleKind.ES2022,
      },
    }).outputText,
  );
  const { hardwareSummary } = await import(
    pathToFileURL(join(temp, "summary.mjs")).href
  );
  let seed = 921062,
    checks = 0;
  const random = () => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed / 2 ** 32;
  };
  const names = [
    "BUSY_CU_CYCLES",
    "ACTIVE_INST_VALU",
    "ACTIVE_INST_VMEM",
    "VALU_MFMA_BUSY_CYCLES",
    "ACTIVE_INST_LDS",
    "ACTIVE_INST_FLAT",
    "ACTIVE_INST_SCA",
    "ACTIVE_INST_MISC",
  ];
  for (let trial = 0; trial < 80; trial++) {
    const records = [],
      series = [];
    for (let bank = 0; bank < 2; bank++)
      for (let se = 0; se < 3; se++) {
        const rows = [];
        for (let cu = 0; cu < 4; cu++) {
          const points = Array.from({ length: 4 }, () => []);
          let time = Math.floor(random() * 30);
          const delta = [32, 64, 66, 68, 96][Math.floor(random() * 5)];
          for (let n = 0; n < 25; n++) {
            time += delta * (random() < 0.2 ? 4 : 1);
            const values = Array.from({ length: 4 }, () =>
              Math.floor(random() * 100),
            );
            rows.push([bank, se, cu, time, ...values]);
            points.forEach((p, i) => p.push([time, values[i]]));
          }
          points.forEach((p, i) =>
            series.push({
              name: `SQ_${names[bank * 4 + i]}`,
              scope: `SE ${se} · CU ${cu} · bank ${bank}`,
              unit: "raw count",
              points: p,
            }),
          );
        }
        rows.sort((a, b) => a[3] - b[3]);
        records.push(...rows);
      }
    const output = execFileSync(join(temp, "oracle"), {
      input: `${records.length}\n${records.map((r) => r.join(" ")).join("\n")}\n`,
      encoding: "utf8",
    });
    const summary = hardwareSummary("gfx942", series);
    const delta = +output.match(/^delta (\d+)$/m)[1];
    if (summary.delta !== delta)
      throw Error(`Trial ${trial}: delta ${summary.delta} != ${delta}`);
    for (const [, bank, c, value] of output.matchAll(
      /^peak (\d+) (\d+) ([\d.e+-]+)$/gm,
    )) {
      const name = names[+bank * 4 + +c],
        scale = /^ACTIVE_INST_(LDS|VMEM|FLAT)$/.test(name) ? 2 : 1;
      const actual = summary.peakCounts[summary.counters.indexOf(name)];
      if (actual !== +value * scale)
        throw Error(`Trial ${trial} ${name}: ${actual} != ${+value * scale}`);
      checks++;
    }
  }
  console.log(
    `${checks} aggregate counter peaks and 80 sampling intervals matched unchanged upstream C++, including skew, multiple banks, CUs and sample gaps.`,
  );
} finally {
  rmSync(temp, { recursive: true, force: true });
}
