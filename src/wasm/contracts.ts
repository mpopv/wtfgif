import type { WasmCoreModule, WasmEncodeCoreModule } from "../types";

const ENCODE_EXPORTS = [
	"core_version",
	"encode_indexed_lzw",
	"encode_indexed_lzw_scratch",
	"encode_indexed_literal_lzw_scratch",
	"indexed_lzw_input_scratch_reserve",
	"encode_indexed_lzw_scratch_from_input",
	"indexed_lzw_scratch_ptr",
	"wasm_memory",
	"encode_indexed_gif",
	"encode_indexed_gif_with_delays",
	"encode_indexed_literal_gif",
	"encode_indexed_literal_gif_scratch_from_input",
	"encode_indexed_literal_gif_with_delays",
	"encode_indexed_literal_delta_gif",
	"encode_indexed_literal_delta_gif_with_delays",
	"encode_indexed_delta_gif",
	"encode_indexed_delta_gif_with_delays",
	"encode_rgba_gif",
	"encode_rgba_gif_with_options",
	"encode_rgba_literal_gif",
	"encode_rgba_literal_gif_with_options",
	"encode_rgba_literal_delta_gif",
	"encode_rgba_literal_delta_gif_with_options",
	"encode_rgba_gif_advanced",
	"encode_rgba_gif_advanced_from_input",
	"encode_rgba_gif_advanced_scratch_from_input",
	"encode_rgba_quality_gif_from_input",
	"encode_rgba_quality_gif_scratch_from_input",
	"gif_output_scratch_ptr",
] as const satisfies readonly (keyof WasmEncodeCoreModule)[];

const DECODE_EXPORTS = [
	"WtfGifCore",
	"parse_metadata_json",
	"decode_frame_indices",
	"decode_frame_rgba",
	"decode_frame_bgra",
	"decode_all_rgba",
	"reencode_gif_pixel_perfect",
	"remux_gif_pixel_perfect",
	"prepare_composited_rgba",
	"prepare_composited_bgra",
	"prepare_composited_delta_rgba",
	"prepare_composited_delta_bgra",
] as const satisfies readonly (keyof WasmCoreModule)[];

function hasFunctions(value: unknown, names: readonly string[]): boolean {
	if ((typeof value !== "object" && typeof value !== "function") || !value) {
		return false;
	}
	const candidate = value as Record<string, unknown>;
	return names.every((name) => typeof candidate[name] === "function");
}

export function isWasmEncodeCoreModule(
	value: unknown,
): value is WasmEncodeCoreModule {
	return hasFunctions(value, ENCODE_EXPORTS);
}

export function isWasmCoreModule(value: unknown): value is WasmCoreModule {
	return isWasmEncodeCoreModule(value) && hasFunctions(value, DECODE_EXPORTS);
}
