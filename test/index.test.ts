import { GifReader as OmgGifReader, GifWriter as OmgGifWriter } from "omggif";
import { describe, expect, test } from "vitest";
import {
	GifReader as WtfGifReader,
	GifWriter as WtfGifWriter,
} from "../src/index";
import type { Frame } from "../src/types";
import {
	expectGifSemanticsEqual,
	gifFixtures as gifFiles,
	readGifFixture,
} from "./helpers/gif";

describe("GIF file inventory", () => {
	test("discovers all GIF files in test directory", () => {
		expect(gifFiles.length).toBeGreaterThan(0);
	});
});

describe("Palette edge cases", () => {
	test("non power-of-two palette parity with omggif", () => {
		const width = 2;
		const height = 2;
		const palette3 = [0x000000, 0xffffff, 0xff0000];
		const frame = new Uint8Array([0, 1, 2, 0]);
		const bufOmg = new Uint8Array(100);
		const bufWtf = new Uint8Array(100);
		const padded = [...palette3, 0x000000];
		const omgWriter = new OmgGifWriter(bufOmg, width, height, {
			palette: padded,
		});
		omgWriter.addFrame(0, 0, width, height, Array.from(frame));
		const omgLen = omgWriter.end();
		const wtfWriter = new WtfGifWriter(bufWtf, width, height, {
			palette: palette3,
		});
		wtfWriter.addFrame(0, 0, width, height, frame);
		const wtfLen = wtfWriter.end();
		expectGifSemanticsEqual(bufWtf.slice(0, wtfLen), bufOmg.slice(0, omgLen));
	});

	test("two-color palette decodes correctly", () => {
		const width = 2;
		const height = 2;
		const palette = [0x000000, 0xffffff];
		const pixels = new Uint8Array([0, 1, 0, 1]);
		const buf = new Uint8Array(100);
		const writer = new WtfGifWriter(buf, width, height, { palette });
		writer.addFrame(0, 0, width, height, pixels);
		const len = writer.end();
		const gif = buf.slice(0, len);
		const reader = new WtfGifReader(gif);
		const wtfPixels = new Uint8Array(width * height * 4);
		const omgPixels = new Uint8Array(width * height * 4);
		reader.decodeAndBlitFrameRGBA(0, wtfPixels);
		new OmgGifReader(gif).decodeAndBlitFrameRGBA(0, omgPixels);
		expect(wtfPixels).toStrictEqual(omgPixels);
	});
});

describe("Small-frame streaming decoder", () => {
	test("matches omggif for interlaced frames", () => {
		const width = 5;
		const height = 5;
		const palette = [0x000000, 0xff0000, 0x00ff00, 0xffffff];
		const output = new Uint8Array(4096);
		const writer = new OmgGifWriter(output, width, height, { palette });
		const pixels = Array.from(
			{ length: width * height },
			(_, index) => index & 3,
		);
		writer.addFrame(0, 0, width, height, pixels, { delay: 1 });
		const gif = output.slice(0, writer.end());
		for (let index = 0; index < gif.length; index++) {
			if (gif[index] === 0x2c) {
				gif[index + 9] = gif[index + 9]! | 0x40;
				break;
			}
		}

		const omg = new OmgGifReader(gif);
		const wtf = new WtfGifReader(gif);
		const omgPixels = new Uint8Array(width * height * 4);
		const wtfPixels = new Uint8Array(width * height * 4);
		omg.decodeAndBlitFrameRGBA(0, omgPixels);
		wtf.decodeAndBlitFrameRGBA(0, wtfPixels);
		expect(wtfPixels).toStrictEqual(omgPixels);

		const omgBgra = new Uint8Array(width * height * 4);
		const wtfBgra = new Uint8Array(width * height * 4);
		omg.decodeAndBlitFrameBGRA(0, omgBgra);
		wtf.decodeAndBlitFrameBGRA(0, wtfBgra);
		expect(wtfBgra).toStrictEqual(omgBgra);
	});
});

describe("GifReader parity with omggif", () => {
	for (const file of gifFiles) {
		test(file, () => {
			const gif = readGifFixture(file);
			const omg = new OmgGifReader(gif);
			const wtf = new WtfGifReader(gif);
			expect(wtf.width).toBe(omg.width);
			expect(wtf.height).toBe(omg.height);
			expect(wtf.numFrames()).toBe(omg.numFrames());

			const fields = [
				"x",
				"y",
				"width",
				"height",
				"has_local_palette",
				"palette_offset",
				"palette_size",
				"data_offset",
				"data_length",
				"transparent_index",
				"interlaced",
				"delay",
				"disposal",
			] as (keyof Frame)[];
			const numFrames = omg.numFrames();
			const frameLimit = Math.min(numFrames, 3);
			const len = wtf.width * wtf.height * 4;
			for (let i = 0; i < frameLimit; i++) {
				const omgInfo = omg.frameInfo(i);
				const wtfInfo = wtf.frameInfo(i);
				const filteredWtf = Object.fromEntries(
					fields.map((f) => [f, wtfInfo[f]]),
				);
				expect(filteredWtf).toStrictEqual(omgInfo);

				// Exercise decoding APIs for coverage
				const bufRGBA = wtf.decodeFrameToTransferableRGBA(i);
				expect(bufRGBA.byteLength).toBe(len);
				const bufBGRA = wtf.decodeFrameToTransferableBGRA(i);
				expect(bufBGRA.byteLength).toBe(len);
				const abR = new ArrayBuffer(len);
				wtf.decodeFrameIntoBuffer(i, abR, "rgba");
				const abB = new ArrayBuffer(len);
				wtf.decodeFrameIntoBuffer(i, abB, "bgra");
				wtf.decodeAndBlitFrameRGBA(i, new Uint8Array(len));
				wtf.decodeAndBlitFrameBGRA(i, new Uint8Array(len));
			}

			expect(wtf.loopCount()).toBe(omg.loopCount());
			wtf.dispose();
		});
	}
});

describe("Pixel-perfect decoding compatibility", () => {
	for (const file of gifFiles) {
		test(`${file} - pixel data matches omggif exactly`, () => {
			const gif = readGifFixture(file);
			const omg = new OmgGifReader(gif);
			const wtf = new WtfGifReader(gif);

			const pixelCount = omg.width * omg.height * 4;
			const frameCount = Math.min(omg.numFrames(), 2); // Test first 2 frames for performance

			for (let frameIdx = 0; frameIdx < frameCount; frameIdx++) {
				// Test RGBA decoding
				const omgRGBA = new Uint8Array(pixelCount);
				const wtfRGBA = new Uint8Array(pixelCount);
				omg.decodeAndBlitFrameRGBA(frameIdx, omgRGBA);
				wtf.decodeAndBlitFrameRGBA(frameIdx, wtfRGBA);
				for (let i = 0; i < pixelCount; i += 4) {
					if (omgRGBA[i + 3] === 0) {
						omgRGBA[i] = omgRGBA[i + 1] = omgRGBA[i + 2] = 0;
					}
					if (wtfRGBA[i + 3] === 0) {
						wtfRGBA[i] = wtfRGBA[i + 1] = wtfRGBA[i + 2] = 0;
					}
				}
				expect(wtfRGBA).toStrictEqual(omgRGBA);

				// Test BGRA decoding
				const omgBGRA = new Uint8Array(pixelCount);
				const wtfBGRA = new Uint8Array(pixelCount);
				omg.decodeAndBlitFrameBGRA(frameIdx, omgBGRA);
				wtf.decodeAndBlitFrameBGRA(frameIdx, wtfBGRA);
				for (let i = 0; i < pixelCount; i += 4) {
					if (omgBGRA[i + 3] === 0) {
						omgBGRA[i] = omgBGRA[i + 1] = omgBGRA[i + 2] = 0;
					}
					if (wtfBGRA[i + 3] === 0) {
						wtfBGRA[i] = wtfBGRA[i + 1] = wtfBGRA[i + 2] = 0;
					}
				}
				expect(wtfBGRA).toStrictEqual(omgBGRA);
			}

			wtf.dispose();
		}, 60_000);
	}
});

describe("GifWriter compatibility with omggif", () => {
	test("encodes equivalent pixels and metadata", () => {
		const width = 2;
		const height = 2;
		const palette = [0x000000, 0xffffff, 0xff0000, 0x00ff00];
		const frame = new Uint8Array([0, 1, 1, 0]);
		const bufOmg = new Uint8Array(1000);
		const bufWtf = new Uint8Array(1000);
		const omgWriter = new OmgGifWriter(bufOmg, width, height, { palette });
		omgWriter.addFrame(0, 0, width, height, Array.from(frame));
		const omgLen = omgWriter.end();
		const wtfWriter = new WtfGifWriter(bufWtf, width, height, { palette });
		wtfWriter.addFrame(0, 0, width, height, frame);
		const wtfLen = wtfWriter.end();
		const omgGif = bufOmg.slice(0, omgLen);
		const wtfGif = bufWtf.slice(0, wtfLen);
		expectGifSemanticsEqual(wtfGif, omgGif);
	});

	test("starts each literal frame with a fresh code stream", () => {
		const width = 2;
		const height = 2;
		const palette = [0x000000, 0xffffff, 0xff0000, 0x00ff00];
		const frame1 = new Uint8Array([0, 1, 1, 0]);
		const frame2 = new Uint8Array([1, 0, 0, 1]);
		const buf = new Uint8Array(1000);
		const writer = new WtfGifWriter(buf, width, height, { palette });
		writer.addFrame(0, 0, width, height, frame1);
		writer.addFrame(0, 0, width, height, frame2);
		const len = writer.end();
		const gif = buf.slice(0, len);
		const omgReader = new OmgGifReader(gif);
		const wtfReader = new WtfGifReader(gif);
		const outLen = width * height * 4;
		const omgPixels = new Uint8Array(outLen);
		const wtfPixels = new Uint8Array(outLen);
		omgReader.decodeAndBlitFrameRGBA(1, omgPixels);
		wtfReader.decodeAndBlitFrameRGBA(1, wtfPixels);
		expect(wtfPixels).toStrictEqual(omgPixels);
	});

	test("encodes many small literal frames", () => {
		const width = 2;
		const height = 2;
		const palette = [0x000000, 0xffffff, 0xff0000, 0x00ff00];
		const frame1 = new Uint8Array([0, 1, 1, 0]);
		const frame2 = new Uint8Array([1, 0, 0, 1]);
		const frames = [frame1, frame2];
		const frameCount = 100;
		const buf = new Uint8Array(10000);
		const writer = new WtfGifWriter(buf, width, height, { palette });
		for (let i = 0; i < frameCount; i++) {
			const f = frames[i & 1]!;
			writer.addFrame(0, 0, width, height, f);
		}
		const len = writer.end();
		const gif = buf.slice(0, len);

		const omgReader = new OmgGifReader(gif);
		const wtfReader = new WtfGifReader(gif);
		const outLen = width * height * 4;

		// Precompute expected RGBA for the two frame patterns
		const expected = frames.map((data) => {
			const rgba = new Uint8Array(outLen);
			for (let i = 0; i < data.length; i++) {
				const color = palette[data[i]!]!;
				const o = i * 4;
				rgba[o] = (color >> 16) & 0xff;
				rgba[o + 1] = (color >> 8) & 0xff;
				rgba[o + 2] = color & 0xff;
				rgba[o + 3] = 0xff;
			}
			return rgba;
		});

		for (let i = 0; i < frameCount; i++) {
			const omgPixels = new Uint8Array(outLen);
			const wtfPixels = new Uint8Array(outLen);
			omgReader.decodeAndBlitFrameRGBA(i, omgPixels);
			wtfReader.decodeAndBlitFrameRGBA(i, wtfPixels);
			const expectedPixels = expected[i & 1];
			expect(wtfPixels).toStrictEqual(omgPixels);
			expect(wtfPixels).toStrictEqual(expectedPixels);
		}
	});

	test("local palette support", () => {
		const width = 2;
		const height = 2;
		const palette = [0x000000, 0xffffff, 0xff0000, 0x00ff00];
		const frame = new Uint8Array([0, 1, 1, 0]);
		const bufOmg = new Uint8Array(1000);
		const bufWtf = new Uint8Array(1000);
		const omgWriter = new OmgGifWriter(bufOmg, width, height);
		omgWriter.addFrame(0, 0, width, height, Array.from(frame), { palette });
		const omgLen = omgWriter.end();
		const wtfWriter = new WtfGifWriter(bufWtf, width, height);
		wtfWriter.addFrame(0, 0, width, height, frame, { palette });
		const wtfLen = wtfWriter.end();
		expectGifSemanticsEqual(bufWtf.slice(0, wtfLen), bufOmg.slice(0, omgLen));
	});

	test("supports explicit background index 0", () => {
		const width = 2;
		const height = 2;
		const palette = [0x000000, 0xffffff];
		const frame = new Uint8Array([0, 1, 1, 0]);
		const buf = new Uint8Array(100);
		const writer = new WtfGifWriter(buf, width, height, {
			palette,
			background: 0,
		});
		writer.addFrame(0, 0, width, height, frame);
		const len = writer.end();
		const gif = buf.slice(0, len);

		// Ensure other libraries can read the output
		const omgReader = new OmgGifReader(gif);
		const wtfReader = new WtfGifReader(gif);
		const outLen = width * height * 4;
		const omgPixels = new Uint8Array(outLen);
		const wtfPixels = new Uint8Array(outLen);
		omgReader.decodeAndBlitFrameRGBA(0, omgPixels);
		wtfReader.decodeAndBlitFrameRGBA(0, wtfPixels);
		expect(wtfPixels).toStrictEqual(omgPixels);
	});

	test("supports non-zero background index", () => {
		const width = 2;
		const height = 2;
		const palette = [0x000000, 0xffffff, 0xff0000, 0x00ff00];
		const frame = new Uint8Array([0, 1, 1, 0]);
		const bufOmg = new Uint8Array(100);
		const bufWtf = new Uint8Array(100);
		const omgWriter = new OmgGifWriter(bufOmg, width, height, {
			palette,
			background: 1,
		});
		omgWriter.addFrame(0, 0, width, height, Array.from(frame));
		const omgLen = omgWriter.end();
		const wtfWriter = new WtfGifWriter(bufWtf, width, height, {
			palette,
			background: 1,
		});
		wtfWriter.addFrame(0, 0, width, height, frame);
		const wtfLen = wtfWriter.end();
		const omgGif = bufOmg.slice(0, omgLen);
		const wtfGif = bufWtf.slice(0, wtfLen);
		expect(wtfGif[11]).toBe(1);
		expectGifSemanticsEqual(wtfGif, omgGif);
	});

	test("multi-frame parity with transparency and looping", () => {
		const width = 2;
		const height = 2;
		const palette = [0x000000, 0xffffff, 0xff0000, 0x00ff00];
		const frame1 = new Uint8Array([0, 1, 1, 0]);
		const frame2 = new Uint8Array([2, 3, 3, 2]);
		const bufOmg = new Uint8Array(1000);
		const bufWtf = new Uint8Array(1000);
		const omgWriter = new OmgGifWriter(bufOmg, width, height, {
			palette,
			loop: 1,
			background: 1,
		});
		omgWriter.addFrame(0, 0, width, height, Array.from(frame1), {
			delay: 5,
			disposal: 1,
		});
		omgWriter.addFrame(0, 0, width, height, Array.from(frame2), {
			delay: 10,
			disposal: 2,
			transparent: 2,
		});
		const omgLen = omgWriter.end();
		const wtfWriter = new WtfGifWriter(bufWtf, width, height, {
			palette,
			loop: 1,
			background: 1,
		});
		wtfWriter.addFrame(0, 0, width, height, frame1, { delay: 5, disposal: 1 });
		wtfWriter.addFrame(0, 0, width, height, frame2, {
			delay: 10,
			disposal: 2,
			transparent: 2,
		});
		const wtfLen = wtfWriter.end();
		const omgGif = bufOmg.slice(0, omgLen);
		const wtfGif = bufWtf.slice(0, wtfLen);
		expectGifSemanticsEqual(wtfGif, omgGif);
	});
});

describe("Cross-library write/read compatibility", () => {
	for (const file of gifFiles.slice(0, 3)) {
		// Test first 3 files to keep runtime reasonable
		test(`${file} - wtfgif can read/write then omggif can read`, () => {
			const originalGif = readGifFixture(file);
			const wtfReader = new WtfGifReader(originalGif);

			// Extract basic properties
			const width = wtfReader.width;
			const height = wtfReader.height;
			// Decode first frame to create a simple palette
			const pixelCount = width * height * 4;
			const firstFramePixels = new Uint8Array(pixelCount);
			wtfReader.decodeAndBlitFrameRGBA(0, firstFramePixels);

			// Create a simple 2-color palette for testing
			const palette = [0x000000, 0xffffff];
			const frameData = new Uint8Array(width * height);

			// Convert first frame to simple binary pattern for testing
			for (let i = 0; i < frameData.length; i++) {
				// Simple thresholding to create binary data
				const pixelOffset = i * 4;
				const brightness =
					(firstFramePixels[pixelOffset]! +
						firstFramePixels[pixelOffset + 1]! +
						firstFramePixels[pixelOffset + 2]!) /
					3;
				frameData[i] = brightness > 127 ? 1 : 0;
			}

			// Write GIF using wtfgif
			const outputBuf = new Uint8Array(originalGif.length * 2); // Allow extra space
			const wtfWriter = new WtfGifWriter(outputBuf, width, height, { palette });
			wtfWriter.addFrame(0, 0, width, height, frameData);
			const outputLen = wtfWriter.end();
			const wtfGif = outputBuf.slice(0, outputLen);

			// Verify omggif can read wtfgif-written GIF
			expect(() => {
				const omgReader = new OmgGifReader(wtfGif);
				expect(omgReader.width).toBe(width);
				expect(omgReader.height).toBe(height);
				expect(omgReader.numFrames()).toBeGreaterThan(0);

				// Test that omggif can decode wtfgif-written pixels
				const omgPixels = new Uint8Array(width * height * 4);
				omgReader.decodeAndBlitFrameRGBA(0, omgPixels);
				expect(omgPixels.length).toBe(width * height * 4);
			}).not.toThrow();

			wtfReader.dispose();
		});
	}

	test("repeated decoding stays pixel-compatible with omggif", () => {
		const file = gifFiles[0]!; // Use first available file
		const gif = readGifFixture(file);
		const omgReader = new OmgGifReader(gif);
		const wtfReader = new WtfGifReader(gif);

		const pixelCount = omgReader.width * omgReader.height * 4;
		const pixels = new Uint8Array(pixelCount);

		// Warm up both decoders
		omgReader.decodeAndBlitFrameRGBA(0, pixels);
		wtfReader.decodeAndBlitFrameRGBA(0, pixels);

		for (let i = 0; i < 10; i++) {
			omgReader.decodeAndBlitFrameRGBA(0, pixels);
		}
		const omgPixels = new Uint8Array(pixels);

		pixels.fill(0);
		for (let i = 0; i < 10; i++) {
			wtfReader.decodeAndBlitFrameRGBA(0, pixels);
		}
		expect(pixels).toStrictEqual(omgPixels);

		wtfReader.dispose();
	});
});

describe("Disposal metadata compatibility", () => {
	test("large partial transparent frames stay pixel-perfect", () => {
		const width = 512;
		const height = 512;
		const palette = [0xffffff, 0xff0000];
		const output = new Uint8Array(1_000_000);
		const writer = new OmgGifWriter(output, width, height, {
			loop: 0,
			palette,
		});
		writer.addFrame(0, 0, width, height, new Array(width * height).fill(0), {
			disposal: 1,
		});
		const rect = new Array(128 * 128).fill(1);
		writer.addFrame(128, 96, 128, 128, rect, {
			disposal: 1,
			transparent: 0,
		});
		const gif = output.slice(0, writer.end());
		const omg = new OmgGifReader(gif);
		const wtf = new WtfGifReader(gif);
		const omgRgba = new Uint8Array(width * height * 4);
		const wtfRgba = new Uint8Array(width * height * 4);
		const omgBgra = new Uint8Array(width * height * 4);
		const wtfBgra = new Uint8Array(width * height * 4);

		for (let frame = 0; frame < 2; frame++) {
			omg.decodeAndBlitFrameRGBA(frame, omgRgba);
			wtf.decodeAndBlitFrameRGBA(frame, wtfRgba);
			omg.decodeAndBlitFrameBGRA(frame, omgBgra);
			wtf.decodeAndBlitFrameBGRA(frame, wtfBgra);
		}

		expect(wtfRgba).toStrictEqual(omgRgba);
		expect(wtfBgra).toStrictEqual(omgBgra);
	});

	test("preserves restore-to-background metadata while blitting like omggif", () => {
		const width = 2;
		const height = 2;
		const palette = [0x000000, 0xff0000, 0x0000ff];
		const buf = new Uint8Array(1000);
		const writer = new WtfGifWriter(buf, width, height, {
			palette,
			background: 0,
		});
		const frame1 = new Uint8Array([1, 1, 1, 1]);
		writer.addFrame(0, 0, width, height, frame1, { disposal: 2 });
		const frame2 = new Uint8Array([2, 2]);
		writer.addFrame(1, 0, 1, 2, frame2);
		const len = writer.end();
		const gif = buf.slice(0, len);
		const omgReader = new OmgGifReader(gif);
		const wtfReader = new WtfGifReader(gif);
		expect(wtfReader.frameInfo(0).disposal).toBe(2);

		const outLen = width * height * 4;
		const wtfPixels = new Uint8Array(outLen);
		const omgPixels = new Uint8Array(outLen);

		omgReader.decodeAndBlitFrameRGBA(0, omgPixels);
		omgReader.decodeAndBlitFrameRGBA(1, omgPixels);
		wtfReader.decodeAndBlitFrameRGBA(0, wtfPixels);
		wtfReader.decodeAndBlitFrameRGBA(1, wtfPixels);

		expect(wtfPixels).toStrictEqual(omgPixels);
	});

	test("preserves restore-to-previous metadata while blitting like omggif", () => {
		const width = 2;
		const height = 2;
		const palette = [0x000000, 0xff0000, 0x00ff00, 0x0000ff];
		const buf = new Uint8Array(1000);
		const writer = new WtfGifWriter(buf, width, height, {
			palette,
			background: 0,
		});
		const frame0 = new Uint8Array([1, 1, 1, 1]);
		writer.addFrame(0, 0, width, height, frame0, { disposal: 1 });
		const frame1 = new Uint8Array([2, 2]);
		writer.addFrame(0, 0, 1, 2, frame1, { disposal: 3 });
		const frame2 = new Uint8Array([3, 3]);
		writer.addFrame(1, 0, 1, 2, frame2);
		const len = writer.end();
		const gif = buf.slice(0, len);
		const omgReader = new OmgGifReader(gif);
		const wtfReader = new WtfGifReader(gif);
		expect(wtfReader.frameInfo(1).disposal).toBe(3);

		const outLen = width * height * 4;
		const wtfPixels = new Uint8Array(outLen);
		const omgPixels = new Uint8Array(outLen);

		omgReader.decodeAndBlitFrameRGBA(0, omgPixels);
		omgReader.decodeAndBlitFrameRGBA(1, omgPixels);
		omgReader.decodeAndBlitFrameRGBA(2, omgPixels);
		wtfReader.decodeAndBlitFrameRGBA(0, wtfPixels);
		wtfReader.decodeAndBlitFrameRGBA(1, wtfPixels);
		wtfReader.decodeAndBlitFrameRGBA(2, wtfPixels);

		expect(wtfPixels).toStrictEqual(omgPixels);
	});
});
