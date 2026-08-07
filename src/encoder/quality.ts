import type { WasmQualityCoreModule } from "../types";
import { getWasmQualityCoreModule } from "../wasm/qualityRuntime";

const DEFAULT_ALPHA_THRESHOLD = 128;
const QUALITY_LOW_RES_BYTE_LIMIT = 1_000_000 * 4;

let scratchModule: WasmQualityCoreModule | null = null;
let scratchMemory: WebAssembly.Memory | null = null;
let scratchPointer = 0;
let scratchCapacity = 0;

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

export function prepareQualityWasmEncoderModule(
	module: WasmQualityCoreModule | null,
): void {
	if (module === scratchModule && scratchMemory) {
		encodeRgbaGifFrames = encodeRgbaGifFramesPrepared;
		return;
	}
	scratchModule = module;
	scratchMemory = module?.wasm_memory() ?? null;
	scratchPointer = 0;
	scratchCapacity = 0;
	encodeRgbaGifFrames = scratchMemory
		? encodeRgbaGifFramesPrepared
		: encodeRgbaGifFramesUnprepared;
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
		return new Uint8Array(
			memory.buffer,
			module.gif_output_scratch_ptr(),
			outputLength,
		).slice();
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
	return new Uint8Array(
		memory.buffer,
		module.gif_output_scratch_ptr(),
		outputLength,
	).slice();
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
	const normalized = new Uint16Array(frameCount);
	for (let frame = 0; frame < frameCount; frame++) {
		normalized[frame] = checkedU16(delay[frame] ?? 0, "Delay invalid.");
	}
	return normalized;
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
