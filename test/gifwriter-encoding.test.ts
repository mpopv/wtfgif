import { describe, expect, test } from "vitest";
import {
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	GifReader,
	GifWriter,
} from "../src/index";

// Unit tests focused on GifWriter's encoding-related utilities

describe("GifWriter encoding utilities", () => {
	test("keeps mutable frame reuse correct", () => {
		const palette = [0x000000, 0xffffff];
		const frame = new Uint8Array([0, 0, 0, 0]);
		const buf = new Uint8Array(256);
		const writer = new GifWriter(buf, 2, 2, { palette });

		writer.addFrame(0, 0, 2, 2, frame);
		frame.fill(1);
		writer.addFrame(0, 0, 2, 2, frame);

		const reader = new GifReader(buf.slice(0, writer.end()));
		const pixels = new Uint8Array(16);
		reader.decodeAndBlitFrameRGBA(1, pixels);
		expect(Array.from(pixels)).toStrictEqual([
			255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
			255,
		]);
	});

	test("allows adding frames after calling end", () => {
		const palette = [0x000000, 0xffffff];
		const buf = new Uint8Array(100);
		const writer = new GifWriter(buf, 1, 1, { palette });
		const frame1 = new Uint8Array([0]);
		const frame2 = new Uint8Array([1]);

		writer.addFrame(0, 0, 1, 1, frame1);
		const len1 = writer.end();

		// Save prefix without trailer to ensure it remains untouched
		const prefix = buf.slice(0, len1 - 1);

		// Adding another frame after end() should remove the trailer and append data
		writer.addFrame(0, 0, 1, 1, frame2);
		const len2 = writer.end();
		expect(len2).toBeGreaterThan(len1);
		expect(buf.slice(0, len1 - 1)).toStrictEqual(prefix);

		// Decode to verify both frames exist and have expected colors
		const gif = buf.slice(0, len2);
		const reader = new GifReader(gif);
		expect(reader.numFrames()).toBe(2);

		const out = new Uint8Array(4);
		reader.decodeAndBlitFrameRGBA(0, out);
		expect(Array.from(out)).toStrictEqual([0, 0, 0, 255]);
		reader.decodeAndBlitFrameRGBA(1, out);
		expect(Array.from(out)).toStrictEqual([255, 255, 255, 255]);
	});

	test("supports swapping output buffer and position", () => {
		const palette = [0x000000, 0xffffff];
		const frame = new Uint8Array([0]);

		// Reference encoding for comparison
		const refBuf = new Uint8Array(50);
		const refWriter = new GifWriter(refBuf, 1, 1, { palette });
		refWriter.addFrame(0, 0, 1, 1, frame);
		const refLen = refWriter.end();
		const refGif = refBuf.slice(0, refLen);

		// Create writer and swap buffer before encoding frame
		const buf1 = new Uint8Array(50);
		const writer = new GifWriter(buf1, 1, 1, { palette });
		const headerLen = writer.getOutputBufferPosition();
		expect(headerLen).toBeGreaterThan(0);

		const buf2 = new Uint8Array(50);
		// Preserve header into new buffer
		buf2.set(buf1.slice(0, headerLen));
		writer.setOutputBuffer(buf2);
		writer.setOutputBufferPosition(headerLen);

		writer.addFrame(0, 0, 1, 1, frame);
		const len = writer.end();

		expect(writer.getOutputBuffer()).toBe(buf2);
		expect(writer.getOutputBufferPosition()).toBe(len);
		expect(buf2.slice(0, len)).toStrictEqual(refGif);
	});

	test("addFrameDelta writes changed rectangles that decode like full frames", () => {
		const width = 4;
		const height = 4;
		const palette = [0x000000, 0xff0000, 0x00ff00];
		const frame0 = new Uint8Array(width * height).fill(1);
		const frame1 = frame0.slice();
		frame1[1] = 2;
		frame1[5] = 2;

		const fullBuf = new Uint8Array(1000);
		const fullWriter = new GifWriter(fullBuf, width, height, { palette });
		fullWriter.addFrame(0, 0, width, height, frame0, { delay: 2 });
		fullWriter.addFrame(0, 0, width, height, frame1, { delay: 3 });
		const fullGif = fullBuf.slice(0, fullWriter.end());

		const deltaBuf = new Uint8Array(1000);
		const deltaWriter = new GifWriter(deltaBuf, width, height, { palette });
		deltaWriter.addFrameDelta(frame0, { delay: 2 });
		deltaWriter.addFrameDelta(frame1, { delay: 3 });
		const deltaGif = deltaBuf.slice(0, deltaWriter.end());

		const deltaReader = new GifReader(deltaGif);
		expect(deltaReader.frameInfo(1).x).toBe(1);
		expect(deltaReader.frameInfo(1).y).toBe(0);
		expect(deltaReader.frameInfo(1).width).toBe(1);
		expect(deltaReader.frameInfo(1).height).toBe(2);
		expect(deltaGif.length).toBeLessThan(fullGif.length);

		const fullReader = new GifReader(fullGif);
		const fullPixels = new Uint8Array(width * height * 4);
		const deltaPixels = new Uint8Array(width * height * 4);
		for (let frameIndex = 0; frameIndex < 2; frameIndex++) {
			fullReader.decodeAndBlitFrameRGBA(frameIndex, fullPixels);
			deltaReader.decodeAndBlitFrameRGBA(frameIndex, deltaPixels);
			expect(deltaPixels).toStrictEqual(fullPixels);
		}
	});

	test("addFrameDelta emits a no-op rectangle for unchanged frames", () => {
		const width = 3;
		const height = 3;
		const palette = [0x000000, 0xffffff];
		const frame = new Uint8Array(width * height).fill(1);
		const buf = new Uint8Array(1000);
		const writer = new GifWriter(buf, width, height, { palette });

		writer.addFrameDelta(frame, { delay: 4 });
		writer.addFrameDelta(frame, { delay: 5 });
		const gif = buf.slice(0, writer.end());
		const reader = new GifReader(gif);

		expect(reader.frameInfo(1).x).toBe(0);
		expect(reader.frameInfo(1).y).toBe(0);
		expect(reader.frameInfo(1).width).toBe(1);
		expect(reader.frameInfo(1).height).toBe(1);
		expect(reader.frameInfo(1).delay).toBe(5);

		const first = new Uint8Array(width * height * 4);
		reader.decodeAndBlitFrameRGBA(0, first);
		const second = first.slice();
		reader.decodeAndBlitFrameRGBA(1, second);
		expect(second).toStrictEqual(first);
	});

	test("addFrameDelta supports number array frames", () => {
		const width = 5;
		const height = 4;
		const palette = [0x000000, 0xffffff, 0x0000ff];
		const frame0 = Array.from({ length: width * height }, () => 1);
		const frame1 = frame0.slice();
		frame1[2 + width] = 2;
		frame1[3 + width] = 2;
		frame1[2 + width * 2] = 2;
		frame1[3 + width * 2] = 2;

		const fullBuf = new Uint8Array(1000);
		const fullWriter = new GifWriter(fullBuf, width, height, { palette });
		fullWriter.addFrame(0, 0, width, height, frame0);
		fullWriter.addFrame(0, 0, width, height, frame1);
		const fullGif = fullBuf.slice(0, fullWriter.end());

		const deltaBuf = new Uint8Array(1000);
		const deltaWriter = new GifWriter(deltaBuf, width, height, { palette });
		deltaWriter.addFrameDelta(frame0);
		deltaWriter.addFrameDelta(frame1);
		const deltaGif = deltaBuf.slice(0, deltaWriter.end());

		const deltaReader = new GifReader(deltaGif);
		expect(deltaReader.frameInfo(1).x).toBe(2);
		expect(deltaReader.frameInfo(1).y).toBe(1);
		expect(deltaReader.frameInfo(1).width).toBe(2);
		expect(deltaReader.frameInfo(1).height).toBe(2);

		const fullReader = new GifReader(fullGif);
		const fullPixels = new Uint8Array(width * height * 4);
		const deltaPixels = new Uint8Array(width * height * 4);
		for (let frameIndex = 0; frameIndex < 2; frameIndex++) {
			fullReader.decodeAndBlitFrameRGBA(frameIndex, fullPixels);
			deltaReader.decodeAndBlitFrameRGBA(frameIndex, deltaPixels);
			expect(deltaPixels).toStrictEqual(fullPixels);
		}
	});

	test("encodeIndexedGifFrames writes flat indexed frames", () => {
		const width = 2;
		const height = 2;
		const palette = [0x000000, 0xff0000, 0x0000ff];
		const gif = encodeIndexedGifFrames({
			width,
			height,
			palette,
			frames: new Uint8Array([1, 1, 1, 1, 2, 0, 0, 2]),
			delay: 6,
			loop: 0,
			backend: "javascript",
		});
		const reader = new GifReader(gif);

		expect(reader.numFrames()).toBe(2);
		expect(reader.frameInfo(0).delay).toBe(6);
		expect(reader.frameInfo(1).delay).toBe(6);

		const first = new Uint8Array(width * height * 4);
		const second = new Uint8Array(width * height * 4);
		reader.decodeAndBlitFrameRGBA(0, first);
		reader.decodeAndBlitFrameRGBA(1, second);
		expect(Array.from(first)).toStrictEqual([
			255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
		]);
		expect(Array.from(second)).toStrictEqual([
			0, 0, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 255, 255,
		]);
	});

	test("encodeIndexedGifFrames writes per-frame delays", () => {
		const gif = encodeIndexedGifFrames({
			width: 1,
			height: 1,
			palette: [0x000000, 0xffffff],
			frames: new Uint8Array([0, 1, 0]),
			delay: [2, 7, 11],
			loop: 0,
		});
		const reader = new GifReader(gif);

		expect(reader.numFrames()).toBe(3);
		expect(reader.frameInfo(0).delay).toBe(2);
		expect(reader.frameInfo(1).delay).toBe(7);
		expect(reader.frameInfo(2).delay).toBe(11);
	});

	test("encodeIndexedGifFrames can write changed rectangles", () => {
		const width = 4;
		const height = 4;
		const palette = [0x000000, 0xff0000, 0x00ff00];
		const frame0 = new Uint8Array(width * height).fill(1);
		const frame1 = frame0.slice();
		frame1[1] = 2;
		frame1[5] = 2;

		const fullGif = encodeIndexedGifFrames({
			width,
			height,
			palette,
			frames: [frame0, frame1],
			delay: 3,
			backend: "javascript",
		});
		const deltaGif = encodeIndexedGifFrames({
			width,
			height,
			palette,
			frames: [frame0, frame1],
			delay: [3, 9],
			backend: "javascript",
			delta: true,
		});

		const fullReader = new GifReader(fullGif);
		const deltaReader = new GifReader(deltaGif);
		expect(deltaReader.frameInfo(1).x).toBe(1);
		expect(deltaReader.frameInfo(1).y).toBe(0);
		expect(deltaReader.frameInfo(1).width).toBe(1);
		expect(deltaReader.frameInfo(1).height).toBe(2);
		expect(deltaReader.frameInfo(0).delay).toBe(3);
		expect(deltaReader.frameInfo(1).delay).toBe(9);
		expect(deltaGif.length).toBeLessThan(fullGif.length);

		const fullPixels = new Uint8Array(width * height * 4);
		const deltaPixels = new Uint8Array(width * height * 4);
		for (let frameIndex = 0; frameIndex < 2; frameIndex++) {
			fullReader.decodeAndBlitFrameRGBA(frameIndex, fullPixels);
			deltaReader.decodeAndBlitFrameRGBA(frameIndex, deltaPixels);
			expect(deltaPixels).toStrictEqual(fullPixels);
		}
	});

	test("encodeRgbaGifFrames writes exact-color RGBA frames", () => {
		const width = 2;
		const height = 2;
		const frames = new Uint8Array([
			255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 255, 0,
			255, 0, 0, 255, 255, 255, 0, 0, 255, 255, 0, 0, 255,
		]);
		const gif = encodeRgbaGifFrames({
			width,
			height,
			frames,
			delay: 7,
			loop: 0,
			backend: "javascript",
		});
		const reader = new GifReader(gif);

		expect(reader.numFrames()).toBe(2);
		expect(reader.frameInfo(0).delay).toBe(7);
		expect(reader.frameInfo(1).delay).toBe(7);

		const first = new Uint8Array(width * height * 4);
		const second = new Uint8Array(width * height * 4);
		reader.decodeAndBlitFrameRGBA(0, first);
		reader.decodeAndBlitFrameRGBA(1, second);
		expect(first).toStrictEqual(frames.subarray(0, 16));
		expect(second).toStrictEqual(frames.subarray(16));
	});

	test("encodeRgbaGifFrames writes per-frame delays", () => {
		const width = 1;
		const height = 1;
		const frames = new Uint8Array([
			255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255,
		]);
		const gif = encodeRgbaGifFrames({
			width,
			height,
			frames,
			palette: [0xff0000, 0x00ff00, 0x0000ff],
			delay: new Uint16Array([4, 8, 12]),
		});
		const reader = new GifReader(gif);

		expect(reader.numFrames()).toBe(3);
		expect(reader.frameInfo(0).delay).toBe(4);
		expect(reader.frameInfo(1).delay).toBe(8);
		expect(reader.frameInfo(2).delay).toBe(12);
	});

	test("encodeRgbaGifFrames preserves transparent alpha with a generated palette", () => {
		const gif = encodeRgbaGifFrames({
			width: 2,
			height: 1,
			frames: new Uint8Array([255, 0, 0, 255, 0, 0, 255, 0]),
		});
		const reader = new GifReader(gif);
		const prepared = reader.preparePlayback();

		expect(reader.frameInfo(0).transparent_index).not.toBeNull();
		expect(Array.from(prepared.getFrameBytes(0)!)).toStrictEqual([
			255, 0, 0, 255, 0, 0, 0, 0,
		]);
		prepared.dispose();
	});

	test("encodeRgbaGifFrames lets callers choose the alpha threshold", () => {
		const frame = new Uint8Array([255, 0, 0, 64, 0, 0, 255, 255]);
		const defaultGif = encodeRgbaGifFrames({
			width: 2,
			height: 1,
			palette: [0xff0000, 0x0000ff],
			frames: frame,
		});
		const lowerThresholdGif = encodeRgbaGifFrames({
			width: 2,
			height: 1,
			palette: [0xff0000, 0x0000ff],
			frames: frame,
			alphaThreshold: 32,
		});

		const defaultReader = new GifReader(defaultGif);
		const defaultPrepared = defaultReader.preparePlayback();
		expect(defaultReader.frameInfo(0).transparent_index).not.toBeNull();
		expect(Array.from(defaultPrepared.getFrameBytes(0)!)).toStrictEqual([
			0, 0, 0, 0, 0, 0, 255, 255,
		]);
		defaultPrepared.dispose();

		const lowerThresholdReader = new GifReader(lowerThresholdGif);
		const lowerThresholdPrepared = lowerThresholdReader.preparePlayback();
		expect(lowerThresholdReader.frameInfo(0).transparent_index).toBeNull();
		expect(Array.from(lowerThresholdPrepared.getFrameBytes(0)!)).toStrictEqual([
			255, 0, 0, 255, 0, 0, 255, 255,
		]);
		lowerThresholdPrepared.dispose();
	});

	test("high-level encoders validate delay arrays and alpha threshold", () => {
		expect(() =>
			encodeIndexedGifFrames({
				width: 1,
				height: 1,
				palette: [0x000000, 0xffffff],
				frames: new Uint8Array([0, 1]),
				delay: [1],
			}),
		).toThrow(/Delay count/);
		expect(() =>
			encodeRgbaGifFrames({
				width: 1,
				height: 1,
				frames: new Uint8Array([0, 0, 0, 255]),
				alphaThreshold: 256,
			}),
		).toThrow(/Alpha threshold/);
	});

	test("encodeRgbaGifFrames reserves a transparent slot with a caller palette", () => {
		const gif = encodeRgbaGifFrames({
			width: 2,
			height: 1,
			palette: [0xff0000],
			frames: new Uint8Array([255, 0, 0, 255, 0, 0, 255, 0]),
		});
		const reader = new GifReader(gif);
		const prepared = reader.preparePlayback();

		expect(reader.frameInfo(0).transparent_index).toBe(1);
		expect(Array.from(prepared.getFrameBytes(0)!)).toStrictEqual([
			255, 0, 0, 255, 0, 0, 0, 0,
		]);
		prepared.dispose();
	});

	test("encodeRgbaGifFrames can write changed rectangles", () => {
		const width = 3;
		const height = 2;
		const frame0 = new Uint8Array([
			255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0,
			255, 255, 0, 0, 255,
		]);
		const frame1 = frame0.slice();
		frame1.set([0, 255, 0, 255], 8);
		const frames = new Uint8Array(frame0.length + frame1.length);
		frames.set(frame0, 0);
		frames.set(frame1, frame0.length);

		const gif = encodeRgbaGifFrames({
			width,
			height,
			frames,
			palette: [0xff0000, 0x00ff00],
			backend: "javascript",
			delta: true,
		});
		const reader = new GifReader(gif);

		expect(reader.frameInfo(1).x).toBe(2);
		expect(reader.frameInfo(1).y).toBe(0);
		expect(reader.frameInfo(1).width).toBe(1);
		expect(reader.frameInfo(1).height).toBe(1);

		const decoded = new Uint8Array(width * height * 4);
		reader.decodeAndBlitFrameRGBA(0, decoded);
		reader.decodeAndBlitFrameRGBA(1, decoded);
		expect(decoded).toStrictEqual(frame1);
	});

	test("fast compression can quantize arbitrary full-color RGBA", () => {
		const width = 300;
		const frames = new Uint8Array(width * 4);
		for (let x = 0; x < width; x++) {
			const offset = x * 4;
			frames[offset] = x & 0xff;
			frames[offset + 1] = (x >> 8) & 0xff;
			frames[offset + 2] = (x * 17) & 0xff;
			frames[offset + 3] = 255;
		}

		const gif = encodeRgbaGifFrames({
			width,
			height: 1,
			frames,
			compression: "fast",
			quantization: "fast",
			backend: "javascript",
		});
		const reader = new GifReader(gif);
		const decoded = new Uint8Array(frames.length);
		reader.decodeAndBlitFrameRGBA(0, decoded);

		expect(reader.numFrames()).toBe(1);
		expect(decoded.some((value) => value !== 0)).toBe(true);
	});

	test("quality quantization reduces error relative to fixed fast quantization", () => {
		const width = 64;
		const height = 64;
		const frames = new Uint8Array(width * height * 4);
		for (let y = 0; y < height; y++) {
			for (let x = 0; x < width; x++) {
				const offset = (y * width + x) * 4;
				frames[offset] = Math.round((x * 255) / (width - 1));
				frames[offset + 1] = Math.round((y * 255) / (height - 1));
				frames[offset + 2] = (x * 3 + y * 5) & 0xff;
				frames[offset + 3] = 255;
			}
		}
		const encode = (quantization: "fast" | "quality") =>
			encodeRgbaGifFrames({
				width,
				height,
				frames,
				compression: "fast",
				quantization,
				backend: "javascript",
			});
		const error = (gif: Uint8Array) => {
			const reader = new GifReader(gif);
			const decoded = new Uint8Array(frames.length);
			reader.decodeAndBlitFrameRGBA(0, decoded);
			let squaredError = 0;
			for (let offset = 0; offset < frames.length; offset += 4) {
				for (let channel = 0; channel < 3; channel++) {
					const difference =
						frames[offset + channel]! - decoded[offset + channel]!;
					squaredError += difference * difference;
				}
			}
			return squaredError;
		};

		expect(error(encode("quality"))).toBeLessThan(error(encode("fast")));
	});

	test("local exact palettes preserve independently indexed frames", () => {
		const width = 16;
		const height = 16;
		const frameBytes = width * height * 4;
		const frames = new Uint8Array(frameBytes * 2);
		for (let value = 0; value < 256; value++) {
			const first = value * 4;
			frames[first] = value;
			frames[first + 3] = 255;
			const second = frameBytes + value * 4;
			frames[second + 1] = value;
			frames[second + 2] = 1;
			frames[second + 3] = 255;
		}

		expect(() =>
			encodeRgbaGifFrames({
				width,
				height,
				frames,
				compression: "fast",
				quantization: "exact",
				backend: "javascript",
			}),
		).toThrow(/at most 256/);

		const gif = encodeRgbaGifFrames({
			width,
			height,
			frames,
			compression: "fast",
			quantization: "exact",
			paletteMode: "local",
			backend: "javascript",
		});
		const reader = new GifReader(gif);
		expect(reader.numFrames()).toBe(2);
		for (let frame = 0; frame < 2; frame++) {
			const decoded = new Uint8Array(frameBytes);
			reader.decodeAndBlitFrameRGBA(frame, decoded);
			expect(decoded).toStrictEqual(
				frames.subarray(frame * frameBytes, (frame + 1) * frameBytes),
			);
			expect(reader.frameInfo(frame).disposal).toBe(2);
		}
	});
});
