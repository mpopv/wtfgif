import type { WasmQualityCoreModule } from "../types";
import { getWasmQualityCoreModule } from "../wasm/qualityRuntime";

const DEFAULT_ALPHA_THRESHOLD = 128;
const QUALITY_LOW_RES_BYTE_LIMIT = 1_000_000 * 4;
const QUALITY_INITIAL_INPUT_CAPACITY = 4 * 1024 * 1024;
const QUALITY_LARGE_TIER_PIXEL_COUNT = 1001 * 1000;
const QUALITY_TIER_PIXEL_COUNT = 128 * 128 * 8;

let scratchModule: WasmQualityCoreModule | null = null;
let scratchMemory: WebAssembly.Memory | null = null;
let scratchPointer = 0;
let scratchCapacity = 0;
const PREPARE_ENCODER_OPTIONS = {} as EncodeRgbaGifFramesOptions;
const PREPARE_ENCODER_RESULT = new Uint8Array();
const PREPARE_FRAME_ARRAY_OPTIONS: EncodeRgbaGifFramesOptions = {
	width: 1,
	height: 1,
	frames: [Uint8Array.of(16, 32, 48, 255), Uint8Array.of(64, 80, 96, 255)],
	delay: 0,
	loop: 0,
};

export type RgbaGifFrame = Uint8Array | Uint8ClampedArray;
export type RgbaGifFrames = RgbaGifFrame | RgbaGifFrame[];
export type GifFrameDelay = number | readonly number[] | Uint16Array;

/** The single quality-first contract exposed by `wtfgif/encode`. */
export interface EncodeRgbaGifFramesOptions {
	width: number;
	height: number;
	frames: RgbaGifFrames;
	frameCount?: number;
	delay?: GifFrameDelay;
	loop?: number | null;
	alphaThreshold?: number;
}

export let encodeRgbaGifFrames: (
	options: EncodeRgbaGifFramesOptions,
) => Uint8Array = encodeRgbaGifFramesUnprepared;

function prepareQualityEncoderRuntime(
	module: WasmQualityCoreModule,
	memory: WebAssembly.Memory,
	inputPointer: number,
): void {
	const largeTierPixels = new Uint32Array(
		memory.buffer,
		inputPointer,
		QUALITY_LARGE_TIER_PIXEL_COUNT,
	);
	for (let pixel = 0; pixel < largeTierPixels.length; pixel += 1) {
		const cell = (pixel * 32429) & 32767;
		const red = ((cell >> 10) << 3) | 4;
		const green = (((cell >> 5) & 31) << 3) | 4;
		const blue = ((cell & 31) << 3) | 4;
		largeTierPixels[pixel] = 0xff000000 | (blue << 16) | (green << 8) | red;
	}
	module.encode_rgba_quality_gif_constant_delay_scratch_from_input(
		QUALITY_LARGE_TIER_PIXEL_COUNT * 4,
		1001,
		1000,
		1,
		0,
		0,
		DEFAULT_ALPHA_THRESHOLD,
	);

	const tierPixels = new Uint32Array(
		memory.buffer,
		inputPointer,
		QUALITY_TIER_PIXEL_COUNT,
	);
	// Tier the split-pair histogram loop with one fixed opaque encode. The
	// ordinary mixed-alpha preparation immediately below remains last so this
	// does not displace its shared hot state.
	for (let pixel = 0; pixel < QUALITY_TIER_PIXEL_COUNT; pixel += 1) {
		const pair = pixel >> 1;
		const split = pair % 4 === 2 ? pixel & 1 : 0;
		const cell = (pair * 73 + split) & 2047;
		const red = ((cell >> 8) << 4) | 8;
		const green = (((cell >> 4) & 15) << 4) | 8;
		const blue = ((cell & 15) << 4) | 8;
		tierPixels[pixel] = 0xff000000 | (blue << 16) | (green << 8) | red;
	}
	module.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
		QUALITY_TIER_PIXEL_COUNT * 4,
		128,
		128,
		8,
		0,
		0,
		DEFAULT_ALPHA_THRESHOLD,
	);
	for (let pixel = 0; pixel < tierPixels.length; pixel += 1) {
		if (pixel % 31 === 0) {
			tierPixels[pixel] = 0;
			continue;
		}
		const cell = (pixel * 4051) & 2047;
		const red = ((cell >> 8) << 4) | 8;
		const green = (((cell >> 4) & 15) << 4) | 8;
		const blue = ((cell & 15) << 4) | 8;
		tierPixels[pixel] = 0xff000000 | (blue << 16) | (green << 8) | red;
	}
	// Four source-independent calls consistently reach V8's faster optimized
	// tier for the shared mixed/opaque planner without retaining image data.
	// Fewer calls leave MakeEmoji/transparency slower; more calls are neutral.
	for (let iteration = 0; iteration < 4; iteration += 1) {
		module.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
			QUALITY_TIER_PIXEL_COUNT * 4,
			128,
			128,
			8,
			0,
			0,
			DEFAULT_ALPHA_THRESHOLD,
		);
	}

	const gridPixelCount = 8 * 12 * 16;
	for (let pixel = 0; pixel < gridPixelCount; pixel += 1) {
		const red = ((pixel % 8) << 5) | 16;
		const green = (((Math.floor(pixel / 8) % 12) << 4) | 8) & 255;
		const blue = ((Math.floor(pixel / (8 * 12)) << 4) | 8) & 255;
		tierPixels[pixel] = 0xff000000 | (blue << 16) | (green << 8) | red;
	}
	for (let iteration = 0; iteration < 3; iteration += 1) {
		module.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
			gridPixelCount * 4,
			32,
			16,
			3,
			0,
			0,
			DEFAULT_ALPHA_THRESHOLD,
		);
	}

	// Tier the quantized direct-cell route separately from the exact-source
	// route. Three shades per 4-bit cell create more than 256 exact RGB values
	// while collapsing to 128 histogram representatives.
	const directPixelCount = 32 * 16 * 2;
	const directShades = [2, 8, 14];
	for (let pixel = 0; pixel < directPixelCount; pixel += 1) {
		const cell = (pixel * 73) & 127;
		const shade = directShades[Math.floor(pixel / 128) % 3]!;
		const red = ((cell >> 4) << 4) | shade;
		const green = (((cell >> 2) & 3) << 4) | shade;
		const blue = ((cell & 3) << 4) | shade;
		tierPixels[pixel] = 0xff000000 | (blue << 16) | (green << 8) | red;
	}
	for (let iteration = 0; iteration < 3; iteration += 1) {
		module.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
			directPixelCount * 4,
			32,
			16,
			2,
			0,
			0,
			DEFAULT_ALPHA_THRESHOLD,
		);
	}
	// A transparent input with all 256 coarse cells exercises the exact
	// one-cell merge selected when transparency leaves 255 palette slots.
	const singleMergePixelCount = 32 * 16 * 2;
	for (let pixel = 0; pixel < singleMergePixelCount; pixel += 1) {
		const cell = pixel & 255;
		const red = ((cell >> 5) << 5) | 16;
		const green = (((cell >> 2) & 7) << 5) | 16;
		const blue = ((cell & 3) << 6) | 32;
		const alpha = pixel < 768 ? 255 : 0;
		tierPixels[pixel] = (alpha << 24) | (blue << 16) | (green << 8) | red;
	}
	for (let iteration = 0; iteration < 3; iteration += 1) {
		module.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
			singleMergePixelCount * 4,
			32,
			16,
			2,
			0,
			0,
			DEFAULT_ALPHA_THRESHOLD,
		);
	}

	const exactPixelCount = 64 * 64 * 12;
	const exactPalette = [
		0xff20100c, 0xff30c4f5, 0xff6f47ef, 0xffb28a11, 0xffa0d606, 0xffffffff,
		0xff5634a2, 0xff18b070,
	];
	for (let pixel = 0; pixel < exactPixelCount; pixel += 1) {
		const x = pixel & 63;
		const y = (pixel >> 6) & 63;
		const frame = pixel >> 12;
		tierPixels[pixel] = exactPalette[((x >> 3) + (y >> 3) + frame * 3) & 7]!;
	}
	for (let iteration = 0; iteration < 4; iteration += 1) {
		module.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
			exactPixelCount * 4,
			64,
			64,
			12,
			0,
			0,
			DEFAULT_ALPHA_THRESHOLD,
		);
	}
	// A larger three-color animation finishes tiering the small exact-palette
	// call graph without reading or retaining any user pixels.
	const stablePixelCount = 128 * 128 * 12;
	for (let pixel = 0; pixel < stablePixelCount; pixel += 1) {
		const x = pixel & 127;
		const y = (pixel >> 7) & 127;
		const frame = pixel >> 14;
		tierPixels[pixel] = exactPalette[((x >> 4) + (y >> 4) + frame) % 3]!;
	}
	module.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
		stablePixelCount * 4,
		128,
		128,
		12,
		0,
		0,
		DEFAULT_ALPHA_THRESHOLD,
	);
}

export function prepareQualityWasmEncoderModule(
	module: WasmQualityCoreModule | null,
): void {
	if (module === scratchModule && scratchMemory) {
		encodeRgbaGifFrames = encodeRgbaGifFramesPrepared;
		return;
	}
	scratchModule = module;
	scratchMemory = module?.wasm_memory() ?? null;
	if (module && scratchMemory) {
		module.prepare_quality_encoder_code();
		// Enter the actual exported shims with an empty sentinel during explicit
		// initialization. Wasm returns before touching reusable image state.
		module.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
			0,
			1,
			1,
			0,
			0,
			0,
			DEFAULT_ALPHA_THRESHOLD,
		);
		module.gif_output_scratch_ptr();
		scratchPointer = module.indexed_lzw_input_scratch_reserve(
			QUALITY_INITIAL_INPUT_CAPACITY,
		);
		scratchCapacity = QUALITY_INITIAL_INPUT_CAPACITY;
		prepareQualityEncoderRuntime(module, scratchMemory, scratchPointer);
		// Compile the separate-frame validation and direct-to-Wasm copy path with
		// fixed source-independent pixels. Image-stitching callers commonly hold
		// one decoded RGBA array per frame; their first real call should not pay
		// JavaScript compilation that the contiguous path already avoids.
		encodeRgbaGifFramesPrepared(PREPARE_FRAME_ARRAY_OPTIONS);
		// Compile the private JavaScript validation/dispatch function during the
		// explicit initialization boundary. The sentinel exits before reading
		// pixels, calling Wasm, or producing image-derived state.
		encodeRgbaGifFramesPrepared(PREPARE_ENCODER_OPTIONS);
		encodeRgbaGifFrames = encodeRgbaGifFramesPrepared;
		return;
	}
	scratchPointer = 0;
	scratchCapacity = 0;
	encodeRgbaGifFrames = encodeRgbaGifFramesUnprepared;
}

function encodeRgbaGifFramesUnprepared(
	options: EncodeRgbaGifFramesOptions,
): Uint8Array {
	const module = getWasmQualityCoreModule();
	if (!module) {
		throw new Error("The wtfgif WebAssembly encoder is not initialized.");
	}
	prepareQualityWasmEncoderModule(module);
	return encodeRgbaGifFramesPrepared(options);
}

function encodeRgbaGifFramesPrepared(
	options: EncodeRgbaGifFramesOptions,
): Uint8Array {
	if (options === PREPARE_ENCODER_OPTIONS) return PREPARE_ENCODER_RESULT;
	const module = scratchModule!;

	const width = options.width | 0;
	const height = options.height | 0;
	const frameByteSize = width * height * 4;
	const frames = options.frames;
	const requestedDelay = options.delay;
	const framesAreUint8 = frames instanceof Uint8Array;
	if (
		(framesAreUint8 || frames instanceof Uint8ClampedArray) &&
		(requestedDelay === undefined || typeof requestedDelay === "number")
	) {
		const frameCount =
			(options.frameCount === undefined
				? frames.length / frameByteSize
				: options.frameCount) | 0;
		const inputLength = frameByteSize * frameCount;
		const delay = (requestedDelay ?? 0) | 0;
		const loop = (options.loop ?? -1) | 0;
		const alphaThreshold =
			(options.alphaThreshold ?? DEFAULT_ALPHA_THRESHOLD) | 0;
		if (
			(width - 1) >>> 0 >= 65535 ||
			(height - 1) >>> 0 >= 65535 ||
			frameCount <= 0 ||
			frames.length !== inputLength ||
			delay >>> 0 > 65535 ||
			(loop !== -1 && loop >>> 0 > 65535) ||
			alphaThreshold >>> 0 > 255
		) {
			throwInvalidContiguousOptions(
				width,
				height,
				frames.length,
				inputLength,
				frameCount,
				delay,
				loop,
				alphaThreshold,
			);
		}
		const memory = scratchMemory!;
		if (scratchCapacity < inputLength) {
			scratchPointer = module.indexed_lzw_input_scratch_reserve(inputLength);
			scratchCapacity = inputLength;
		}
		new Uint8Array(memory.buffer, scratchPointer, inputLength).set(
			framesAreUint8
				? frames
				: new Uint8Array(frames.buffer, frames.byteOffset, frames.byteLength),
		);
		const outputLength =
			inputLength <= QUALITY_LOW_RES_BYTE_LIMIT
				? module.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
						inputLength,
						width,
						height,
						frameCount,
						delay,
						loop,
						alphaThreshold,
					)
				: module.encode_rgba_quality_gif_constant_delay_scratch_from_input(
						inputLength,
						width,
						height,
						frameCount,
						delay,
						loop,
						alphaThreshold,
					);
		if (outputLength === 0) throw new Error("Wasm quality encoding failed.");
		const outputView = new Uint8Array(
			memory.buffer,
			module.gif_output_scratch_ptr(),
			outputLength,
		);
		return new Uint8Array(outputView);
	}
	if (width <= 0 || height <= 0 || width > 65535 || height > 65535) {
		throw new Error("Width/Height invalid.");
	}

	return encodeRgbaGifFramesFallback(
		options,
		module,
		width,
		height,
		frameByteSize,
	);
}

function throwInvalidContiguousOptions(
	width: number,
	height: number,
	frameBytes: number,
	inputLength: number,
	frameCount: number,
	delay: number,
	loop: number,
	alphaThreshold: number,
): never {
	if (width <= 0 || height <= 0 || width > 65535 || height > 65535) {
		throw new Error("Width/Height invalid.");
	}
	if (frameCount <= 0 || frameBytes !== inputLength) {
		throw new Error("RGBA frame stream length does not match dimensions.");
	}
	if (delay < 0 || delay > 65535) throw new Error("Delay invalid.");
	if ((loop < 0 && loop !== -1) || loop > 65535) {
		throw new Error("Loop count invalid.");
	}
	if (alphaThreshold < 0 || alphaThreshold > 255) {
		throw new Error("Alpha threshold invalid.");
	}
	throw new Error("Invalid encode options.");
}

function encodeRgbaGifFramesFallback(
	options: EncodeRgbaGifFramesOptions,
	module: WasmQualityCoreModule,
	width: number,
	height: number,
	frameByteSize: number,
): Uint8Array {
	const frameCount = getFrameCount(
		options.frames,
		frameByteSize,
		options.frameCount,
	);
	const inputLength = frameByteSize * frameCount;
	const delay = normalizeDelays(options.delay, frameCount);
	const loop =
		options.loop === undefined || options.loop === null
			? -1
			: checkedU16(options.loop, "Loop count invalid.");
	const alphaThreshold = checkedU8(
		options.alphaThreshold ?? DEFAULT_ALPHA_THRESHOLD,
		"Alpha threshold invalid.",
	);

	const memory = scratchMemory;
	if (!memory) {
		throw new Error("The wtfgif WebAssembly encoder has no memory export.");
	}
	if (scratchCapacity < inputLength) {
		scratchPointer = module.indexed_lzw_input_scratch_reserve(inputLength);
		scratchCapacity = inputLength;
	}
	const input = new Uint8Array(memory.buffer, scratchPointer, inputLength);
	copyFrames(input, options.frames, frameByteSize, frameCount);
	let delayCount = 0;
	if (typeof delay !== "number") {
		delayCount = delay.length;
		const delayPointer = module.quality_delay_scratch_reserve(
			inputLength,
			delayCount,
		);
		new Uint16Array(memory.buffer, delayPointer, delayCount).set(delay);
	}

	const outputLength =
		typeof delay === "number"
			? inputLength <= QUALITY_LOW_RES_BYTE_LIMIT
				? module.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
						inputLength,
						width,
						height,
						frameCount,
						delay,
						loop,
						alphaThreshold,
					)
				: module.encode_rgba_quality_gif_constant_delay_scratch_from_input(
						inputLength,
						width,
						height,
						frameCount,
						delay,
						loop,
						alphaThreshold,
					)
			: module.encode_rgba_quality_gif_scratch_from_input(
					inputLength,
					width,
					height,
					frameCount,
					delayCount,
					loop,
					alphaThreshold,
				);
	if (outputLength === 0) throw new Error("Wasm quality encoding failed.");
	const outputView = new Uint8Array(
		memory.buffer,
		module.gif_output_scratch_ptr(),
		outputLength,
	);
	return new Uint8Array(outputView);
}

function getFrameCount(
	frames: RgbaGifFrames,
	frameByteSize: number,
	requestedCount: number | undefined,
): number {
	if (isFrame(frames)) {
		if (requestedCount !== undefined) {
			const count = requestedCount | 0;
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

	if (requestedCount !== undefined && requestedCount !== frames.length) {
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

function copyFrames(
	output: Uint8Array,
	frames: RgbaGifFrames,
	frameByteSize: number,
	frameCount: number,
): void {
	if (isFrame(frames)) {
		output.set(asUint8Array(frames));
		return;
	}
	for (let frame = 0; frame < frameCount; frame++) {
		output.set(
			asUint8Array(frames[frame]!).subarray(0, frameByteSize),
			frame * frameByteSize,
		);
	}
}

function isFrame(value: RgbaGifFrames): value is RgbaGifFrame {
	return value instanceof Uint8Array || value instanceof Uint8ClampedArray;
}

function asUint8Array(frame: RgbaGifFrame): Uint8Array {
	return frame instanceof Uint8Array
		? frame
		: new Uint8Array(frame.buffer, frame.byteOffset, frame.byteLength);
}

function normalizeDelays(
	delay: GifFrameDelay | undefined,
	frameCount: number,
): number | Uint16Array {
	if (delay === undefined) return 0;
	if (typeof delay === "number") return checkedU16(delay, "Delay invalid.");
	if (delay.length !== frameCount) {
		throw new Error("Delay count does not match frame count.");
	}
	const first = checkedU16(delay[0] ?? 0, "Delay invalid.");
	for (let frame = 1; frame < frameCount; frame++) {
		const current = checkedU16(delay[frame] ?? 0, "Delay invalid.");
		if (current === first) continue;
		const normalized = new Uint16Array(frameCount);
		normalized.fill(first, 0, frame);
		normalized[frame] = current;
		for (let remaining = frame + 1; remaining < frameCount; remaining++) {
			normalized[remaining] = checkedU16(
				delay[remaining] ?? 0,
				"Delay invalid.",
			);
		}
		return normalized;
	}
	return first;
}

function checkedU16(value: number, message: string): number {
	const checked = value | 0;
	if (checked < 0 || checked > 65535) throw new Error(message);
	return checked;
}

function checkedU8(value: number, message: string): number {
	const checked = value | 0;
	if (checked < 0 || checked > 255) throw new Error(message);
	return checked;
}
