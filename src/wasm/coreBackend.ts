import {
	GifDecodeBackend,
	PreparedFrameFormat,
	PreparedGifFrame,
	PreparedGifFrames,
	PreparedGifPlayer,
	PrepareFramesOptions,
	WasmCoreInstance,
} from "../types";
import { buildPal32 } from "../utils/palette";
import { getWasmCoreModule } from "./runtime";

type WasmFrameMetadata = {
	x: number;
	y: number;
	width: number;
	height: number;
	palette_offset: number;
	palette_size: number;
	transparent_index: number | null;
	delay: number;
	disposal: number;
};

type WasmMetadata = {
	width: number;
	height: number;
	frame_count: number;
	frames: WasmFrameMetadata[];
};

type NativeCompositedFrame = {
	index: number;
	pixels: Uint32Array;
	changedRect: ChangedRect | null;
	changedPixels?: Uint32Array | undefined;
};

type NormalizedBackendOptions = Required<
	Pick<PrepareFramesOptions, "format" | "composited">
> &
	PrepareFramesOptions;

const COMPOSITED_DELTA_MAGIC = 0x31444757;
const COMPOSITED_DELTA_VERSION = 1;
const COMPOSITED_DELTA_HEADER_LEN = 4;
const COMPOSITED_DELTA_ENTRY_LEN = 9;

export function createWasmCoreDecodeBackend(): GifDecodeBackend {
	return {
		name: "wtfgif-rust-wasm",
		isAvailable: () => getWasmCoreModule() !== null,
		prepareFrames: (gifData, options) =>
			prepareWasmCoreFrames(gifData, options),
	};
}

function prepareWasmCoreFrames(
	gifData: Uint8Array,
	options: NormalizedBackendOptions,
): PreparedGifFrames | null {
	const wasmModule = getWasmCoreModule();
	if (!wasmModule) {
		return null;
	}

	const core = new wasmModule.WtfGifCore(gifData);
	try {
		const metadata = JSON.parse(core.metadata_json()) as WasmMetadata;
		const frameIndices = normalizeFrameIndices(
			options.frameIndices,
			metadata.frame_count,
		);
		if (options.composited) {
			return prepareWasmCoreCompositedFrames(
				core,
				wasmModule,
				metadata,
				options,
				frameIndices,
			);
		}

		const frames: PreparedGifFrame[] = [];
		const transparentByIndex = new Map<number, number | null>();
		let byteLength = 0;

		for (const frameIndex of frameIndices) {
			const metadataFrame = metadata.frames[frameIndex];
			if (!metadataFrame) {
				throw new Error("Frame index out of range.");
			}

			const indices = toUint8Array(core.decode_frame_indices(frameIndex));
			const palette = buildPal32(
				gifData,
				metadataFrame.palette_offset,
				metadataFrame.palette_size,
				options.format,
				metadataFrame.transparent_index,
			);
			const frameByteLength = indices.byteLength + palette.byteLength;
			byteLength += frameByteLength;
			enforceByteBudget(byteLength, options.maxBytes);
			transparentByIndex.set(frameIndex, metadataFrame.transparent_index);
			frames.push({
				index: frameIndex,
				x: metadataFrame.x,
				y: metadataFrame.y,
				width: metadataFrame.width,
				height: metadataFrame.height,
				delay: metadataFrame.delay,
				disposal: metadataFrame.disposal,
				byteLength: frameByteLength,
				isFullCanvas: false,
				indices,
				palette,
			});
		}

		return createPreparedFramesResult(
			metadata.width,
			metadata.height,
			options.format,
			false,
			frames,
			byteLength,
			options.maxBytes ?? null,
			transparentByIndex,
			core,
		);
	} catch (error) {
		core.free();
		throw error;
	}
}

function prepareWasmCoreCompositedFrames(
	core: WasmCoreInstance,
	wasmModule: NonNullable<ReturnType<typeof getWasmCoreModule>>,
	metadata: WasmMetadata,
	options: NormalizedBackendOptions,
	frameIndices: readonly number[],
): PreparedGifFrames {
	const requestedFrames = createRequestedFrameFlags(
		frameIndices,
		metadata.frame_count,
	);
	const canvasPixels = metadata.width * metadata.height;
	let wasmMemory: WebAssembly.Memory | undefined;
	try {
		wasmMemory = wasmModule.wasm_memory?.();
	} catch {
		// Custom modules may expose a throwing/partial memory accessor.
	}
	let nativeFrames: NativeCompositedFrame[];
	if (options.deltas) {
		const scratchPrepare =
			options.format === "rgba"
				? core.prepare_composited_delta_rgba_scratch
				: core.prepare_composited_delta_bgra_scratch;
		const scratchPointer = core.composited_scratch_ptr;
		let preparedStream: Uint32Array | null = null;
		if (scratchPrepare && scratchPointer && wasmMemory) {
			try {
				const preparedLength = scratchPrepare.call(core, requestedFrames);
				const pointer = scratchPointer.call(core);
				const byteLength = preparedLength * Uint32Array.BYTES_PER_ELEMENT;
				if (
					preparedLength >= COMPOSITED_DELTA_HEADER_LEN &&
					pointer > 0 &&
					(pointer & (Uint32Array.BYTES_PER_ELEMENT - 1)) === 0 &&
					pointer + byteLength <= wasmMemory.buffer.byteLength
				) {
					preparedStream = new Uint32Array(
						wasmMemory.buffer,
						pointer,
						preparedLength,
					);
				}
			} catch {
				// Older/custom modules may expose only part of the scratch API.
			}
		}
		nativeFrames = readNativeCompositedDeltaFrames(
			preparedStream ??
				toUint32Array(
					options.format === "rgba"
						? core.prepare_composited_delta_rgba(requestedFrames)
						: core.prepare_composited_delta_bgra(requestedFrames),
				),
			canvasPixels,
		);
	} else {
		// A scratch result stays in Wasm memory while the prepared-frame object
		// is alive.  That removes the wasm-bindgen Vec<u32> -> JS typed-array
		// copy from the common playback path.  The core is intentionally kept
		// alive by createPreparedFramesResult until dispose(), so these views
		// cannot outlive the allocation that owns them.
		const scratchPrepare =
			options.format === "rgba"
				? core.prepare_composited_rgba_scratch
				: core.prepare_composited_bgra_scratch;
		const scratchPointer = core.composited_scratch_ptr;
		let preparedPixels: Uint32Array | null = null;
		if (scratchPrepare && scratchPointer && wasmMemory) {
			try {
				const preparedLength = scratchPrepare.call(core, requestedFrames);
				const pointer = scratchPointer.call(core);
				const expectedLength =
					requestedFrames.reduce((count, requested) => count + (requested ? 1 : 0), 0) *
						canvasPixels;
				const byteLength = preparedLength * Uint32Array.BYTES_PER_ELEMENT;
				if (
					preparedLength === expectedLength &&
					pointer > 0 &&
					(pointer & (Uint32Array.BYTES_PER_ELEMENT - 1)) === 0 &&
					pointer + byteLength <= wasmMemory.buffer.byteLength
				) {
					preparedPixels = new Uint32Array(
						wasmMemory.buffer,
						pointer,
						preparedLength,
					);
				}
			} catch {
				// Older/custom modules may expose only part of the scratch API.
				// Fall back to the stable wasm-bindgen return value below.
			}
		}
		if (!preparedPixels) {
			preparedPixels = toUint32Array(
				options.format === "rgba"
					? core.prepare_composited_rgba(requestedFrames)
					: core.prepare_composited_bgra(requestedFrames),
			);
		}
		nativeFrames = readNativeCompositedFullFrames(
			preparedPixels,
			requestedFrames,
			canvasPixels,
		);
	}
	const frames: PreparedGifFrame[] = [];
	const dedupe =
		options.dedupe === "all" ? new Map<number, Uint32Array[]>() : null;
	const dedupeMode = options.dedupe ?? "adjacent";
	const transparentByIndex = new Map<number, number | null>();
	let byteLength = 0;
	let previousPreparedPixels: Uint32Array | null = null;

	for (const nativeFrame of nativeFrames) {
		const frameIndex = nativeFrame.index;
		const metadataFrame = metadata.frames[frameIndex];
		if (!metadataFrame) {
			throw new Error("Frame index out of range.");
		}

		const canvas = nativeFrame.pixels;

		if (canvas.length !== canvasPixels) {
			throw new Error("Prepared frame buffer is shorter than expected.");
		}

		let pixels: Uint32Array | null = null;
		let bucket: Uint32Array[] | undefined;
		let hash = 0;
		if (dedupe) {
			hash = hashPixels(canvas);
			bucket = dedupe.get(hash);
			pixels = findMatchingPixels(canvas, bucket);
		} else if (
			dedupeMode === "adjacent" &&
			previousPreparedPixels &&
			(options.deltas
				? nativeFrame.changedRect === null
				: pixelsEqual(previousPreparedPixels, canvas))
		) {
			pixels = previousPreparedPixels;
		}

		const changedRect = nativeFrame.changedRect;
		const changedPixels = nativeFrame.changedPixels;
		const deltaBytes = changedPixels?.byteLength ?? 0;
		let frameBytes = 0;

		if (!pixels) {
			enforceByteBudget(
				byteLength + canvas.byteLength + deltaBytes,
				options.maxBytes,
			);
			pixels = canvas;
			frameBytes = pixels.byteLength;
			byteLength += frameBytes + deltaBytes;

			if (dedupe) {
				if (bucket) {
					bucket.push(pixels);
				} else {
					dedupe.set(hash, [pixels]);
				}
			}
		} else {
			enforceByteBudget(byteLength + deltaBytes, options.maxBytes);
			byteLength += deltaBytes;
		}

		frames.push({
			index: frameIndex,
			x: 0,
			y: 0,
			width: metadata.width,
			height: metadata.height,
			delay: metadataFrame.delay,
			disposal: metadataFrame.disposal,
			byteLength: frameBytes + deltaBytes,
			isFullCanvas: true,
			...(changedRect
				? {
						changedX: changedRect.x,
						changedY: changedRect.y,
						changedWidth: changedRect.width,
						changedHeight: changedRect.height,
						changedPixels,
					}
				: {}),
			pixels,
		});
		previousPreparedPixels = pixels;
	}

	return createPreparedFramesResult(
		metadata.width,
		metadata.height,
		options.format,
		true,
		frames,
		byteLength,
		options.maxBytes ?? null,
		transparentByIndex,
		core,
	);
}

function readNativeCompositedFullFrames(
	preparedPixels: Uint32Array,
	requestedFrames: Uint8Array,
	canvasPixels: number,
): NativeCompositedFrame[] {
	const frames: NativeCompositedFrame[] = [];
	let offset = 0;

	for (let frameIndex = 0; frameIndex < requestedFrames.length; frameIndex++) {
		if (requestedFrames[frameIndex] === 0) {
			continue;
		}

		const pixels = preparedPixels.subarray(offset, offset + canvasPixels);
		offset += canvasPixels;
		frames.push({ index: frameIndex, pixels, changedRect: null });
	}

	if (offset !== preparedPixels.length) {
		throw new Error("Prepared frame buffer has unused pixels.");
	}

	return frames;
}

function readNativeCompositedDeltaFrames(
	stream: Uint32Array,
	canvasPixels: number,
): NativeCompositedFrame[] {
	if (stream.length < COMPOSITED_DELTA_HEADER_LEN) {
		throw new Error("Native composited delta buffer is too short.");
	}
	if (stream[0] !== COMPOSITED_DELTA_MAGIC) {
		throw new Error("Native composited delta buffer has an invalid header.");
	}
	if (stream[1] !== COMPOSITED_DELTA_VERSION) {
		throw new Error("Unsupported native composited delta buffer version.");
	}
	const frameCount = stream[2] ?? 0;
	const encodedCanvasPixels = stream[3] ?? 0;
	if (encodedCanvasPixels !== canvasPixels) {
		throw new Error("Native composited delta canvas size mismatch.");
	}

	const tableEnd =
		COMPOSITED_DELTA_HEADER_LEN + frameCount * COMPOSITED_DELTA_ENTRY_LEN;
	if (tableEnd > stream.length) {
		throw new Error("Native composited delta frame table is truncated.");
	}

	const frames: NativeCompositedFrame[] = [];
	for (let i = 0; i < frameCount; i++) {
		const entry = COMPOSITED_DELTA_HEADER_LEN + i * COMPOSITED_DELTA_ENTRY_LEN;
		const frameIndex = stream[entry] ?? 0;
		const fullStart = stream[entry + 1] ?? 0;
		const fullLength = stream[entry + 2] ?? 0;
		const changedX = stream[entry + 3] ?? 0;
		const changedY = stream[entry + 4] ?? 0;
		const changedWidth = stream[entry + 5] ?? 0;
		const changedHeight = stream[entry + 6] ?? 0;
		const deltaStart = stream[entry + 7] ?? 0;
		const deltaLength = stream[entry + 8] ?? 0;

		if (fullLength !== canvasPixels || fullStart + fullLength > stream.length) {
			throw new Error("Native composited delta full frame is invalid.");
		}
		if (deltaStart + deltaLength > stream.length) {
			throw new Error("Native composited delta pixels are invalid.");
		}
		if (deltaLength !== changedWidth * changedHeight) {
			throw new Error("Native composited delta rectangle is invalid.");
		}

		frames.push({
			index: frameIndex,
			pixels: stream.subarray(fullStart, fullStart + fullLength),
			changedRect:
				deltaLength > 0
					? {
							x: changedX,
							y: changedY,
							width: changedWidth,
							height: changedHeight,
						}
					: null,
			changedPixels:
				deltaLength > 0
					? stream.subarray(deltaStart, deltaStart + deltaLength)
					: undefined,
		});
	}

	return frames;
}

function createPreparedFramesResult(
	width: number,
	height: number,
	format: PreparedFrameFormat,
	composited: boolean,
	frames: PreparedGifFrame[],
	byteLength: number,
	maxBytes: number | null,
	transparentByIndex: Map<number, number | null>,
	core: WasmCoreInstance,
): PreparedGifFrames {
	let maxIndex = -1;
	for (const frame of frames) {
		if (frame.index > maxIndex) {
			maxIndex = frame.index;
		}
	}
	const byIndex = new Array<PreparedGifFrame | undefined>(maxIndex + 1);
	for (const frame of frames) {
		byIndex[frame.index] = frame;
	}

	const copyFrame = (index: number, target: Uint8Array | Uint32Array): void => {
		const frame = byIndex[index];
		if (!frame) {
			throw new Error("Frame index out of range.");
		}
		copyPreparedFrame(
			width,
			height,
			frame,
			transparentByIndex.get(index),
			target,
		);
	};
	let disposed = false;

	return {
		width,
		height,
		format,
		composited,
		frames,
		byteLength,
		maxBytes,
		getFrame: (index) => byIndex[index],
		getFramePixels: (index) => byIndex[index]?.pixels,
		getFrameBytes: (index) => {
			const pixels = byIndex[index]?.pixels;
			return pixels
				? new Uint8Array(pixels.buffer, pixels.byteOffset, pixels.byteLength)
				: undefined;
		},
		copyFrame,
		createPlayer: (target?: Uint8Array | Uint32Array) =>
			createPreparedPlayer(width, height, byIndex, transparentByIndex, target),
		dispose: () => {
			if (disposed) {
				return;
			}
			disposed = true;
			byIndex.length = 0;
			frames.length = 0;
			core.free();
		},
	};
}

function createPreparedPlayer(
	width: number,
	height: number,
	byIndex: readonly (PreparedGifFrame | undefined)[],
	transparentByIndex: Map<number, number | null>,
	target?: Uint8Array | Uint32Array,
): PreparedGifPlayer {
	const target32 = target
		? getTarget32(target, width, height)
		: new Uint32Array(width * height);
	let currentIndex = -1;
	const drawFrame = (index: number): Uint32Array => {
		const frame = byIndex[index];
		if (!frame) {
			throw new Error("Frame index out of range.");
		}

		const previousFrame = byIndex[currentIndex];
		if (currentIndex === index - 1 && previousFrame?.pixels === frame.pixels) {
			currentIndex = index;
			return target32;
		}

		const changedPixelCount = frame.changedPixels?.length ?? 0;
		const fullPixelCount = frame.pixels?.length ?? target32.length;
		if (
			frame.changedPixels &&
			frame.changedX !== undefined &&
			frame.changedY !== undefined &&
			frame.changedWidth !== undefined &&
			frame.changedHeight !== undefined &&
			changedPixelCount <= fullPixelCount >>> 3 &&
			currentIndex === index - 1
		) {
			blitRectPixels(
				frame.changedPixels,
				target32,
				width,
				frame.changedX,
				frame.changedY,
				frame.changedWidth,
				frame.changedHeight,
			);
		} else {
			copyPreparedFrame32(
				width,
				frame,
				transparentByIndex.get(index),
				target32,
			);
		}
		currentIndex = index;
		return target32;
	};

	return {
		target: target32,
		get currentIndex() {
			return currentIndex;
		},
		drawFrame,
		next: () => {
			const nextIndex = currentIndex + 1;
			return byIndex[nextIndex] ? drawFrame(nextIndex) : target32;
		},
		reset: () => {
			currentIndex = -1;
			target32.fill(0);
		},
	};
}

function copyPreparedFrame(
	width: number,
	height: number,
	frame: PreparedGifFrame,
	transparentIndex: number | null | undefined,
	target: Uint8Array | Uint32Array,
): void {
	const target32 = getTarget32(target, width, height);
	copyPreparedFrame32(width, frame, transparentIndex, target32);
}

function copyPreparedFrame32(
	width: number,
	frame: PreparedGifFrame,
	transparentIndex: number | null | undefined,
	target32: Uint32Array,
): void {
	if (frame.pixels) {
		target32.set(frame.pixels);
		return;
	}

	const indices = frame.indices!;
	const palette = frame.palette!;
	const transparent = transparentIndex ?? 256;
	let src = 0;
	let dst = frame.y * width + frame.x;
	const rowStride = width - frame.width;

	for (let y = 0; y < frame.height; y++) {
		for (let x = 0; x < frame.width; x++) {
			const index = indices[src++]!;
			if (index !== transparent) {
				target32[dst] = palette[index]!;
			}
			dst++;
		}
		dst += rowStride;
	}
}

function getTarget32(
	target: Uint8Array | Uint32Array,
	width: number,
	height: number,
): Uint32Array {
	const requiredPixels = width * height;
	if (target instanceof Uint32Array) {
		if (target.length < requiredPixels) {
			throw new Error(
				`Buffer too small: need ${requiredPixels * 4} bytes, got ${
					target.byteLength
				}`,
			);
		}
		return target;
	}

	if (target.byteLength < requiredPixels * 4) {
		throw new Error(
			`Buffer too small: need ${requiredPixels * 4} bytes, got ${
				target.byteLength
			}`,
		);
	}

	return new Uint32Array(target.buffer, target.byteOffset, requiredPixels);
}

type ChangedRect = {
	x: number;
	y: number;
	width: number;
	height: number;
};

function blitRectPixels(
	source: Uint32Array,
	target: Uint32Array,
	targetWidth: number,
	x: number,
	y: number,
	width: number,
	height: number,
): void {
	for (let row = 0; row < height; row++) {
		const src = row * width;
		const dst = (y + row) * targetWidth + x;
		target.set(source.subarray(src, src + width), dst);
	}
}

function hashPixels(pixels: Uint32Array): number {
	let hash = 2166136261;
	for (let i = 0; i < pixels.length; i++) {
		hash ^= pixels[i]!;
		hash = Math.imul(hash, 16777619);
	}
	return hash >>> 0;
}

function findMatchingPixels(
	pixels: Uint32Array,
	bucket: Uint32Array[] | undefined,
): Uint32Array | null {
	if (!bucket) {
		return null;
	}

	for (const candidate of bucket) {
		if (candidate.length !== pixels.length) {
			continue;
		}

		if (pixelsEqual(candidate, pixels)) {
			return candidate;
		}
	}
	return null;
}

function pixelsEqual(a: Uint32Array, b: Uint32Array): boolean {
	if (a.length !== b.length) {
		return false;
	}

	for (let i = 0; i < a.length; i++) {
		if (a[i] !== b[i]) {
			return false;
		}
	}
	return true;
}

function createRequestedFrameFlags(
	frameIndices: readonly number[],
	frameCount: number,
): Uint8Array {
	if (frameIndices.length === 0) {
		return new Uint8Array(0);
	}

	const maxFrame = Math.max(...frameIndices);
	if (maxFrame >= frameCount) {
		throw new Error("Frame index out of range.");
	}

	const requestedFrames = new Uint8Array(maxFrame + 1);
	for (const frameIndex of frameIndices) {
		requestedFrames[frameIndex] = 1;
	}
	return requestedFrames;
}

function normalizeFrameIndices(
	frameIndices: readonly number[] | undefined,
	frameCount: number,
): number[] {
	if (!frameIndices) {
		return Array.from({ length: frameCount }, (_, index) => index);
	}

	const seen = new Set<number>();
	const normalized: number[] = [];
	for (const frameIndex of frameIndices) {
		if (!Number.isInteger(frameIndex)) {
			throw new Error("Frame index out of range.");
		}
		const index = frameIndex | 0;
		if (index < 0 || index >= frameCount) {
			throw new Error("Frame index out of range.");
		}
		if (!seen.has(index)) {
			seen.add(index);
			normalized.push(index);
		}
	}
	return normalized;
}

function enforceByteBudget(byteLength: number, maxBytes?: number): void {
	if (maxBytes !== undefined && byteLength > maxBytes) {
		throw new Error(
			`Prepared frame cache exceeds maxBytes (${byteLength} > ${maxBytes}).`,
		);
	}
}

function toUint8Array(value: Uint8Array | number[]): Uint8Array {
	return value instanceof Uint8Array ? value : Uint8Array.from(value);
}

function toUint32Array(value: Uint32Array | number[]): Uint32Array {
	return value instanceof Uint32Array ? value : Uint32Array.from(value);
}
