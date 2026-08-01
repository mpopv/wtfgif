export interface WasmCoreInstance {
	width: () => number;
	height: () => number;
	frame_count: () => number;
	metadata_json: () => string;
	decode_frame_indices: (frameIndex: number) => Uint8Array;
	decode_frame_rgba: (frameIndex: number) => Uint8Array;
	decode_frame_bgra: (frameIndex: number) => Uint8Array;
	decode_and_blit_frame_rgba?: (
		frameIndex: number,
		pixels: Uint8Array,
	) => void;
	decode_and_blit_frame_bgra?: (
		frameIndex: number,
		pixels: Uint8Array,
	) => void;
	decode_all_rgba?: () => Uint32Array;
	reencode_gif_pixel_perfect?: () => Uint8Array;
	prepare_composited_rgba: (requestedFrames: Uint8Array) => Uint32Array;
	prepare_composited_bgra: (requestedFrames: Uint8Array) => Uint32Array;
	prepare_composited_delta_rgba: (requestedFrames: Uint8Array) => Uint32Array;
	prepare_composited_delta_bgra: (requestedFrames: Uint8Array) => Uint32Array;
	free: () => void;
}

export interface WasmCoreModule {
	WtfGifCore: new (data: Uint8Array) => WasmCoreInstance;
	core_version?: () => string;
	parse_metadata_json?: (data: Uint8Array) => string;
	decode_frame_indices?: (data: Uint8Array, frameIndex: number) => Uint8Array;
	decode_frame_rgba?: (data: Uint8Array, frameIndex: number) => Uint8Array;
	decode_frame_bgra?: (data: Uint8Array, frameIndex: number) => Uint8Array;
	decode_all_rgba?: (data: Uint8Array) => Uint32Array;
	reencode_gif_pixel_perfect?: (data: Uint8Array) => Uint8Array;
	remux_gif_pixel_perfect?: (data: Uint8Array) => Uint8Array;
	prepare_composited_rgba?: (
		data: Uint8Array,
		requestedFrames: Uint8Array,
	) => Uint32Array;
	prepare_composited_bgra?: (
		data: Uint8Array,
		requestedFrames: Uint8Array,
	) => Uint32Array;
	prepare_composited_delta_rgba?: (
		data: Uint8Array,
		requestedFrames: Uint8Array,
	) => Uint32Array;
	prepare_composited_delta_bgra?: (
		data: Uint8Array,
		requestedFrames: Uint8Array,
	) => Uint32Array;
	encode_indexed_lzw?: (
		indexStream: Uint8Array,
		minCodeSize: number,
		colorCount: number,
	) => Uint8Array;
	encode_indexed_lzw_scratch?: (
		indexStream: Uint8Array,
		minCodeSize: number,
		colorCount: number,
	) => number;
	encode_indexed_literal_lzw_scratch?: (
		indexStream: Uint8Array,
		minCodeSize: number,
		colorCount: number,
	) => number;
	indexed_lzw_input_scratch_reserve?: (length: number) => number;
	encode_indexed_lzw_scratch_from_input?: (
		length: number,
		minCodeSize: number,
		colorCount: number,
		literal: boolean,
	) => number;
	indexed_lzw_scratch_ptr?: () => number;
	wasm_memory?: () => WebAssembly.Memory;
	encode_indexed_gif?: (
		indexStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delay: number,
		loopCount: number,
	) => Uint8Array;
	encode_indexed_gif_with_delays?: (
		indexStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
	) => Uint8Array;
	encode_indexed_literal_gif?: (
		indexStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delay: number,
		loopCount: number,
	) => Uint8Array;
	encode_indexed_literal_gif_with_delays?: (
		indexStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
	) => Uint8Array;
	encode_indexed_literal_delta_gif?: (
		indexStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delay: number,
		loopCount: number,
	) => Uint8Array;
	encode_indexed_literal_delta_gif_with_delays?: (
		indexStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
	) => Uint8Array;
	encode_indexed_delta_gif?: (
		indexStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delay: number,
		loopCount: number,
	) => Uint8Array;
	encode_indexed_delta_gif_with_delays?: (
		indexStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
	) => Uint8Array;
	encode_rgba_gif?: (
		rgbaStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delay: number,
		loopCount: number,
		deltas: boolean,
	) => Uint8Array;
	encode_rgba_gif_with_options?: (
		rgbaStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
		deltas: boolean,
		alphaThreshold: number,
	) => Uint8Array;
	encode_rgba_literal_gif?: (
		rgbaStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delay: number,
		loopCount: number,
	) => Uint8Array;
	encode_rgba_literal_gif_with_options?: (
		rgbaStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
		alphaThreshold: number,
	) => Uint8Array;
	encode_rgba_literal_delta_gif?: (
		rgbaStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delay: number,
		loopCount: number,
	) => Uint8Array;
	encode_rgba_literal_delta_gif_with_options?: (
		rgbaStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
		alphaThreshold: number,
	) => Uint8Array;
	encode_rgba_gif_advanced?: (
		rgbaStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
		deltas: boolean,
		alphaThreshold: number,
		literal: boolean,
		quantization: number,
		paletteMode: number,
	) => Uint8Array;
	encode_rgba_gif_advanced_from_input?: (
		length: number,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
		deltas: boolean,
		alphaThreshold: number,
		literal: boolean,
		quantization: number,
		paletteMode: number,
	) => Uint8Array;
	encode_rgba_gif_advanced_scratch_from_input?: (
		length: number,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
		deltas: boolean,
		alphaThreshold: number,
		literal: boolean,
		quantization: number,
		paletteMode: number,
	) => number;
	gif_output_scratch_ptr?: () => number;
}

export interface PooledDecoderTables {
	decTable: Int32Array;
	stack: Uint8Array;
	firstByte: Int16Array;
}

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
	compression?: "balanced" | "fast";
	loop?: number | null;
	palette?: PaletteRGB | null;
}

export interface FrameOptions {
	delay?: number;
	disposal?: number;
	palette?: PaletteRGB | null;
	transparent?: number | null;
}

export type PreparedFrameFormat = "rgba" | "bgra";

export type PreparedFrameCacheMode =
	| "auto"
	| "indices"
	| "rgba"
	| "sparse-rgba"
	| "composited";

export type PreparedFrameBackendPreference = "auto" | "javascript" | "native";

export type PreparedFrameDedupeMode = "none" | "adjacent" | "all";

export interface PrepareFramesOptions {
	format?: PreparedFrameFormat;
	composited?: boolean;
	cache?: PreparedFrameCacheMode;
	frameIndices?: readonly number[];
	maxBytes?: number;
	backend?: PreparedFrameBackendPreference;
	deltas?: boolean;
	dedupe?: PreparedFrameDedupeMode;
}

export interface PreparedGifFrame {
	index: number;
	x: number;
	y: number;
	width: number;
	height: number;
	delay: number;
	disposal: number;
	byteLength: number;
	isFullCanvas: boolean;
	changedX?: number;
	changedY?: number;
	changedWidth?: number;
	changedHeight?: number;
	changedPixels?: Uint32Array | undefined;
	pixels?: Uint32Array;
	colors?: Uint32Array;
	spans?: Uint32Array;
	positions?: Uint32Array;
	indices?: Uint8Array;
	palette?: Uint32Array;
}

export interface PreparedGifFrames {
	width: number;
	height: number;
	format: PreparedFrameFormat;
	composited: boolean;
	frames: PreparedGifFrame[];
	byteLength: number;
	maxBytes: number | null;
	getFrame: (index: number) => PreparedGifFrame | undefined;
	getFramePixels: (index: number) => Uint32Array | undefined;
	getFrameBytes: (index: number) => Uint8Array | undefined;
	copyFrame: (index: number, target: Uint8Array | Uint32Array) => void;
	createPlayer: (target?: Uint8Array | Uint32Array) => PreparedGifPlayer;
	dispose: () => void;
}

export interface PreparedGifPlayer {
	target: Uint32Array;
	currentIndex: number;
	drawFrame: (index: number) => Uint32Array;
	next: () => Uint32Array;
	reset: () => void;
}

export interface GifDecodeBackendStatus {
	name: string;
	available: boolean;
}

export interface GifDecodeBackend {
	name: string;
	isAvailable: () => boolean;
	prepareFrames?: (
		gifData: Uint8Array,
		options: Required<Pick<PrepareFramesOptions, "format" | "composited">> &
			PrepareFramesOptions,
	) => PreparedGifFrames | null;
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
	decodeCount?: number;
};
