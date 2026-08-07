import { afterAll, beforeAll, describe, expect, test } from "vitest";
import {
	cleanupWasm,
	encodeRgbaGifFrames,
	initializeWasmGlobally,
} from "../src/encode";
import {
	encodeRgbaGifFrames as encodeRgbaGifFramesGeneral,
	GifReader,
	initializeWasmGlobally as initializeGeneralWasm,
} from "../src/index";

const frames = Uint8Array.of(
	255,
	0,
	0,
	255,
	0,
	0,
	255,
	255,
	0,
	255,
	0,
	255,
	255,
	255,
	0,
	255,
);

beforeAll(async () => {
	await Promise.all([initializeWasmGlobally(), initializeGeneralWasm()]);
});

afterAll(() => {
	cleanupWasm();
});

describe("encode-only quality entry", () => {
	test("matches the established quality encoder byte for byte", () => {
		const options = {
			width: 2,
			height: 1,
			frames,
			frameCount: 2,
			delay: Uint16Array.of(3, 7),
			loop: 0,
			alphaThreshold: 128,
		} as const;
		const encoded = encodeRgbaGifFrames(options);
		const expected = encodeRgbaGifFramesGeneral({
			...options,
			backend: "wasm",
			quantization: "quality",
			paletteMode: "global",
		});

		expect(encoded).toStrictEqual(expected);
	});

	test("can switch repeatedly between constant and per-frame delays", () => {
		for (const delay of [10, Uint16Array.of(3, 7), 5, Uint16Array.of(11, 13)]) {
			const reader = new GifReader(
				encodeRgbaGifFrames({
					width: 2,
					height: 1,
					frames,
					frameCount: 2,
					delay,
					loop: 0,
				}),
			);
			const expected = typeof delay === "number" ? [delay, delay] : [...delay];
			expect([reader.frameInfo(0).delay, reader.frameInfo(1).delay]).toEqual(
				expected,
			);
		}
	});

	test("normalizes an equal delay array to the constant-delay encoding path", () => {
		const options = {
			width: 2,
			height: 1,
			frames,
			frameCount: 2,
			loop: 0,
		} as const;
		expect(
			encodeRgbaGifFrames({ ...options, delay: Uint16Array.of(11, 11) }),
		).toStrictEqual(encodeRgbaGifFrames({ ...options, delay: 11 }));
	});
});
