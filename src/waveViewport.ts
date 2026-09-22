import type { Wave } from "./types";

export function waveOccurrence(wave: Wave, instruction: number, occurrence: number) {
  if (wave.tokenIdentities) {
    const index = wave.tokens.findIndex((token, n) =>
      token.instruction === instruction && wave.tokenIdentities![n].occurrence === occurrence);
    return wave.tokens[index];
  }
  return wave.tokens.filter((token) => token.instruction === instruction)[occurrence];
}
