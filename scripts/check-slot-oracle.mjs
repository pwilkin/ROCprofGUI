import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import ts from "typescript";
const temp = mkdtempSync(join(tmpdir(), "rocprof-slots-"));
try {
  execFileSync("c++", [
    "-std=c++17",
    "-O2",
    "-Ithird_party/rcv",
    "tests/oracles/slot-main.cpp",
    "-o",
    join(temp, "oracle"),
  ]);
  writeFileSync(
    join(temp, "layout.mjs"),
    ts.transpileModule(readFileSync("src/timelineLayout.ts", "utf8"), {
      compilerOptions: {
        target: ts.ScriptTarget.ES2022,
        module: ts.ModuleKind.ES2022,
      },
    }).outputText,
  );
  const { tokenSlots } = await import(
    pathToFileURL(join(temp, "layout.mjs")).href
  );
  let seed = 192609;
  const random = () => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed / 2 ** 32;
  };
  const cases = Array.from({ length: 200 }, () => {
    let start = 0;
    return Array.from({ length: 100 }, () => {
      if (random() > 0.7) start += Math.floor(random() * 10);
      return {
        start,
        duration: Math.floor(random() * 30),
        stall: Math.floor(random() * 10),
      };
    });
  });
  const input = cases
    .map(
      (rows) =>
        `${rows.length}\n${rows.map((r) => `${r.start} ${Math.max(r.duration, r.stall)}`).join("\n")}`,
    )
    .join("\n");
  const lines = execFileSync(join(temp, "oracle"), { input, encoding: "utf8" })
    .trim()
    .split("\n");
  cases.forEach((rows, n) => {
    if (
      JSON.stringify(tokenSlots(rows)) !==
      JSON.stringify(lines[n].trim().split(/\s+/).map(Number))
    )
      throw Error(`Mismatch in case ${n}`);
  });
  console.log(
    "20,000 token display slots matched unchanged upstream C++ across 200 streams.",
  );
} finally {
  rmSync(temp, { recursive: true, force: true });
}
