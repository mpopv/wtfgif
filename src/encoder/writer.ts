import { GIF } from "../constants/gif";
import { getNativeAddonModule } from "../native/encodeRuntime";
import type {
	FrameOptions,
	GifBinary,
	GifOptions,
	PaletteRGB,
	WasmCoreModule,
	WasmEncodeCoreModule,
	WasmQualityCoreModule,
} from "../types";
import { writeNetscapeLoopCount } from "../utils/netscape";
import { checkPalette, log2Pow2 } from "../utils/palette";
import { findChangedRect } from "../utils/pixels";
import { getWasmEncodeCoreModule } from "../wasm/encodeRuntime";
import { getWasmQualityCoreModule } from "../wasm/qualityRuntime";

const WASM_LZW_MIN_INDEX_COUNT = 8192;
const TRANSPARENT_ALPHA_THRESHOLD = 128;
let lzwScratchMemoryModule:
	| WasmCoreModule
	| WasmEncodeCoreModule
	| WasmQualityCoreModule
	| null = null;
let lzwScratchMemory: WebAssembly.Memory | null = null;
let lzwInputScratchPointer = 0;
let lzwInputScratchCapacity = 0;
const EMPTY_PALETTE_UINT32 = new Uint32Array(0);
const EMPTY_PALETTE: PaletteRGB = [];
let uniformDelayCache = new Uint16Array(0);
let uniformDelayCacheValue = -1;

function getEncoderWasmCoreModule():
	| WasmCoreModule
	| WasmEncodeCoreModule
	| null {
	return getWasmEncodeCoreModule();
}

function getQualityEncoderWasmCoreModule(): WasmQualityCoreModule | null {
	return getWasmQualityCoreModule() ?? getWasmEncodeCoreModule();
}

function copyRgbaFramesToWasmScratch(
	wasmCore: WasmCoreModule | WasmEncodeCoreModule | WasmQualityCoreModule,
	frames: RgbaGifFrames,
	frameByteSize: number,
	frameCount: number,
): WebAssembly.Memory | null {
	prepareWasmEncoderModule(wasmCore);
	const inputLength = frameByteSize * frameCount;
	const scratchMemory = lzwScratchMemory;
	if (!scratchMemory) return null;
	if (lzwInputScratchCapacity < inputLength) {
		lzwInputScratchPointer =
			wasmCore.indexed_lzw_input_scratch_reserve(inputLength);
		lzwInputScratchCapacity = inputLength;
	}
	const input = new Uint8Array(
		scratchMemory.buffer,
		lzwInputScratchPointer,
		inputLength,
	);
	if (isRgbaFrame(frames)) {
		input.set(asUint8Array(frames));
	} else {
		for (let frameIndex = 0; frameIndex < frameCount; frameIndex += 1) {
			input.set(
				asUint8Array(frames[frameIndex]!).subarray(0, frameByteSize),
				frameIndex * frameByteSize,
			);
		}
	}
	return scratchMemory;
}

/**
 * Bind the Wasm memory without allocating source or output scratch. The first
 * encode reserves exactly the input range it needs, keeping initialization
 * cold and allocation-free.
 */
export function prepareWasmEncoderModule(
	wasmCore:
		| WasmCoreModule
		| WasmEncodeCoreModule
		| WasmQualityCoreModule
		| null,
): void {
	if (!wasmCore) {
		lzwScratchMemoryModule = null;
		lzwScratchMemory = null;
		lzwInputScratchPointer = 0;
		lzwInputScratchCapacity = 0;
		return;
	}
	if (lzwScratchMemoryModule === wasmCore && lzwScratchMemory) return;
	lzwScratchMemoryModule = wasmCore;
	lzwScratchMemory = wasmCore.wasm_memory();
	lzwInputScratchPointer = 0;
	lzwInputScratchCapacity = 0;
}

export type IndexedGifFrame = Uint8Array | number[];
export type IndexedGifFrames = Uint8Array | IndexedGifFrame[];
export type EncodeIndexedGifFramesBackend =
	| "auto"
	| "wasm"
	| "native-addon"
	| "javascript";
export type RgbaGifFrame = Uint8Array | Uint8ClampedArray;
export type RgbaGifFrames = Uint8Array | Uint8ClampedArray | RgbaGifFrame[];
export type GifFrameDelay = number | readonly number[] | Uint16Array;
export type GifCompressionMode = "balanced" | "fast";
export type GifQuantizationMode = "exact" | "fast" | "quality";
export type GifPaletteMode = "global" | "local";

export interface EncodeIndexedGifFramesOptions {
	width: number;
	height: number;
	frames: IndexedGifFrames;
	frameCount?: number;
	palette: PaletteRGB;
	delay?: GifFrameDelay;
	loop?: number | null;
	backend?: EncodeIndexedGifFramesBackend;
	delta?: boolean;
	compression?: GifCompressionMode;
}

export interface EncodeRgbaGifFramesOptions {
	width: number;
	height: number;
	frames: RgbaGifFrames;
	frameCount?: number;
	palette?: PaletteRGB;
	delay?: GifFrameDelay;
	loop?: number | null;
	backend?: EncodeIndexedGifFramesBackend;
	delta?: boolean;
	alphaThreshold?: number;
	compression?: GifCompressionMode;
	quantization?: GifQuantizationMode;
	paletteMode?: GifPaletteMode;
}

type IndexedSourceRect = {
	data: IndexedGifFrame;
	offset: number;
	width: number;
	height: number;
	stride: number;
	length: number;
};

type LzwIndexStream = IndexedGifFrame | IndexedSourceRect;

function resolveEncoderBackends(backend: EncodeIndexedGifFramesBackend) {
	const nativeAddon =
		backend === "auto" || backend === "native-addon"
			? getNativeAddonModule()
			: null;
	const wasmCore =
		backend === "auto" || backend === "wasm"
			? getEncoderWasmCoreModule()
			: null;
	if (backend === "native-addon" && !nativeAddon) {
		throw new Error("The wtfgif native addon is not installed.");
	}
	if (backend === "wasm" && !wasmCore) {
		throw new Error("The wtfgif WebAssembly encoder is not initialized.");
	}
	return { nativeAddon, wasmCore };
}

export function encodeIndexedGifFrames(
	options: EncodeIndexedGifFramesOptions,
): Uint8Array {
	const width = options.width | 0;
	const height = options.height | 0;
	if (width <= 0 || height <= 0 || width > 65535 || height > 65535) {
		throw new Error("Width/Height invalid.");
	}

	const colorCount = checkPalette(options.palette);
	const frameSize = width * height;
	const frameCount = getIndexedFrameCount(
		options.frames,
		frameSize,
		options.frameCount,
	);
	const delays = normalizeFrameDelays(options.delay, frameCount);
	const loop =
		options.loop === undefined || options.loop === null
			? null
			: checkedU16(options.loop, "Loop count invalid.");
	const backend = options.backend ?? "auto";
	const { nativeAddon, wasmCore } = resolveEncoderBackends(backend);
	const fastCompression = options.compression !== "balanced";
	const delta = options.delta === true;
	const palette = paletteToUint32Array(options.palette);

	if (nativeAddon) {
		const flatFrames = flattenIndexedFrames(
			options.frames,
			frameSize,
			frameCount,
			colorCount,
			false,
		);
		const encode = fastCompression
			? nativeAddon.encodeIndexedFast
			: nativeAddon.encodeIndexedBalanced;
		return encode(
			flatFrames,
			width,
			height,
			frameCount,
			palette,
			delayArray(delays, frameCount),
			loop === null ? -1 : loop,
			delta,
		);
	}

	if (wasmCore) {
		const flatFrames = flattenIndexedFrames(
			options.frames,
			frameSize,
			frameCount,
			colorCount,
			false,
		);
		if (typeof delays === "number") {
			if (fastCompression && !delta) {
				return encodeIndexedGifWithWasmScratch(
					wasmCore,
					flatFrames,
					width,
					height,
					frameCount,
					palette,
					delays,
					loop === null ? -1 : loop,
				);
			}
			const encode = fastCompression
				? delta
					? wasmCore.encode_indexed_literal_delta_gif
					: wasmCore.encode_indexed_literal_gif
				: delta
					? wasmCore.encode_indexed_delta_gif
					: wasmCore.encode_indexed_gif;
			return encode(
				flatFrames,
				width,
				height,
				frameCount,
				palette,
				delays,
				loop === null ? -1 : loop,
			);
		}
		const encode = fastCompression
			? delta
				? wasmCore.encode_indexed_literal_delta_gif_with_delays
				: wasmCore.encode_indexed_literal_gif_with_delays
			: delta
				? wasmCore.encode_indexed_delta_gif_with_delays
				: wasmCore.encode_indexed_gif_with_delays;
		return encode(
			flatFrames,
			width,
			height,
			frameCount,
			palette,
			delays,
			loop === null ? -1 : loop,
		);
	}

	return encodeIndexedGifFramesJavascript(
		options.frames,
		width,
		height,
		frameSize,
		frameCount,
		options.palette,
		delays,
		loop,
		delta,
	);
}

export function encodeRgbaGifFrames(
	options: EncodeRgbaGifFramesOptions,
): Uint8Array {
	if (isFastQualityWasmRequest(options)) {
		const wasmOutput = encodeRgbaQualityWasm(options);
		if (wasmOutput !== null) return wasmOutput;
	}
	return encodeRgbaGifFramesGeneral(options);
}

function isFastQualityWasmRequest(
	options: EncodeRgbaGifFramesOptions,
): boolean {
	if (options.backend === "native-addon" || options.backend === "javascript") {
		return false;
	}
	if (options.backend === "auto" && getNativeAddonModule()) return false;
	return (
		options.compression !== "balanced" &&
		(options.quantization ?? "quality") === "quality" &&
		(options.paletteMode ?? "global") === "global" &&
		options.palette === undefined &&
		options.delta !== true
	);
}

function encodeRgbaQualityWasm(
	options: EncodeRgbaGifFramesOptions,
): Uint8Array | null {
	const wasmCore = getQualityEncoderWasmCoreModule();
	if (!wasmCore) return null;
	const frames = options.frames;
	const width = options.width | 0;
	const height = options.height | 0;
	if (width <= 0 || height <= 0 || width > 65535 || height > 65535) {
		throw new Error("Width/Height invalid.");
	}
	const frameSize = width * height;
	const frameByteSize = frameSize * 4;
	const frameCount = getRgbaFrameCount(
		options.frames,
		frameByteSize,
		options.frameCount,
	);
	const delays = normalizeFrameDelays(options.delay, frameCount);
	const alphaThreshold = normalizeAlphaThreshold(options.alphaThreshold);
	const loop =
		options.loop === undefined || options.loop === null
			? null
			: checkedU16(options.loop, "Loop count invalid.");
	const inputLength = frameByteSize * frameCount;
	const scratchMemory = copyRgbaFramesToWasmScratch(
		wasmCore,
		frames,
		frameByteSize,
		frameCount,
	);
	if (!scratchMemory) return null;
	const outputLength =
		typeof delays === "number"
			? wasmCore.encode_rgba_quality_gif_constant_delay_scratch_from_input(
					inputLength,
					width,
					height,
					frameCount,
					delays,
					loop === null ? -1 : loop,
					alphaThreshold,
				)
			: wasmCore.encode_rgba_quality_gif_scratch_from_input(
					inputLength,
					width,
					height,
					frameCount,
					delays,
					loop === null ? -1 : loop,
					alphaThreshold,
				);
	return new Uint8Array(
		scratchMemory.buffer,
		wasmCore.gif_output_scratch_ptr(),
		outputLength,
	).slice();
}

function encodeRgbaGifFramesGeneral(
	options: EncodeRgbaGifFramesOptions,
): Uint8Array {
	const width = options.width | 0;
	const height = options.height | 0;
	if (width <= 0 || height <= 0 || width > 65535 || height > 65535) {
		throw new Error("Width/Height invalid.");
	}
	if (options.palette !== undefined) checkPalette(options.palette);

	const frameSize = width * height;
	const frameByteSize = frameSize * 4;
	const frameCount = getRgbaFrameCount(
		options.frames,
		frameByteSize,
		options.frameCount,
	);
	const delays = normalizeFrameDelays(options.delay, frameCount);
	const alphaThreshold = normalizeAlphaThreshold(options.alphaThreshold);
	const loop =
		options.loop === undefined || options.loop === null
			? null
			: checkedU16(options.loop, "Loop count invalid.");
	const backend = options.backend ?? "auto";
	const { nativeAddon, wasmCore } = resolveEncoderBackends(backend);
	const fastCompression = options.compression !== "balanced";
	const quantization = options.quantization ?? "quality";
	const paletteMode = options.paletteMode ?? "global";
	const delta = options.delta === true;
	if (paletteMode === "local" && options.palette !== undefined) {
		throw new Error(
			"Local palette mode cannot use a caller-supplied global palette.",
		);
	}
	if (paletteMode === "local" && delta) {
		throw new Error("Local palette mode does not support delta frames.");
	}
	const useAdvancedEncoder =
		paletteMode === "local" || quantization !== "exact";
	const encodedLoop = loop === null ? -1 : loop;
	const palette = paletteToUint32Array(options.palette ?? EMPTY_PALETTE);

	if (nativeAddon) {
		const rgbaFrames = flattenRgbaFrames(
			options.frames,
			frameByteSize,
			frameCount,
		);
		if (
			useAdvancedEncoder &&
			fastCompression &&
			quantization === "quality" &&
			paletteMode === "global" &&
			options.palette === undefined &&
			!delta
		) {
			return nativeAddon.encodeRgbaQuality(
				rgbaFrames,
				width,
				height,
				frameCount,
				delayArray(delays, frameCount),
				encodedLoop,
				alphaThreshold,
			);
		}
		if (!fastCompression && paletteMode === "global") {
			return nativeAddon.encodeRgbaBalanced(
				rgbaFrames,
				width,
				height,
				frameCount,
				palette,
				delayArray(delays, frameCount),
				encodedLoop,
				delta,
				alphaThreshold,
				quantizationCode(quantization),
			);
		}
		if (
			!useAdvancedEncoder &&
			fastCompression &&
			alphaThreshold === TRANSPARENT_ALPHA_THRESHOLD
		) {
			return nativeAddon.encodeRgbaFast(
				rgbaFrames,
				width,
				height,
				frameCount,
				palette,
				delayArray(delays, frameCount),
				encodedLoop,
				delta,
			);
		}
		if (backend === "native-addon") {
			throw new Error(
				"The native addon does not support the requested RGBA encoding mode.",
			);
		}
	}

	if (wasmCore) {
		if (useAdvancedEncoder) {
			return encodeRgbaAdvancedWithWasmScratch(
				wasmCore,
				options.frames,
				frameByteSize,
				width,
				height,
				frameCount,
				palette,
				delays,
				encodedLoop,
				delta,
				alphaThreshold,
				fastCompression,
				quantizationCode(quantization),
				paletteMode === "local" ? 1 : 0,
			);
		}
		const rgbaFrames = flattenRgbaFrames(
			options.frames,
			frameByteSize,
			frameCount,
		);
		if (
			fastCompression &&
			typeof delays === "number" &&
			alphaThreshold === TRANSPARENT_ALPHA_THRESHOLD
		) {
			const encode = delta
				? wasmCore.encode_rgba_literal_delta_gif
				: wasmCore.encode_rgba_literal_gif;
			return encode(
				rgbaFrames,
				width,
				height,
				frameCount,
				palette,
				delays,
				encodedLoop,
			);
		}
		if (fastCompression) {
			const encode = delta
				? wasmCore.encode_rgba_literal_delta_gif_with_options
				: wasmCore.encode_rgba_literal_gif_with_options;
			return encode(
				rgbaFrames,
				width,
				height,
				frameCount,
				palette,
				delayArray(delays, frameCount),
				encodedLoop,
				alphaThreshold,
			);
		}
		return wasmCore.encode_rgba_gif_with_options(
			rgbaFrames,
			width,
			height,
			frameCount,
			palette,
			delayArray(delays, frameCount),
			encodedLoop,
			delta,
			alphaThreshold,
		);
	}

	const rgbaFrames = flattenRgbaFrames(
		options.frames,
		frameByteSize,
		frameCount,
	);
	if (paletteMode === "local") {
		return encodeRgbaGifFramesLocalJavascript(
			rgbaFrames,
			width,
			height,
			frameCount,
			delays,
			loop,
			alphaThreshold,
			fastCompression,
			quantization,
		);
	}
	const {
		indexed,
		palette: indexedPalette,
		transparentIndex,
	} = indexRgbaFramesWithQuantizationJavascript(
		rgbaFrames,
		options.palette,
		alphaThreshold,
		quantization,
	);
	return encodeIndexedGifFramesJavascript(
		indexed,
		width,
		height,
		frameSize,
		frameCount,
		indexedPalette,
		delays,
		loop,
		delta,
		transparentIndex,
		fastCompression ? "fast" : "balanced",
	);
}

export class GifWriter {
	private p = 0;
	private ended = false;

	private loopCount: number | null;
	private globalPalette: PaletteRGB | null;
	private background = 0;
	private compression: GifCompressionMode;
	private globalColorCount = 0;
	private globalColorTableSizeBits = 0;
	private globalMinCodeSize = 0;
	private previousIndexedFrame: Uint8Array | null = null;

	constructor(
		private buf: GifBinary,
		public readonly width: number,
		public readonly height: number,
		gopts?: GifOptions,
	) {
		const go = gopts ?? {};
		this.loopCount = go.loop === undefined ? null : go.loop;
		this.globalPalette = go.palette === undefined ? null : go.palette;
		this.compression = go.compression ?? "fast";

		if (width <= 0 || height <= 0 || width > 65535 || height > 65535)
			throw new Error("Width/Height invalid.");

		this.buf[this.p++] = GIF.G;
		this.buf[this.p++] = GIF.I;
		this.buf[this.p++] = GIF.F;
		this.buf[this.p++] = GIF._8;
		this.buf[this.p++] = GIF._9;
		this.buf[this.p++] = GIF.A;

		let gpPow2Bits = 0;
		if (this.globalPalette !== null) {
			const n = checkPalette(this.globalPalette);
			this.globalColorCount = n;
			const pow = log2Pow2(n); // 1..8
			gpPow2Bits = (pow - 1) & 7; // 0..7 per spec
			this.globalColorTableSizeBits = gpPow2Bits;
			this.globalMinCodeSize = Math.max(2, pow);

			if (go.background !== undefined) {
				this.background = go.background | 0;
				if (this.background < 0 || this.background >= n)
					throw new Error("Background index out of range.");
			}
		}

		/* Logical Screen Descriptor */
		this.buf[this.p++] = width & 0xff;
		this.buf[this.p++] = (width >> 8) & 0xff;
		this.buf[this.p++] = height & 0xff;
		this.buf[this.p++] = (height >> 8) & 0xff;

		const gctFlag = this.globalPalette !== null ? 0x80 : 0x00;
		this.buf[this.p++] = gctFlag | gpPow2Bits;
		this.buf[this.p++] = this.background & 0xff; // background color index
		this.buf[this.p++] = 0; // pixel aspect ratio

		// Global Color Table
		if (this.globalPalette !== null) {
			for (let i = 0; i < this.globalColorCount; i++) {
				const rgb = (this.globalPalette[i] ?? 0) >>> 0;
				this.buf[this.p++] = (rgb >> 16) & 0xff;
				this.buf[this.p++] = (rgb >> 8) & 0xff;
				this.buf[this.p++] = rgb & 0xff;
			}
		}

		// Netscape loop count
		if (this.loopCount !== null) {
			const lc = this.loopCount | 0;
			if (lc < 0 || lc > 65535) throw new Error("Loop count invalid.");
			this.p = writeNetscapeLoopCount(this.buf, this.p, lc);
		}
	}

	addFrame(
		x: number,
		y: number,
		w: number,
		h: number,
		indexedPixels: IndexedGifFrame,
		opts?: FrameOptions,
	): number {
		x |= 0;
		y |= 0;
		w |= 0;
		h |= 0;

		if (x < 0 || y < 0 || x > 65535 || y > 65535)
			throw new Error("x/y invalid.");
		if (w <= 0 || h <= 0 || w > 65535 || h > 65535)
			throw new Error("Width/Height invalid.");
		if (indexedPixels.length < w * h)
			throw new Error("Not enough pixels for the frame size.");

		return this.writeFrame(
			x,
			y,
			w,
			h,
			indexedPixels,
			createIndexedSourceRect(indexedPixels, 0, w, h, w),
			opts,
		);
	}

	addFrameDelta(indexedPixels: IndexedGifFrame, opts?: FrameOptions): number {
		if (indexedPixels.length < this.width * this.height) {
			throw new Error("Not enough pixels for the frame size.");
		}

		if (opts?.palette !== undefined && opts.palette !== null) {
			return this.addFrame(0, 0, this.width, this.height, indexedPixels, opts);
		}

		const previous = this.previousIndexedFrame;
		if (!previous) {
			return this.addFrame(0, 0, this.width, this.height, indexedPixels, opts);
		}

		const rect = findChangedRect(
			previous,
			indexedPixels,
			this.width,
			this.height,
		);
		if (!rect) {
			const source = createIndexedSourceRect(
				indexedPixels,
				0,
				1,
				1,
				this.width,
			);
			return this.writeFrame(0, 0, 1, 1, source, source, opts);
		}

		if (rect.width === this.width && rect.height === this.height) {
			return this.addFrame(0, 0, this.width, this.height, indexedPixels, opts);
		}

		const source = createIndexedSourceRect(
			indexedPixels,
			rect.y * this.width + rect.x,
			rect.width,
			rect.height,
			this.width,
		);
		return this.writeFrame(
			rect.x,
			rect.y,
			rect.width,
			rect.height,
			source,
			source,
			opts,
		);
	}

	private writeFrame(
		x: number,
		y: number,
		w: number,
		h: number,
		lzwSource: LzwIndexStream,
		previousSource: IndexedSourceRect,
		opts?: FrameOptions,
	): number {
		if (this.ended) {
			this.p--;
			this.ended = false;
		} // un-end if user adds more frames

		if (indexedSourceLength(lzwSource) < w * h) {
			throw new Error("Not enough pixels for the frame size.");
		}

		const o = opts ?? {};
		let usingLocal = true;
		let palette: PaletteRGB | null | undefined = o.palette;
		if (palette == null) {
			usingLocal = false;
			palette = this.globalPalette;
		}
		if (palette == null)
			throw new Error("Must supply either a local or global palette.");

		const numColors = usingLocal
			? checkPalette(palette)
			: this.globalColorCount;
		const palettePower = usingLocal ? log2Pow2(numColors) : 0;
		const colorTableSizeBits = usingLocal
			? (palettePower - 1) & 7
			: this.globalColorTableSizeBits;
		const minCodeSize = usingLocal
			? Math.max(2, palettePower)
			: this.globalMinCodeSize;

		const delay = (o.delay ?? 0) | 0;
		const disposal = (o.disposal ?? 0) | 0;
		if (disposal < 0 || disposal > 3) throw new Error("Disposal out of range.");

		let useTrans = false;
		let transparentIndex = 0;
		if (o.transparent !== undefined && o.transparent !== null) {
			useTrans = true;
			transparentIndex = (o.transparent as number) | 0;
			if (transparentIndex < 0 || transparentIndex >= numColors)
				throw new Error("Transparent color index out of range.");
		}

		// Graphics Control Extension (only when needed)
		if (disposal !== 0 || useTrans || delay !== 0) {
			this.buf[this.p++] = GIF.EXT;
			this.buf[this.p++] = GIF.GCE;
			this.buf[this.p++] = 4; // block size
			this.buf[this.p++] = (disposal << 2) | (useTrans ? 1 : 0);
			this.buf[this.p++] = delay & 0xff;
			this.buf[this.p++] = (delay >> 8) & 0xff;
			this.buf[this.p++] = transparentIndex & 0xff;
			this.buf[this.p++] = 0; // terminator
		}

		// Image Descriptor
		this.buf[this.p++] = GIF.IMG;
		this.buf[this.p++] = x & 0xff;
		this.buf[this.p++] = (x >> 8) & 0xff;
		this.buf[this.p++] = y & 0xff;
		this.buf[this.p++] = (y >> 8) & 0xff;
		this.buf[this.p++] = w & 0xff;
		this.buf[this.p++] = (w >> 8) & 0xff;
		this.buf[this.p++] = h & 0xff;
		this.buf[this.p++] = (h >> 8) & 0xff;
		// local color table flag + size (no sort, non-interlaced)
		this.buf[this.p++] = usingLocal ? 0x80 | colorTableSizeBits : 0x00;

		if (usingLocal) {
			for (let i = 0; i < numColors; i++) {
				const rgb = (palette[i] ?? 0) >>> 0;
				this.buf[this.p++] = (rgb >> 16) & 0xff;
				this.buf[this.p++] = (rgb >> 8) & 0xff;
				this.buf[this.p++] = rgb & 0xff;
			}
		}

		this.p = GifWriterOutputLZWCodeStream_fast(
			this.buf,
			this.p,
			minCodeSize,
			lzwSource,
			numColors,
			this.compression === "fast",
		);

		this.updatePreviousIndexedFrame(
			x,
			y,
			w,
			h,
			previousSource,
			useTrans ? transparentIndex : null,
			disposal,
		);

		return this.p;
	}

	end(): number {
		if (!this.ended) {
			this.buf[this.p++] = GIF.TRAILER;
			this.ended = true;
		}
		return this.p;
	}
	getOutputBuffer(): GifBinary {
		return this.buf;
	}
	setOutputBuffer(v: GifBinary): void {
		this.buf = v;
	}
	getOutputBufferPosition(): number {
		return this.p;
	}
	setOutputBufferPosition(v: number) {
		this.p = v | 0;
	}

	private updatePreviousIndexedFrame(
		x: number,
		y: number,
		w: number,
		h: number,
		source: IndexedSourceRect,
		transparentIndex: number | null,
		disposal: number,
	): void {
		if (disposal === 2 || disposal === 3) {
			this.previousIndexedFrame = null;
			return;
		}

		if (!this.previousIndexedFrame) {
			this.previousIndexedFrame = new Uint8Array(this.width * this.height);
		}

		const previous = this.previousIndexedFrame;
		if (transparentIndex === null && source.data instanceof Uint8Array) {
			if (
				x === 0 &&
				y === 0 &&
				w === this.width &&
				h === this.height &&
				source.offset === 0 &&
				source.stride === this.width
			) {
				previous.set(source.data.subarray(0, this.width * this.height));
			} else {
				for (let row = 0; row < h; row++) {
					const src = source.offset + row * source.stride;
					const dst = (y + row) * this.width + x;
					previous.set(source.data.subarray(src, src + w), dst);
				}
			}
			return;
		}

		for (let row = 0; row < h; row++) {
			let src = source.offset + row * source.stride;
			let dst = (y + row) * this.width + x;
			for (let col = 0; col < w; col++) {
				const index = source.data[src++]! | 0;
				if (transparentIndex === null || index !== transparentIndex) {
					previous[dst] = index;
				}
				dst++;
			}
		}
	}
}

function encodeIndexedGifFramesJavascript(
	frames: IndexedGifFrames,
	width: number,
	height: number,
	frameSize: number,
	frameCount: number,
	palette: PaletteRGB,
	delays: NormalizedFrameDelays,
	loop: number | null,
	delta: boolean,
	transparentIndex: number | null = null,
	compression: GifCompressionMode = "balanced",
): Uint8Array {
	const colorCount = checkPalette(palette);
	const estimatedSize =
		13 + colorCount * 3 + 20 + frameCount * (frameSize * 2 + 32) + 1;
	const output = new Uint8Array(estimatedSize);
	const writer = new GifWriter(output, width, height, {
		palette,
		loop,
		compression,
	});
	for (let frameIndex = 0; frameIndex < frameCount; frameIndex++) {
		const frame = getIndexedFrame(frames, frameSize, frameIndex);
		const frameOptions = {
			delay: frameDelay(delays, frameIndex),
			disposal: delta || transparentIndex === null ? 0 : 2,
			...(transparentIndex === null ? {} : { transparent: transparentIndex }),
		};
		if (delta) {
			writer.addFrameDelta(frame, frameOptions);
		} else {
			writer.addFrame(0, 0, width, height, frame, frameOptions);
		}
	}
	return output.slice(0, writer.end());
}

function getIndexedFrameCount(
	frames: IndexedGifFrames,
	frameSize: number,
	frameCount: number | undefined,
): number {
	if (frames instanceof Uint8Array) {
		if (frameCount !== undefined) {
			const count = frameCount | 0;
			if (count <= 0 || frames.length !== frameSize * count) {
				throw new Error(
					"Indexed frame stream length does not match dimensions.",
				);
			}
			return count;
		}
		if (frames.length === 0 || frames.length % frameSize !== 0) {
			throw new Error("Indexed frame stream length does not match dimensions.");
		}
		return frames.length / frameSize;
	}

	if (frameCount !== undefined && frameCount !== frames.length) {
		throw new Error("Frame count does not match frames length.");
	}
	if (frames.length === 0) {
		throw new Error("Frame count must be greater than zero.");
	}
	for (const frame of frames) {
		if (frame.length < frameSize) {
			throw new Error("Not enough pixels for the frame size.");
		}
	}
	return frames.length;
}

function getRgbaFrameCount(
	frames: RgbaGifFrames,
	frameByteSize: number,
	frameCount: number | undefined,
): number {
	if (isRgbaFrame(frames)) {
		if (frameCount !== undefined) {
			const count = frameCount | 0;
			if (count <= 0 || frames.length !== frameByteSize * count) {
				throw new Error("RGBA frame stream length does not match dimensions.");
			}
			return count;
		}
		if (frames.length === 0 || frames.length % frameByteSize !== 0) {
			throw new Error("RGBA frame stream length does not match dimensions.");
		}
		return frames.length / frameByteSize;
	}

	if (frameCount !== undefined && frameCount !== frames.length) {
		throw new Error("Frame count does not match frames length.");
	}
	if (frames.length === 0) {
		throw new Error("Frame count must be greater than zero.");
	}
	for (const frame of frames) {
		if (frame.length < frameByteSize) {
			throw new Error("Not enough pixels for the frame size.");
		}
	}
	return frames.length;
}

function flattenIndexedFrames(
	frames: IndexedGifFrames,
	frameSize: number,
	frameCount: number,
	colorCount: number,
	validateUint8Frames = true,
): Uint8Array {
	if (frames instanceof Uint8Array) {
		if (validateUint8Frames) {
			validateIndexedFramePixels(frames, colorCount);
		}
		return frames;
	}

	const flatFrames = new Uint8Array(frameSize * frameCount);
	for (let frameIndex = 0; frameIndex < frameCount; frameIndex++) {
		const frame = frames[frameIndex]!;
		const dst = frameIndex * frameSize;
		if (frame instanceof Uint8Array) {
			if (validateUint8Frames) {
				validateIndexedFramePixels(frame.subarray(0, frameSize), colorCount);
			}
			flatFrames.set(frame.subarray(0, frameSize), dst);
			continue;
		}
		for (let i = 0; i < frameSize; i++) {
			const index = frame[i]! | 0;
			if (index >>> 0 >= colorCount) {
				throw new Error("Pixel index out of range.");
			}
			flatFrames[dst + i] = index;
		}
	}
	return flatFrames;
}

function flattenRgbaFrames(
	frames: RgbaGifFrames,
	frameByteSize: number,
	frameCount: number,
): Uint8Array {
	if (isRgbaFrame(frames)) {
		return asUint8Array(frames);
	}

	const flatFrames = new Uint8Array(frameByteSize * frameCount);
	for (let frameIndex = 0; frameIndex < frameCount; frameIndex++) {
		const frame = asUint8Array(frames[frameIndex]!);
		flatFrames.set(
			frame.subarray(0, frameByteSize),
			frameIndex * frameByteSize,
		);
	}
	return flatFrames;
}

function isRgbaFrame(value: RgbaGifFrames): value is RgbaGifFrame {
	return value instanceof Uint8Array || value instanceof Uint8ClampedArray;
}

function asUint8Array(frame: RgbaGifFrame): Uint8Array {
	return frame instanceof Uint8Array
		? frame
		: new Uint8Array(frame.buffer, frame.byteOffset, frame.byteLength);
}

function validateIndexedFramePixels(
	frame: Uint8Array,
	colorCount: number,
): void {
	for (let i = 0; i < frame.length; i++) {
		if (frame[i]! >= colorCount) {
			throw new Error("Pixel index out of range.");
		}
	}
}

function getIndexedFrame(
	frames: IndexedGifFrames,
	frameSize: number,
	frameIndex: number,
): IndexedGifFrame {
	if (frames instanceof Uint8Array) {
		const start = frameIndex * frameSize;
		return frames.subarray(start, start + frameSize);
	}
	return frames[frameIndex]!;
}

function quantizationCode(quantization: GifQuantizationMode): number {
	switch (quantization) {
		case "exact":
			return 0;
		case "fast":
			return 1;
		case "quality":
			return 2;
	}
}

function encodeRgbaGifFramesLocalJavascript(
	rgba: Uint8Array,
	width: number,
	height: number,
	frameCount: number,
	delays: NormalizedFrameDelays,
	loop: number | null,
	alphaThreshold: number,
	fastCompression: boolean,
	quantization: GifQuantizationMode,
): Uint8Array {
	const frameSize = width * height;
	const frameByteSize = frameSize * 4;
	const estimatedSize = 13 + 20 + frameCount * (frameSize * 2 + 800) + 1;
	const output = new Uint8Array(estimatedSize);
	const writer = new GifWriter(output, width, height, {
		loop,
		compression: fastCompression ? "fast" : "balanced",
	});
	for (let frameIndex = 0; frameIndex < frameCount; frameIndex++) {
		const start = frameIndex * frameByteSize;
		const frame = rgba.subarray(start, start + frameByteSize);
		const { indexed, palette, transparentIndex } =
			indexRgbaFramesWithQuantizationJavascript(
				frame,
				undefined,
				alphaThreshold,
				quantization,
			);
		writer.addFrame(0, 0, width, height, indexed, {
			palette,
			delay: frameDelay(delays, frameIndex),
			disposal: 2,
			...(transparentIndex === null ? {} : { transparent: transparentIndex }),
		});
	}
	return output.slice(0, writer.end());
}

function indexRgbaFramesWithQuantizationJavascript(
	rgba: Uint8Array,
	palette: PaletteRGB | undefined,
	alphaThreshold: number,
	quantization: GifQuantizationMode,
): {
	indexed: Uint8Array;
	palette: PaletteRGB;
	transparentIndex: number | null;
} {
	const hasTransparentPixels = rgbaHasTransparentPixels(rgba, alphaThreshold);
	if (palette !== undefined) {
		return indexRgbaFramesToPaletteJavascript(
			rgba,
			palette,
			hasTransparentPixels,
			alphaThreshold,
			quantization === "exact",
		);
	}

	const exact = tryIndexRgbaFramesExactJavascript(
		rgba,
		hasTransparentPixels,
		alphaThreshold,
		quantization === "exact",
	);
	if (exact) {
		return exact;
	}
	if (quantization === "exact") {
		throw new Error(
			"Exact GIF quantization requires at most 256 colors and binary alpha.",
		);
	}
	return quantization === "quality"
		? indexRgbaFramesMedianCutJavascript(
				rgba,
				hasTransparentPixels,
				alphaThreshold,
			)
		: indexRgbaFrames332Javascript(rgba, hasTransparentPixels, alphaThreshold);
}

function tryIndexRgbaFramesExactJavascript(
	rgba: Uint8Array,
	hasTransparentPixels: boolean,
	alphaThreshold: number,
	exactOnly: boolean,
): {
	indexed: Uint8Array;
	palette: PaletteRGB;
	transparentIndex: number | null;
} | null {
	const palette: number[] = [];
	const colorToIndex = new Map<number, number>();
	const indexed = new Uint8Array(rgba.length >> 2);

	if (!hasTransparentPixels) {
		let output = 0;
		for (let offset = 0; offset < rgba.length; offset += 4) {
			if (exactOnly && rgba[offset + 3] !== 255) {
				throw new Error(
					"Exact GIF quantization requires alpha values of exactly 0 or 255.",
				);
			}
			const color = rgbKey(rgba[offset]!, rgba[offset + 1]!, rgba[offset + 2]!);
			let index = colorToIndex.get(color);
			if (index === undefined) {
				if (palette.length === 256) {
					return null;
				}
				index = palette.length;
				colorToIndex.set(color, index);
				palette.push(color);
			}
			indexed[output++] = index;
		}
		return { indexed, palette, transparentIndex: null };
	}

	for (let offset = 0; offset < rgba.length; offset += 4) {
		if (exactOnly && rgba[offset + 3] !== 0 && rgba[offset + 3] !== 255) {
			throw new Error(
				"Exact GIF quantization requires alpha values of exactly 0 or 255.",
			);
		}
		if (rgba[offset + 3]! < alphaThreshold) {
			continue;
		}

		const color = rgbKey(rgba[offset]!, rgba[offset + 1]!, rgba[offset + 2]!);
		let index = colorToIndex.get(color);
		if (index === undefined) {
			if (palette.length === (hasTransparentPixels ? 255 : 256)) {
				return null;
			}
			index = palette.length;
			colorToIndex.set(color, index);
			palette.push(color);
		}
	}

	if (hasTransparentPixels && palette.length === 256) {
		return null;
	}

	const transparentIndex = hasTransparentPixels ? palette.length : null;
	if (transparentIndex !== null) {
		palette.push(0);
	}

	let output = 0;
	for (let offset = 0; offset < rgba.length; offset += 4) {
		if (rgba[offset + 3]! < alphaThreshold) {
			indexed[output++] = transparentIndex!;
			continue;
		}
		const color = rgbKey(rgba[offset]!, rgba[offset + 1]!, rgba[offset + 2]!);
		indexed[output++] = colorToIndex.get(color)!;
	}

	return { indexed, palette, transparentIndex };
}

function indexRgbaFramesToPaletteJavascript(
	rgba: Uint8Array,
	palette: PaletteRGB,
	hasTransparentPixels: boolean,
	alphaThreshold: number,
	exactOnly: boolean,
): {
	indexed: Uint8Array;
	palette: PaletteRGB;
	transparentIndex: number | null;
} {
	const exact = new Map<number, number>();
	for (let index = 0; index < palette.length; index++) {
		const color = (palette[index] ?? 0) & 0x00ff_ffff;
		if (!exact.has(color)) {
			exact.set(color, index);
		}
	}

	if (!hasTransparentPixels) {
		const indexed = new Uint8Array(rgba.length >> 2);
		let output = 0;
		for (let offset = 0; offset < rgba.length; offset += 4) {
			if (exactOnly && rgba[offset + 3] !== 255) {
				throw new Error(
					"Exact GIF quantization requires alpha values of exactly 0 or 255.",
				);
			}
			const r = rgba[offset]!;
			const g = rgba[offset + 1]!;
			const b = rgba[offset + 2]!;
			const color = rgbKey(r, g, b);
			const exactIndex = exact.get(color);
			if (exactOnly && exactIndex === undefined) {
				throw new Error(
					"Exact GIF quantization found an RGBA color outside the supplied palette.",
				);
			}
			indexed[output++] = exactIndex ?? nearestPaletteIndex(r, g, b, palette);
		}
		return { indexed, palette, transparentIndex: null };
	}

	const usedIndexes = new Set<number>();
	for (let offset = 0; offset < rgba.length; offset += 4) {
		if (exactOnly && rgba[offset + 3] !== 0 && rgba[offset + 3] !== 255) {
			throw new Error(
				"Exact GIF quantization requires alpha values of exactly 0 or 255.",
			);
		}
		if (rgba[offset + 3]! < alphaThreshold) {
			continue;
		}
		const r = rgba[offset]!;
		const g = rgba[offset + 1]!;
		const b = rgba[offset + 2]!;
		const color = rgbKey(r, g, b);
		const exactIndex = exact.get(color);
		if (exactOnly && exactIndex === undefined) {
			throw new Error(
				"Exact GIF quantization found an RGBA color outside the supplied palette.",
			);
		}
		usedIndexes.add(exactIndex ?? nearestPaletteIndex(r, g, b, palette));
	}

	let outputPalette = palette;
	let transparentIndex: number | null = null;
	if (hasTransparentPixels) {
		if (palette.length < 256) {
			outputPalette = palette.slice();
			transparentIndex = outputPalette.length;
			outputPalette.push(0);
		} else {
			for (let index = 0; index < palette.length; index++) {
				if (!usedIndexes.has(index)) {
					transparentIndex = index;
					break;
				}
			}
			if (transparentIndex === null) {
				throw new Error(
					"RGBA frames contain transparent pixels, but the palette has no unused transparent slot.",
				);
			}
		}
	}

	const indexed = new Uint8Array(rgba.length >> 2);
	let output = 0;
	for (let offset = 0; offset < rgba.length; offset += 4) {
		if (rgba[offset + 3]! < alphaThreshold) {
			indexed[output++] = transparentIndex!;
			continue;
		}
		const r = rgba[offset]!;
		const g = rgba[offset + 1]!;
		const b = rgba[offset + 2]!;
		const color = rgbKey(r, g, b);
		const exactIndex = exact.get(color);
		if (exactOnly && exactIndex === undefined) {
			throw new Error(
				"Exact GIF quantization found an RGBA color outside the supplied palette.",
			);
		}
		indexed[output++] = exactIndex ?? nearestPaletteIndex(r, g, b, palette);
	}

	return { indexed, palette: outputPalette, transparentIndex };
}

interface JavascriptQuantizedColor {
	histogramIndex: number;
	count: number;
	red: number;
	green: number;
	blue: number;
}

interface JavascriptQuantizedColorBox {
	colors: JavascriptQuantizedColor[];
	weight: number;
	redRange: number;
	greenRange: number;
	blueRange: number;
}

const QUALITY_HISTOGRAM_BITS = 5;
const QUALITY_HISTOGRAM_LENGTH = 1 << (QUALITY_HISTOGRAM_BITS * 3);

function qualityHistogramIndex(
	red: number,
	green: number,
	blue: number,
): number {
	return (
		((red >> (8 - QUALITY_HISTOGRAM_BITS)) << (QUALITY_HISTOGRAM_BITS * 2)) |
		((green >> (8 - QUALITY_HISTOGRAM_BITS)) << QUALITY_HISTOGRAM_BITS) |
		(blue >> (8 - QUALITY_HISTOGRAM_BITS))
	);
}

function createJavascriptColorBox(
	colors: JavascriptQuantizedColor[],
): JavascriptQuantizedColorBox {
	let weight = 0;
	let minRed = 255;
	let minGreen = 255;
	let minBlue = 255;
	let maxRed = 0;
	let maxGreen = 0;
	let maxBlue = 0;
	for (const color of colors) {
		weight += color.count;
		minRed = Math.min(minRed, color.red);
		minGreen = Math.min(minGreen, color.green);
		minBlue = Math.min(minBlue, color.blue);
		maxRed = Math.max(maxRed, color.red);
		maxGreen = Math.max(maxGreen, color.green);
		maxBlue = Math.max(maxBlue, color.blue);
	}
	return {
		colors,
		weight,
		redRange: colors.length === 0 ? 0 : maxRed - minRed,
		greenRange: colors.length === 0 ? 0 : maxGreen - minGreen,
		blueRange: colors.length === 0 ? 0 : maxBlue - minBlue,
	};
}

function splitJavascriptColorBox(
	colorBox: JavascriptQuantizedColorBox,
): [JavascriptQuantizedColorBox, JavascriptQuantizedColorBox] | null {
	if (colorBox.colors.length < 2) {
		return null;
	}
	const colors = colorBox.colors.slice();
	if (
		colorBox.redRange >= colorBox.greenRange &&
		colorBox.redRange >= colorBox.blueRange
	) {
		colors.sort((left, right) => left.red - right.red);
	} else if (colorBox.greenRange >= colorBox.blueRange) {
		colors.sort((left, right) => left.green - right.green);
	} else {
		colors.sort((left, right) => left.blue - right.blue);
	}
	const midpoint = Math.ceil(colorBox.weight / 2);
	let accumulated = 0;
	let splitIndex = 1;
	for (let index = 0; index < colors.length; index++) {
		accumulated += colors[index]!.count;
		if (accumulated >= midpoint) {
			splitIndex = Math.min(index + 1, colors.length - 1);
			break;
		}
	}
	return [
		createJavascriptColorBox(colors.slice(0, splitIndex)),
		createJavascriptColorBox(colors.slice(splitIndex)),
	];
}

function javascriptColorBoxRepresentative(
	colorBox: JavascriptQuantizedColorBox,
): number {
	let red = 0;
	let green = 0;
	let blue = 0;
	let count = 0;
	for (const color of colorBox.colors) {
		red += color.red * color.count;
		green += color.green * color.count;
		blue += color.blue * color.count;
		count += color.count;
	}
	if (count === 0) {
		return 0;
	}
	return (
		(Math.round(red / count) << 16) |
		(Math.round(green / count) << 8) |
		Math.round(blue / count)
	);
}

function indexRgbaFramesMedianCutJavascript(
	rgba: Uint8Array,
	hasTransparentPixels: boolean,
	alphaThreshold: number,
): {
	indexed: Uint8Array;
	palette: PaletteRGB;
	transparentIndex: number | null;
} {
	const counts = new Float64Array(QUALITY_HISTOGRAM_LENGTH);
	const redSums = new Float64Array(QUALITY_HISTOGRAM_LENGTH);
	const greenSums = new Float64Array(QUALITY_HISTOGRAM_LENGTH);
	const blueSums = new Float64Array(QUALITY_HISTOGRAM_LENGTH);
	for (let offset = 0; offset < rgba.length; offset += 4) {
		if (rgba[offset + 3]! < alphaThreshold) {
			continue;
		}
		const histogramIndex = qualityHistogramIndex(
			rgba[offset]!,
			rgba[offset + 1]!,
			rgba[offset + 2]!,
		);
		counts[histogramIndex]! += 1;
		redSums[histogramIndex]! += rgba[offset]!;
		greenSums[histogramIndex]! += rgba[offset + 1]!;
		blueSums[histogramIndex]! += rgba[offset + 2]!;
	}

	const colors: JavascriptQuantizedColor[] = [];
	for (
		let histogramIndex = 0;
		histogramIndex < QUALITY_HISTOGRAM_LENGTH;
		histogramIndex++
	) {
		const count = counts[histogramIndex]!;
		if (count === 0) {
			continue;
		}
		colors.push({
			histogramIndex,
			count,
			red: Math.round(redSums[histogramIndex]! / count),
			green: Math.round(greenSums[histogramIndex]! / count),
			blue: Math.round(blueSums[histogramIndex]! / count),
		});
	}

	const opaqueColorLimit = hasTransparentPixels ? 255 : 256;
	const boxes = colors.length === 0 ? [] : [createJavascriptColorBox(colors)];
	while (boxes.length < opaqueColorLimit) {
		let splitIndex = -1;
		let bestScore = -1;
		for (let index = 0; index < boxes.length; index++) {
			const colorBox = boxes[index]!;
			if (colorBox.colors.length < 2) {
				continue;
			}
			const range = Math.max(
				colorBox.redRange,
				colorBox.greenRange,
				colorBox.blueRange,
			);
			const score = colorBox.weight * range * range;
			if (score > bestScore) {
				bestScore = score;
				splitIndex = index;
			}
		}
		if (splitIndex < 0) {
			break;
		}
		const split = splitJavascriptColorBox(boxes[splitIndex]!);
		if (!split) {
			break;
		}
		boxes.splice(splitIndex, 1, split[0], split[1]);
	}
	boxes.sort(
		(left, right) =>
			javascriptColorBoxRepresentative(left) -
			javascriptColorBoxRepresentative(right),
	);

	const palette: number[] = [];
	const histogramToPalette = new Uint8Array(QUALITY_HISTOGRAM_LENGTH);
	for (let paletteIndex = 0; paletteIndex < boxes.length; paletteIndex++) {
		const colorBox = boxes[paletteIndex]!;
		palette.push(javascriptColorBoxRepresentative(colorBox));
		for (const color of colorBox.colors) {
			histogramToPalette[color.histogramIndex] = paletteIndex;
		}
	}
	const transparentIndex = hasTransparentPixels ? palette.length : null;
	if (transparentIndex !== null) {
		palette.push(0);
	}
	if (palette.length === 0) {
		palette.push(0);
	}

	const indexed = new Uint8Array(rgba.length >> 2);
	for (let offset = 0, output = 0; offset < rgba.length; offset += 4) {
		indexed[output++] =
			rgba[offset + 3]! < alphaThreshold
				? (transparentIndex ?? 0)
				: histogramToPalette[
						qualityHistogramIndex(
							rgba[offset]!,
							rgba[offset + 1]!,
							rgba[offset + 2]!,
						)
					]!;
	}
	return { indexed, palette, transparentIndex };
}

function indexRgbaFrames332Javascript(
	rgba: Uint8Array,
	hasTransparentPixels: boolean,
	alphaThreshold: number,
): {
	indexed: Uint8Array;
	palette: PaletteRGB;
	transparentIndex: number | null;
} {
	const indexed = new Uint8Array(rgba.length >> 2);
	let output = 0;
	if (hasTransparentPixels) {
		const transparentIndex = 255;
		for (let offset = 0; offset < rgba.length; offset += 4) {
			indexed[output++] =
				rgba[offset + 3]! < alphaThreshold
					? transparentIndex
					: Math.min(
							rgb332Index(rgba[offset]!, rgba[offset + 1]!, rgba[offset + 2]!),
							transparentIndex - 1,
						);
		}
		const palette = fixed332Palette();
		palette[transparentIndex] = 0;
		return { indexed, palette, transparentIndex };
	}

	for (let offset = 0; offset < rgba.length; offset += 4) {
		indexed[output++] = rgb332Index(
			rgba[offset]!,
			rgba[offset + 1]!,
			rgba[offset + 2]!,
		);
	}
	return { indexed, palette: fixed332Palette(), transparentIndex: null };
}

function rgbaHasTransparentPixels(
	rgba: Uint8Array,
	alphaThreshold: number,
): boolean {
	for (let offset = 3; offset < rgba.length; offset += 4) {
		if (rgba[offset]! < alphaThreshold) {
			return true;
		}
	}
	return false;
}

function rgbKey(r: number, g: number, b: number): number {
	return ((r & 0xff) << 16) | ((g & 0xff) << 8) | (b & 0xff);
}

function rgb332Index(r: number, g: number, b: number): number {
	return (r & 0xe0) | ((g >> 3) & 0x1c) | (b >> 6);
}

function fixed332Palette(): PaletteRGB {
	const palette = new Array<number>(256);
	for (let index = 0; index < 256; index++) {
		const r = ((((index >> 5) & 7) * 255 + 3) / 7) | 0;
		const g = ((((index >> 2) & 7) * 255 + 3) / 7) | 0;
		const b = (((index & 3) * 255 + 1) / 3) | 0;
		palette[index] = (r << 16) | (g << 8) | b;
	}
	return palette;
}

function nearestPaletteIndex(
	r: number,
	g: number,
	b: number,
	palette: PaletteRGB,
): number {
	let bestIndex = 0;
	let bestDistance = Number.MAX_SAFE_INTEGER;
	for (let index = 0; index < palette.length; index++) {
		const color = (palette[index] ?? 0) >>> 0;
		const dr = r - ((color >> 16) & 0xff);
		const dg = g - ((color >> 8) & 0xff);
		const db = b - (color & 0xff);
		const distance = dr * dr + dg * dg + db * db;
		if (distance < bestDistance) {
			bestDistance = distance;
			bestIndex = index;
			if (distance === 0) break;
		}
	}
	return bestIndex;
}

function paletteToUint32Array(palette: PaletteRGB): Uint32Array {
	if (palette.length === 0) {
		return EMPTY_PALETTE_UINT32;
	}
	const paletteData = new Uint32Array(palette.length);
	for (let i = 0; i < palette.length; i++) {
		paletteData[i] = (palette[i] ?? 0) >>> 0;
	}
	return paletteData;
}

function checkedU16(value: number, message: string): number {
	const checked = value | 0;
	if (checked < 0 || checked > 65535) {
		throw new Error(message);
	}
	return checked;
}

type NormalizedFrameDelays = number | Uint16Array;

function normalizeFrameDelays(
	delay: GifFrameDelay | undefined,
	frameCount: number,
): NormalizedFrameDelays {
	if (delay === undefined) {
		return 0;
	}
	if (typeof delay === "number") {
		return checkedU16(delay, "Delay invalid.");
	}
	if (delay.length !== frameCount) {
		throw new Error("Delay count does not match frame count.");
	}
	const normalized = new Uint16Array(frameCount);
	for (let i = 0; i < frameCount; i++) {
		normalized[i] = checkedU16(delay[i] ?? 0, "Delay invalid.");
	}
	return normalized;
}

function delayArray(
	delays: NormalizedFrameDelays,
	frameCount: number,
): Uint16Array {
	if (delays instanceof Uint16Array) {
		return delays;
	}
	if (
		uniformDelayCacheValue !== delays ||
		uniformDelayCache.length < frameCount
	) {
		uniformDelayCache = new Uint16Array(frameCount);
		uniformDelayCache.fill(delays);
		uniformDelayCacheValue = delays;
	}
	return uniformDelayCache.subarray(0, frameCount);
}

function frameDelay(delays: NormalizedFrameDelays, frameIndex: number): number {
	return delays instanceof Uint16Array ? delays[frameIndex]! : delays;
}

function normalizeAlphaThreshold(alphaThreshold: number | undefined): number {
	return checkedU8(
		alphaThreshold ?? TRANSPARENT_ALPHA_THRESHOLD,
		"Alpha threshold invalid.",
	);
}

function checkedU8(value: number, message: string): number {
	const checked = value | 0;
	if (checked < 0 || checked > 255) {
		throw new Error(message);
	}
	return checked;
}

function createIndexedSourceRect(
	data: IndexedGifFrame,
	offset: number,
	width: number,
	height: number,
	stride: number,
): IndexedSourceRect {
	offset |= 0;
	width |= 0;
	height |= 0;
	stride |= 0;

	if (offset < 0 || width <= 0 || height <= 0 || stride < width) {
		throw new Error("Not enough pixels for the frame size.");
	}
	const lastPixelEnd = offset + (height - 1) * stride + width;
	if (lastPixelEnd > data.length) {
		throw new Error("Not enough pixels for the frame size.");
	}
	return {
		data,
		offset,
		width,
		height,
		stride,
		length: width * height,
	};
}

function isIndexedSourceRect(
	indexStream: LzwIndexStream,
): indexStream is IndexedSourceRect {
	return !(indexStream instanceof Uint8Array) && !Array.isArray(indexStream);
}

function indexedSourceLength(indexStream: LzwIndexStream): number {
	return indexStream.length | 0;
}

/* ===== Encoder internals: typed-array hash + tight bit packer ===== */

/** Emit LZW code stream for indexed pixels. */
function GifWriterOutputLZWCodeStream_fast(
	buf: GifBinary,
	p0: number,
	minCodeSize: number,
	indexStream: LzwIndexStream,
	colorCount: number,
	fastCompression: boolean,
): number {
	const wasmEncoded = tryEncodeLzwWithWasm(
		minCodeSize,
		indexStream,
		colorCount,
		fastCompression,
	);
	if (wasmEncoded) {
		if (buf instanceof Uint8Array) {
			buf.set(wasmEncoded, p0);
			return p0 + wasmEncoded.length;
		}
		if (Array.isArray(buf)) {
			buf.length = p0 + wasmEncoded.length;
		}
		for (let i = 0; i < wasmEncoded.length; i++) {
			buf[p0 + i] = wasmEncoded[i]!;
		}
		return p0 + wasmEncoded.length;
	}

	let p = p0;

	// Write LZW min code size and set up first sub-block
	buf[p++] = minCodeSize & 0xff;
	let subLenPos = p++; // reserve length
	let subLen = 0;

	const CLEAR = 1 << minCodeSize;
	const EOI = CLEAR + 1;
	let nextCode = EOI + 1;
	let codeSize = minCodeSize + 1;
	let codeMask = (1 << codeSize) - 1;

	// Bit buffer
	let bits = 0 >>> 0;
	let bitCount = 0;

	// ---- typed-array open addressing hash ----
	// Key space is 20 bits: (prefix<<8)|k, values up to 12 bits.
	// Capacity: power-of-two above the 4096-code GIF dictionary keeps probe
	// counts low on patterned frames while still fitting comfortably in cache.
	const CAP = 16384;
	const cachedKeys = GifWriterOutputLZWCodeStream_fast._keys;
	const keys = cachedKeys ?? new Int32Array(CAP);
	if (!cachedKeys) {
		GifWriterOutputLZWCodeStream_fast._keys = keys;
	}
	const cachedVals = GifWriterOutputLZWCodeStream_fast._vals;
	const vals = cachedVals ?? new Int16Array(CAP);
	if (!cachedVals) {
		GifWriterOutputLZWCodeStream_fast._vals = vals;
	}
	const cachedGen = GifWriterOutputLZWCodeStream_fast._gen;
	const gen = cachedGen ?? new Int32Array(CAP);
	if (!cachedGen) {
		GifWriterOutputLZWCodeStream_fast._gen = gen;
	}
	let epoch = ((GifWriterOutputLZWCodeStream_fast._epoch ?? 0) + 1) | 0;
	GifWriterOutputLZWCodeStream_fast._epoch = epoch;
	if (epoch <= 0) {
		gen.fill(0);
		epoch = 1;
		GifWriterOutputLZWCodeStream_fast._epoch = epoch;
	}
	let EPOCH = epoch;

	function tableReset() {
		// Instead of clearing arrays, bump generation.
		EPOCH = (EPOCH + 1) | 0;
		if (EPOCH <= 0) {
			gen.fill(0);
			EPOCH = 1;
		}
	}

	function emit(code: number) {
		bits |= (code & 0xffff) << bitCount;
		bitCount += codeSize;
		while (bitCount >= 8) {
			buf[p++] = bits & 0xff;
			bits >>>= 8;
			bitCount -= 8;
			// sub-block manage
			if (++subLen === 255) {
				buf[subLenPos] = 255;
				subLenPos = p++;
				subLen = 0;
			}
		}
	}

	// Ensure fresh dictionary per image
	tableReset();

	// Emit initial clear
	emit(CLEAR);

	const n = indexedSourceLength(indexStream);
	if (n <= 0) throw new Error("Not enough pixels for the frame size.");

	let stridedData: IndexedGifFrame | null = null;
	let stridedIndex = 0;
	let stridedRowRemaining = 0;
	let stridedRowWidth = 0;
	let stridedRowSkip = 0;
	let ib: number;
	if (isIndexedSourceRect(indexStream)) {
		stridedData = indexStream.data;
		stridedIndex = indexStream.offset + 1;
		stridedRowWidth = indexStream.width;
		stridedRowRemaining = indexStream.width - 1;
		stridedRowSkip = indexStream.stride - indexStream.width;
		ib = (stridedData[indexStream.offset] as number) | 0;
	} else {
		ib = ((indexStream as IndexedGifFrame)[0] as number) | 0;
	}
	if (ib >>> 0 >= colorCount) throw new Error("Pixel index out of range.");

	for (let i = 1; i < n; i++) {
		let k: number;
		if (stridedData !== null) {
			if (stridedRowRemaining === 0) {
				stridedIndex += stridedRowSkip;
				stridedRowRemaining = stridedRowWidth;
			}
			k = (stridedData[stridedIndex++] as number) | 0;
			stridedRowRemaining--;
		} else {
			k = ((indexStream as IndexedGifFrame)[i] as number) | 0;
		}
		if (k >>> 0 >= colorCount) throw new Error("Pixel index out of range.");
		const key = (ib << 8) | k;
		let slot = Math.imul(key, -1640531527) >>> 18;
		let found = -1;
		while (gen[slot] === EPOCH) {
			if (keys[slot] === key) {
				found = vals[slot]! | 0;
				break;
			}
			slot = (slot + 1) & (CAP - 1);
		}
		if (found >= 0 && found < nextCode) {
			ib = found;
			continue;
		}

		// emit buffer
		emit(ib);

		if (nextCode === GIF.MAX_CODE) {
			// Clear
			emit(CLEAR);
			nextCode = EOI + 1;
			codeSize = (minCodeSize + 1) | 0;
			codeMask = (1 << codeSize) - 1;
			tableReset();
		} else {
			if (minCodeSize === 1) {
				// For 1-bit palettes, codes 0 and 1 use 2-bit code size. We should
				// only increase code size after adding a new dictionary entry.
				gen[slot] = EPOCH;
				keys[slot] = key | 0;
				vals[slot] = nextCode | 0;
				nextCode++;
				if (nextCode > codeMask && codeSize < 12) {
					codeSize++;
					codeMask = (1 << codeSize) - 1;
				}
			} else {
				// For larger palettes, match omggif's timing by growing the code size
				// before inserting the entry that would overflow the current mask.
				if (nextCode >= codeMask + 1 && codeSize < 12) {
					codeSize++;
					codeMask = (1 << codeSize) - 1;
				}
				gen[slot] = EPOCH;
				keys[slot] = key | 0;
				vals[slot] = nextCode | 0;
				nextCode++;
			}
		}

		ib = k;
	}

	emit(ib);
	emit(EOI);

	// flush remaining bits
	if (bitCount > 0) {
		buf[p++] = bits & 0xff;
		if (++subLen === 255) {
			buf[subLenPos] = 255;
			subLenPos = p++;
			subLen = 0;
		}
		bits = 0;
		bitCount = 0;
	}

	// finalize sub-blocks
	buf[subLenPos] = subLen & 0xff;
	// Only write a terminating zero-length block if the last sub-block
	// actually contained data. Otherwise the zero-length at subLenPos is
	// itself the terminator (avoids emitting an extra empty block).
	if (subLen > 0) {
		buf[p++] = 0; // terminator
	}

	return p;
}
namespace GifWriterOutputLZWCodeStream_fast {
	export let _keys: Int32Array | undefined;
	export let _vals: Int16Array | undefined;
	export let _gen: Int32Array | undefined;
	export let _epoch: number | undefined;
}

function encodeRgbaAdvancedWithWasmScratch(
	wasmCore: WasmCoreModule | WasmEncodeCoreModule,
	frames: RgbaGifFrames,
	frameByteSize: number,
	width: number,
	height: number,
	frameCount: number,
	palette: Uint32Array,
	delays: NormalizedFrameDelays,
	loop: number,
	deltas: boolean,
	alphaThreshold: number,
	literal: boolean,
	quantization: number,
	paletteMode: number,
): Uint8Array {
	const wasmMemory = copyRgbaFramesToWasmScratch(
		wasmCore,
		frames,
		frameByteSize,
		frameCount,
	);
	if (!wasmMemory) {
		throw new Error("WebAssembly encoder memory is unavailable.");
	}
	const getDelayArray = () => delayArray(delays, frameCount);
	const inputLength = frameByteSize * frameCount;
	const outputLength = wasmCore.encode_rgba_gif_advanced_scratch_from_input(
		inputLength,
		width,
		height,
		frameCount,
		palette,
		getDelayArray(),
		loop,
		deltas,
		alphaThreshold,
		literal,
		quantization,
		paletteMode,
	);
	return new Uint8Array(
		wasmMemory.buffer,
		wasmCore.gif_output_scratch_ptr(),
		outputLength,
	).slice();
}

function tryEncodeLzwWithWasm(
	minCodeSize: number,
	indexStream: LzwIndexStream,
	colorCount: number,
	fastCompression: boolean,
): Uint8Array | null {
	if (!(indexStream instanceof Uint8Array)) {
		return null;
	}
	if (indexStream.length < WASM_LZW_MIN_INDEX_COUNT) {
		return null;
	}

	const wasmCore = getEncoderWasmCoreModule();
	if (!wasmCore) return null;
	// Fixed-width 8-bit literals beat tiny bit-packed streams in the fast
	// Wasm path through 64 colors. GIF permits a larger minimum code size than
	// the palette needs, and the wider direct writer is substantially cheaper.
	const wasmMinCodeSize = fastCompression && minCodeSize <= 6 ? 7 : minCodeSize;
	const encodeIntoScratch = fastCompression
		? wasmCore.encode_indexed_literal_lzw_scratch
		: wasmCore.encode_indexed_lzw_scratch;
	const canUseDirectInput = minCodeSize > 4;
	prepareWasmEncoderModule(wasmCore);
	const scratchMemory = lzwScratchMemory;
	if (!scratchMemory) return null;
	let length: number;
	if (canUseDirectInput) {
		if (lzwInputScratchCapacity < indexStream.length) {
			lzwInputScratchPointer = wasmCore.indexed_lzw_input_scratch_reserve(
				indexStream.length,
			);
			lzwInputScratchCapacity = indexStream.length;
		}
		new Uint8Array(
			scratchMemory.buffer,
			lzwInputScratchPointer,
			indexStream.length,
		).set(indexStream);
		length = wasmCore.encode_indexed_lzw_scratch_from_input(
			indexStream.length,
			wasmMinCodeSize,
			colorCount,
			fastCompression,
		);
	} else {
		length = encodeIntoScratch(indexStream, wasmMinCodeSize, colorCount);
	}
	return new Uint8Array(
		scratchMemory.buffer,
		wasmCore.indexed_lzw_scratch_ptr(),
		length,
	);
}

function encodeIndexedGifWithWasmScratch(
	wasmCore: WasmCoreModule | WasmEncodeCoreModule,
	indexStream: Uint8Array,
	width: number,
	height: number,
	frameCount: number,
	palette: Uint32Array,
	delay: number,
	loopCount: number,
): Uint8Array {
	prepareWasmEncoderModule(wasmCore);
	const scratchMemory = lzwScratchMemory;
	if (!scratchMemory) {
		throw new Error("WebAssembly encoder memory is unavailable.");
	}
	if (lzwInputScratchCapacity < indexStream.length) {
		lzwInputScratchPointer = wasmCore.indexed_lzw_input_scratch_reserve(
			indexStream.length,
		);
		lzwInputScratchCapacity = indexStream.length;
	}
	new Uint8Array(
		scratchMemory.buffer,
		lzwInputScratchPointer,
		indexStream.length,
	).set(indexStream);
	const outputLength = wasmCore.encode_indexed_literal_gif_scratch_from_input(
		indexStream.length,
		width,
		height,
		frameCount,
		palette,
		delay,
		loopCount,
	);
	return new Uint8Array(
		scratchMemory.buffer,
		wasmCore.gif_output_scratch_ptr(),
		outputLength,
	).slice();
}
