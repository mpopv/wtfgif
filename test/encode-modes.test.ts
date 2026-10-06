import { readFileSync } from "node:fs";
import { join } from "node:path";
import { GifReader as OmggifReader } from "omggif";
import { beforeAll, describe, expect, test } from "vitest";
import {
	encodeRgbaGifFrames as encodeQuality,
	initializeWasmGlobally as initializeEncodeWasm,
} from "../src/encode";
import {
	encodeRgbaGifFrames as encodeGeneral,
	GifReader,
	initializeWasmGlobally,
} from "../src/index";
import { expectGifSemanticsEqual } from "./helpers/gif";

const makeEmoji = new Uint8Array(
	readFileSync(join(__dirname, "rgba", "makeemoji-128x128x8.rgba")),
);

function syntheticAnimation(width: number, height: number, frames: number) {
	const rgba = new Uint8Array(width * height * 4 * frames);
	for (let frame = 0; frame < frames; frame += 1) {
		for (let y = 0; y < height; y += 1) {
			for (let x = 0; x < width; x += 1) {
				const offset = ((frame * height + y) * width + x) * 4;
				const sprite = Math.abs(x - frame * 3) < 5 && Math.abs(y - 10) < 5;
				rgba.set(
					sprite
						? [255, 40 * frame, 0, 255]
						: [(x * 7) & 255, (y * 5) & 255, 90, x < 3 ? 0 : 255],
					offset,
				);
			}
		}
	}
	return rgba;
}

const inputs = [
	{ name: "MakeEmoji", width: 128, height: 128, rgba: makeEmoji },
	{
		name: "moving sprite",
		width: 40,
		height: 24,
		rgba: syntheticAnimation(40, 24, 6),
	},
];

beforeAll(async () => {
	await Promise.all([initializeEncodeWasm(), initializeWasmGlobally()]);
});

describe.each([
	["wtfgif/encode", encodeQuality],
	["wtfgif", encodeGeneral],
] as const)("%s modes", (_entry, encode) => {
	describe.each(inputs)("$name", ({ width, height, rgba }) => {
		const base = { width, height, frames: rgba, delay: 7, loop: 0 };

		test.each([false, true])(
			"smallest decodes like fastest and is never larger (independentFrames %s)",
			(independentFrames) => {
				const fastest = encode({ ...base, independentFrames, mode: "fastest" });
				const smallest = encode({
					...base,
					independentFrames,
					mode: "smallest",
				});
				expectGifSemanticsEqual(smallest, fastest);
				expect(smallest.length).toBeLessThanOrEqual(fastest.length);
			},
		);

		test("defaults to the fastest mode", () => {
			expect(encode(base)).toStrictEqual(encode({ ...base, mode: "fastest" }));
		});
	});

	test("rejects an unknown mode", () => {
		expect(() =>
			encode({
				width: 1,
				height: 1,
				frames: Uint8Array.of(1, 2, 3, 255),
				mode: "tiny" as "smallest",
			}),
		).toThrow(/Mode must be "fastest" or "smallest"/);
	});
});

describe("smallest mode", () => {
	test("halves MakeEmoji", () => {
		const options = { width: 128, height: 128, frames: makeEmoji, delay: 10 };
		const fastest = encodeQuality(options);
		const smallest = encodeQuality({ ...options, mode: "smallest" });
		expect(smallest.length).toBeLessThan(fastest.length * 0.6);
	});

	test("needs the default quality encoder in wtfgif", () => {
		expect(() =>
			encodeGeneral({
				width: 1,
				height: 1,
				frames: Uint8Array.of(1, 2, 3, 255),
				quantization: "exact",
				mode: "smallest",
			}),
		).toThrow(/needs the default quality encoder/);
	});

	test("decodes through GifReader when codes widen in a tiny image", () => {
		// Found by fuzzing: this image's LZW stream has the length of a literal
		// stream, which a literal-only decoder fast path once misread.
		const rgba = Uint8Array.of(
			0,
			0,
			0,
			0,
			0,
			0,
			8,
			0,
			0,
			0,
			0,
			0,
			0,
			0,
			0,
			0,
			34,
			1,
			0,
			0,
			0,
			0,
			0,
			0,
		);
		const gif = encodeQuality({
			width: 6,
			height: 1,
			frames: rgba,
			alphaThreshold: 0,
			mode: "smallest",
		});
		const expected = new Uint8Array(24);
		new OmggifReader(gif).decodeAndBlitFrameRGBA(0, expected);
		const reader = new GifReader(gif);
		const playback = reader.preparePlayback();
		const actual = new Uint8Array(24);
		playback.copyFrame(0, actual);
		expect(actual).toStrictEqual(expected);
		expect(Array.from(actual.subarray(16, 20))).toEqual([34, 1, 0, 255]);
		playback.dispose();
		reader.dispose();
	});
});
