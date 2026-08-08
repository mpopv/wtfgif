import type {
	WasmCoreModule,
	WasmEncodeCoreModule,
	WasmQualityCoreModule,
} from "../types";

const QUALITY_EXPORTS = [
	"prepare_quality_encoder_code",
	"indexed_lzw_input_scratch_reserve",
	"wasm_memory",
	"encode_rgba_quality_low_res_constant_delay_scratch_from_input",
	"quality_delay_scratch_reserve",
	"encode_rgba_quality_gif_scratch_from_input",
	"encode_rgba_quality_gif_constant_delay_scratch_from_input",
	"gif_output_scratch_ptr",
] as const satisfies readonly (keyof WasmQualityCoreModule)[];

const ENCODE_ONLY_EXPORTS = [
	"core_version",
	"encode_indexed_literal_lzw_scratch",
	"encode_indexed_lzw_scratch_from_input",
	"indexed_lzw_scratch_ptr",
	"encode_indexed_literal_gif",
	"encode_indexed_literal_gif_scratch_from_input",
	"encode_indexed_literal_gif_with_delays",
	"encode_indexed_literal_delta_gif",
	"encode_indexed_literal_delta_gif_with_delays",
	"encode_rgba_literal_gif",
	"encode_rgba_literal_gif_with_options",
	"encode_rgba_literal_delta_gif",
	"encode_rgba_literal_delta_gif_with_options",
	"encode_rgba_gif_advanced",
	"encode_rgba_gif_advanced_from_input",
	"encode_rgba_gif_advanced_scratch_from_input",
] as const satisfies readonly (keyof WasmEncodeCoreModule)[];

const ENCODE_EXPORTS = [
	...QUALITY_EXPORTS,
	...ENCODE_ONLY_EXPORTS,
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

export function isWasmQualityCoreModule(
	value: unknown,
): value is WasmQualityCoreModule {
	return hasFunctions(value, QUALITY_EXPORTS);
}

export function isWasmCoreModule(value: unknown): value is WasmCoreModule {
	return isWasmEncodeCoreModule(value) && hasFunctions(value, DECODE_EXPORTS);
}
