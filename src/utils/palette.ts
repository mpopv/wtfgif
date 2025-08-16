import { PaletteRGB } from "../types";

export function log2Pow2(n: number): number {
  return 31 - Math.clz32(n);
}

export function checkPalette(pal: PaletteRGB): number {
  const n = pal.length >>> 0;
  if (n === 0 || n > 256)
    throw new Error("Invalid palette size (must be 1..256).");
  let pow2 = 1;
  while (pow2 < n) pow2 <<= 1;
  if (pow2 < 2) pow2 = 2;
  if (pow2 > 256) throw new Error("Invalid palette size (must be 1..256).");
  return pow2;
}

export function buildPal32(
  buf: Uint8Array,
  paletteOffset: number,
  paletteSize: number,
  order: "rgba" | "bgra",
  transparentIndex: number | null = null
): Uint32Array {
  const pal32 = new Uint32Array(256);
  const limit = Math.min(paletteSize, 256);
  if (order === "rgba") {
    for (let i = 0; i < limit; i++) {
      const r = buf[paletteOffset + i * 3] | 0;
      const g = buf[paletteOffset + i * 3 + 1] | 0;
      const b = buf[paletteOffset + i * 3 + 2] | 0;
      const alpha =
        transparentIndex !== null && i === transparentIndex ? 0 : 255;
      pal32[i] = (alpha << 24) | (b << 16) | (g << 8) | r;
    }
  } else {
    for (let i = 0; i < limit; i++) {
      const r = buf[paletteOffset + i * 3] | 0;
      const g = buf[paletteOffset + i * 3 + 1] | 0;
      const b = buf[paletteOffset + i * 3 + 2] | 0;
      const alpha =
        transparentIndex !== null && i === transparentIndex ? 0 : 255;
      pal32[i] = (alpha << 24) | (r << 16) | (g << 8) | b;
    }
  }
  return pal32;
}
