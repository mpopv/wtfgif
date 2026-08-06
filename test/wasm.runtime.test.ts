import { afterEach, describe, expect, test, vi } from "vitest";
import type { WasmCoreInstance, WasmCoreModule } from "../src/types";
import {
	cleanupWasm,
	getWasmStatus,
	initializeGlobalWasm,
	initializeWasmModule,
	setWasmCoreModule,
} from "../src/wasm/runtime";
import { supportsWasmSimd } from "../src/wasm/simd";

class FakeWtfGifCore implements WasmCoreInstance {
	width = () => 1;
	height = () => 1;
	frame_count = () => 0;
	metadata_json = () => "{}";
	decode_frame_indices = () => new Uint8Array();
	decode_frame_rgba = () => new Uint8Array();
	decode_frame_bgra = () => new Uint8Array();
	decode_and_blit_frame_rgba = () => undefined;
	decode_and_blit_frame_bgra = () => undefined;
	decode_frame_rgba_scratch = () => 0;
	decode_frame_bgra_scratch = () => 0;
	decode_frame_rect_rgba_scratch = () => 0;
	decode_frame_rect_bgra_scratch = () => 0;
	decode_scratch_ptr = () => 0;
	decode_all_rgba = () => new Uint32Array();
	reencode_gif_pixel_perfect = () => new Uint8Array();
	prepare_composited_rgba = () => new Uint32Array();
	prepare_composited_bgra = () => new Uint32Array();
	prepare_composited_rgba_scratch = () => 0;
	prepare_composited_bgra_scratch = () => 0;
	composited_scratch_ptr = () => 0;
	prepare_composited_delta_rgba = () => new Uint32Array();
	prepare_composited_delta_bgra = () => new Uint32Array();
	prepare_composited_delta_rgba_scratch = () => 0;
	prepare_composited_delta_bgra_scratch = () => 0;
	free = () => undefined;
}

const emptyBytes = () => new Uint8Array();
const zero = () => 0;
const fakeCoreModule = {
	WtfGifCore: FakeWtfGifCore,
	core_version: () => "test",
	parse_metadata_json: () => "{}",
	decode_frame_indices: emptyBytes,
	decode_frame_rgba: emptyBytes,
	decode_frame_bgra: emptyBytes,
	decode_all_rgba: () => new Uint32Array(),
	reencode_gif_pixel_perfect: emptyBytes,
	remux_gif_pixel_perfect: emptyBytes,
	prepare_composited_rgba: () => new Uint32Array(),
	prepare_composited_bgra: () => new Uint32Array(),
	prepare_composited_delta_rgba: () => new Uint32Array(),
	prepare_composited_delta_bgra: () => new Uint32Array(),
	encode_indexed_lzw: emptyBytes,
	encode_indexed_lzw_scratch: zero,
	encode_indexed_literal_lzw_scratch: zero,
	indexed_lzw_input_scratch_reserve: zero,
	encode_indexed_lzw_scratch_from_input: zero,
	indexed_lzw_scratch_ptr: zero,
	wasm_memory: () => new WebAssembly.Memory({ initial: 1 }),
	encode_indexed_gif: emptyBytes,
	encode_indexed_gif_with_delays: emptyBytes,
	encode_indexed_literal_gif: emptyBytes,
	encode_indexed_literal_gif_scratch_from_input: zero,
	encode_indexed_literal_gif_with_delays: emptyBytes,
	encode_indexed_literal_delta_gif: emptyBytes,
	encode_indexed_literal_delta_gif_with_delays: emptyBytes,
	encode_indexed_delta_gif: emptyBytes,
	encode_indexed_delta_gif_with_delays: emptyBytes,
	encode_rgba_gif: emptyBytes,
	encode_rgba_gif_with_options: emptyBytes,
	encode_rgba_literal_gif: emptyBytes,
	encode_rgba_literal_gif_with_options: emptyBytes,
	encode_rgba_literal_delta_gif: emptyBytes,
	encode_rgba_literal_delta_gif_with_options: emptyBytes,
	encode_rgba_gif_advanced: emptyBytes,
	encode_rgba_gif_advanced_from_input: emptyBytes,
	encode_rgba_gif_advanced_scratch_from_input: zero,
	encode_rgba_quality_gif_from_input: emptyBytes,
	encode_rgba_quality_gif_scratch_from_input: zero,
	encode_rgba_quality_gif_constant_delay_scratch_from_input: zero,
	gif_output_scratch_ptr: zero,
} satisfies WasmCoreModule;

afterEach(() => {
	cleanupWasm();
});

describe("WebAssembly runtime integration", () => {
	test("accepts a provided core module through the initializer", async () => {
		cleanupWasm();
		await initializeGlobalWasm(fakeCoreModule);

		expect(getWasmStatus()).toMatchObject({
			supported: true,
			initialized: true,
		});
	});

	test("does not execute unrelated work during initialization", async () => {
		const reencodeCall = vi.fn(() => new Uint8Array());
		const remuxCall = vi.fn(() => new Uint8Array());
		const decodeCall = vi.fn(() => new Uint32Array());
		await initializeGlobalWasm({
			...fakeCoreModule,
			reencode_gif_pixel_perfect: reencodeCall,
			remux_gif_pixel_perfect: remuxCall,
			decode_all_rgba: decodeCall,
		});

		expect(reencodeCall).not.toHaveBeenCalled();
		expect(remuxCall).not.toHaveBeenCalled();
		expect(decodeCall).not.toHaveBeenCalled();
	});

	test("initializes statically imported bindings for edge runtimes", async () => {
		const init = vi.fn(async () => undefined);
		const module = { ...fakeCoreModule, default: init };
		const compiledModule = {} as WebAssembly.Module;

		await initializeWasmModule(module, compiledModule);

		expect(init).toHaveBeenCalledWith({ module_or_path: compiledModule });
		expect(getWasmStatus().initialized).toBe(true);
	});

	test("cleanup disables the configured core", () => {
		setWasmCoreModule(fakeCoreModule);
		expect(getWasmStatus().initialized).toBe(true);

		cleanupWasm();
		expect(getWasmStatus().initialized).toBe(false);
	});

	test("reports runtime WebAssembly capabilities", () => {
		expect(getWasmStatus()).toMatchObject({
			supported: typeof WebAssembly !== "undefined",
			simd: supportsWasmSimd(),
		});
	});
});
