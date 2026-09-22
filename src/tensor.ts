export type Shape = [number, number, number, number];
export const axes = ["XCC", "SE", "CU", "Time"];
const LIMIT = 2_000_000;
let allocationBudget: number | undefined;
export function withTensorBudget<T>(cells: number, run: () => T): T {
  const previous = allocationBudget;
  allocationBudget = cells;
  try {
    return run();
  } finally {
    allocationBudget = previous;
  }
}
export class Tensor {
  axisIndices: (number[] | null)[];
  constructor(
    public shape: Shape,
    public values: number[],
  ) {
    const size = shape.reduce((a, b) => a * b, 1);
    if (
      shape.some((n) => !Number.isInteger(n) || n < 1) ||
      size > LIMIT ||
      size !== values.length
    )
      throw Error("Invalid or oversized tensor shape");
    if (allocationBudget !== undefined) {
      allocationBudget -= size;
      if (allocationBudget < 0)
        throw Error(
          "Counter evaluation allocation budget exceeded; simplify the definitions or reduce the domain",
        );
    }
    this.axisIndices = shape.map((n) => Array.from({ length: n }, (_, i) => i));
  }
  static scalar(n: number) {
    const value = new Tensor([1, 1, 1, 1], [n]);
    value.axisIndices = [null, null, null, null];
    return value;
  }
  scalar() {
    if (this.values.length !== 1) throw Error("Expected scalar");
    return this.values[0];
  }
  index(c: number[]) {
    return c.reduce((n, v, i) => n * this.shape[i] + v, 0);
  }
  coords(n: number) {
    const c = [0, 0, 0, 0];
    for (let i = 3; i >= 0; i--) {
      c[i] = n % this.shape[i];
      n = Math.floor(n / this.shape[i]);
    }
    return c;
  }
  binary(b: Tensor, fn: (a: number, b: number) => number) {
    const shape = this.shape.map((a, i) => {
      if (a !== b.shape[i] && a !== 1 && b.shape[i] !== 1)
        throw Error(`Incompatible ${axes[i]} dimensions`);
      return Math.max(a, b.shape[i]);
    }) as Shape;
    const out = allocate(shape);
    out.axisIndices = shape.map((_, i) =>
      this.shape[i] >= b.shape[i] && this.axisIndices[i]
        ? this.axisIndices[i]
        : b.axisIndices[i],
    );
    out.values = out.values.map((_, n) => {
      const c = out.coords(n),
        aValue =
          this.values[
            this.index(c.map((v, i) => (this.shape[i] === 1 ? 0 : v)))
          ],
        bValue = b.values[b.index(c.map((v, i) => (b.shape[i] === 1 ? 0 : v)))];
      return Number.isFinite(aValue) && Number.isFinite(bValue)
        ? fn(aValue, bValue)
        : NaN;
    });
    return out;
  }
  reduce(kind: string, selected: number[]) {
    const dims = selected.length ? selected : [0, 1, 2, 3];
    const out = allocate(
      this.shape.map((n, i) => (dims.includes(i) ? 1 : n)) as Shape,
    );
    out.axisIndices = this.axisIndices.map((indices, i) =>
      dims.includes(i) ? null : indices,
    );
    const counts = out.values.map(() => 0);
    this.values.forEach((v, n) => {
      if (!Number.isFinite(v)) return;
      const c = this.coords(n).map((v, i) => (dims.includes(i) ? 0 : v)),
        j = out.index(c);
      out.values[j] = !counts[j]
        ? v
        : kind === "min"
          ? Math.min(out.values[j], v)
          : kind === "max"
            ? Math.max(out.values[j], v)
            : out.values[j] + v;
      counts[j]++;
    });
    if (kind === "mean")
      out.values = out.values.map((v, n) => (counts[n] ? v / counts[n] : NaN));
    return out;
  }
  take(axis: number, indices: number[]) {
    if (
      !indices.length ||
      indices.some(
        (n) => !Number.isInteger(n) || n < 0 || n >= this.shape[axis],
      )
    )
      throw Error("Selection outside tensor dimension");
    const shape = [...this.shape] as Shape;
    shape[axis] = indices.length;
    const out = allocate(shape);
    out.axisIndices = this.axisIndices.map((ids, i) =>
      i === axis && ids ? indices.map((n) => ids[n]) : ids,
    );
    out.values = out.values.map((_, n) => {
      const c = out.coords(n);
      c[axis] = indices[c[axis]];
      return this.values[this.index(c)];
    });
    return out;
  }
}
function allocate(shape: Shape) {
  const n = shape.reduce((a, b) => a * b, 1);
  if (n > LIMIT || n < 1)
    throw Error("Tensor exceeds two million cells or has an empty dimension");
  return new Tensor(shape, Array(n).fill(NaN));
}
const precedence: Record<string, number> = {
  "|": 1,
  "&": 2,
  "==": 3,
  "<": 3,
  ">": 3,
  "<=": 3,
  ">=": 3,
  "+": 4,
  "-": 4,
  "*": 5,
  "/": 5,
};
const operations: Record<string, (a: number, b: number) => number> = {
  "+": (a, b) => a + b,
  "-": (a, b) => a - b,
  "*": (a, b) => a * b,
  "/": (a, b) => (b === 0 ? NaN : a / b),
  "==": (a, b) => +(a === b),
  "<": (a, b) => +(a < b),
  ">": (a, b) => +(a > b),
  "<=": (a, b) => +(a <= b),
  ">=": (a, b) => +(a >= b),
  "|": (a, b) => +(!!a || !!b),
  "&": (a, b) => +(!!a && !!b),
};
export function tensorExpression(
  source: string,
  resolve: (name: string) => Tensor,
): Tensor {
  if (source.length > 8192) throw Error("Expression too long");
  const tokens: string[] = [];
  const pattern =
    /\s*(\d+(?:\.\d*)?(?:[eE][+-]?\d+)?|\.\d+(?:[eE][+-]?\d+)?|[A-Za-z_]\w*|==|<=|>=|[+\-*/<>&|()[\],:=])/y;
  let offset = 0;
  while (offset < source.trimEnd().length) {
    pattern.lastIndex = offset;
    const m = pattern.exec(source);
    if (!m) throw Error(`Invalid expression at ${offset}`);
    tokens.push(m[1]);
    offset = pattern.lastIndex;
  }
  let pos = 0,
    depth = 0;
  const peek = () => tokens[pos];
  const accept = (t: string) => peek() === t && (++pos, true);
  const expect = (t: string) => {
    if (!accept(t)) throw Error(`Expected ${t}, got ${peek() || "end"}`);
  };
  function axis() {
    const negative = accept("-");
    const name = (negative ? "-" : "") + tokens[pos++];
    const index = (
      {
        XCC: 0,
        xcc: 0,
        SE: 1,
        se: 1,
        CU: 2,
        WGP: 2,
        wgp: 2,
        INSTANCE: 2,
        instance: 2,
        cu: 2,
        Time: 3,
        time: 3,
        TIME: 3,
        SAMPLES: 3,
        samples: 3,
        "0": 0,
        "1": 1,
        "2": 2,
        "3": 3,
        All: -1,
        all: -1,
        "-1": -1,
      } as Record<string, number>
    )[name];
    if (index === undefined) throw Error(`Unknown axis ${name}`);
    return index;
  }
  function axisList(defaults: number[]) {
    if (!accept(",")) return defaults;
    expect("axis");
    expect("=");
    const result: number[] = [];
    if (accept("[")) {
      result.push(axis());
      while (accept(",")) result.push(axis());
      expect("]");
    } else result.push(axis());
    return result.includes(-1) ? [0, 1, 2, 3] : [...new Set(result)];
  }
  function expression(min = 0): Tensor {
    if (++depth > 64) throw Error("Expression nesting too deep");
    let left = primary();
    while (peek() in precedence && precedence[peek()] >= min) {
      const op = tokens[pos++];
      left = left.binary(expression(precedence[op] + 1), operations[op]);
    }
    depth--;
    return left;
  }
  function primary(): Tensor {
    if (accept("-"))
      return Tensor.scalar(-1).binary(primary(), operations["*"]);
    if (accept("+")) return primary();
    if (accept("(")) {
      const v = expression();
      expect(")");
      return v;
    }
    const name = tokens[pos++];
    if (!name) throw Error("Expected expression");
    if (/^(?:\d|\.)/.test(name)) return Tensor.scalar(Number(name));
    if (!/^[A-Za-z_]\w*$/.test(name)) throw Error(`Unexpected ${name}`);
    if (accept("(")) {
      const first = expression();
      if (name === "linear") {
        const dim = axisList([3]);
        expect(")");
        if (dim.length !== 1) throw Error("linear requires one axis");
        const shape: Shape = [1, 1, 1, 1];
        shape[dim[0]] = first.scalar();
        const out = allocate(shape);
        out.values = out.values.map((_, n) => n);
        return out;
      }
      const args = [first];
      while (accept(",")) args.push(expression());
      expect(")");
      if (name === "abs" && args.length === 1)
        return first.binary(Tensor.scalar(0), (a) => Math.abs(a));
      if (!["max", "min"].includes(name) || args.length < 2)
        throw Error(`Invalid function ${name}`);
      return args.reduce((a, b) =>
        a.binary(b, name === "max" ? Math.max : Math.min),
      );
    }
    if (accept("[")) {
      const value = expression();
      if (["mean", "sum", "min", "max"].includes(name)) {
        const dims = axisList([]);
        expect("]");
        return value.reduce(name, dims);
      }
      let start = 0,
        stop: number | undefined,
        step = 1;
      if (name === "select" || name === "remove") {
        expect(",");
        start = expression().scalar();
        if (name === "select" && accept(":")) {
          stop = expression().scalar();
          if (accept(":")) step = expression().scalar();
        }
      } else if (name !== "delta") throw Error(`Unknown function ${name}`);
      const dims = axisList([3]);
      expect("]");
      if (dims.length !== 1) throw Error(`${name} requires one axis`);
      const dim = dims[0],
        size = value.shape[dim];
      if (name === "delta") {
        const ids = Array.from({ length: size - 1 }, (_, n) => n);
        return value
          .take(
            dim,
            ids.map((n) => n + 1),
          )
          .binary(value.take(dim, ids), operations["-"]);
      }
      if (!Number.isInteger(start) || !Number.isInteger(step) || step < 1)
        throw Error("Selection requires integer indices and positive step");
      if (start < 0) start += size;
      if (name === "remove") {
        if (start < 0 || start >= size)
          throw Error("Removal outside dimension");
        return value.take(
          dim,
          Array.from({ length: size }, (_, n) => n).filter((n) => n !== start),
        );
      }
      if (stop === undefined) return value.take(dim, [start]);
      if (
        !Number.isInteger(stop) ||
        stop < 0 ||
        stop > size ||
        start < 0 ||
        start >= stop
      )
        throw Error("Invalid slice bounds");
      return value.take(
        dim,
        Array.from(
          { length: Math.ceil((stop - start) / step) },
          (_, n) => start + n * step,
        ),
      );
    }
    return resolve(name);
  }
  const result = expression();
  if (pos !== tokens.length) throw Error(`Unexpected ${peek()}`);
  return result;
}
export function tensorDefinitions(source: string, raw: Record<string, Tensor>) {
  const definitions = new Map<string, string>(),
    cache = new Map<string, Tensor>(),
    active = new Set<string>();
  for (const line of source.split("\n")) {
    const clean = line.replace(/#.*/, "").trim();
    if (!clean) continue;
    const match = clean.match(/^([A-Za-z_]\w*)\s*:=\s*(.+)$/);
    if (!match || definitions.has(match[1]) || Object.hasOwn(raw, match[1]))
      throw Error("Invalid, duplicate or shadowing definition");
    definitions.set(match[1], match[2]);
  }
  function resolve(name: string): Tensor {
    if (Object.hasOwn(raw, name)) return raw[name];
    const builtin = ["NUM_XCC", "NUM_SE", "NUM_CU", "NUM_SAMPLES"].indexOf(
      name,
    );
    if (builtin >= 0 && Object.values(raw).length)
      return Tensor.scalar(Object.values(raw)[0].shape[builtin]);
    if (cache.has(name)) return cache.get(name)!;
    if (!definitions.has(name)) throw Error(`Unknown counter ${name}`);
    if (active.has(name)) throw Error(`Circular definition ${name}`);
    if (active.size >= 64) throw Error("Too many dependent definitions");
    active.add(name);
    const result = tensorExpression(definitions.get(name)!, resolve);
    cache.set(name, result);
    active.delete(name);
    return result;
  }
  for (const name of definitions.keys()) resolve(name);
  return cache;
}
