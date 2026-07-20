import { readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const { GifReader: OmgGifReader, GifWriter: OmgGifWriter } = require("omggif");
const wasmCore = require("../crates/wtfgif-core/pkg/wtfgif_core.js");

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const gifsDir = join(root, "test", "gifs");
const frameLimit = Number(process.env.VALIDATE_WASM_FRAME_LIMIT ?? 8);
const COMPOSITED_DELTA_MAGIC = 0x31444757;
const COMPOSITED_DELTA_VERSION = 1;
const COMPOSITED_DELTA_HEADER_LEN = 4;
const COMPOSITED_DELTA_ENTRY_LEN = 9;

function assertEqual(actual, expected, label) {
	if (actual !== expected) {
		throw new Error(`${label}: expected ${expected}, got ${actual}`);
	}
}

function assertFrameEqual(actual, expected, label) {
	assertEqual(actual.x, expected.x, `${label} x`);
	assertEqual(actual.y, expected.y, `${label} y`);
	assertEqual(actual.width, expected.width, `${label} width`);
	assertEqual(actual.height, expected.height, `${label} height`);
	assertEqual(
		actual.has_local_palette,
		expected.has_local_palette,
		`${label} has_local_palette`,
	);
	assertEqual(
		actual.palette_offset,
		expected.palette_offset,
		`${label} palette_offset`,
	);
	assertEqual(
		actual.palette_size,
		expected.palette_size,
		`${label} palette_size`,
	);
	assertEqual(actual.data_offset, expected.data_offset, `${label} data_offset`);
	assertEqual(actual.data_length, expected.data_length, `${label} data_length`);
	assertEqual(
		actual.transparent_index,
		expected.transparent_index ?? null,
		`${label} transparent_index`,
	);
	assertEqual(actual.interlaced, expected.interlaced, `${label} interlaced`);
	assertEqual(actual.delay, expected.delay, `${label} delay`);
	assertEqual(actual.disposal, expected.disposal, `${label} disposal`);
}

function validateGif(data, label) {
	const omg = new OmgGifReader(data);
	const decoder = new wasmCore.WtfGifCore(data);
	try {
		const metadata = JSON.parse(decoder.metadata_json());

		assertEqual(decoder.width(), omg.width, `${label} decoder width`);
		assertEqual(decoder.height(), omg.height, `${label} decoder height`);
		assertEqual(
			decoder.frame_count(),
			omg.numFrames(),
			`${label} decoder frame_count`,
		);
		assertEqual(metadata.width, omg.width, `${label} width`);
		assertEqual(metadata.height, omg.height, `${label} height`);
		assertEqual(metadata.frame_count, omg.numFrames(), `${label} frame_count`);
		assertEqual(
			metadata.frames.length,
			omg.numFrames(),
			`${label} frames.length`,
		);

		for (let frameIndex = 0; frameIndex < omg.numFrames(); frameIndex++) {
			assertFrameEqual(
				metadata.frames[frameIndex],
				omg.frameInfo(frameIndex),
				`${label} frame ${frameIndex}`,
			);
		}

		for (const frameIndex of selectFramesForPixelValidation(metadata)) {
			validateDecodedFrame(data, label, metadata, omg, decoder, frameIndex);
		}
		validateCompositedFrames(label, metadata, omg, decoder);
	} finally {
		decoder.free();
	}
}

function selectFramesForPixelValidation(metadata) {
	const frames = new Set();
	const count = metadata.frame_count;
	for (let i = 0; i < Math.min(count, frameLimit); i++) {
		frames.add(i);
	}
	if (count > 0) {
		frames.add(count - 1);
		frames.add(Math.floor(count / 2));
	}
	for (let i = 0; i < metadata.frames.length; i++) {
		if (metadata.frames[i].interlaced) {
			frames.add(i);
		}
	}
	return [...frames].sort((a, b) => a - b);
}

function validateDecodedFrame(data, label, metadata, omg, decoder, frameIndex) {
	const frame = metadata.frames[frameIndex];
	const indices = toByteArray(decoder.decode_frame_indices(frameIndex));
	const expectedLength = frame.width * frame.height;

	assertEqual(
		indices.length,
		expectedLength,
		`${label} frame ${frameIndex} index length`,
	);

	const expected = createSentinelCanvas(metadata);
	const actual = createSentinelCanvas(metadata);
	omg.decodeAndBlitFrameRGBA(frameIndex, expected);
	blitIndicesToRgba(data, metadata, frame, indices, actual, label, frameIndex);
	assertBytesEqual(actual, expected, `${label} frame ${frameIndex} RGBA`);

	const directRgbaExpected = new Uint8Array(
		metadata.width * metadata.height * 4,
	);
	omg.decodeAndBlitFrameRGBA(frameIndex, directRgbaExpected);
	assertBytesEqual(
		toByteArray(decoder.decode_frame_rgba(frameIndex)),
		directRgbaExpected,
		`${label} frame ${frameIndex} direct RGBA`,
	);
	assertBytesEqual(
		toByteArray(decoder.decode_frame_bgra(frameIndex)),
		rgbaToBgra(directRgbaExpected),
		`${label} frame ${frameIndex} direct BGRA`,
	);
}

function validateCompositedFrames(label, metadata, omg, decoder) {
	const frameCount = Math.min(metadata.frame_count, frameLimit);
	const requestedFrames = new Uint8Array(frameCount);
	requestedFrames.fill(1);
	const expectedRgba = composeFramesWithOmggif(metadata, omg, requestedFrames);
	const actualRgba = toUint32Bytes(
		decoder.prepare_composited_rgba(requestedFrames),
	);
	assertBytesEqual(actualRgba, expectedRgba, `${label} composited RGBA`);
	assertBytesEqual(
		toUint32Bytes(decoder.prepare_composited_bgra(requestedFrames)),
		rgbaToBgra(expectedRgba),
		`${label} composited BGRA`,
	);
	validateCompositedDeltaStream(
		toUint32Array(decoder.prepare_composited_delta_rgba(requestedFrames)),
		toUint32ArrayFromBytes(expectedRgba),
		metadata,
		requestedFrames,
		`${label} composited delta RGBA`,
	);
	const expectedBgra = rgbaToBgra(expectedRgba);
	validateCompositedDeltaStream(
		toUint32Array(decoder.prepare_composited_delta_bgra(requestedFrames)),
		toUint32ArrayFromBytes(expectedBgra),
		metadata,
		requestedFrames,
		`${label} composited delta BGRA`,
	);
}

function composeFramesWithOmggif(metadata, omg, requestedFrames) {
	const canvas = new Uint8Array(metadata.width * metadata.height * 4);
	const output = new Uint8Array(
		requestedFrames.reduce((count, flag) => count + (flag ? 1 : 0), 0) *
			canvas.length,
	);
	let outputOffset = 0;

	for (let frameIndex = 0; frameIndex < requestedFrames.length; frameIndex++) {
		const frame = metadata.frames[frameIndex];
		const restore = frame.disposal === 3 ? canvas.slice() : null;
		omg.decodeAndBlitFrameRGBA(frameIndex, canvas);

		if (requestedFrames[frameIndex]) {
			output.set(canvas, outputOffset);
			outputOffset += canvas.length;
		}

		if (frame.disposal === 2) {
			clearFrameRect(metadata, frame, canvas);
		} else if (restore) {
			canvas.set(restore);
		}
	}

	return output;
}

function clearFrameRect(metadata, frame, canvas) {
	const x = Math.max(0, frame.x | 0);
	const y = Math.max(0, frame.y | 0);
	const right = Math.min(metadata.width, x + (frame.width | 0));
	const bottom = Math.min(metadata.height, y + (frame.height | 0));
	const width = right - x;
	if (width <= 0) {
		return;
	}

	for (let row = y; row < bottom; row++) {
		const start = (row * metadata.width + x) * 4;
		canvas.fill(0, start, start + width * 4);
	}
}

function validateCompositedDeltaStream(
	stream,
	expectedFrames,
	metadata,
	requestedFrames,
	label,
) {
	const canvasPixels = metadata.width * metadata.height;
	const frameCount = requestedFrames.reduce(
		(count, flag) => count + (flag ? 1 : 0),
		0,
	);
	assertEqual(stream[0], COMPOSITED_DELTA_MAGIC, `${label} magic`);
	assertEqual(stream[1], COMPOSITED_DELTA_VERSION, `${label} version`);
	assertEqual(stream[2], frameCount, `${label} frame_count`);
	assertEqual(stream[3], canvasPixels, `${label} canvas_pixels`);

	let expectedOffset = 0;
	let previousExpected = null;
	let outputFrameIndex = 0;
	for (let frameIndex = 0; frameIndex < requestedFrames.length; frameIndex++) {
		if (!requestedFrames[frameIndex]) {
			continue;
		}

		const entry =
			COMPOSITED_DELTA_HEADER_LEN +
			outputFrameIndex * COMPOSITED_DELTA_ENTRY_LEN;
		const fullStart = stream[entry + 1];
		const fullLength = stream[entry + 2];
		const changedX = stream[entry + 3];
		const changedY = stream[entry + 4];
		const changedWidth = stream[entry + 5];
		const changedHeight = stream[entry + 6];
		const deltaStart = stream[entry + 7];
		const deltaLength = stream[entry + 8];
		const expected = expectedFrames.subarray(
			expectedOffset,
			expectedOffset + canvasPixels,
		);
		const actual = stream.subarray(fullStart, fullStart + fullLength);

		assertEqual(
			stream[entry],
			frameIndex,
			`${label} frame ${frameIndex} index`,
		);
		assertEqual(
			fullLength,
			canvasPixels,
			`${label} frame ${frameIndex} full length`,
		);
		assertBytesEqual(actual, expected, `${label} frame ${frameIndex} full`);

		const changedRect = previousExpected
			? findChangedRect(
					previousExpected,
					expected,
					metadata.width,
					metadata.height,
				)
			: null;
		if (!changedRect) {
			assertEqual(deltaLength, 0, `${label} frame ${frameIndex} delta length`);
		} else {
			assertEqual(changedX, changedRect.x, `${label} frame ${frameIndex} dx`);
			assertEqual(changedY, changedRect.y, `${label} frame ${frameIndex} dy`);
			assertEqual(
				changedWidth,
				changedRect.width,
				`${label} frame ${frameIndex} dw`,
			);
			assertEqual(
				changedHeight,
				changedRect.height,
				`${label} frame ${frameIndex} dh`,
			);
			const expectedDelta = copyRectPixels(
				expected,
				metadata.width,
				changedRect,
			);
			assertEqual(
				deltaLength,
				expectedDelta.length,
				`${label} frame ${frameIndex} delta length`,
			);
			assertBytesEqual(
				stream.subarray(deltaStart, deltaStart + deltaLength),
				expectedDelta,
				`${label} frame ${frameIndex} delta pixels`,
			);
		}

		previousExpected = expected.slice();
		expectedOffset += canvasPixels;
		outputFrameIndex++;
	}
}

function findChangedRect(previous, current, width, height) {
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

function copyRectPixels(source, sourceWidth, rect) {
	const pixels = new Uint32Array(rect.width * rect.height);
	for (let y = 0; y < rect.height; y++) {
		const src = (rect.y + y) * sourceWidth + rect.x;
		pixels.set(source.subarray(src, src + rect.width), y * rect.width);
	}
	return pixels;
}

function toByteArray(value) {
	return value instanceof Uint8Array ? value : Uint8Array.from(value);
}

function toUint32Array(value) {
	return value instanceof Uint32Array ? value : Uint32Array.from(value);
}

function toUint32ArrayFromBytes(bytes) {
	return new Uint32Array(
		bytes.buffer,
		bytes.byteOffset,
		bytes.byteLength >>> 2,
	);
}

function toUint32Bytes(value) {
	const pixels = value instanceof Uint32Array ? value : Uint32Array.from(value);
	return new Uint8Array(pixels.buffer, pixels.byteOffset, pixels.byteLength);
}

function rgbaToBgra(rgba) {
	const bgra = new Uint8Array(rgba.length);
	for (let offset = 0; offset < rgba.length; offset += 4) {
		bgra[offset] = rgba[offset + 2];
		bgra[offset + 1] = rgba[offset + 1];
		bgra[offset + 2] = rgba[offset];
		bgra[offset + 3] = rgba[offset + 3];
	}
	return bgra;
}

function createSentinelCanvas(metadata) {
	const canvas = new Uint8Array(metadata.width * metadata.height * 4);
	for (let offset = 0; offset < canvas.length; offset += 4) {
		canvas[offset] = 0x11;
		canvas[offset + 1] = 0x22;
		canvas[offset + 2] = 0x33;
		canvas[offset + 3] = 0x44;
	}
	return canvas;
}

function blitIndicesToRgba(
	data,
	metadata,
	frame,
	indices,
	target,
	label,
	frameIndex,
) {
	const transparentIndex = frame.transparent_index;
	let src = 0;
	for (let y = 0; y < frame.height; y++) {
		let dst = ((frame.y + y) * metadata.width + frame.x) * 4;
		for (let x = 0; x < frame.width; x++) {
			const index = indices[src++];
			if (index === transparentIndex) {
				dst += 4;
				continue;
			}
			if (index >= frame.palette_size) {
				throw new Error(
					`${label} frame ${frameIndex}: palette index ${index} exceeds palette size ${frame.palette_size}`,
				);
			}
			const paletteOffset = frame.palette_offset + index * 3;
			target[dst] = data[paletteOffset] ?? 0;
			target[dst + 1] = data[paletteOffset + 1] ?? 0;
			target[dst + 2] = data[paletteOffset + 2] ?? 0;
			target[dst + 3] = 255;
			dst += 4;
		}
	}
}

function assertBytesEqual(actual, expected, label) {
	for (let i = 0; i < expected.length; i++) {
		if (actual[i] !== expected[i]) {
			throw new Error(
				`${label}: byte ${i} expected ${expected[i]}, got ${actual[i]}`,
			);
		}
	}
}

function validatePixelPerfectReencode(data, label) {
	const sourceReader = new OmgGifReader(data);
	const sourcePixels = new Uint8Array(wasmCore.decode_all_rgba(data).buffer);
	for (const [operation, output] of [
		[
			"reencode",
			toByteArray(wasmCore.reencode_gif_pixel_perfect(data)),
		],
		["remux", toByteArray(wasmCore.remux_gif_pixel_perfect(data))],
	]) {
		const outputReader = new OmgGifReader(output);
		assertEqual(
			outputReader.width,
			sourceReader.width,
			`${label} ${operation} width`,
		);
		assertEqual(
			outputReader.height,
			sourceReader.height,
			`${label} ${operation} height`,
		);
		assertEqual(
			outputReader.numFrames(),
			sourceReader.numFrames(),
			`${label} ${operation} frame count`,
		);
		assertEqual(
			outputReader.loopCount(),
			sourceReader.loopCount(),
			`${label} ${operation} loop count`,
		);
		assertBytesEqual(
			new Uint8Array(wasmCore.decode_all_rgba(output).buffer),
			sourcePixels,
			`${label} ${operation} composited RGBA`,
		);
	}
}

function createLocalPaletteGif() {
	const buffer = new Uint8Array(512);
	const writer = new OmgGifWriter(buffer, 2, 2, {
		palette: [0x000000, 0xffffff],
	});
	writer.addFrame(0, 0, 2, 2, new Uint8Array([0, 1, 1, 0]), {
		palette: [0xff0000, 0x00ff00],
		delay: 7,
		disposal: 2,
		transparent: 0,
	});
	return buffer.slice(0, writer.end());
}

function createOffsetFrameGif() {
	const buffer = new Uint8Array(512);
	const writer = new OmgGifWriter(buffer, 4, 4, {
		palette: [0x000000, 0xffffff, 0xff0000, 0x0000ff],
	});
	writer.addFrame(1, 2, 2, 1, new Uint8Array([2, 3]), {
		delay: 3,
		disposal: 1,
	});
	return buffer.slice(0, writer.end());
}

function createTransparentOffsetFrameGif() {
	const buffer = new Uint8Array(512);
	const writer = new OmgGifWriter(buffer, 4, 4, {
		palette: [0x000000, 0xff0000],
	});
	writer.addFrame(1, 1, 2, 2, new Uint8Array([0, 1, 1, 0]), {
		delay: 4,
		disposal: 2,
		transparent: 0,
	});
	return buffer.slice(0, writer.end());
}

function validateNativeIndexedGifEncoding() {
	const width = 2;
	const height = 2;
	const encoded = toByteArray(
		wasmCore.encode_indexed_gif(
			new Uint8Array([1, 1, 1, 1, 2, 0, 0, 2]),
			width,
			height,
			2,
			new Uint32Array([0x000000, 0xff0000, 0x0000ff]),
			5,
			0,
		),
	);
	const omg = new OmgGifReader(encoded);
	const decoder = new wasmCore.WtfGifCore(encoded);
	try {
		assertEqual(omg.width, width, "native encode width");
		assertEqual(omg.height, height, "native encode height");
		assertEqual(omg.numFrames(), 2, "native encode frame count");
		assertEqual(omg.frameInfo(0).delay, 5, "native encode frame 0 delay");
		assertEqual(omg.frameInfo(1).delay, 5, "native encode frame 1 delay");
		assertBytesEqual(
			toByteArray(decoder.decode_frame_indices(0)),
			new Uint8Array([1, 1, 1, 1]),
			"native encode frame 0 indices",
		);
		assertBytesEqual(
			toByteArray(decoder.decode_frame_indices(1)),
			new Uint8Array([2, 0, 0, 2]),
			"native encode frame 1 indices",
		);
	} finally {
		decoder.free();
	}
}

function validateNativeIndexedGifPerFrameDelays() {
	const encoded = toByteArray(
		wasmCore.encode_indexed_gif_with_delays(
			new Uint8Array([0, 1, 0]),
			1,
			1,
			3,
			new Uint32Array([0x000000, 0xffffff]),
			new Uint16Array([2, 5, 13]),
			0,
		),
	);
	const omg = new OmgGifReader(encoded);

	assertEqual(omg.numFrames(), 3, "native per-frame delay frame count");
	assertEqual(omg.frameInfo(0).delay, 2, "native per-frame delay frame 0");
	assertEqual(omg.frameInfo(1).delay, 5, "native per-frame delay frame 1");
	assertEqual(omg.frameInfo(2).delay, 13, "native per-frame delay frame 2");
}

function validateNativeIndexedDeltaGifEncoding() {
	const width = 4;
	const height = 4;
	const frame0 = new Uint8Array(width * height).fill(1);
	const frame1 = frame0.slice();
	frame1[1] = 2;
	frame1[5] = 2;
	const frames = new Uint8Array(width * height * 3);
	frames.set(frame0, 0);
	frames.set(frame1, width * height);
	frames.set(frame1, width * height * 2);

	const encoded = toByteArray(
		wasmCore.encode_indexed_delta_gif(
			frames,
			width,
			height,
			3,
			new Uint32Array([0x000000, 0xff0000, 0x00ff00]),
			4,
			0,
		),
	);
	const omg = new OmgGifReader(encoded);
	const decoder = new wasmCore.WtfGifCore(encoded);
	try {
		assertEqual(omg.numFrames(), 3, "native delta encode frame count");
		assertEqual(omg.frameInfo(1).x, 1, "native delta frame 1 x");
		assertEqual(omg.frameInfo(1).y, 0, "native delta frame 1 y");
		assertEqual(omg.frameInfo(1).width, 1, "native delta frame 1 width");
		assertEqual(omg.frameInfo(1).height, 2, "native delta frame 1 height");
		assertEqual(omg.frameInfo(2).x, 0, "native delta frame 2 x");
		assertEqual(omg.frameInfo(2).y, 0, "native delta frame 2 y");
		assertEqual(omg.frameInfo(2).width, 1, "native delta frame 2 width");
		assertEqual(omg.frameInfo(2).height, 1, "native delta frame 2 height");
		assertBytesEqual(
			toByteArray(decoder.decode_frame_indices(1)),
			new Uint8Array([2, 2]),
			"native delta frame 1 indices",
		);
	} finally {
		decoder.free();
	}
}

function validateNativeIndexedDeltaGifPerFrameDelays() {
	const width = 2;
	const height = 2;
	const encoded = toByteArray(
		wasmCore.encode_indexed_delta_gif_with_delays(
			new Uint8Array([
				1, 1, 1, 1, //
				1, 2, 1, 1, //
				1, 2, 1, 1,
			]),
			width,
			height,
			3,
			new Uint32Array([0x000000, 0xff0000, 0x00ff00]),
			new Uint16Array([3, 7, 11]),
			0,
		),
	);
	const omg = new OmgGifReader(encoded);

	assertEqual(omg.numFrames(), 3, "native delta per-frame delay frame count");
	assertEqual(omg.frameInfo(0).delay, 3, "native delta frame 0 delay");
	assertEqual(omg.frameInfo(1).delay, 7, "native delta frame 1 delay");
	assertEqual(omg.frameInfo(2).delay, 11, "native delta frame 2 delay");
	assertEqual(omg.frameInfo(1).x, 1, "native delta per-frame frame 1 x");
	assertEqual(omg.frameInfo(2).width, 1, "native delta per-frame no-op width");
}

function validateNativeRgbaGifEncoding() {
	const width = 2;
	const height = 2;
	const frames = new Uint8Array([
		255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255,
		0, 255, 0, 255, 0, 0, 255, 255, 255, 0, 0, 255, 255, 0, 0, 255,
	]);
	const encoded = toByteArray(
		wasmCore.encode_rgba_gif(
			frames,
			width,
			height,
			2,
			new Uint32Array(),
			5,
			0,
			false,
		),
	);
	const omg = new OmgGifReader(encoded);
	const decoder = new wasmCore.WtfGifCore(encoded);
	try {
		assertEqual(omg.numFrames(), 2, "native RGBA encode frame count");
		assertEqual(omg.frameInfo(0).delay, 5, "native RGBA encode frame 0 delay");
		assertBytesEqual(
			toByteArray(decoder.decode_frame_rgba(0)),
			frames.subarray(0, 16),
			"native RGBA encode frame 0 pixels",
		);
		assertBytesEqual(
			toByteArray(decoder.decode_frame_rgba(1)),
			frames.subarray(16),
			"native RGBA encode frame 1 pixels",
		);
	} finally {
		decoder.free();
	}
}

function validateNativeRgbaGifOptionsEncoding() {
	const width = 2;
	const height = 1;
	const frames = new Uint8Array([
		255, 0, 0, 64, 0, 0, 255, 255, 255, 0, 0, 255, 0, 0, 255, 255,
	]);
	const encoded = toByteArray(
		wasmCore.encode_rgba_gif_with_options(
			frames,
			width,
			height,
			2,
			new Uint32Array([0xff0000, 0x0000ff]),
			new Uint16Array([4, 9]),
			0,
			false,
			32,
		),
	);
	const omg = new OmgGifReader(encoded);
	const decoder = new wasmCore.WtfGifCore(encoded);
	try {
		assertEqual(omg.numFrames(), 2, "native RGBA options frame count");
		assertEqual(omg.frameInfo(0).delay, 4, "native RGBA options frame 0 delay");
		assertEqual(omg.frameInfo(1).delay, 9, "native RGBA options frame 1 delay");
		assertEqual(
			omg.frameInfo(0).transparent_index ?? null,
			null,
			"native RGBA options transparent index",
		);
		assertBytesEqual(
			toByteArray(decoder.decode_frame_rgba(0)),
			new Uint8Array([255, 0, 0, 255, 0, 0, 255, 255]),
			"native RGBA options frame 0 pixels",
		);
	} finally {
		decoder.free();
	}
}

function validateNativeRgbaDeltaGifEncoding() {
	const width = 3;
	const height = 2;
	const frame0 = new Uint8Array([
		255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
		255, 0, 0, 255, 255, 0, 0, 255,
	]);
	const frame1 = frame0.slice();
	frame1.set([0, 255, 0, 255], 8);
	const frames = new Uint8Array(frame0.length + frame1.length);
	frames.set(frame0, 0);
	frames.set(frame1, frame0.length);

	const encoded = toByteArray(
		wasmCore.encode_rgba_gif(
			frames,
			width,
			height,
			2,
			new Uint32Array([0xff0000, 0x00ff00]),
			0,
			0,
			true,
		),
	);
	const omg = new OmgGifReader(encoded);
	const decoder = new wasmCore.WtfGifCore(encoded);
	try {
		assertEqual(omg.frameInfo(1).x, 2, "native RGBA delta frame 1 x");
		assertEqual(omg.frameInfo(1).y, 0, "native RGBA delta frame 1 y");
		assertEqual(omg.frameInfo(1).width, 1, "native RGBA delta frame 1 width");
		assertEqual(omg.frameInfo(1).height, 1, "native RGBA delta frame 1 height");
		assertBytesEqual(
			toByteArray(decoder.decode_frame_indices(1)),
			new Uint8Array([1]),
			"native RGBA delta frame 1 indices",
		);
	} finally {
		decoder.free();
	}
}

const fixtures = readdirSync(gifsDir)
	.filter((file) => file.endsWith(".gif"))
	.sort()
	.map((file) => [file, readFileSync(join(gifsDir, file))]);

fixtures.push(["synthetic-local-palette.gif", createLocalPaletteGif()]);
fixtures.push(["synthetic-offset-frame.gif", createOffsetFrameGif()]);
fixtures.push([
	"synthetic-transparent-offset-frame.gif",
	createTransparentOffsetFrameGif(),
]);

for (const [label, data] of fixtures) {
	validateGif(data, label);
	validatePixelPerfectReencode(data, label);
}
validateNativeIndexedGifEncoding();
validateNativeIndexedGifPerFrameDelays();
validateNativeIndexedDeltaGifEncoding();
validateNativeIndexedDeltaGifPerFrameDelays();
validateNativeRgbaGifEncoding();
validateNativeRgbaGifOptionsEncoding();
validateNativeRgbaDeltaGifEncoding();

console.log(
	`wtfgif Rust/Wasm metadata + index + frame/composited/delta RGBA/BGRA + indexed/RGBA encode validation passed: ${fixtures.length} GIFs, ${wasmCore.core_version()} core`,
);
