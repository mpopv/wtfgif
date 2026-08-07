import fc from "fast-check";
import { GifReader as OmgGifReader } from "omggif";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import {
	cleanupWasm,
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	initializeWasmGlobally,
} from "../src/index";

function nextRandom(state: { value: number }): number {
	let value = state.value >>> 0;
	value ^= value << 13;
	value ^= value >>> 17;
	value ^= value << 5;
	state.value = value >>> 0;
	return state.value;
}

function indexedExpected(indices: Uint8Array, palette: number[]): Uint8Array {
	const rgba = new Uint8Array(indices.length * 4);
	for (let pixel = 0; pixel < indices.length; pixel += 1) {
		const color = palette[indices[pixel]!]!;
		rgba[pixel * 4] = color >>> 16;
		rgba[pixel * 4 + 1] = color >>> 8;
		rgba[pixel * 4 + 2] = color;
		rgba[pixel * 4 + 3] = 255;
	}
	return rgba;
}

beforeAll(async () => {
	await initializeWasmGlobally();
});

afterAll(() => {
	cleanupWasm();
});

describe("encoder properties", () => {
	test("literal indexed GIFs preserve arbitrary valid palettes, frames, delays, and deltas", () => {
		fc.assert(
			fc.property(
				fc.record({
					width: fc.integer({ min: 1, max: 32 }),
					height: fc.integer({ min: 1, max: 32 }),
					frameCount: fc.integer({ min: 1, max: 5 }),
					paletteBits: fc.integer({ min: 1, max: 8 }),
					seed: fc.integer(),
					delta: fc.boolean(),
				}),
				({ width, height, frameCount, paletteBits, seed, delta }) => {
					const colorCount = 1 << paletteBits;
					const state = { value: seed };
					const palette = Array.from(
						{ length: colorCount },
						() => nextRandom(state) & 0x00ff_ffff,
					);
					const framePixels = width * height;
					const frames = new Uint8Array(framePixels * frameCount);
					for (let index = 0; index < frames.length; index += 1) {
						frames[index] = nextRandom(state) & (colorCount - 1);
					}
					const delays = Uint16Array.from(
						{ length: frameCount },
						() => nextRandom(state) % 200,
					);
					const encoded = encodeIndexedGifFrames({
						width,
						height,
						frames,
						frameCount,
						palette,
						delay: delays,
						delta,
						backend: "javascript",
					});
					const reader = new OmgGifReader(encoded);
					expect(reader.width).toBe(width);
					expect(reader.height).toBe(height);
					expect(reader.numFrames()).toBe(frameCount);
					const canvas = new Uint8Array(framePixels * 4);
					for (let frame = 0; frame < frameCount; frame += 1) {
						expect(reader.frameInfo(frame).delay).toBe(delays[frame]);
						reader.decodeAndBlitFrameRGBA(frame, canvas);
						const expected = indexedExpected(
							frames.subarray(frame * framePixels, (frame + 1) * framePixels),
							palette,
						);
						expect(canvas).toStrictEqual(expected);
					}
				},
			),
			{ numRuns: 100, seed: 0x57f91f },
		);
	});

	test("arbitrary RGBA inputs produce decodable binary-alpha animations", () => {
		fc.assert(
			fc.property(
				fc.record({
					width: fc.integer({ min: 1, max: 24 }),
					height: fc.integer({ min: 1, max: 24 }),
					frameCount: fc.integer({ min: 1, max: 4 }),
					seed: fc.integer(),
				}),
				({ width, height, frameCount, seed }) => {
					const state = { value: seed };
					const rgba = new Uint8Array(width * height * frameCount * 4);
					for (let offset = 0; offset < rgba.length; offset += 4) {
						const random = nextRandom(state);
						rgba[offset] = random;
						rgba[offset + 1] = random >>> 8;
						rgba[offset + 2] = random >>> 16;
						rgba[offset + 3] = random >>> 24;
					}
					const encoded = encodeRgbaGifFrames({
						width,
						height,
						frames: rgba,
						frameCount,
						delay: 7,
						alphaThreshold: 128,
						quantization: "quality",
						backend: "wasm",
					});
					const reader = new OmgGifReader(encoded);
					expect(reader.width).toBe(width);
					expect(reader.height).toBe(height);
					expect(reader.numFrames()).toBe(frameCount);
					const framePixels = width * height;
					for (let frame = 0; frame < frameCount; frame += 1) {
						const decoded = new Uint8Array(framePixels * 4);
						reader.decodeAndBlitFrameRGBA(frame, decoded);
						expect(reader.frameInfo(frame).delay).toBe(7);
						for (let pixel = 0; pixel < framePixels; pixel += 1) {
							const expectedAlpha =
								rgba[(frame * framePixels + pixel) * 4 + 3]! >= 128 ? 255 : 0;
							expect(decoded[pixel * 4 + 3]).toBe(expectedAlpha);
						}
					}
				},
			),
			{ numRuns: 100, seed: 0x4a17c3 },
		);
	});
});
