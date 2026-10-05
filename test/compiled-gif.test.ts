import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "vitest";
import {
	boomerangGifPixelPerfect,
	compileGif,
	GifReader,
	GifWriter,
	retimeGifPixelPerfect,
	reverseGifPixelPerfect,
} from "../src/index";

function createGif(
	options: { loop?: number; delays?: readonly number[] } = {},
) {
	const delays = options.delays ?? [4, 9];
	const output = new Uint8Array(1024);
	const writer = new GifWriter(output, 2, 2, {
		palette: [0x000000, 0xffffff],
		...(options.loop === undefined ? {} : { loop: options.loop }),
	});
	writer.addFrame(0, 0, 2, 2, Uint8Array.of(0, 1, 1, 0), {
		delay: delays[0]!,
	});
	writer.addFrame(0, 0, 2, 2, Uint8Array.of(1, 0, 0, 1), {
		delay: delays[1]!,
	});
	return output.slice(0, writer.end());
}

function createTransparentGif(
	options: { partial?: boolean; disposal?: number } = {},
): Uint8Array {
	const output = new Uint8Array(1024);
	const writer = new GifWriter(output, 2, 2, {
		loop: 0,
		palette: [0x000000, 0xffffff],
	});
	const width = options.partial ? 1 : 2;
	const height = options.partial ? 1 : 2;
	const framePixels = options.partial
		? [Uint8Array.of(1), Uint8Array.of(0)]
		: [Uint8Array.of(0, 1, 1, 0), Uint8Array.of(1, 0, 0, 1)];
	for (const [index, delay] of [4, 9].entries()) {
		writer.addFrame(0, 0, width, height, framePixels[index]!, {
			delay,
			disposal: options.disposal ?? 2,
			transparent: 0,
		});
	}
	return output.slice(0, writer.end());
}

function findSequence(bytes: Uint8Array, sequence: ArrayLike<number>): number {
	outer: for (
		let offset = 0;
		offset <= bytes.length - sequence.length;
		offset++
	) {
		for (let index = 0; index < sequence.length; index++) {
			if (bytes[offset + index] !== sequence[index]) {
				continue outer;
			}
		}
		return offset;
	}
	return -1;
}

function insertBytes(
	source: Uint8Array,
	offset: number,
	insertion: Uint8Array,
): Uint8Array {
	const output = new Uint8Array(source.length + insertion.length);
	output.set(source.subarray(0, offset));
	output.set(insertion, offset);
	output.set(source.subarray(offset), offset + insertion.length);
	return output;
}

function decodeFrames(gif: Uint8Array): Uint8Array[] {
	const reader = new GifReader(gif);
	return Array.from({ length: reader.numFrames() }, (_, index) => {
		const pixels = new Uint8Array(reader.width * reader.height * 4);
		reader.decodeAndBlitFrameRGBA(index, pixels);
		return pixels;
	});
}

function decodeSampledFrames(gif: Uint8Array): Uint8Array[] {
	const reader = new GifReader(gif);
	const indices = [
		...new Set([
			0,
			Math.floor(reader.numFrames() / 2),
			Math.max(0, reader.numFrames() - 1),
		]),
	];
	return indices.map((index) => {
		const pixels = new Uint8Array(reader.width * reader.height * 4);
		reader.decodeAndBlitFrameRGBA(index, pixels);
		return pixels;
	});
}

function decodeCompositedFrames(gif: Uint8Array): Uint8Array[] {
	const reader = new GifReader(gif);
	const canvas = new Uint8Array(reader.width * reader.height * 4);
	const frames: Uint8Array[] = [];
	for (let index = 0; index < reader.numFrames(); index++) {
		const info = reader.frameInfo(index);
		const restore = info.disposal === 3 ? new Uint8Array(canvas) : null;
		reader.decodeAndBlitFrameRGBA(index, canvas);
		frames.push(new Uint8Array(canvas));
		if (info.disposal === 2) {
			for (let row = info.y; row < info.y + info.height; row++) {
				canvas.fill(
					0,
					(row * reader.width + info.x) * 4,
					(row * reader.width + info.x + info.width) * 4,
				);
			}
		} else if (restore) {
			canvas.set(restore);
		}
	}
	return frames;
}

function compressedFrameBytes(gif: Uint8Array): Uint8Array[] {
	const reader = new GifReader(gif);
	return Array.from({ length: reader.numFrames() }, (_, index) => {
		const frame = reader.frameInfo(index);
		return gif.slice(frame.data_offset, frame.data_offset + frame.data_length);
	});
}

describe("CompiledGif", () => {
	test("retimes existing controls without changing pixels or compressed data", () => {
		const source = createGif({ loop: 3 });
		const firstControl = findSequence(source, [0x21, 0xf9, 0x04]);
		expect(firstControl).toBeGreaterThanOrEqual(0);
		source[firstControl + 3] = 0x0f;
		source[firstControl + 6] = 0x01;
		const compiled = compileGif(source);

		expect(compiled.width).toBe(2);
		expect(compiled.height).toBe(2);
		expect(compiled.frameCount).toBe(2);
		expect(compiled.delays).toStrictEqual([4, 9]);
		expect(compiled.loopCount).toBe(3);
		expect(compiled.withDelays(compiled.delays).toUint8Array()).toStrictEqual(
			source,
		);

		const output = compiled.withDelays(Uint16Array.of(12, 345)).toUint8Array();
		const outputReader = new GifReader(output);
		expect(outputReader.frameInfo(0).delay).toBe(12);
		expect(outputReader.frameInfo(1).delay).toBe(345);
		expect(decodeFrames(output)).toStrictEqual(decodeFrames(source));
		expect(compressedFrameBytes(output)).toStrictEqual(
			compressedFrameBytes(source),
		);

		const differingOffsets = Array.from(source.keys()).filter(
			(offset) => source[offset] !== output[offset],
		);
		// 4 -> 12 changes one byte; 9 -> 345 changes both bytes.
		expect(differingOffsets).toHaveLength(3);
		expect(output[firstControl + 3]).toBe(0x0f);
		expect(output[firstControl + 6]).toBe(0x01);
	});

	test("inserts controls for frames that did not have them", () => {
		const source = Uint8Array.of(
			// GIF89a, 1x1, two-entry global palette
			0x47,
			0x49,
			0x46,
			0x38,
			0x39,
			0x61,
			0x01,
			0x00,
			0x01,
			0x00,
			0x80,
			0x00,
			0x00,
			0x00,
			0x00,
			0x00,
			0xff,
			0xff,
			0xff,
			// Image descriptor and one-pixel LZW stream
			0x2c,
			0x00,
			0x00,
			0x00,
			0x00,
			0x01,
			0x00,
			0x01,
			0x00,
			0x00,
			0x02,
			0x02,
			0x44,
			0x01,
			0x00,
			0x3b,
		);

		const output = retimeGifPixelPerfect(source, Uint16Array.of(27));
		expect(new GifReader(output).frameInfo(0).delay).toBe(27);
		expect(output.length).toBe(source.length + 8);

		const imageOffset = source.indexOf(0x2c);
		expect(output.slice(0, imageOffset)).toStrictEqual(
			source.slice(0, imageOffset),
		);
		expect(output.slice(imageOffset + 8)).toStrictEqual(
			source.slice(imageOffset),
		);
		expect(decodeFrames(output)).toStrictEqual(decodeFrames(source));
		expect(compressedFrameBytes(output)).toStrictEqual(
			compressedFrameBytes(source),
		);
	});

	test("promotes GIF87a only when adding an extension", () => {
		const source = Uint8Array.of(
			0x47,
			0x49,
			0x46,
			0x38,
			0x37,
			0x61,
			0x01,
			0x00,
			0x01,
			0x00,
			0x80,
			0x00,
			0x00,
			0x00,
			0x00,
			0x00,
			0xff,
			0xff,
			0xff,
			0x2c,
			0x00,
			0x00,
			0x00,
			0x00,
			0x01,
			0x00,
			0x01,
			0x00,
			0x00,
			0x02,
			0x02,
			0x44,
			0x01,
			0x00,
			0x3b,
		);
		const compiled = compileGif(source);
		expect(compiled.toUint8Array()).toStrictEqual(source);

		const retimed = compiled.withDelays(5).toUint8Array();
		expect(String.fromCharCode(...retimed.subarray(0, 6))).toBe("GIF89a");
		expect(
			retimed.filter((byte, index) => byte !== source[index]),
		).not.toHaveLength(0);
		expect(decodeFrames(retimed)).toStrictEqual(decodeFrames(source));
	});

	test("plain text consumes a pending control before the next image", () => {
		const source = createGif({ delays: [4, 9] });
		const firstImage = source.indexOf(0x2c);
		const plainText = Uint8Array.of(
			0x21,
			0x01,
			0x0c,
			0,
			0,
			0,
			0,
			1,
			0,
			1,
			0,
			1,
			1,
			0,
			0,
			0,
		);
		const withPlainText = insertBytes(source, firstImage, plainText);
		const retimed = compileGif(withPlainText)
			.withDelays(Uint16Array.of(27, 31))
			.toUint8Array();
		const shiftedImage = firstImage + plainText.length;

		// The original GCE belongs to the Plain Text block and remains untouched.
		expect(retimed.slice(firstImage - 8, firstImage)).toStrictEqual(
			source.slice(firstImage - 8, firstImage),
		);
		// The image receives its own newly inserted GCE.
		expect(retimed.slice(shiftedImage, shiftedImage + 8)).toStrictEqual(
			Uint8Array.of(0x21, 0xf9, 0x04, 0, 27, 0, 0, 0),
		);
	});

	test("patches an existing loop extension and inserts a missing one", () => {
		const withLoop = createGif({ loop: 2 });
		const patched = compileGif(withLoop).withLoop(0).toUint8Array();
		expect(new GifReader(patched).loopCount()).toBe(0);
		expect(patched.length).toBe(withLoop.length);
		expect(compressedFrameBytes(patched)).toStrictEqual(
			compressedFrameBytes(withLoop),
		);

		const withoutLoop = createGif();
		const inserted = compileGif(withoutLoop).withLoop(7).toUint8Array();
		expect(new GifReader(inserted).loopCount()).toBe(7);
		expect(inserted.length).toBe(withoutLoop.length + 19);
		expect(decodeFrames(inserted)).toStrictEqual(decodeFrames(withoutLoop));
		expect(compressedFrameBytes(inserted)).toStrictEqual(
			compressedFrameBytes(withoutLoop),
		);

		const animExts = withLoop.slice();
		const netscapeId = new TextEncoder().encode("NETSCAPE2.0");
		const animExtsId = new TextEncoder().encode("ANIMEXTS1.0");
		const idOffset = findSequence(animExts, netscapeId);
		expect(idOffset).toBeGreaterThanOrEqual(0);
		animExts.set(animExtsId, idOffset);
		const animExtsPatched = compileGif(animExts).withLoop(65535).toUint8Array();
		expect(compileGif(animExtsPatched).loopCount).toBe(65535);
		expect(animExtsPatched.slice(idOffset, idOffset + 11)).toStrictEqual(
			animExtsId,
		);
	});

	test("reverses and boomerangs structurally independent frames", () => {
		const source = createGif({ loop: 3, delays: [4, 9] });
		const compiled = compileGif(source);
		expect(compiled.canReorderFrames).toBe(true);
		expect(compiled.reorderFrames(Uint16Array.of(0, 1))).toStrictEqual(source);

		const reversed = compiled.reverseFrames();
		const reversedReader = new GifReader(reversed);
		expect(reversedReader.loopCount()).toBe(3);
		expect(
			Array.from(
				{ length: reversedReader.numFrames() },
				(_, index) => reversedReader.frameInfo(index).delay,
			),
		).toStrictEqual([9, 4]);
		expect(compressedFrameBytes(reversed)).toStrictEqual(
			compressedFrameBytes(source).slice().reverse(),
		);
		expect(decodeFrames(reversed)).toStrictEqual(
			decodeFrames(source).slice().reverse(),
		);
		expect(reverseGifPixelPerfect(source)).toStrictEqual(reversed);
		const retimedReverse = compiled
			.withDelays(Uint16Array.of(20, 30))
			.withLoop(7)
			.reverseFrames();
		const retimedReverseReader = new GifReader(retimedReverse);
		expect(retimedReverseReader.loopCount()).toBe(7);
		expect(
			Array.from(
				{ length: retimedReverseReader.numFrames() },
				(_, index) => retimedReverseReader.frameInfo(index).delay,
			),
		).toStrictEqual([30, 20]);

		const boomerang = boomerangGifPixelPerfect(source);
		const boomerangReader = new GifReader(boomerang);
		expect(
			Array.from(
				{ length: boomerangReader.numFrames() },
				(_, index) => boomerangReader.frameInfo(index).delay,
			),
		).toStrictEqual([4, 9, 9, 4]);
		expect(compressedFrameBytes(boomerang)).toStrictEqual([
			...compressedFrameBytes(source),
			...compressedFrameBytes(source).slice().reverse(),
		]);
		expect(() => compiled.reorderFrames([])).toThrow(/at least one/);
		expect(() => compiled.reorderFrames([0, 2])).toThrow(/outside/);
	});

	test("allows safe transparent disposal and rejects dependent compositions", () => {
		const safeSource = createTransparentGif({ disposal: 2 });
		const safe = compileGif(safeSource);
		expect(safe.canReorderFrames).toBe(true);
		expect(decodeCompositedFrames(safe.reverseFrames())).toStrictEqual(
			decodeCompositedFrames(safeSource).slice().reverse(),
		);

		const restoredSource = createTransparentGif({ disposal: 3 });
		const restored = compileGif(restoredSource);
		expect(restored.canReorderFrames).toBe(true);
		expect(decodeCompositedFrames(restored.reverseFrames())).toStrictEqual(
			decodeCompositedFrames(restoredSource).slice().reverse(),
		);

		const dependent = compileGif(createTransparentGif({ disposal: 1 }));
		expect(dependent.canReorderFrames).toBe(false);
		expect(() => dependent.reverseFrames()).toThrow(/original canvas order/);

		const partial = compileGif(
			createTransparentGif({ disposal: 2, partial: true }),
		);
		expect(partial.canReorderFrames).toBe(false);
		expect(() => partial.boomerangFrames()).toThrow(/full-canvas/);

		const anchoredSource = createGif();
		const firstControl = findSequence(anchoredSource, [0x21, 0xf9, 0x04]);
		const relativeSecondControl = findSequence(
			anchoredSource.subarray(firstControl + 3),
			[0x21, 0xf9, 0x04],
		);
		const secondControl = firstControl + 3 + relativeSecondControl;
		const anchored = compileGif(
			insertBytes(
				anchoredSource,
				secondControl,
				Uint8Array.of(0x21, 0xfe, 0x01, 0x41, 0x00),
			),
		);
		expect(anchored.canReorderFrames).toBe(false);
		expect(() => anchored.reverseFrames()).toThrow(/anchored metadata/);
	});

	test("snapshots input and returns independent output copies", () => {
		const source = createGif({ loop: 1 });
		const expected = source.slice();
		const compiled = compileGif(source);
		source.fill(0);

		const first = compiled.toUint8Array();
		expect(first).toStrictEqual(expected);
		first.fill(0);
		expect(compiled.toUint8Array()).toStrictEqual(expected);
	});

	test("preserves real-world frame payloads while changing every delay", () => {
		for (const file of [
			"18d30677-d255-4cc9-9933-c8d35306c1d5.gif",
			"Disappear Homer Simpson GIF.gif",
			"Dramatic Chipmunk GIF.gif",
			"party_blob.gif",
			"partyparrot.gif",
			"tenor.gif",
		]) {
			const source = new Uint8Array(
				readFileSync(join(__dirname, "gifs", file)),
			);
			const compiled = compileGif(source);
			const delays = Uint16Array.from(
				{ length: compiled.frameCount },
				(_, index) => (index * 997 + 0x1234) & 0xffff,
			);
			const output = compiled.withDelays(delays).withLoop(5).toUint8Array();
			const reader = new GifReader(output);

			expect(reader.loopCount(), file).toBe(5);
			expect(
				Array.from(
					{ length: reader.numFrames() },
					(_, index) => reader.frameInfo(index).delay,
				),
				file,
			).toStrictEqual(Array.from(delays));
			expect(decodeSampledFrames(output), file).toStrictEqual(
				decodeSampledFrames(source),
			);
			expect(compressedFrameBytes(output), file).toStrictEqual(
				compressedFrameBytes(source),
			);
		}
	});

	test("rejects invalid delays, loop counts, and malformed input", () => {
		const compiled = compileGif(createGif());
		expect(() => compiled.withDelays([1])).toThrow(/Expected 2 frame delays/);
		for (const value of [-1, 1.5, 65536, Number.NaN, Infinity, -Infinity]) {
			expect(() => compiled.withDelays([1, value])).toThrow(/integer|65535/);
			expect(() => compiled.withLoop(value)).toThrow(/integer|65535/);
		}
		expect(() => compileGif(Uint8Array.of(0x47, 0x49, 0x46))).toThrow(
			/Unexpected end/,
		);
		expect(() => compileGif(new Uint8Array(13))).toThrow(/Invalid GIF/);
		const missingTrailer = createGif().slice(0, -1);
		expect(() => compileGif(missingTrailer)).toThrow(/trailer/);
	});
});
