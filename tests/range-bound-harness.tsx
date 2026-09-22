import { useState } from "react";
import { createRoot } from "react-dom/client";
import RangeBound from "../src/components/RangeBound";
import type { Range } from "../src/types";

function Harness() {
  const [range, setRange] = useState<Range>([0, 1000]);
  return <>
    <RangeBound edge={0} range={range} bounds={[0, 1000]} onCommit={setRange} />
    <RangeBound edge={1} range={range} bounds={[0, 1000]} onCommit={setRange} />
    <button onMouseDown={e => e.preventDefault()} onClick={() => setRange(r => [100, r[1]])}>External start</button>
    <button onMouseDown={e => e.preventDefault()} onClick={() => setRange(r => [r[0], 800])}>External end</button>
    <output>{JSON.stringify(range)}</output>
  </>;
}

export function mount() {
  const host = document.createElement("div");
  host.id = "range-bound-test";
  document.body.prepend(host);
  createRoot(host).render(<Harness />);
}
