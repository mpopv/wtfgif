import { GifReader as OmggifReader } from "omggif";
import { beforeAll, describe, expect, test } from "vitest";
import { GifReader, initializeWasmGlobally } from "../src/index";
import { gifFixtures, readGifFixture } from "./helpers/gif";

// omggif's decodeAndBlitFrame* writes only the frame's opaque pixels and never
// applies disposal; callers composite by reusing one buffer. Each scenario
// replays the same caller-owned buffer through both readers and requires the
// same bytes after every frame.
type Prepare = (pixels: Uint8Array, frame: number) => void;

const scenarios: [string, Prepare][] = [
	["blits every frame into one buffer", () => {}],
	["extracts each frame into a cleared buffer", (pixels) => pixels.fill(0)],
	[
		"keeps pixels the caller drew between frames",
		(pixels, frame) => pixels.fill(frame * 37 + 11),
	],
];

function mismatchedFrames(gif: Uint8Array, prepare: Prepare): number[] {
	const expected = new OmggifReader(gif);
	const actual = new GifReader(gif);
	const size = expected.width * expected.height * 4;
	const expectedPixels = new Uint8Array(size);
	const actualPixels = new Uint8Array(size);
	const mismatched: number[] = [];
	for (let frame = 0; frame < expected.numFrames(); frame += 1) {
		prepare(expectedPixels, frame);
		prepare(actualPixels, frame);
		expected.decodeAndBlitFrameRGBA(frame, expectedPixels);
		actual.decodeAndBlitFrameRGBA(frame, actualPixels);
		if (Buffer.compare(expectedPixels, actualPixels) !== 0) {
			mismatched.push(frame);
		}
	}
	actual.dispose();
	return mismatched;
}

beforeAll(async () => {
	await initializeWasmGlobally();
});

describe("decodeAndBlitFrameRGBA matches omggif on a reused buffer", () => {
	for (const [name, prepare] of scenarios) {
		test.each(gifFixtures)(`${name}: %s`, (file) => {
			expect(mismatchedFrames(readGifFixture(file), prepare)).toEqual([]);
		});
	}
});
