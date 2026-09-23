import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { KernelTuning } from "./KernelTuning";
import { ShaderTuning } from "./ShaderTuning";

/** Opens the laboratory that matches the session: HIP kernels or Vulkan shaders. */
export function TuningWindow({ id }: { id: string }) {
  const [mode, setMode] = useState<string | null>(null);
  useEffect(() => {
    invoke<{ session: { mode?: string } }>("tuning_status", { id })
      .then((s) => setMode(s.session.mode || "hip"))
      .catch(() => setMode("hip"));
  }, [id]);
  if (!mode)
    return (
      <div className="tuning-window">
        <p role="status">Opening session…</p>
      </div>
    );
  return mode === "vulkan" ? <ShaderTuning id={id} /> : <KernelTuning id={id} />;
}
