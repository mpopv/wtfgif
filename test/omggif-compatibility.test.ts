import { describe, expect, test } from "vitest";
import {
	type Frame,
	type FrameOptions,
	type GifBinary,
	type GifOptions,
	GifReader,
	GifWriter,
} from "../src/index";

describe("omggif drop-in compatibility", () => {
	test("supports plain arrays for input, output, and decoded pixels", () => {
		const output: number[] = new Array(256).fill(0);
		const options: GifOptions = {
			loop: 0,
			palette: [0x000000, 0xffffff],
		};
		const frameOptions: FrameOptions = { delay: 4, disposal: 1 };
		const writer = new GifWriter(output, 2, 2, options);

		expect(writer.width).toBe(2);
		expect(writer.height).toBe(2);
		writer.addFrame(0, 0, 2, 2, [0, 1, 1, 0], frameOptions);

		const gif: GifBinary = output.slice(0, writer.end());
		const reader = new GifReader(gif);
		const pixels: number[] = new Array(16).fill(23);
		reader.decodeAndBlitFrameRGBA(0, pixels);

		expect(pixels).toStrictEqual([
			0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255,
		]);
		const frame: Frame = reader.frameInfo(0);
		expect(frame.delay).toBe(4);
		expect(reader.loopCount()).toBe(0);
	});

	test("supports Uint8ClampedArray and unaligned Uint8Array targets", () => {
		const output = new Uint8Array(256);
		const writer = new GifWriter(output, 1, 1, {
			palette: [0x123456, 0xffffff],
		});
		writer.addFrame(0, 0, 1, 1, [0]);
		const reader = new GifReader(output.subarray(0, writer.end()));

		const clamped = new Uint8ClampedArray(4);
		reader.decodeAndBlitFrameRGBA(0, clamped);
		expect(Array.from(clamped)).toStrictEqual([0x12, 0x34, 0x56, 0xff]);

		const storage = new Uint8Array(5);
		const unaligned = storage.subarray(1);
		reader.decodeAndBlitFrameBGRA(0, unaligned);
		expect(Array.from(unaligned)).toStrictEqual([0x56, 0x34, 0x12, 0xff]);
	});

	test("reports nullable palette metadata like omggif", () => {
		const output = new Uint8Array(256);
		const writer = new GifWriter(output, 1, 1);
		writer.addFrame(0, 0, 1, 1, [0], {
			palette: [0x000000, 0xffffff],
		});
		const reader = new GifReader(output.subarray(0, writer.end()));

		expect(reader.frameInfo(0).palette_offset).not.toBeNull();
		expect(reader.frameInfo(0).palette_size).toBe(2);
	});
});
