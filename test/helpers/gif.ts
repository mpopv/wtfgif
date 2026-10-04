import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { GifReader } from "omggif";
import { expect } from "vitest";

export const gifDirectory = join(__dirname, "..", "gifs");

export const gifFixtures = readdirSync(gifDirectory).filter((file) =>
	file.endsWith(".gif"),
);

export function readGifFixture(file: string): Uint8Array {
	return readFileSync(join(gifDirectory, file));
}

export function expectGifSemanticsEqual(
	actualGif: Uint8Array,
	expectedGif: Uint8Array,
): void {
	const actual = new GifReader(actualGif);
	const expected = new GifReader(expectedGif);
	expect(actual.width).toBe(expected.width);
	expect(actual.height).toBe(expected.height);
	expect(actual.numFrames()).toBe(expected.numFrames());
	expect(actual.loopCount()).toBe(expected.loopCount());

	const actualPixels = new Uint8Array(actual.width * actual.height * 4);
	const expectedPixels = new Uint8Array(expected.width * expected.height * 4);
	for (let frame = 0; frame < actual.numFrames(); frame += 1) {
		const actualInfo = actual.frameInfo(frame);
		const expectedInfo = expected.frameInfo(frame);
		for (const field of [
			"x",
			"y",
			"width",
			"height",
			"transparent_index",
			"interlaced",
			"delay",
			"disposal",
		] as const) {
			expect(actualInfo[field]).toBe(expectedInfo[field]);
		}
		actual.decodeAndBlitFrameRGBA(frame, actualPixels);
		expected.decodeAndBlitFrameRGBA(frame, expectedPixels);
		expect(actualPixels).toStrictEqual(expectedPixels);
	}
}
