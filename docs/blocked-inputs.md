# Blocked inputs and the producer contract

This document records the external inputs the project is waiting on, and the evidence behind each. It exists so those questions are answered from the repository rather than re-derived from working notes every session: `.codex/` is an untracked working directory and is not present in a clone.

## The rule

**Nothing here is unblocked by guessing.** A private or inferred schema is not a producer contract, and a filename that looks like a dispatch ID is not a dispatch relationship. Where identity cannot be established, the application keeps it explicitly unknown and says so in the UI. An adapter written against a guessed layout would silently misattribute measurements, which is worse than the current refusal to load.

## 1. Embedded ROCPD ATT adapter

**Needed:** a supported producer's versioned table/column/payload contract for ATT bytes stored inside a ROCPD database, or a representative ROCPD file that contains ATT data.

**Evidence.** The upstream viewer's `RocpdEmitter::run` is a skeleton: it names embedded blobs and relative external paths in comments, throws `"rocpd support not yet implemented"`, and supplies no table names, field names, schema version or implemented query.

The official-schema-generated ROCPD fixtures for 3.0.0 through 3.0.4 were reopened read-only and every table and column recorded. Across the five fixtures there are **109 tables, zero columns declared `BLOB`, and no table named for `att`, `thread_trace`, `artifact` or similar**; there are **89 generic JSONB `extdata` columns**. The code-object table stores a URI, a load range and FILE/MEMORY provenance, and defines no ATT payload contract.

This is evidence about these schemas, not proof that no third-party or future ROCPD extension can carry trace bytes. SQLite's dynamic typing and the generic `extdata` columns mean column declarations alone cannot rule out embedded data — which is exactly why an adapter must not be written speculatively.

## 2. Automatic ATT-to-dispatch association

**Needed:** paired producer ROCPD/ATT data that retains capture mode and process/node/GUID identity.

**Evidence.** In the producer, the ATT output stem is built from agent, shader engine and a value called `dispatch_id`; when user data is zero, that value comes from `att_device_trace_id` for device, consecutive and marker modes. It is therefore **not unconditionally a kernel dispatch ID**. The JSON export flattens the in-memory ATT filename map to basenames, losing its structured keys, and the same agent/trace key is reused for `ui_output_agent_*_dispatch_*` directory names — so the directory spelling does not repair the ambiguity either. Upstream documentation also describes consecutive-kernel and selected-region captures.

Consequently a basename or directory match **cannot** establish a dispatch relationship. External-file discovery may offer candidates, but an association workflow has to retain explicit provenance and keep dispatch traces, device/region traces and unknown-mode traces distinct. Independent shader clocks stay separate until measured alignment exists, and code-object URI/load metadata keeps its process/node/GUID scope.

### Required acceptance tests

These are the cases the remaining ROCPD artifact work must satisfy. They cannot be written until input 1 or 2 arrives.

1. Single-dispatch producer metadata with explicit scope and a known capture mode.
2. A device-mode trace ID colliding with a real dispatch ID produces **no** automatic dispatch link.
3. Duplicate agent/dispatch numbers across process/node/GUID partitions produce **no** cross-partition association.
4. Multi-dispatch, consecutive and selected-region traces retain multi-dispatch or unknown ownership.
5. Missing, relocated, duplicate and changed external files produce explicit outcomes and preserve recorded provenance.
6. An embedded payload adapter requires a real versioned table/column contract or a representative producer fixture, with bounded extraction and decoder integration. Arbitrary `BLOB` or `extdata` fields must not be guessed to be ATT.

## 3. Hardware qualification

**Needed:** eligible GPU hosts, or representative captures, for the outstanding topology, clock, collection-loss/wrap, multi-SE SPM/SQTT and architecture-specific export cases.

**Evidence.** The qualification host reports gfx1151 (Radeon 8060S, 40 CUs). It exposes SPM configuration intervals of 32–65,504 shader-clock cycles but **no eligible SPM counters** under either the installed SDK 1.3.5 or a local SDK 1.4.0: the AQLprofile event filter excludes gfx1151 even though configuration enumeration succeeds. The capability listing reports no PC-sampling-capable agent. A bounded HIP workload passed all 65,536 CPU-oracle comparisons, but no SPM collection was forced and none is claimed. These are live capability queries, not a successful hardware collection, and not a claim about every driver/SDK combination.

A single real capture carrying **explicit architecture and counter metadata** would unblock the summary model, marker goldens, counter semantics and wave-state semantics together — currently four separate hardware-blocked items.

## 4. Display qualification

**Needed:** a multiple-display or mixed-scale environment, or an explicitly agreed change to the acceptance scope.

**Evidence.** The host has a single 1920×1080 HDMI-A-1 output. Changing the user's monitor configuration is not a substitute for qualifying physical multi-monitor WebKit behaviour.

## 5. Memory acceptance

**Needed:** representative required workloads, and an acceptable memory ceiling.

**Evidence.** Post-close retained footprint is high, variable and concentrated in WebKit, and grows across open/close cycles. The Rust side is demonstrably released. No whole-app memory bound is claimed and none can be accepted or rejected until a ceiling is named. This does not preclude fixing a demonstrated defect; it blocks declaring memory behaviour acceptable.

## Status

All five remain unanswered. They are completion requirements, not a claim that every possible improvement is exhausted. Rephrasing them does not make them answered, and an audit that restates them is not progress.
