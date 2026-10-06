import { beforeAll, describe, expect, test } from "vitest";
import {
	encodeRgbaGifFrames as encodeQuality,
	initializeWasmGlobally as initializeEncodeWasm,
} from "../src/encode";
import {
	encodeRgbaGifFrames as encodeGeneral,
	encodeIndexedGifFrames,
	initializeWasmGlobally,
} from "../src/index";
import { getWasmCoreModule } from "../src/wasm/runtime";

const WIDTH = 2;
const HEIGHT = 2;
const FRAME_BYTES = WIDTH * HEIGHT * 4;

function rgba(frameCount: number): Uint8Array {
	const pixels = new Uint8Array(FRAME_BYTES * frameCount);
	for (let offset = 0; offset < pixels.length; offset += 4) {
		pixels.set([offset & 255, 64, 128, 255], offset);
	}
	return pixels;
}

type RgbaOptions = Parameters<typeof encodeGeneral>[0];
type Encode = (options: RgbaOptions) => Uint8Array;

const encoders: [string, Encode][] = [
	["wtfgif/encode", (options) => encodeQuality(options)],
	["wtfgif quality", (options) => encodeGeneral(options)],
	[
		"wtfgif exact",
		(options) => encodeGeneral({ ...options, quantization: "exact" }),
	],
];

// Each case once took a silent `| 0` coercion: 2 ** 32 wrapped to 0 (loop
// forever), fractions truncated, and NaN or strings became numbers.
const invalidCases: [string, Partial<RgbaOptions>, RegExp][] = [
	["fractional width", { width: 2.5 }, /Width\/Height invalid/],
	[
		"string height",
		{ height: "2" as unknown as number },
		/Width\/Height invalid/,
	],
	["NaN width", { width: Number.NaN }, /Width\/Height invalid/],
	["width that wraps to 2", { width: 2 ** 32 + 2 }, /Width\/Height invalid/],
	["fractional delay", { delay: 1.5 }, /Delay invalid/],
	["NaN delay", { delay: Number.NaN }, /Delay invalid/],
	["delay that wraps to 7", { delay: 2 ** 32 + 7 }, /Delay invalid/],
	["fractional per-frame delay", { delay: [10, 2.5] }, /Delay invalid/],
	["loop that wraps to forever", { loop: 2 ** 32 }, /Loop count invalid/],
	["fractional loop", { loop: 1.5 }, /Loop count invalid/],
	[
		"fractional alpha threshold",
		{ alphaThreshold: 127.5 },
		/Alpha threshold invalid/,
	],
	[
		"fractional frame count",
		{ frameCount: 1.5 },
		/Frame count|length does not match/,
	],
];

beforeAll(async () => {
	await Promise.all([initializeEncodeWasm(), initializeWasmGlobally()]);
});

describe.each(encoders)("%s option validation", (_name, encode) => {
	const valid: RgbaOptions = {
		width: WIDTH,
		height: HEIGHT,
		frames: rgba(2),
		delay: 10,
		loop: 0,
	};

	test("accepts integer options", () => {
		expect(encode(valid).length).toBeGreaterThan(0);
		expect(
			encode({ ...valid, delay: [10, 20], loop: null }).length,
		).toBeGreaterThan(0);
	});

	test.each(invalidCases)("rejects a %s", (_case, override, message) => {
		expect(() => encode({ ...valid, ...override })).toThrow(message);
	});

	test("rejects a frame with fewer bytes than the dimensions", () => {
		const short = new Uint8Array(FRAME_BYTES - 4);
		expect(() => encode({ ...valid, frames: [rgba(1), short] })).toThrow(
			/Not enough pixels/,
		);
	});
});

describe("encodeIndexedGifFrames option validation", () => {
	const valid = {
		width: WIDTH,
		height: HEIGHT,
		frames: [Uint8Array.of(0, 1, 1, 0)],
		palette: [0x000000, 0xffffff],
	};

	test("rejects fractional dimensions and short frames", () => {
		expect(encodeIndexedGifFrames(valid).length).toBeGreaterThan(0);
		expect(() => encodeIndexedGifFrames({ ...valid, width: 2.5 })).toThrow(
			/Width\/Height invalid/,
		);
		expect(() =>
			encodeIndexedGifFrames({
				...valid,
				frames: [Uint8Array.of(0, 1, 1)],
			}),
		).toThrow(/Not enough pixels/);
	});
});

describe("WebAssembly errors", () => {
	function thrownBy(run: () => unknown): unknown {
		try {
			run();
		} catch (error) {
			return error;
		}
		throw new Error("Expected the call to throw.");
	}

	test("reject with Error objects, not bare strings", () => {
		const wasm = getWasmCoreModule();
		expect(wasm).not.toBeNull();
		const truncated = Uint8Array.of(0x47, 0x49, 0x46);
		for (const error of [
			thrownBy(() => wasm!.parse_metadata_json(truncated)),
			thrownBy(() => new wasm!.WtfGifCore(truncated)),
		]) {
			expect(error).toBeInstanceOf(Error);
			expect((error as Error).message).toMatch(/GIF/);
			expect((error as Error).stack).toBeTypeOf("string");
		}
	});
});
