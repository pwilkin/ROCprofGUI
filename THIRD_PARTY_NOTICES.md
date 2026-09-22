# Upstream format reference

ROCprof Compute Viewer was inspected to document its functionality and implement compatible readers. The source reference was https://github.com/ROCm/rocprof-compute-viewer at commit ea1ce2b87562b28e3fb5c8819b5e12573cc14424. The unchanged `hidden_latency.cpp`, `hidden_latency.h`, `derived_counter.cpp`, `derived_counter.h`, `marker_walker.cpp`, `marker_walker.h`, `marker_types.h` and the required `util` headers are vendored under `third_party/rcv` as independent test oracles. `src/tensor.ts` implements compatible counter-expression behavior with an independent TypeScript parser and tensor representation. The named-counter reconstruction in `crates/profile-core/src/waits.rs` adapts the upstream waitcnt algorithms with occurrence identities and conservative parsing. The Rust implementation in `crates/profile-core/src/hidden.rs` adapts that algorithm. `tests/oracles` supplies a minimal compatibility harness to compile the upstream implementation without Qt. The unchanged wait-counter source files are also vendored under `third_party/rcv/data/waitcnt` for independent comparison. The rest of the application and trace decoder are not vendored. The following license covers the upstream reference material and adapted algorithm.

The synthetic `fixtures/rocpd/sdk-3.0.4.db` was generated using the ROCm SDK schema and fixture generator from `ROCm/rocm-systems`. It contains no captured user data. The packed SPM coordinate layout in `crates/profile-core/src/spm_dimensions.rs` follows the SDK `counters/id_decode.hpp` field definitions; explicit instance metadata remains authoritative. That upstream material is also MIT licensed by Advanced Micro Devices.

MIT License

Copyright (c) 2026 Advanced Micro Devices, Inc. All rights reserved.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## AMD ISA reference

`public/isa/*.json` is generated from AMD GPUOpen machine-readable ISA XML specifications, downloaded 2026-09-19 from https://gpuopen.com/download/machine-readable-isa/latest/. Each XML declares the MIT license and Copyright (c) 2026 Advanced Micro Devices, Inc., or its affiliates. The license is reproduced in `public/isa/LICENSE.txt` and bundled with the desktop application. The manifest preserves source filenames, XML release/schema versions, per-file SHA-256 values and archive SHA-256. `scripts/build-isa-reference.py` performs the conversion; manual-specific notes are original concise summaries with direct source links. AMD PDF manuals are linked, not redistributed.
