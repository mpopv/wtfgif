import type { WasmQualityCoreModule } from "../types";
import { getWasmQualityCoreModule } from "../wasm/qualityRuntime";

const DEFAULT_ALPHA_THRESHOLD = 128;

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

export function prepareQualityWasmEncoderModule(
	module: WasmQualityCoreModule | null,
): void {
	if (module === scratchModule && scratchMemory) return;
	scratchModule = module;
	scratchMemory = module?.wasm_memory() ?? null;
	scratchPointer = 0;
	scratchCapacity = 0;
}

export function encodeRgbaGifFrames(
	options: EncodeRgbaGifFramesOptions,
): Uint8Array {
	const module = getWasmQualityCoreModule();
	if (!module) {
		throw new Error("The wtfgif WebAssembly encoder is not initialized.");
	}
	prepareQualityWasmEncoderModule(module);

	const width = options.width | 0;
	const height = options.height | 0;
	if (width <= 0 || height <= 0 || width > 65535 || height > 65535) {
		throw new Error("Width/Height invalid.");
	}

	const frameByteSize = width * height * 4;
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

	if (!scratchMemory) {
		throw new Error("The wtfgif WebAssembly encoder has no memory export.");
	}
	if (scratchCapacity < inputLength) {
		scratchPointer = module.indexed_lzw_input_scratch_reserve(inputLength);
		scratchCapacity = inputLength;
	}
	const input = new Uint8Array(
		scratchMemory.buffer,
		scratchPointer,
		inputLength,
	);
	copyFrames(input, options.frames, frameByteSize, frameCount);

	const outputLength =
		typeof delay === "number"
			? module.encode_rgba_quality_gif_constant_delay_scratch_from_input(
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
					delay,
					loop,
					alphaThreshold,
				);
	return new Uint8Array(
		scratchMemory.buffer,
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
