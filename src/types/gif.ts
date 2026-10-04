/**
 * The byte-container contract used by omggif.
 *
 * Arrays, Uint8Array instances, and Node Buffers all satisfy this interface.
 */
export interface GifBinary {
	readonly length: number;
	[index: number]: number;
}

export type GifPixelBuffer = number[] | Uint8Array | Uint8ClampedArray;

export type PaletteRGB = number[]; // array of 24-bit 0xRRGGBB

export interface GifOptions {
	background?: number;
	loop?: number | null;
	palette?: PaletteRGB | null;
}

export interface FrameOptions {
	delay?: number;
	disposal?: number;
	palette?: PaletteRGB | null;
	transparent?: number | null;
}

/** Frame metadata compatible with omggif's public Frame type. */
export interface Frame {
	x: number;
	y: number;
	width: number;
	height: number;
	has_local_palette: boolean;
	palette_offset: number | null;
	palette_size: number | null;
	data_offset: number;
	data_length: number;
	transparent_index: number | null;
	interlaced: boolean;
	delay: number;
	disposal: number;
}

export type FrameInfo = Frame & {
	min_code_size: number;
	codes?: Uint8Array;
	indices?: Uint8Array;
	pal32rgba?: Uint32Array;
	pal32bgra?: Uint32Array;
	rgbaColors?: Uint32Array;
	bgraColors?: Uint32Array;
	opaqueSpans?: Uint32Array;
	opaquePositions?: Uint32Array;
};
