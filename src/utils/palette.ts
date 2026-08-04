import type { PaletteRGB } from "../types";

// Precompute Uint32 representations for channel shifts to avoid per-entry math
const SHIFT_0 = new Uint32Array(256);
const SHIFT_8 = new Uint32Array(256);
const SHIFT_16 = new Uint32Array(256);
const SHIFT_24 = new Uint32Array(256);

for (let i = 0; i < 256; i++) {
	SHIFT_0[i] = i;
	SHIFT_8[i] = i << 8;
	SHIFT_16[i] = i << 16;
	SHIFT_24[i] = i << 24;
}

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
	transparentIndex: number | null = null,
): Uint32Array {
	const pal32 = new Uint32Array(256);
	const limit = Math.min(paletteSize, 256);
	if (order === "rgba") {
		for (let i = 0; i < limit; i++) {
			const r = buf[paletteOffset + i * 3]! | 0;
			const g = buf[paletteOffset + i * 3 + 1]! | 0;
			const b = buf[paletteOffset + i * 3 + 2]! | 0;
			if (transparentIndex !== null && i === transparentIndex) {
				pal32[i] = SHIFT_16[b]! | SHIFT_8[g]! | SHIFT_0[r]!;
			} else {
				pal32[i] = SHIFT_24[255]! | SHIFT_16[b]! | SHIFT_8[g]! | SHIFT_0[r]!;
			}
		}
	} else {
		for (let i = 0; i < limit; i++) {
			const r = buf[paletteOffset + i * 3]! | 0;
			const g = buf[paletteOffset + i * 3 + 1]! | 0;
			const b = buf[paletteOffset + i * 3 + 2]! | 0;
			if (transparentIndex !== null && i === transparentIndex) {
				pal32[i] = SHIFT_16[r]! | SHIFT_8[g]! | SHIFT_0[b]!;
			} else {
				pal32[i] = SHIFT_24[255]! | SHIFT_16[r]! | SHIFT_8[g]! | SHIFT_0[b]!;
			}
		}
	}
	return pal32;
}
