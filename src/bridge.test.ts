import { expect, it } from "vitest";
import { isRawAttSetupPath } from "./bridge";

// The ATT filename grammar lives in three copies (att_runs.rs SE_MARKER /
// split_stem, decode.py's regex, and this bridge routing). This pins the routing
// side: a single raw .att needs decoder setup; a `_shader_engine_<SE>_<RUN>.att`
// run member imports directly. Keep in step with the Rust/Python copies.
it("routes a bare raw .att to setup but a shader-engine run member to import", () => {
  // Needs raw-ATT setup (a single .att, not a decoded run member).
  for (const path of ["/x/trace.att", "/x/TRACE.ATT", "/x/kernel_dump.att"]) {
    expect(isRawAttSetupPath(path), path).toBe(true);
  }
  // A decoded run member (two trailing digit fields) imports directly.
  for (const path of [
    "/x/run_shader_engine_0_3.att",
    "/x/run_shader_engine_12_04.att",
    "/x/RUN_SHADER_ENGINE_2_3.ATT",
  ]) {
    expect(isRawAttSetupPath(path), path).toBe(false);
  }
  // Not an .att at all.
  for (const path of ["/x/trace.csv", "/x/a.att.json", "/x/shader_engine_0_3.rgp"]) {
    expect(isRawAttSetupPath(path), path).toBe(false);
  }
});
