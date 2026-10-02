# The kernel-name layer, and what a new backend family must provide

`Dispatch.kernel` (and `Interval.kernel`) is an opaque `String`
(`crates/profile-core/src/model.rs`). There is no single "kernel-name grammar": a
name is decided per input format at import time, and four disjoint places touch it,
none sharing code with the others. This note records that layering so a new backend
family — the upcoming HRX/loom support is the motivating case — can be added without
rediscovering it, and without disturbing the exact-string identity the frontend and
the tuning workers depend on.

## The hard constraint: names are identity

`kernelStats` (`src/analysis.ts`) groups dispatches into a map keyed by the exact
`Dispatch.kernel` string, and the kernel-tuning "Tune" button reverse-looks-up the
dispatch with `d.kernel === k.name`. Two names merge only if they are byte-equal.
**A normaliser must therefore never rewrite the stored name.** Classification and
display are read-side only (`src/kernelName.ts`), layered on top of the stored string.

A few strings are load-bearing sentinels that existing code matches on; preserve them
or handle them explicitly:

- `… (RADV internal)` — a driver-internal Vulkan command shader; the kernel table hides
  its Tune button (`src/components/AnalysisViews.tsx`, via `isInternalKernel`).
- `pipeline <16-hex>` — a RADV pipeline named only by PSO hash.
- `code object N` — a RADV code object with no PSO name.
- a trailing ` [clone .kd]` — a ROCm kernel-descriptor symbol suffix stripped by the
  tuning worker's `canonical_kernel`.
- a leading `_Z` — an Itanium-mangled C++ symbol, demangled by the tuning worker
  (`c++filt`); it cannot be demangled in the Rust importer or the browser.

## The four places a name passes through

1. **Production / synthesis — the importer arm.** Most importers set `Dispatch.kernel`
   by pass-through: the rocprofv3 CSV arm copies `Kernel_Name`, the decoder/SQTT event
   arm copies `kernel_name`, the occupancy arm looks the dispatch id up in the
   `dispatches` map, and ROCPD prefers `display_name` then `kernel_name`
   (`crates/profile-core/src/import.rs`, `rocpd.rs`). Only RADV/RGP *synthesises* a name.
2. **Marker decoding (RADV only, the one real grammar).** `crates/profile-core/src/rgp.rs`
   turns thread-trace instrumentation markers into names: a debug label
   (`pipeline_names`, with the trailing ` (x,y,z)` workgroup suffix stripped) else the
   API PSO name/hash (`PlacedCodeObject::label`), then an internal-command rename
   (`attach_markers` + `internal_command`) that produces the `(RADV internal)` sentinel.
3. **Canonicalisation (HIP tuning only).** `src-tauri/src/tuning_worker.py`
   `canonical_kernel` (demangle `_Z…` via `c++filt`, strip ` [clone .kd]`) and
   `kernel_base` (bare function name, ignoring templates/params/namespaces) parse a
   name into structure for source matching, disassembly and the replay/counter identity.
   This is the only structural parser; `tuning_calibration.py` has a deliberately weaker
   suffix-strip-only variant for matching counter-CSV `Kernel_Name`.
4. **Identity (how a captured kernel is matched back to its source).** Vulkan matches a
   captured shader to a build shader by **SPIR-V structural digest**
   (`src-tauri/src/shader_worker.py` `normalized`). HIP has no byte-identity equivalent;
   it does a textual `kernel_base` search over `compile_commands.json` and HSACO symbols.

## The read-side classifier (`src/kernelName.ts`)

A shared, pure, tested helper the frontend (and a new backend's display code) can use
without touching identity:

- `classifyKernelName(name) → { family, base, mangled, internal }` — recognises the RADV
  sentinels, mangled vs plain names, and the empty/unknown case.
- `kernelBase(name)` — the browser-side port of the worker's `kernel_base` (returns a
  mangled name unchanged, since demangling needs the worker).
- `isInternalKernel(name)` — centralises the `(RADV internal)` sentinel the dispatch
  table gates on.
- `registerKernelFamily(matcher)` — the extension point. A matcher is tried before the
  built-ins and returns a classification for names it recognises or `null` to defer.

## Checklist for a new backend family (e.g. HRX/loom)

Establish the family's actual naming from its producer first — do not guess it. Then:

1. **Importer arm.** Give the backend an arm in `import.rs` (or a sibling module) that
   sets `Dispatch.kernel`/`Interval.kernel` from the backend's records. If its names are
   already mangled or plain C++ symbols, the built-in classification already covers them.
2. **Marker decoding.** Only if the backend carries its own in-trace instrumentation
   markers (as RADV does) does it need an `rgp.rs`-style decoder; a plain kernel-trace
   CSV or ELF/ATT input does not.
3. **Name canonicalisation.** If the family needs a tuning path, reuse the worker's
   `canonical_kernel`/`kernel_base`; extend them only if the family's symbols are not
   Itanium-mangled C++. For display/classification, register a `KernelFamilyMatcher`.
4. **Identity mechanism.** Decide how a captured kernel is matched back to source: a
   byte digest (as Vulkan does with SPIR-V) if the format admits one, else a documented
   textual match — and keep it honest about what it does and does not prove.

The HRX beta campaign feeds ordinary rocprofv3 kernel/counter/ATT inputs through the
existing importer arms, so steps 1–2 may need nothing new for that path; the native
IRPF bundle path is where a dedicated arm and identity mechanism will be decided, once
its producer schema is pinned.
