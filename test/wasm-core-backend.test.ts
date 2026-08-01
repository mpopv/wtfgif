import { readFileSync } from "node:fs";
import { join } from "node:path";
import { GifReader as OmgGifReader } from "omggif";
import { afterEach, describe, expect, test } from "vitest";
import {
	createWasmCoreDecodeBackend,
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	GifReader,
	GifWriter,
	installWasmCoreBackend,
} from "../src/index";

const backend = createWasmCoreDecodeBackend();
const maybeTest = backend.isAvailable() ? test : test.skip;

function sentinelCanvas(width: number, height: number): Uint8Array {
	const canvas = new Uint8Array(width * height * 4);
	for (let offset = 0; offset < canvas.length; offset += 4) {
		canvas[offset] = 0x11;
		canvas[offset + 1] = 0x22;
		canvas[offset + 2] = 0x33;
		canvas[offset + 3] = 0x44;
	}
	return canvas;
}

function makeDisposalGif(): Uint8Array {
	const palette = [0x000000, 0xff0000, 0x00ff00, 0x0000ff];
	const buf = new Uint8Array(128);
	const writer = new GifWriter(buf, 2, 2, { palette });
	writer.addFrame(0, 0, 2, 2, new Uint8Array([1, 1, 1, 1]), {
		disposal: 1,
	});
	writer.addFrame(0, 0, 1, 2, new Uint8Array([2, 2]), {
		disposal: 3,
	});
	writer.addFrame(1, 0, 1, 2, new Uint8Array([3, 3]));
	return buf.slice(0, writer.end());
}

function makeDuplicateFrameGif(): Uint8Array {
	const palette = [0x000000, 0xff0000];
	const buf = new Uint8Array(100);
	const writer = new GifWriter(buf, 2, 2, { palette });
	writer.addFrame(0, 0, 2, 2, new Uint8Array([1, 1, 1, 1]));
	writer.addFrame(0, 0, 2, 2, new Uint8Array([1, 1, 1, 1]));
	return buf.slice(0, writer.end());
}

function makeQualityGradient(width: number, height: number): Uint8Array {
	const rgba = new Uint8Array(width * height * 4);
	let offset = 0;
	for (let y = 0; y < height; y += 1) {
		for (let x = 0; x < width; x += 1) {
			rgba[offset++] = (x * 13 + y * 7) & 255;
			rgba[offset++] = (x * 5 + y * 17) & 255;
			rgba[offset++] = (x * 19 + y * 3) & 255;
			rgba[offset++] = 255;
		}
	}
	return rgba;
}

describe("Rust/Wasm core decode backend", () => {
	afterEach(() => {
		GifReader.setDecodeBackend(null);
	});

	maybeTest("installs as the GifReader Wasm backend", () => {
		expect(installWasmCoreBackend()).toStrictEqual({
			name: "wtfgif-rust-wasm",
			available: true,
		});
	});

	maybeTest("prepares indexed frames that copy like omggif RGBA", () => {
		GifReader.setDecodeBackend(backend);
		const gif = readFileSync(join(__dirname, "gifs", "party_blob.gif"));
		const omg = new OmgGifReader(gif);
		const wtf = new GifReader(gif);
		const prepared = wtf.prepareFrames({
			backend: "native",
			cache: "indices",
			frameIndices: [0, 1, 2],
		});

		expect(prepared.frames.length).toBe(3);
		expect(prepared.frames[0]?.indices).toBeInstanceOf(Uint8Array);
		expect(prepared.frames[0]?.palette).toBeInstanceOf(Uint32Array);

		for (const frameIndex of [0, 1, 2]) {
			const expected = sentinelCanvas(omg.width, omg.height);
			const actual = sentinelCanvas(omg.width, omg.height);
			omg.decodeAndBlitFrameRGBA(frameIndex, expected);
			prepared.copyFrame(frameIndex, actual);
			expect(actual).toStrictEqual(expected);
		}

		prepared.dispose();
		wtf.returnToPool();
	});

	maybeTest("prepares composited playback frames like the JS backend", () => {
		const gif = makeDisposalGif();
		const jsReader = new GifReader(gif);
		const nativeReader = new GifReader(gif);
		GifReader.setDecodeBackend(null);
		const expectedPrepared = jsReader.preparePlayback({
			backend: "javascript",
			deltas: true,
		});
		GifReader.setDecodeBackend(backend);
		const actualPrepared = nativeReader.preparePlayback({
			backend: "native",
			deltas: true,
		});

		expect(actualPrepared.composited).toBe(true);
		expect(actualPrepared.frames.length).toBe(expectedPrepared.frames.length);
		expect(actualPrepared.getFramePixels(0)).toBeInstanceOf(Uint32Array);
		expect(actualPrepared.getFrameBytes(0)?.byteLength).toBe(16);

		for (let frameIndex = 0; frameIndex < 3; frameIndex++) {
			const expectedFrame = expectedPrepared.getFrame(frameIndex);
			const actualFrame = actualPrepared.getFrame(frameIndex);
			expect(actualFrame?.changedX).toBe(expectedFrame?.changedX);
			expect(actualFrame?.changedY).toBe(expectedFrame?.changedY);
			expect(actualFrame?.changedWidth).toBe(expectedFrame?.changedWidth);
			expect(actualFrame?.changedHeight).toBe(expectedFrame?.changedHeight);
			expect(actualFrame?.changedPixels).toStrictEqual(
				expectedFrame?.changedPixels,
			);
		}

		for (let frameIndex = 0; frameIndex < 3; frameIndex++) {
			const expected = new Uint8Array(16);
			const actual = new Uint8Array(16);
			expectedPrepared.copyFrame(frameIndex, expected);
			actualPrepared.copyFrame(frameIndex, actual);
			expect(actual).toStrictEqual(expected);
		}

		const expectedPlayer = expectedPrepared.createPlayer(new Uint8Array(16));
		const actualPlayer = actualPrepared.createPlayer(new Uint8Array(16));
		for (let frameIndex = 0; frameIndex < 3; frameIndex++) {
			expectedPlayer.next();
			actualPlayer.next();
			expect(actualPlayer.target).toStrictEqual(expectedPlayer.target);
			expect(actualPlayer.currentIndex).toBe(frameIndex);
		}

		expectedPrepared.dispose();
		actualPrepared.dispose();
		jsReader.returnToPool();
		nativeReader.returnToPool();
	});

	maybeTest("deduplicates identical Wasm composited frames", () => {
		GifReader.setDecodeBackend(backend);
		const reader = new GifReader(makeDuplicateFrameGif());
		const prepared = reader.preparePlayback({ backend: "native" });

		expect(prepared.frames[0]?.pixels).toBe(prepared.frames[1]?.pixels);
		expect(prepared.byteLength).toBe(16);

		prepared.dispose();
		reader.returnToPool();
	});

	maybeTest("respects maxBytes", () => {
		GifReader.setDecodeBackend(backend);
		const gif = readFileSync(join(__dirname, "gifs", "party_blob.gif"));
		const reader = new GifReader(gif);

		expect(() =>
			reader.prepareFrames({
				backend: "native",
				cache: "indices",
				frameIndices: [0],
				maxBytes: 1,
			}),
		).toThrow(/maxBytes/);

		reader.returnToPool();
	});

	maybeTest("respects maxBytes for composited frames", () => {
		GifReader.setDecodeBackend(backend);
		const reader = new GifReader(makeDisposalGif());

		expect(() =>
			reader.preparePlayback({
				backend: "native",
				maxBytes: 1,
			}),
		).toThrow(/maxBytes/);

		reader.returnToPool();
	});

	maybeTest("encodes indexed delta GIFs through the Wasm wrapper", () => {
		const width = 4;
		const height = 4;
		const palette = [0x000000, 0xff0000, 0x00ff00];
		const frame0 = new Uint8Array(width * height).fill(1);
		const frame1 = frame0.slice();
		frame1[1] = 2;
		frame1[5] = 2;
		const frames = new Uint8Array(width * height * 2);
		frames.set(frame0, 0);
		frames.set(frame1, width * height);

		const encoded = encodeIndexedGifFrames({
			width,
			height,
			palette,
			frames,
			delay: 3,
			backend: "native",
			delta: true,
			compression: "fast",
		});
		const reader = new GifReader(encoded);

		expect(reader.numFrames()).toBe(2);
		expect(reader.frameInfo(1).x).toBe(1);
		expect(reader.frameInfo(1).y).toBe(0);
		expect(reader.frameInfo(1).width).toBe(1);
		expect(reader.frameInfo(1).height).toBe(2);
	});

	maybeTest("encodes per-frame delays through the Wasm wrapper", () => {
		const encoded = encodeIndexedGifFrames({
			width: 1,
			height: 1,
			palette: [0x000000, 0xffffff],
			frames: new Uint8Array([0, 1, 0]),
			delay: new Uint16Array([2, 5, 13]),
			backend: "native",
		});
		const reader = new GifReader(encoded);

		expect(reader.numFrames()).toBe(3);
		expect(reader.frameInfo(0).delay).toBe(2);
		expect(reader.frameInfo(1).delay).toBe(5);
		expect(reader.frameInfo(2).delay).toBe(13);
	});

	maybeTest("keeps Wasm pixel index validation in Rust", () => {
		expect(() =>
			encodeIndexedGifFrames({
				width: 1,
				height: 1,
				palette: [0x000000, 0xffffff],
				frames: new Uint8Array([0, 2]),
				frameCount: 2,
				backend: "native",
				delta: true,
			}),
		).toThrow(/Pixel index out of range/);
	});

	maybeTest("encodes RGBA GIFs through the Wasm wrapper", () => {
		const width = 2;
		const height = 2;
		const frames = new Uint8Array([
			255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 255, 0,
			255, 0, 0, 255, 255, 255, 0, 0, 255, 255, 0, 0, 255,
		]);

		const encoded = encodeRgbaGifFrames({
			width,
			height,
			frames,
			delay: 5,
			backend: "native",
		});
		const repeated = encodeRgbaGifFrames({
			width,
			height,
			frames,
			delay: 5,
			backend: "native",
		});
		const reader = new GifReader(encoded);
		const first = new Uint8Array(width * height * 4);
		const second = new Uint8Array(width * height * 4);

		expect(repeated).toStrictEqual(encoded);
		expect(reader.numFrames()).toBe(2);
		expect(reader.frameInfo(0).delay).toBe(5);
		reader.decodeAndBlitFrameRGBA(0, first);
		reader.decodeAndBlitFrameRGBA(1, second);
		expect(first).toStrictEqual(frames.subarray(0, 16));
		expect(second).toStrictEqual(frames.subarray(16));
	});

	maybeTest("reuses quality histograms across normal and large RGBA encodes", () => {
		const smallFrame = makeQualityGradient(128, 128);
		const smallFrames = new Uint8Array(smallFrame.length * 2);
		smallFrames.set(smallFrame);
		smallFrames.set(smallFrame, smallFrame.length);
		const small = encodeRgbaGifFrames({
			width: 128,
			height: 128,
			frameCount: 2,
			frames: smallFrames,
			backend: "native",
			quantization: "quality",
		});
		const large = encodeRgbaGifFrames({
			width: 1024,
			height: 1024,
			frames: makeQualityGradient(1024, 1024),
			backend: "native",
			quantization: "quality",
		});
		const repeated = encodeRgbaGifFrames({
			width: 128,
			height: 128,
			frames: makeQualityGradient(128, 128),
			backend: "native",
			quantization: "quality",
		});

		expect(new GifReader(small).numFrames()).toBe(2);
		expect(new GifReader(large).numFrames()).toBe(1);
		expect(new GifReader(repeated).numFrames()).toBe(1);
	});

	maybeTest("encodes pixel-perfect fast-mode indexed and RGBA GIFs", () => {
		const indexedFrames = new Uint8Array([0, 1, 2, 3, 3, 2, 1, 0]);
		const palette = [0x000000, 0xffffff, 0xff0000, 0x00ff00];
		const indexedGif = encodeIndexedGifFrames({
			width: 2,
			height: 2,
			frames: indexedFrames,
			palette,
			delay: [3, 7],
			compression: "fast",
			backend: "native",
		});
		const rgbaFrames = new Uint8Array([
			0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 255,
			0, 255, 255, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255,
		]);
		const rgbaGif = encodeRgbaGifFrames({
			width: 2,
			height: 2,
			frames: rgbaFrames,
			palette,
			delay: [3, 7],
			compression: "fast",
			backend: "native",
		});

		for (const encoded of [indexedGif, rgbaGif]) {
			const reader = new GifReader(encoded);
			expect(reader.numFrames()).toBe(2);
			for (let frame = 0; frame < 2; frame++) {
				const decoded = new Uint8Array(16);
				reader.decodeAndBlitFrameRGBA(frame, decoded);
				expect(decoded).toStrictEqual(
					rgbaFrames.subarray(frame * 16, (frame + 1) * 16),
				);
				expect(reader.frameInfo(frame).delay).toBe(frame === 0 ? 3 : 7);
			}
		}
	});

	maybeTest(
		"quantizes arbitrary RGBA with independent compression and palette modes",
		() => {
			const width = 16;
			const height = 16;
			const frameBytes = width * height * 4;
			const frames = new Uint8Array(frameBytes * 2);
			for (let value = 0; value < 256; value++) {
				const first = value * 4;
				frames[first] = value;
				frames[first + 1] = (value * 3) & 0xff;
				frames[first + 3] = 255;
				const second = frameBytes + value * 4;
				frames[second + 1] = value;
				frames[second + 2] = (value * 5 + 1) & 0xff;
				frames[second + 3] = 255;
			}

			const fastGlobal = encodeRgbaGifFrames({
				width,
				height,
				frames,
				compression: "fast",
				quantization: "fast",
				backend: "native",
			});
			const qualityGlobal = encodeRgbaGifFrames({
				width,
				height,
				frames,
				compression: "fast",
				quantization: "quality",
				backend: "native",
			});
			const exactLocal = encodeRgbaGifFrames({
				width,
				height,
				frames,
				compression: "fast",
				quantization: "exact",
				paletteMode: "local",
				backend: "native",
			});

			expect(new GifReader(fastGlobal).numFrames()).toBe(2);
			expect(new GifReader(qualityGlobal).numFrames()).toBe(2);
			const localReader = new GifReader(exactLocal);
			expect(localReader.numFrames()).toBe(2);
			for (let frame = 0; frame < 2; frame++) {
				const decoded = new Uint8Array(frameBytes);
				localReader.decodeAndBlitFrameRGBA(frame, decoded);
				expect(decoded).toStrictEqual(
					frames.subarray(frame * frameBytes, (frame + 1) * frameBytes),
				);
				expect(localReader.frameInfo(frame).disposal).toBe(2);
			}
		},
	);

	maybeTest(
		"fast RGBA mode rejects colors and alpha GIF cannot preserve",
		() => {
			expect(() =>
				encodeRgbaGifFrames({
					width: 1,
					height: 1,
					frames: new Uint8Array([1, 2, 3, 128]),
					compression: "fast",
					backend: "native",
				}),
			).toThrow(/alpha values of exactly 0 or 255/);
			expect(() =>
				encodeRgbaGifFrames({
					width: 1,
					height: 1,
					palette: [0x000000, 0xffffff],
					frames: new Uint8Array([1, 2, 3, 255]),
					compression: "fast",
					backend: "native",
				}),
			).toThrow(/outside the supplied palette/);
		},
	);

	maybeTest("encodes RGBA transparency through the Wasm wrapper", () => {
		const encoded = encodeRgbaGifFrames({
			width: 2,
			height: 1,
			frames: new Uint8Array([255, 0, 0, 255, 0, 0, 255, 0]),
			backend: "native",
		});
		const reader = new GifReader(encoded);
		const prepared = reader.preparePlayback();

		expect(reader.frameInfo(0).transparent_index).not.toBeNull();
		expect(Array.from(prepared.getFrameBytes(0)!)).toStrictEqual([
			255, 0, 0, 255, 0, 0, 0, 0,
		]);
		prepared.dispose();
	});

	maybeTest(
		"encodes custom RGBA alpha thresholds through the Wasm wrapper",
		() => {
			const encoded = encodeRgbaGifFrames({
				width: 2,
				height: 1,
				palette: [0xff0000, 0x0000ff],
				frames: new Uint8Array([255, 0, 0, 64, 0, 0, 255, 255]),
				alphaThreshold: 32,
				backend: "native",
			});
			const reader = new GifReader(encoded);
			const prepared = reader.preparePlayback();

			expect(reader.frameInfo(0).transparent_index).toBeNull();
			expect(Array.from(prepared.getFrameBytes(0)!)).toStrictEqual([
				255, 0, 0, 255, 0, 0, 255, 255,
			]);
			prepared.dispose();
		},
	);

	maybeTest("encodes RGBA delta GIFs through the Wasm wrapper", () => {
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

		const encoded = encodeRgbaGifFrames({
			width,
			height,
			frames,
			palette: [0xff0000, 0x00ff00],
			backend: "native",
			delta: true,
			compression: "fast",
		});
		const reader = new GifReader(encoded);
		const decoded = new Uint8Array(width * height * 4);

		expect(reader.frameInfo(1).x).toBe(2);
		expect(reader.frameInfo(1).y).toBe(0);
		expect(reader.frameInfo(1).width).toBe(1);
		expect(reader.frameInfo(1).height).toBe(1);
		reader.decodeAndBlitFrameRGBA(0, decoded);
		reader.decodeAndBlitFrameRGBA(1, decoded);
		expect(decoded).toStrictEqual(frame1);
	});
});
