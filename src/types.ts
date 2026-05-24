export interface UnifiedGPUGifRenderer {
	initialize: (canvas?: HTMLCanvasElement) => Promise<boolean>;
	renderFrame: (
		indexData: Uint8Array,
		palette: Uint32Array,
		width: number,
		height: number,
	) => Promise<HTMLCanvasElement | null>;
	renderToCanvas: (
		indexData: Uint8Array,
		palette: Uint32Array,
		width: number,
		height: number,
		targetCanvas: HTMLCanvasElement,
	) => Promise<boolean>;
	updatePalette: (palette: Uint32Array) => void;
	getBackend: () => string;
	isGPUAccelerated: () => boolean;
	benchmark: (width?: number, height?: number) => Promise<number>;
	dispose: () => void;
}

export interface ColorMapWasm {
	maxRowWidth?: number;
	heapU8: Uint8Array;
	heapU32: Uint32Array;
	palPtr: number;
	idxPtr: number;
	outPtr: number;
	map32: (
		idxPtr: number,
		outPtr: number,
		palPtr: number,
		width: number,
		height: number,
	) => void;
}

export interface WasmGifDecoder {
	memory: WebAssembly.Memory;
	decode_rgba: (
		gifPtr: number,
		gifLen: number,
		frameIndex: number,
		outPtr: number,
		outLen: number,
	) => number;
	decode_rgba_threaded: (
		gifPtr: number,
		gifLen: number,
		frameIndex: number,
		outPtr: number,
		outLen: number,
		numThreads: number,
	) => number;
	init_heap: () => void;
	reset_heap: () => void;
	get_heap_usage: () => number;
	test_simd: () => number;
	wasm_malloc: (size: number) => number;
	wasm_free: (ptr: number) => void;
	heapU8: Uint8Array;
	heapU32: Uint32Array;
}

export interface WasmWorkerPool {
	decodeFrame: (
		gifData: Uint8Array,
		frameIndex: number,
	) => Promise<{ pixels: Uint32Array; delay: number }>;
	decodeFrames: (
		gifData: Uint8Array,
		frameIndices: number[],
	) => Promise<{ pixels: Uint32Array; delay: number }[]>;
	terminate: () => void;
	getStats: () => {
		activeWorkers: number;
		completedJobs: number;
		avgDecodeTime: number;
	};
}

export interface WasmCoreInstance {
	width: () => number;
	height: () => number;
	frame_count: () => number;
	metadata_json: () => string;
	decode_frame_indices: (frameIndex: number) => Uint8Array;
	decode_frame_rgba: (frameIndex: number) => Uint8Array;
	decode_frame_bgra: (frameIndex: number) => Uint8Array;
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
	encode_indexed_gif?: (
		indexStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delay: number,
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
}

export interface PooledDecoderTables {
	decTable: Int32Array;
	stack: Uint8Array;
	firstByte: Int16Array;
	out32Cache: WeakMap<Uint8Array, Uint32Array>;
	hash: string;
}

export type PaletteRGB = number[]; // array of 24-bit 0xRRGGBB

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
	changedPixels?: Uint32Array;
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

export type FrameInfo = {
	x: number;
	y: number;
	width: number;
	height: number;
	has_local_palette: boolean;
	palette_offset: number;
	palette_size: number;
	data_offset: number;
	data_length: number;
	transparent_index: number | null;
	interlaced: boolean;
	delay: number;
	disposal: number;
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
