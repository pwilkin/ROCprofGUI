import { expect, it } from "vitest";
import {
  classifyKernelName,
  kernelBase,
  isInternalKernel,
  registerKernelFamily,
  type KernelNameInfo,
} from "./kernelName";

it("reduces a demangled signature to its bare function name", () => {
  // The same cases the tuning worker's kernel_base covers (tests/test_tuning_worker.py).
  expect(kernelBase("void mul_mat_q<(ggml_type)8, 32, false>(char const*)")).toBe(
    "mul_mat_q",
  );
  expect(kernelBase("void k_bin_bcast<&(op_add(float, float)), float>(float*)")).toBe(
    "k_bin_bcast",
  );
  expect(kernelBase("space::kernel<int>(float*)")).toBe("kernel");
  expect(kernelBase("void a::(anonymous namespace)::Gemm<64, true>(void const*)")).toBe(
    "Gemm",
  );
  expect(kernelBase("a::(anonymous namespace)::Row(float const*)")).toBe("Row");
});

it("strips the kernel-descriptor clone suffix but leaves mangled names for the worker", () => {
  expect(kernelBase("scale_f32(float*) [clone .kd]")).toBe("scale_f32");
  // A mangled symbol cannot be demangled in the browser; it is returned unchanged.
  expect(kernelBase("_Z5saxpyPKfS0_Pfi")).toBe("_Z5saxpyPKfS0_Pfi");
});

it("classifies the RADV sentinels the dispatch grammar emits", () => {
  const internal = classifyKernelName("vkCmdCopyBuffer (RADV internal)");
  expect(internal.family).toBe("radv-internal");
  expect(internal.internal).toBe(true);
  expect(internal.base).toBe("vkCmdCopyBuffer");
  expect(classifyKernelName("pipeline 0123456789abcdef").family).toBe(
    "radv-pipeline",
  );
  expect(classifyKernelName("code object 7").family).toBe("radv-code-object");
  // A pipeline named only by hash is still a real user pipeline, so it is tunable.
  expect(isInternalKernel("pipeline 0123456789abcdef")).toBe(false);
  expect(isInternalKernel("vkCmdCopyBuffer (RADV internal)")).toBe(true);
});

it("distinguishes mangled, plain and empty names", () => {
  const mangled = classifyKernelName("_Z5saxpyPKfS0_Pfi");
  expect([mangled.family, mangled.mangled]).toEqual(["mangled", true]);
  const plain = classifyKernelName("scale_f32(float*)");
  expect([plain.family, plain.mangled, plain.base]).toEqual([
    "plain",
    false,
    "scale_f32",
  ]);
  const empty = classifyKernelName("   ");
  expect([empty.family, empty.base]).toEqual(["unknown", "Unknown kernel"]);
});

it("lets a new backend family register a matcher that takes precedence", () => {
  const hrx: KernelNameInfo = {
    family: "plain",
    base: "fused_attention",
    mangled: false,
    internal: false,
  };
  const unregister = registerKernelFamily((name) =>
    name.startsWith("loom::") ? { ...hrx } : null,
  );
  try {
    expect(classifyKernelName("loom::fused_attention").base).toBe(
      "fused_attention",
    );
    // Names the matcher declines still fall through to the built-ins.
    expect(classifyKernelName("code object 3").family).toBe("radv-code-object");
  } finally {
    unregister();
  }
  // After unregistering, the custom family no longer applies.
  expect(classifyKernelName("loom::fused_attention").family).toBe("plain");
});
