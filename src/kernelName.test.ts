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

it("classifies the HRX/loom kernel families from their observed name prefixes", () => {
  // ggml compute kernels (decoded-dispatches.json / HRX kernel+counter CSVs).
  for (const name of [
    "ggml_rope_f32",
    "ggml_mul_mat_swiglu_q4_q8_1_x4_lowtoken_dot_q8_output",
    "ggml_flash_attention_decode_split_f32_f16_wmma_next_q8",
    "ggml_rmsnorm_binary_q8_1_x4",
  ]) {
    const info = classifyKernelName(name);
    expect(info.family).toBe("hrx-ggml");
    expect(info.mangled).toBe(false);
    expect(info.base).toBe(name);
    // ggml compute kernels are user kernels, so not gated as internal.
    expect(isInternalKernel(name)).toBe(false);
  }
  // IREE HAL AMDGPU device-library kernels are HRX runtime internals.
  const runtime = classifyKernelName("iree_hal_amdgpu_device_buffer_copy_block_x16");
  expect(runtime.family).toBe("hrx-runtime");
  expect(runtime.internal).toBe(true);
  expect(isInternalKernel("iree_hal_amdgpu_device_buffer_copy_block_x16")).toBe(true);
  // Model-specific fused kernels carry no stable backend prefix and stay plain.
  for (const name of [
    "qwen_attention_metadata",
    "qwen3_moe_attention_postprocess_f32_f16",
    "llm_attention_v_matmul_set_rows_legacy_f32_f32",
  ]) {
    expect(classifyKernelName(name).family).toBe("plain");
  }
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

it("treats a throwing matcher as a defer, not a crash", () => {
  // classifyKernelName runs during render, so a buggy backend matcher must not
  // throw out of it; a throw defers to the next matcher / the built-ins.
  const unregister = registerKernelFamily(() => {
    throw new Error("matcher bug");
  });
  try {
    expect(() => classifyKernelName("scale_f32(float*)")).not.toThrow();
    expect(classifyKernelName("scale_f32(float*)").family).toBe("plain");
    // A later matcher still runs even though the earlier one threw.
    const after = registerKernelFamily((name) =>
      name === "scale_f32(float*)"
        ? { family: "plain", base: "scaled", mangled: false, internal: false }
        : null,
    );
    try {
      expect(classifyKernelName("scale_f32(float*)").base).toBe("scaled");
    } finally {
      after();
    }
  } finally {
    unregister();
  }
});
