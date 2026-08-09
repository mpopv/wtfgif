import { afterAll, beforeAll, describe, expect, test, vi } from "vitest";
import {
	cleanupWasm,
	encodeRgbaGifFrames,
	getWasmCoreModule,
	initializeWasmGlobally,
	setWasmCoreModule,
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
	test("prepares code and exported shims before the first real encode", () => {
		const module = getWasmCoreModule();
		if (!module) throw new Error("Expected initialized encode module.");
		const prepare = vi.fn(module.prepare_quality_encoder_code);
		const prepareEncode = vi.fn(
			module.encode_rgba_quality_low_res_constant_delay_scratch_from_input,
		);
		const prepareLargeEncode = vi.fn(
			module.encode_rgba_quality_gif_constant_delay_scratch_from_input,
		);
		const prepareOutputPointer = vi.fn(module.gif_output_scratch_ptr);
		const reserve = vi.fn(module.indexed_lzw_input_scratch_reserve);
		setWasmCoreModule({
			...module,
			prepare_quality_encoder_code: prepare,
			encode_rgba_quality_low_res_constant_delay_scratch_from_input:
				prepareEncode,
			encode_rgba_quality_gif_constant_delay_scratch_from_input:
				prepareLargeEncode,
			gif_output_scratch_ptr: prepareOutputPointer,
			indexed_lzw_input_scratch_reserve: reserve,
		});

		try {
			expect(prepare).toHaveBeenCalledTimes(1);
			expect(prepareEncode).toHaveBeenNthCalledWith(1, 0, 1, 1, 0, 0, 0, 128);
			expect(prepareEncode).toHaveBeenNthCalledWith(
				2,
				128 * 128 * 8 * 4,
				128,
				128,
				8,
				0,
				0,
				128,
			);
			expect(prepareEncode).toHaveBeenCalledTimes(18);
			expect(prepareEncode).toHaveBeenLastCalledWith(8, 1, 1, 2, 0, 0, 128);
			expect(prepareLargeEncode).toHaveBeenCalledExactlyOnceWith(
				1001 * 1000 * 4,
				1001,
				1000,
				1,
				0,
				0,
				128,
			);
			expect(prepareOutputPointer).toHaveBeenCalledTimes(2);
			expect(reserve).toHaveBeenCalledExactlyOnceWith(4 * 1024 * 1024);
			encodeRgbaGifFrames({
				width: 2,
				height: 1,
				frames,
				frameCount: 2,
				delay: 10,
			});
			expect(prepare).toHaveBeenCalledTimes(1);
			expect(prepareEncode).toHaveBeenCalledTimes(19);
			expect(prepareLargeEncode).toHaveBeenCalledTimes(1);
			expect(prepareOutputPointer).toHaveBeenCalledTimes(3);
			expect(reserve).toHaveBeenCalledTimes(1);
		} finally {
			setWasmCoreModule(module);
		}
	});

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
		expect(
			encodeRgbaGifFrames({
				...options,
				frames: [frames.subarray(0, 8), frames.subarray(8)],
			}),
		).toStrictEqual(encoded);
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
