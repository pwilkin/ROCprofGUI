import type { Wave, WaveIndex } from "./types";
export function waveCuLabel(wave: Wave): string {
  return wave.cu === null || wave.cu === undefined
    ? `CU unknown [${wave.id}]`
    : `CU ${wave.cu}`;
}
export function waveLocation(wave: Wave, index?: WaveIndex): string {
  return `SE ${index?.se ?? "?"} · ${waveCuLabel(wave)} · SIMD ${index?.simd ?? "?"} · slot ${index?.slot ?? "?"} · wave ${index?.wid ?? "?"}`;
}
export function cuWaveLane(wave: Wave, index?: WaveIndex): string {
  return `${waveCuLabel(wave)} · S${index?.simd ?? "?"} slot${index?.slot ?? "?"}`;
}
export function utilizationLane(
  wave: Wave,
  index: WaveIndex | undefined,
  shared: boolean,
  pipe: string,
): string {
  return `${waveCuLabel(wave)} · ${shared ? "Shared" : `S${index?.simd ?? "?"}`} · ${pipe}`;
}

export function otherSimdMarks(
  capture: import("./types").Capture,
  se: number,
  simd: number,
): import("./components/Plots").Mark[] {
  return (capture.otherSimd || []).flatMap((record, index) =>
    record.se !== se
      ? []
      : [
          {
            start: record.start,
            end: record.start + record.duration,
            lane: `Other S${simd ^ 2} · WGP ${record.wgp ?? "?"} · cat ${record.category ?? "?"}${record.wgp === null ? ` [${record.source}]` : ""}`,
            color: "#8bb9de",
            key: `other-simd:${index}`,
            recordKind: "activity" as const,
            recordIndex: index,
            label: `Other-SIMD activity · SE ${se} · WGP ${record.wgp ?? "unknown"} · category ${record.category ?? "unknown"} · ${record.source} · paired-SIMD projection S${simd ^ 2}; ${record.wgp == null ? "CU association unverified" : "recorded WGP scope; hidden analysis requires matching GFX10+ wave identity"}`,
          },
        ],
  );
}
