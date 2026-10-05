import type { GifBinary } from "./types";

const GIF_EXTENSION = 0x21;
const GIF_IMAGE = 0x2c;
const GIF_TRAILER = 0x3b;
const GRAPHIC_CONTROL_LABEL = 0xf9;
const APPLICATION_LABEL = 0xff;
const PLAIN_TEXT_LABEL = 0x01;

const NETSCAPE_APPLICATION_ID = [
	0x4e, 0x45, 0x54, 0x53, 0x43, 0x41, 0x50, 0x45, 0x32, 0x2e, 0x30,
] as const;
const ANIMEXTS_APPLICATION_ID = [
	0x41, 0x4e, 0x49, 0x4d, 0x45, 0x58, 0x54, 0x53, 0x31, 0x2e, 0x30,
] as const;

interface CompiledFrameLayout {
	readonly imageOffset: number;
	readonly imageEndOffset: number;
	readonly controlOffset: number | null;
	readonly controlEndOffset: number | null;
	readonly delayOffset: number | null;
	readonly delay: number;
	readonly disposal: number;
	readonly hasTransparency: boolean;
	readonly isFullCanvas: boolean;
}

interface CompiledLayout {
	readonly source: Uint8Array;
	readonly width: number;
	readonly height: number;
	readonly frames: readonly CompiledFrameLayout[];
	readonly loopCount: number | null;
	readonly loopCountOffsets: readonly number[];
	readonly extensionInsertionOffset: number;
	readonly isGif87a: boolean;
	readonly frameReorderError: string | null;
}

type ByteInsertions = Map<number, Uint8Array[]>;

/**
 * A delay in GIF centiseconds, or one delay for every image frame.
 */
export type GifFrameDelays = number | ArrayLike<number>;

/**
 * A parsed GIF whose compressed image data and unrelated metadata can be
 * copied without decoding or re-encoding pixels.
 *
 * Instances are immutable. Transformations share the parsed source layout;
 * bytes are copied only when `toUint8Array()` is called.
 */
export class CompiledGif {
	readonly width: number;
	readonly height: number;
	readonly frameCount: number;
	readonly delays: readonly number[];
	readonly loopCount: number | null;
	readonly canReorderFrames: boolean;

	private readonly layout: CompiledLayout;
	private readonly requestedDelays: readonly number[] | null;
	private readonly requestedLoopCount: number | null;
	private readonly hasLoopOverride: boolean;

	private constructor(
		layout: CompiledLayout,
		requestedDelays: readonly number[] | null = null,
		requestedLoopCount: number | null = null,
		hasLoopOverride = false,
	) {
		this.layout = layout;
		this.width = layout.width;
		this.height = layout.height;
		this.frameCount = layout.frames.length;
		this.requestedDelays = requestedDelays;
		this.requestedLoopCount = requestedLoopCount;
		this.hasLoopOverride = hasLoopOverride;
		this.delays =
			requestedDelays ??
			Object.freeze(layout.frames.map((frame) => frame.delay));
		this.loopCount = hasLoopOverride ? requestedLoopCount : layout.loopCount;
		this.canReorderFrames = layout.frameReorderError === null;
	}

	/** Parse a GIF into an immutable compiled representation. */
	static from(input: GifBinary): CompiledGif {
		return new CompiledGif(parseGif(input));
	}

	/**
	 * Return a compiled view with new per-frame delays.
	 *
	 * A scalar applies to every frame. An array-like value must have exactly
	 * one entry per image frame. Values are integer centiseconds in
	 * the GIF range 0...65535.
	 */
	withDelays(delays: GifFrameDelays): CompiledGif {
		const normalized = normalizeDelays(delays, this.frameCount);
		return new CompiledGif(
			this.layout,
			normalized,
			this.requestedLoopCount,
			this.hasLoopOverride,
		);
	}

	/**
	 * Return a compiled view with a Netscape loop count.
	 *
	 * `0` means loop forever. If the source has no Netscape extension, a
	 * canonical extension is inserted without touching image data.
	 */
	withLoop(loopCount: number): CompiledGif {
		assertUint16(loopCount, "Loop count");
		return new CompiledGif(this.layout, this.requestedDelays, loopCount, true);
	}

	/**
	 * Reorder or duplicate structurally independent frames without decoding.
	 *
	 * This strict fast path accepts full-canvas frames that are either all
	 * opaque or all restore a canonical base with disposal 2/3. It throws for
	 * GIFs whose frame composition could depend on the original order.
	 */
	reorderFrames(order: ArrayLike<number>): Uint8Array {
		const normalizedOrder = normalizeFrameOrder(order, this.frameCount);
		if (this.requestedDelays !== null || this.hasLoopOverride) {
			return compileGif(this.toUint8Array()).reorderFrames(normalizedOrder);
		}
		if (this.layout.frameReorderError !== null) {
			throw new Error(
				`GIF frames cannot be reordered pixel-perfectly: ${this.layout.frameReorderError}`,
			);
		}
		return materializeFrameOrder(this.layout, normalizedOrder);
	}

	/** Reverse all frames through the proven-safe structural fast path. */
	reverseFrames(): Uint8Array {
		return this.reorderFrames(
			Array.from(
				{ length: this.frameCount },
				(_, index) => this.frameCount - index - 1,
			),
		);
	}

	/**
	 * Append the complete reverse sequence, matching MakeEmoji's boomerang
	 * semantics: `[A, B, C, C, B, A]`.
	 */
	boomerangFrames(): Uint8Array {
		return this.reorderFrames(
			Array.from({ length: this.frameCount * 2 }, (_, index) =>
				index < this.frameCount ? index : this.frameCount * 2 - index - 1,
			),
		);
	}

	/**
	 * Materialize the transformed GIF.
	 *
	 * Existing delays and loop counts are patched in place. A frame without a
	 * graphic-control extension receives the minimal eight-byte extension.
	 */
	toUint8Array(): Uint8Array {
		const { layout } = this;
		if (this.requestedDelays === null && !this.hasLoopOverride) {
			return layout.source.slice();
		}

		const patches = new Map<number, number>();
		const insertions: ByteInsertions = new Map();
		let insertedExtension = false;

		if (this.hasLoopOverride) {
			const loopCount = this.requestedLoopCount!;
			if (layout.loopCountOffsets.length === 0) {
				addInsertion(
					insertions,
					layout.extensionInsertionOffset,
					createNetscapeLoopExtension(loopCount),
				);
				insertedExtension = true;
			} else {
				for (const offset of layout.loopCountOffsets) {
					patchUint16(patches, offset, loopCount);
				}
			}
		}

		if (this.requestedDelays !== null) {
			for (let index = 0; index < layout.frames.length; index++) {
				const frame = layout.frames[index]!;
				const delay = this.requestedDelays[index]!;
				if (frame.delayOffset === null) {
					addInsertion(
						insertions,
						frame.imageOffset,
						createGraphicControlExtension(delay),
					);
					insertedExtension = true;
				} else {
					patchUint16(patches, frame.delayOffset, delay);
				}
			}
		}

		if (insertedExtension && layout.isGif87a) {
			// A graphic-control or application extension requires GIF89a.
			patches.set(4, 0x39);
		}

		return materialize(layout.source, patches, insertions);
	}
}

/**
 * Parse a GIF once so metadata-only transformations can reuse its original
 * palettes, image descriptors, extensions, and compressed LZW sub-blocks.
 */
export function compileGif(input: GifBinary): CompiledGif {
	return CompiledGif.from(input);
}

/**
 * Rewrite frame delays without decoding or recompressing a single pixel.
 */
export function retimeGifPixelPerfect(
	input: GifBinary,
	delays: GifFrameDelays,
): Uint8Array {
	return compileGif(input).withDelays(delays).toUint8Array();
}

/** Reverse structurally independent GIF frames without touching pixel data. */
export function reverseGifPixelPerfect(input: GifBinary): Uint8Array {
	return compileGif(input).reverseFrames();
}

/** Create a MakeEmoji-style forward-then-reverse GIF without pixel work. */
export function boomerangGifPixelPerfect(input: GifBinary): Uint8Array {
	return compileGif(input).boomerangFrames();
}

function parseGif(input: GifBinary): CompiledLayout {
	const source = new Uint8Array(input.length);
	source.set(input as ArrayLike<number>);
	const length = source.length;
	if (length < 13) {
		throw new Error("Unexpected end of GIF data.");
	}

	const isGif87a =
		source[0] === 0x47 &&
		source[1] === 0x49 &&
		source[2] === 0x46 &&
		source[3] === 0x38 &&
		source[4] === 0x37 &&
		source[5] === 0x61;
	const isGif89a =
		source[0] === 0x47 &&
		source[1] === 0x49 &&
		source[2] === 0x46 &&
		source[3] === 0x38 &&
		source[4] === 0x39 &&
		source[5] === 0x61;
	if (!isGif87a && !isGif89a) {
		throw new Error("Invalid GIF 87a/89a header.");
	}

	const width = readUint16(source, 6);
	const height = readUint16(source, 8);
	const packed = source[10]!;
	const globalColorTableLength =
		packed & 0x80 ? 3 * (1 << ((packed & 0x07) + 1)) : 0;
	let position = 13 + globalColorTableLength;
	ensureRange(source, position, 0);
	const extensionInsertionOffset = position;

	const frames: CompiledFrameLayout[] = [];
	const loopCountOffsets: number[] = [];
	let loopCount: number | null = null;
	let pendingDelayOffset: number | null = null;
	let pendingDelay = 0;
	let pendingControlOffset: number | null = null;
	let pendingControlEndOffset: number | null = null;
	let pendingDisposal = 0;
	let pendingHasTransparency = false;
	let hasPlainText = false;
	let hasDuplicateControl = false;
	let sawTrailer = false;

	while (position < length) {
		const blockOffset = position;
		const blockType = source[position++]!;

		if (blockType === GIF_TRAILER) {
			sawTrailer = true;
			break;
		}

		if (blockType === GIF_EXTENSION) {
			ensureRange(source, position, 1);
			const label = source[position++]!;

			if (label === GRAPHIC_CONTROL_LABEL) {
				ensureRange(source, position, 6);
				if (source[position] !== 0x04 || source[position + 5] !== 0x00) {
					throw new Error("Invalid graphics extension block.");
				}
				if (pendingControlOffset !== null) {
					hasDuplicateControl = true;
				}
				pendingControlOffset = blockOffset;
				pendingControlEndOffset = position + 6;
				pendingDelayOffset = position + 2;
				pendingDelay = readUint16(source, pendingDelayOffset);
				const controlPacked = source[position + 1]!;
				pendingDisposal = (controlPacked >>> 2) & 0x07;
				pendingHasTransparency = (controlPacked & 0x01) !== 0;
				position += 6;
				continue;
			}

			if (label === APPLICATION_LABEL) {
				const app = readNetscapeLoopExtension(source, position);
				if (app !== null) {
					loopCountOffsets.push(app.loopCountOffset);
					loopCount = readUint16(source, app.loopCountOffset);
				}
			}

			position = skipSubBlocks(source, position);
			if (label === PLAIN_TEXT_LABEL) {
				// A GCE scopes the next graphic-rendering block. Plain text is
				// such a block, even though it is not one of our image frames.
				hasPlainText = true;
				pendingControlOffset = null;
				pendingControlEndOffset = null;
				pendingDelayOffset = null;
				pendingDelay = 0;
				pendingDisposal = 0;
				pendingHasTransparency = false;
			}
			continue;
		}

		if (blockType === GIF_IMAGE) {
			ensureRange(source, position, 9);
			const imagePacked = source[position + 8]!;
			position += 9;
			if (imagePacked & 0x80) {
				const localColorTableLength = 3 * (1 << ((imagePacked & 0x07) + 1));
				ensureRange(source, position, localColorTableLength);
				position += localColorTableLength;
			}

			// LZW minimum code size followed by data sub-blocks.
			ensureRange(source, position, 1);
			position++;
			position = skipSubBlocks(source, position);
			const imageEndOffset = position;
			const x = readUint16(source, blockOffset + 1);
			const y = readUint16(source, blockOffset + 3);
			const frameWidth = readUint16(source, blockOffset + 5);
			const frameHeight = readUint16(source, blockOffset + 7);
			frames.push({
				imageOffset: blockOffset,
				imageEndOffset,
				controlOffset: pendingControlOffset,
				controlEndOffset: pendingControlEndOffset,
				delayOffset: pendingDelayOffset,
				delay: pendingDelay,
				disposal: pendingDisposal,
				hasTransparency: pendingHasTransparency,
				isFullCanvas:
					x === 0 && y === 0 && frameWidth === width && frameHeight === height,
			});
			pendingControlOffset = null;
			pendingControlEndOffset = null;
			pendingDelayOffset = null;
			pendingDelay = 0;
			pendingDisposal = 0;
			pendingHasTransparency = false;
			continue;
		}

		throw new Error(
			`Unknown GIF block at byte ${blockOffset}: 0x${blockType.toString(16)}`,
		);
	}
	if (!sawTrailer) {
		throw new Error("GIF data is missing its trailer.");
	}

	const frameReorderError = getFrameReorderError(
		source,
		frames,
		hasPlainText,
		hasDuplicateControl,
	);

	return {
		source,
		width,
		height,
		frames,
		loopCount,
		loopCountOffsets,
		extensionInsertionOffset,
		isGif87a,
		frameReorderError,
	};
}

function readNetscapeLoopExtension(
	source: Uint8Array,
	position: number,
): { loopCountOffset: number } | null {
	if (source[position] !== NETSCAPE_APPLICATION_ID.length) {
		return null;
	}
	ensureRange(source, position, 12);
	const matchesApplicationId = (applicationId: readonly number[]) =>
		applicationId.every((byte, index) => source[position + 1 + index] === byte);
	if (
		!matchesApplicationId(NETSCAPE_APPLICATION_ID) &&
		!matchesApplicationId(ANIMEXTS_APPLICATION_ID)
	) {
		return null;
	}

	const subBlockOffset = position + 12;
	ensureRange(source, subBlockOffset, 5);
	if (
		source[subBlockOffset] !== 0x03 ||
		source[subBlockOffset + 1] !== 0x01 ||
		source[subBlockOffset + 4] !== 0x00
	) {
		return null;
	}
	return { loopCountOffset: subBlockOffset + 2 };
}

function skipSubBlocks(source: Uint8Array, position: number): number {
	while (true) {
		ensureRange(source, position, 1);
		const size = source[position++]!;
		if (size === 0) {
			return position;
		}
		ensureRange(source, position, size);
		position += size;
	}
}

function ensureRange(
	source: Uint8Array,
	position: number,
	byteLength: number,
): void {
	if (position < 0 || byteLength < 0 || position > source.length - byteLength) {
		throw new Error("Unexpected end of GIF data.");
	}
}

function readUint16(source: Uint8Array, offset: number): number {
	ensureRange(source, offset, 2);
	return (source[offset]! | (source[offset + 1]! << 8)) >>> 0;
}

function assertUint16(value: number, label: string): void {
	if (!Number.isInteger(value) || value < 0 || value > 0xffff) {
		throw new RangeError(`${label} must be an integer from 0 to 65535.`);
	}
}

function normalizeDelays(
	delays: GifFrameDelays,
	frameCount: number,
): readonly number[] {
	if (typeof delays === "number") {
		assertUint16(delays, "Frame delay");
		return Object.freeze(Array.from({ length: frameCount }, () => delays));
	}
	if (delays.length !== frameCount) {
		throw new RangeError(
			`Expected ${frameCount} frame delays, received ${delays.length}.`,
		);
	}
	const normalized = Array.from(delays);
	for (const delay of normalized) {
		assertUint16(delay, "Frame delay");
	}
	return Object.freeze(normalized);
}

function normalizeFrameOrder(
	order: ArrayLike<number>,
	frameCount: number,
): readonly number[] {
	if (!Number.isInteger(order.length) || order.length < 1) {
		throw new RangeError("Frame order must contain at least one frame.");
	}
	const normalized = Array.from(order);
	for (const frameIndex of normalized) {
		if (
			!Number.isInteger(frameIndex) ||
			frameIndex < 0 ||
			frameIndex >= frameCount
		) {
			throw new RangeError(
				`Frame index ${frameIndex} is outside 0...${Math.max(0, frameCount - 1)}.`,
			);
		}
	}
	return normalized;
}

function getFrameReorderError(
	source: Uint8Array,
	frames: readonly CompiledFrameLayout[],
	hasPlainText: boolean,
	hasDuplicateControl: boolean,
): string | null {
	if (frames.length === 0) {
		return "the GIF has no image frames";
	}
	if (hasPlainText) {
		return "Plain Text is a rendering block and cannot be moved safely";
	}
	if (hasDuplicateControl) {
		return "multiple graphic controls target one rendering block";
	}

	for (let index = 0; index < frames.length; index++) {
		const frame = frames[index]!;
		if (frame.controlOffset === null || frame.controlEndOffset === null) {
			return `frame ${index} has no graphic control block`;
		}
		if (frame.controlEndOffset !== frame.imageOffset) {
			return `frame ${index} has extensions between its control and image`;
		}
		if (!frame.isFullCanvas) {
			return `frame ${index} is not an exact full-canvas image`;
		}
		if (frame.disposal > 3) {
			return `frame ${index} uses reserved disposal ${frame.disposal}`;
		}
		if (
			index > 0 &&
			frames[index - 1]!.imageEndOffset !== frame.controlOffset
		) {
			return `frame ${index} is separated by anchored metadata`;
		}
	}

	const lastFrame = frames[frames.length - 1]!;
	if (source[lastFrame.imageEndOffset] !== GIF_TRAILER) {
		return "metadata follows the last image before the trailer";
	}

	const everyFrameOpaque = frames.every((frame) => !frame.hasTransparency);
	const everyFrameRestoresBase = frames.every(
		(frame) => frame.disposal === 2 || frame.disposal === 3,
	);
	if (!everyFrameOpaque && !everyFrameRestoresBase) {
		return "transparent frames depend on the original canvas order";
	}
	return null;
}

function createGraphicControlExtension(delay: number): Uint8Array {
	return Uint8Array.of(
		GIF_EXTENSION,
		GRAPHIC_CONTROL_LABEL,
		0x04,
		0x00,
		delay & 0xff,
		(delay >>> 8) & 0xff,
		0x00,
		0x00,
	);
}

function createNetscapeLoopExtension(loopCount: number): Uint8Array {
	return Uint8Array.of(
		GIF_EXTENSION,
		APPLICATION_LABEL,
		0x0b,
		...NETSCAPE_APPLICATION_ID,
		0x03,
		0x01,
		loopCount & 0xff,
		(loopCount >>> 8) & 0xff,
		0x00,
	);
}

function patchUint16(
	patches: Map<number, number>,
	offset: number,
	value: number,
): void {
	patches.set(offset, value & 0xff);
	patches.set(offset + 1, (value >>> 8) & 0xff);
}

function addInsertion(
	insertions: ByteInsertions,
	offset: number,
	bytes: Uint8Array,
): void {
	const existing = insertions.get(offset);
	if (existing) {
		existing.push(bytes);
	} else {
		insertions.set(offset, [bytes]);
	}
}

function materialize(
	source: Uint8Array,
	patches: ReadonlyMap<number, number>,
	insertions: ByteInsertions,
): Uint8Array {
	const patchedSource = source.slice();
	for (const [offset, byte] of patches) {
		patchedSource[offset] = byte;
	}
	if (insertions.size === 0) {
		return patchedSource;
	}

	let outputLength = source.length;
	for (const groups of insertions.values()) {
		for (const bytes of groups) {
			outputLength += bytes.length;
		}
	}

	const output = new Uint8Array(outputLength);
	let outputPosition = 0;
	let sourcePosition = 0;
	const insertionOffsets = [...insertions.keys()].sort(
		(left, right) => left - right,
	);
	for (const insertionOffset of insertionOffsets) {
		const sourceSpan = patchedSource.subarray(sourcePosition, insertionOffset);
		output.set(sourceSpan, outputPosition);
		outputPosition += sourceSpan.length;
		for (const bytes of insertions.get(insertionOffset)!) {
			output.set(bytes, outputPosition);
			outputPosition += bytes.length;
		}
		sourcePosition = insertionOffset;
	}
	output.set(patchedSource.subarray(sourcePosition), outputPosition);
	return output;
}

function materializeFrameOrder(
	layout: CompiledLayout,
	order: readonly number[],
): Uint8Array {
	const firstFrame = layout.frames[0]!;
	const lastFrame = layout.frames[layout.frames.length - 1]!;
	const prefixEnd = firstFrame.controlOffset!;
	const suffixStart = lastFrame.imageEndOffset;
	let outputLength = prefixEnd + (layout.source.length - suffixStart);
	for (const frameIndex of order) {
		const frame = layout.frames[frameIndex]!;
		outputLength += frame.imageEndOffset - frame.controlOffset!;
		if (!Number.isSafeInteger(outputLength)) {
			throw new RangeError("Reordered GIF would be too large.");
		}
	}

	const output = new Uint8Array(outputLength);
	let outputPosition = 0;
	output.set(layout.source.subarray(0, prefixEnd), outputPosition);
	outputPosition += prefixEnd;
	for (const frameIndex of order) {
		const frame = layout.frames[frameIndex]!;
		const atom = layout.source.subarray(
			frame.controlOffset!,
			frame.imageEndOffset,
		);
		output.set(atom, outputPosition);
		outputPosition += atom.length;
	}
	output.set(layout.source.subarray(suffixStart), outputPosition);
	return output;
}
