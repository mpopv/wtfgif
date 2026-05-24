import { PaletteRGB } from "../types";
import { GIF } from "../constants/gif";
import { log2Pow2, checkPalette } from "../utils/palette";
import { writeNetscapeLoopCount } from "../utils/netscape";
import { getWasmCoreModule } from "../wasm/runtime";

const WASM_LZW_MIN_INDEX_COUNT = 8192;

export type IndexedGifFrame = Uint8Array | number[];
export type IndexedGifFrames = Uint8Array | IndexedGifFrame[];
export type EncodeIndexedGifFramesBackend = "auto" | "javascript" | "native";
export type RgbaGifFrame = Uint8Array | Uint8ClampedArray;
export type RgbaGifFrames = Uint8Array | Uint8ClampedArray | RgbaGifFrame[];

export interface EncodeIndexedGifFramesOptions {
	width: number;
	height: number;
	frames: IndexedGifFrames;
	frameCount?: number;
	palette: PaletteRGB;
	delay?: number;
	loop?: number | null;
	backend?: EncodeIndexedGifFramesBackend;
	delta?: boolean;
}

export interface EncodeRgbaGifFramesOptions {
	width: number;
	height: number;
	frames: RgbaGifFrames;
	frameCount?: number;
	palette?: PaletteRGB;
	delay?: number;
	loop?: number | null;
	backend?: EncodeIndexedGifFramesBackend;
	delta?: boolean;
}

type IndexedPixels = IndexedGifFrame;

type IndexedSourceRect = {
	data: IndexedPixels;
	offset: number;
	width: number;
	height: number;
	stride: number;
	length: number;
};

type LzwIndexStream = IndexedPixels | IndexedSourceRect;

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
	const delay = checkedU16(options.delay ?? 0, "Delay invalid.");
	const loop =
		options.loop === undefined || options.loop === null
			? null
			: checkedU16(options.loop, "Loop count invalid.");
	const backend = options.backend ?? "auto";

	if (backend !== "javascript") {
		const wasmCore = getWasmCoreModule();
		const encodeIndexedGif = options.delta
			? wasmCore?.encode_indexed_delta_gif
			: wasmCore?.encode_indexed_gif;
		if (encodeIndexedGif) {
			const flatFrames = flattenIndexedFrames(
				options.frames,
				frameSize,
				frameCount,
				colorCount,
				false,
			);
			return encodeIndexedGif(
				flatFrames,
				width,
				height,
				frameCount,
				paletteToUint32Array(options.palette),
				delay,
				loop === null ? -1 : loop,
			);
		}
		if (backend === "native") {
			throw new Error("Rust/Wasm GIF encoder unavailable.");
		}
	}

	return encodeIndexedGifFramesJavascript(
		options.frames,
		width,
		height,
		frameSize,
		frameCount,
		options.palette,
		delay,
		loop,
		options.delta === true,
	);
}

export function encodeRgbaGifFrames(
	options: EncodeRgbaGifFramesOptions,
): Uint8Array {
	const width = options.width | 0;
	const height = options.height | 0;
	if (width <= 0 || height <= 0 || width > 65535 || height > 65535) {
		throw new Error("Width/Height invalid.");
	}

	if (options.palette !== undefined) {
		checkPalette(options.palette);
	}

	const frameSize = width * height;
	const frameByteSize = frameSize * 4;
	const frameCount = getRgbaFrameCount(
		options.frames,
		frameByteSize,
		options.frameCount,
	);
	const delay = checkedU16(options.delay ?? 0, "Delay invalid.");
	const loop =
		options.loop === undefined || options.loop === null
			? null
			: checkedU16(options.loop, "Loop count invalid.");
	const backend = options.backend ?? "auto";

	if (backend !== "javascript") {
		const encodeRgbaGif = getWasmCoreModule()?.encode_rgba_gif;
		if (encodeRgbaGif) {
			return encodeRgbaGif(
				flattenRgbaFrames(options.frames, frameByteSize, frameCount),
				width,
				height,
				frameCount,
				paletteToUint32Array(options.palette ?? []),
				delay,
				loop === null ? -1 : loop,
				options.delta === true,
			);
		}
		if (backend === "native") {
			throw new Error("Rust/Wasm GIF encoder unavailable.");
		}
	}

	const { indexed, palette } = indexRgbaFramesJavascript(
		flattenRgbaFrames(options.frames, frameByteSize, frameCount),
		options.palette,
	);
	return encodeIndexedGifFramesJavascript(
		indexed,
		width,
		height,
		frameSize,
		frameCount,
		palette,
		delay,
		loop,
		options.delta === true,
	);
}

export class GifWriter {
	private p = 0;
	private ended = false;

	private loopCount: number | null;
	private globalPalette: PaletteRGB | null;
	private background = 0;
	private globalColorCount = 0;
	private previousIndexedFrame: Uint8Array | null = null;

	constructor(
		private buf: Uint8Array,
		private width: number,
		private height: number,
		gopts?: {
			loop?: number | null;
			palette?: PaletteRGB | null;
			background?: number;
		},
	) {
		const go = gopts ?? {};
		this.loopCount = go.loop === undefined ? null : go.loop;
		this.globalPalette = go.palette === undefined ? null : go.palette;

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
		indexedPixels: IndexedPixels,
		opts?: {
			palette?: PaletteRGB | null;
			delay?: number;
			disposal?: number;
			transparent?: number | null;
		},
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

	addFrameDelta(
		indexedPixels: IndexedPixels,
		opts?: {
			palette?: PaletteRGB | null;
			delay?: number;
			disposal?: number;
			transparent?: number | null;
		},
	): number {
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

		const rect = findChangedIndexedRect(
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
		opts?: {
			palette?: PaletteRGB | null;
			delay?: number;
			disposal?: number;
			transparent?: number | null;
		},
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

		const numColors = checkPalette(palette);
		const colorTableSizeBits = (log2Pow2(numColors) - 1) & 7;
		const minCodeSize = Math.max(2, log2Pow2(numColors));

		const delay = (o.delay ?? 0) | 0;
		let disposal = (o.disposal ?? 0) | 0;
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
	getOutputBuffer(): Uint8Array {
		return this.buf;
	}
	setOutputBuffer(v: Uint8Array) {
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
		if (!this.previousIndexedFrame) {
			this.previousIndexedFrame = new Uint8Array(this.width * this.height);
		}

		const previous = this.previousIndexedFrame;
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

		if (disposal === 2 || disposal === 3) {
			this.previousIndexedFrame = null;
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
	delay: number,
	loop: number | null,
	delta: boolean,
): Uint8Array {
	const colorCount = checkPalette(palette);
	const estimatedSize =
		13 + colorCount * 3 + 20 + frameCount * (frameSize * 2 + 32) + 1;
	const output = new Uint8Array(estimatedSize);
	const writer = new GifWriter(output, width, height, { palette, loop });
	for (let frameIndex = 0; frameIndex < frameCount; frameIndex++) {
		const frame = getIndexedFrame(frames, frameSize, frameIndex);
		if (delta) {
			writer.addFrameDelta(frame, { delay });
		} else {
			writer.addFrame(0, 0, width, height, frame, {
				delay,
			});
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
				throw new Error(
					"RGBA frame stream length does not match dimensions.",
				);
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
		flatFrames.set(frame.subarray(0, frameByteSize), frameIndex * frameByteSize);
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

function indexRgbaFramesJavascript(
	rgba: Uint8Array,
	palette: PaletteRGB | undefined,
): { indexed: Uint8Array; palette: PaletteRGB } {
	if (palette !== undefined) {
		return {
			indexed: indexRgbaFramesToPaletteJavascript(rgba, palette),
			palette,
		};
	}

	const exact = tryIndexRgbaFramesExactJavascript(rgba);
	return exact ?? indexRgbaFrames332Javascript(rgba);
}

function tryIndexRgbaFramesExactJavascript(
	rgba: Uint8Array,
): { indexed: Uint8Array; palette: PaletteRGB } | null {
	const palette: number[] = [];
	const colorToIndex = new Map<number, number>();
	const indexed = new Uint8Array(rgba.length >> 2);
	let output = 0;

	for (let offset = 0; offset < rgba.length; offset += 4) {
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

	return { indexed, palette };
}

function indexRgbaFramesToPaletteJavascript(
	rgba: Uint8Array,
	palette: PaletteRGB,
): Uint8Array {
	const exact = new Map<number, number>();
	for (let index = 0; index < palette.length; index++) {
		const color = (palette[index] ?? 0) & 0x00ff_ffff;
		if (!exact.has(color)) {
			exact.set(color, index);
		}
	}

	const indexed = new Uint8Array(rgba.length >> 2);
	let output = 0;
	for (let offset = 0; offset < rgba.length; offset += 4) {
		const r = rgba[offset]!;
		const g = rgba[offset + 1]!;
		const b = rgba[offset + 2]!;
		const color = rgbKey(r, g, b);
		indexed[output++] = exact.get(color) ?? nearestPaletteIndex(r, g, b, palette);
	}
	return indexed;
}

function indexRgbaFrames332Javascript(rgba: Uint8Array): {
	indexed: Uint8Array;
	palette: PaletteRGB;
} {
	const indexed = new Uint8Array(rgba.length >> 2);
	let output = 0;
	for (let offset = 0; offset < rgba.length; offset += 4) {
		indexed[output++] = rgb332Index(
			rgba[offset]!,
			rgba[offset + 1]!,
			rgba[offset + 2]!,
		);
	}
	return { indexed, palette: fixed332Palette() };
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

type IndexedRect = {
	x: number;
	y: number;
	width: number;
	height: number;
};

function findChangedIndexedRect(
	previous: Uint8Array,
	current: IndexedPixels,
	width: number,
	height: number,
): IndexedRect | null {
	let top = 0;
	let bottom = height - 1;

	while (top < height) {
		const row = top * width;
		let changed = false;
		for (let x = 0; x < width; x++) {
			if (previous[row + x] !== current[row + x]) {
				changed = true;
				break;
			}
		}
		if (changed) break;
		top++;
	}

	if (top === height) {
		return null;
	}

	while (bottom > top) {
		const row = bottom * width;
		let changed = false;
		for (let x = 0; x < width; x++) {
			if (previous[row + x] !== current[row + x]) {
				changed = true;
				break;
			}
		}
		if (changed) break;
		bottom--;
	}

	let left = width - 1;
	let right = 0;
	for (let y = top; y <= bottom; y++) {
		const row = y * width;
		for (let x = 0; x < width; x++) {
			if (previous[row + x] !== current[row + x]) {
				if (x < left) left = x;
				if (x > right) right = x;
			}
		}
	}

	return {
		x: left,
		y: top,
		width: right - left + 1,
		height: bottom - top + 1,
	};
}

function createIndexedSourceRect(
	data: IndexedPixels,
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
	buf: Uint8Array,
	p0: number,
	minCodeSize: number,
	indexStream: LzwIndexStream,
	colorCount: number,
): number {
	const wasmEncoded = tryEncodeLzwWithWasm(
		minCodeSize,
		indexStream,
		colorCount,
	);
	if (wasmEncoded) {
		buf.set(wasmEncoded, p0);
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
	const keys = (GifWriterOutputLZWCodeStream_fast._keys ??= new Int32Array(
		CAP,
	)) as Int32Array;
	const vals = (GifWriterOutputLZWCodeStream_fast._vals ??= new Int16Array(
		CAP,
	)) as Int16Array;
	const gen = (GifWriterOutputLZWCodeStream_fast._gen ??= new Int32Array(
		CAP,
	)) as Int32Array;
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

	let stridedData: IndexedPixels | null = null;
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
		ib = (indexStream[0] as number) | 0;
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
			k = (indexStream[i] as number) | 0;
		}
		if (k >>> 0 >= colorCount) throw new Error("Pixel index out of range.");
		const key = (ib << 8) | k;
		let slot = key & (CAP - 1);
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

function tryEncodeLzwWithWasm(
	minCodeSize: number,
	indexStream: LzwIndexStream,
	colorCount: number,
): Uint8Array | null {
	if (!(indexStream instanceof Uint8Array)) {
		return null;
	}
	if (indexStream.length < WASM_LZW_MIN_INDEX_COUNT) {
		return null;
	}

	const encodeIndexedLzw = getWasmCoreModule()?.encode_indexed_lzw;
	return encodeIndexedLzw
		? encodeIndexedLzw(indexStream, minCodeSize, colorCount)
		: null;
}
