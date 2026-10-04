export type PreparedFrameFormat = "rgba" | "bgra";

export type PreparedFrameCacheMode =
	| "auto"
	| "indices"
	| "rgba"
	| "sparse-rgba"
	| "composited";

export type PreparedFrameBackendPreference = "auto" | "javascript" | "wasm";

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
