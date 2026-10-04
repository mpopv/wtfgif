import type { WasmCoreInstance, WasmCoreModule } from "../../src/types";

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

export function createFakeWasmCoreModule(
	overrides: Partial<WasmCoreModule> = {},
): WasmCoreModule {
	return {
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
		encode_indexed_literal_lzw_scratch: zero,
		prepare_quality_encoder_code: () => undefined,
		indexed_lzw_input_scratch_reserve: zero,
		quality_delay_scratch_reserve: zero,
		encode_indexed_lzw_scratch_from_input: zero,
		indexed_lzw_scratch_ptr: zero,
		wasm_memory: () => new WebAssembly.Memory({ initial: 1 }),
		encode_indexed_literal_gif: emptyBytes,
		encode_indexed_literal_gif_scratch_from_input: zero,
		encode_indexed_literal_gif_with_delays: emptyBytes,
		encode_indexed_literal_delta_gif: emptyBytes,
		encode_indexed_literal_delta_gif_with_delays: emptyBytes,
		encode_rgba_literal_gif: emptyBytes,
		encode_rgba_literal_gif_with_options: emptyBytes,
		encode_rgba_literal_delta_gif: emptyBytes,
		encode_rgba_literal_delta_gif_with_options: emptyBytes,
		encode_rgba_gif_advanced: emptyBytes,
		encode_rgba_gif_advanced_from_input: emptyBytes,
		encode_rgba_gif_advanced_scratch_from_input: zero,
		encode_rgba_quality_gif_scratch_from_input: zero,
		encode_rgba_quality_gif_constant_delay_scratch_from_input: zero,
		encode_rgba_quality_low_res_constant_delay_scratch_from_input: zero,
		gif_output_scratch_ptr: zero,
		...overrides,
	};
}
