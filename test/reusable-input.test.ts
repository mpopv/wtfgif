import { GifReader } from "omggif";
import { beforeAll, describe, expect, test } from "vitest";
import { encodeRgbaGifFrames, initializeWasmGlobally } from "../src/encode";
import {
	encodeRgbaGifFrames as encodeRgbaGifFramesGeneral,
	initializeWasmGlobally as initializeGeneralWasm,
} from "../src/index";

// Two frames of this size fill the encoder's initial 4 MiB input buffer, so
// reserving per-frame delays used to grow (and move) that buffer while
// JavaScript kept writing the next encode's pixels to the old address.
const WIDTH = 1024;
const HEIGHT = 512;

function solid(red: number, green: number, blue: number): Uint8Array {
	const frame = new Uint8Array(WIDTH * HEIGHT * 4);
	for (let offset = 0; offset < frame.length; offset += 4) {
		frame.set([red, green, blue, 255], offset);
	}
	return frame;
}

function firstPixel(gif: Uint8Array): number[] {
	const reader = new GifReader(gif);
	const pixels = new Uint8Array(WIDTH * HEIGHT * 4);
	reader.decodeAndBlitFrameRGBA(0, pixels);
	return Array.from(pixels.subarray(0, 3));
}

beforeAll(async () => {
	await Promise.all([initializeWasmGlobally(), initializeGeneralWasm()]);
});

describe("reusable input buffer", () => {
	test.each([
		["wtfgif/encode", encodeRgbaGifFrames],
		["wtfgif", encodeRgbaGifFramesGeneral],
	] as const)(
		"%s encodes the current pixels after an encode with per-frame delays",
		(_name, encode) => {
			const red = solid(255, 0, 0);
			const blue = solid(0, 0, 255);
			expect(
				firstPixel(
					encode({
						width: WIDTH,
						height: HEIGHT,
						frames: [red, red],
						delay: 5,
					}),
				),
			).toEqual([255, 0, 0]);
			expect(
				firstPixel(
					encode({
						width: WIDTH,
						height: HEIGHT,
						frames: [red, red],
						delay: [3, 7],
					}),
				),
			).toEqual([255, 0, 0]);
			expect(
				firstPixel(
					encode({
						width: WIDTH,
						height: HEIGHT,
						frames: [blue, blue],
						delay: 5,
					}),
				),
			).toEqual([0, 0, 255]);
		},
	);
});
