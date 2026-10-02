// Classification of the opaque `Dispatch.kernel` / `Interval.kernel` string.
//
// This is a READ-SIDE helper. It never rewrites the stored name: `kernelStats`
// (src/analysis.ts) and the kernel-tuning reverse-lookup group dispatches by exact
// string identity, so the displayed name must stay byte-equal to `Dispatch.kernel`.
// The classifier only *describes* a name — which backend family produced it, a short
// display base, whether it is a mangled symbol, and whether it is a driver-internal
// shader rather than a user kernel.
//
// Name production itself lives in four disjoint places today (RGP synthesis in
// crates/profile-core/src/rgp.rs, pass-through in import.rs/rocpd.rs, the C++ symbol
// parser in src-tauri/src/tuning_worker.py, and the SPIR-V digest in shader_worker.py);
// see docs/kernel-name-layer.md. Adding a backend family (e.g. HRX/loom) means giving
// it an importer arm that sets `Dispatch.kernel` and, if its names are not already
// classified by the built-ins below, registering a `KernelFamilyMatcher` here. Do not
// guess a family's grammar before its producer is known.

export type KernelFamily =
  | "radv-internal" // "… (RADV internal)": a driver-internal Vulkan command shader
  | "radv-pipeline" // "pipeline <hex>": a RADV pipeline named only by its PSO hash
  | "radv-code-object" // "code object N": a RADV code object with no PSO name
  | "mangled" // an Itanium-mangled C++ symbol ("_Z…"); demangle in the tuning worker
  | "plain" // a plain or already-demangled name
  | "unknown"; // empty or unrecognised

export interface KernelNameInfo {
  /** The recognised family. */
  family: KernelFamily;
  /** A short display base: the bare function name where one can be derived, else the
   *  name itself. Never a replacement for the identity string. */
  base: string;
  /** Starts with "_Z": the name is a mangled symbol and must be demangled by the
   *  tuning worker (c++filt) before `base` is meaningful. */
  mangled: boolean;
  /** A driver-internal shader rather than a user kernel, so not independently tunable. */
  internal: boolean;
}

/** A backend-family detector. Returns a classification for names it recognises, or
 *  null to defer to the next matcher / the built-ins. A new backend family registers
 *  one of these instead of editing `classifyKernelName`. */
export type KernelFamilyMatcher = (name: string) => KernelNameInfo | null;

const registered: KernelFamilyMatcher[] = [];

/** Register a backend-family matcher, consulted before the built-in families. Returns
 *  an unregister function (used by tests to stay isolated). */
export function registerKernelFamily(matcher: KernelFamilyMatcher): () => void {
  registered.push(matcher);
  return () => {
    const index = registered.indexOf(matcher);
    if (index >= 0) registered.splice(index, 1);
  };
}

/** The bare function name of a kernel, mirroring the tuning worker's `kernel_base`:
 *  strip the ` [clone .kd]` descriptor suffix and `(anonymous namespace)`, ignore
 *  template arguments and the parameter list, and take the last `::` scope and token
 *  (dropping a leading return type). A mangled ("_Z…") name is returned unchanged,
 *  because demangling needs c++filt, which only the tuning worker runs. */
export function kernelBase(name: string): string {
  if (name.startsWith("_Z")) return name;
  const canonical = name.replace(/ \[clone \.kd\]$/, "");
  const cleaned = canonical.replace(/\(anonymous namespace\)/g, "");
  let depth = 0;
  const head: string[] = [];
  for (const ch of cleaned) {
    if (ch === "(" && depth === 0) break;
    else if (ch === "<") depth++;
    else if (ch === ">") depth = Math.max(0, depth - 1);
    else if (depth === 0) head.push(ch);
  }
  const scope = head.join("").split("::").pop() ?? "";
  const token = scope.trim().split(/\s+/).pop() ?? "";
  return token || canonical;
}

/** Classify an opaque kernel/dispatch name. Registered family matchers are tried
 *  first, then the built-in RADV sentinels, then mangled vs plain. */
export function classifyKernelName(name: string): KernelNameInfo {
  const trimmed = name.trim();
  if (!trimmed)
    return {
      family: "unknown",
      base: "Unknown kernel",
      mangled: false,
      internal: false,
    };
  for (const matcher of registered) {
    const hit = matcher(trimmed);
    if (hit) return hit;
  }
  if (trimmed.endsWith("(RADV internal)"))
    return {
      family: "radv-internal",
      base: trimmed.replace(/\s*\(RADV internal\)$/, ""),
      mangled: false,
      internal: true,
    };
  if (/^pipeline [0-9a-f]{16}$/.test(trimmed))
    return {
      family: "radv-pipeline",
      base: trimmed,
      mangled: false,
      internal: false,
    };
  if (/^code object \d+$/.test(trimmed))
    return {
      family: "radv-code-object",
      base: trimmed,
      mangled: false,
      internal: false,
    };
  const mangled = trimmed.startsWith("_Z");
  return {
    family: mangled ? "mangled" : "plain",
    base: kernelBase(trimmed),
    mangled,
    internal: false,
  };
}

/** Whether a name is a driver-internal shader rather than an independently tunable
 *  user kernel. Centralises the `(RADV internal)` sentinel the dispatch table gates on. */
export function isInternalKernel(name: string): boolean {
  return classifyKernelName(name).internal;
}
