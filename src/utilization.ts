import type { Capture, Token } from "./types";
export interface ClockModel {
  base: 1 | 4;
  family: "rdna" | "gcn";
}
export function clockModel(
  capture: Pick<Capture, "architecture" | "metadata">,
): ClockModel | null {
  const arch = capture.architecture.trim().toLowerCase();
  if (arch === "navi" || /^gfx1[012][0-9a-f]{2}$/.test(arch))
    return { base: 1, family: "rdna" };
  if (arch === "vega" || /^gfx[6789][0-9a-f]{2}$/.test(arch))
    return { base: 4, family: "gcn" };
  const gfxip = Number(capture.metadata.gfxip);
  if (Number.isInteger(gfxip) && gfxip >= 10 && gfxip <= 12)
    return { base: 1, family: "rdna" };
  if (Number.isInteger(gfxip) && gfxip >= 6 && gfxip <= 9)
    return { base: 4, family: "gcn" };
  return null;
}
export function utilizationToken(
  token: Token,
  model: ClockModel,
  isa: string,
  separateLds = false,
) {
  const category = /^v_(?:s?mfma|s?wmma)/i.test(isa.trim())
    ? 13
    : token.category;
  if (!Number.isInteger(category) || category < 0 || category > 13) return null;
  const special = category === 9 || category === 10 || category === 11;
  const cycles = Math.max(token.duration, token.stall);
  const stall = special
    ? Math.max(0, cycles - model.base)
    : Math.max(0, Math.min(cycles - model.base, token.stall));
  let pipe = "SCAL",
    shared = false;
  if (category === 9 || category === 10) {
    pipe = "IMMED";
    shared = true;
  } else if (category === 11) {
    pipe = "MSG";
    shared = true;
  } else if (category === 13)
    pipe = model.family === "rdna" ? "MATRIX" : "VALU";
  else if (category === 6) pipe = "VALU";
  else if (category === 5)
    pipe = model.family === "rdna" || separateLds ? "LDS" : "VMEM";
  else if (category === 12) pipe = model.family === "rdna" ? "RAY" : "VMEM";
  else if (category === 3 || category === 4) pipe = "VMEM";
  else if (model.family === "rdna" && (category === 7 || category === 8)) {
    pipe = "JUMP";
    shared = true;
  }
  return {
    start: token.start + stall,
    end: token.start + cycles,
    pipe,
    shared,
    stall,
  };
}
