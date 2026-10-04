export interface WasmCoreInstance {
	width: () => number;
	height: () => number;
	frame_count: () => number;
	metadata_json: () => string;
	decode_frame_indices: (frameIndex: number) => Uint8Array;
	decode_frame_rgba: (frameIndex: number) => Uint8Array;
	decode_frame_bgra: (frameIndex: number) => Uint8Array;
	decode_and_blit_frame_rgba: (frameIndex: number, pixels: Uint8Array) => void;
	decode_and_blit_frame_bgra: (frameIndex: number, pixels: Uint8Array) => void;
	decode_frame_rgba_scratch: (frameIndex: number) => number;
	decode_frame_bgra_scratch: (frameIndex: number) => number;
	decode_frame_rect_rgba_scratch: (frameIndex: number) => number;
	decode_frame_rect_bgra_scratch: (frameIndex: number) => number;
	decode_scratch_ptr: () => number;
	decode_all_rgba: () => Uint32Array;
	reencode_gif_pixel_perfect: () => Uint8Array;
	prepare_composited_rgba: (requestedFrames: Uint8Array) => Uint32Array;
	prepare_composited_bgra: (requestedFrames: Uint8Array) => Uint32Array;
	prepare_composited_rgba_scratch: (requestedFrames: Uint8Array) => number;
	prepare_composited_bgra_scratch: (requestedFrames: Uint8Array) => number;
	composited_scratch_ptr: () => number;
	prepare_composited_delta_rgba: (requestedFrames: Uint8Array) => Uint32Array;
	prepare_composited_delta_bgra: (requestedFrames: Uint8Array) => Uint32Array;
	prepare_composited_delta_rgba_scratch: (
		requestedFrames: Uint8Array,
	) => number;
	prepare_composited_delta_bgra_scratch: (
		requestedFrames: Uint8Array,
	) => number;
	free: () => void;
}

export interface WasmCoreModule {
	WtfGifCore: new (data: Uint8Array) => WasmCoreInstance;
	core_version: () => string;
	parse_metadata_json: (data: Uint8Array) => string;
	decode_frame_indices: (data: Uint8Array, frameIndex: number) => Uint8Array;
	decode_frame_rgba: (data: Uint8Array, frameIndex: number) => Uint8Array;
	decode_frame_bgra: (data: Uint8Array, frameIndex: number) => Uint8Array;
	decode_all_rgba: (data: Uint8Array) => Uint32Array;
	reencode_gif_pixel_perfect: (data: Uint8Array) => Uint8Array;
	remux_gif_pixel_perfect: (data: Uint8Array) => Uint8Array;
	prepare_composited_rgba: (
		data: Uint8Array,
		requestedFrames: Uint8Array,
	) => Uint32Array;
	prepare_composited_bgra: (
		data: Uint8Array,
		requestedFrames: Uint8Array,
	) => Uint32Array;
	prepare_composited_delta_rgba: (
		data: Uint8Array,
		requestedFrames: Uint8Array,
	) => Uint32Array;
	prepare_composited_delta_bgra: (
		data: Uint8Array,
		requestedFrames: Uint8Array,
	) => Uint32Array;
	encode_indexed_literal_lzw_scratch: (
		indexStream: Uint8Array,
		minCodeSize: number,
		colorCount: number,
	) => number;
	prepare_quality_encoder_code: () => void;
	indexed_lzw_input_scratch_reserve: (length: number) => number;
	encode_indexed_lzw_scratch_from_input: (
		length: number,
		minCodeSize: number,
		colorCount: number,
	) => number;
	indexed_lzw_scratch_ptr: () => number;
	wasm_memory: () => WebAssembly.Memory;
	encode_indexed_literal_gif: (
		indexStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delay: number,
		loopCount: number,
	) => Uint8Array;
	encode_indexed_literal_gif_scratch_from_input: (
		length: number,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delay: number,
		loopCount: number,
	) => number;
	encode_indexed_literal_gif_with_delays: (
		indexStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
	) => Uint8Array;
	encode_indexed_literal_delta_gif: (
		indexStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delay: number,
		loopCount: number,
	) => Uint8Array;
	encode_indexed_literal_delta_gif_with_delays: (
		indexStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
	) => Uint8Array;
	encode_rgba_literal_gif: (
		rgbaStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delay: number,
		loopCount: number,
	) => Uint8Array;
	encode_rgba_literal_gif_with_options: (
		rgbaStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
		alphaThreshold: number,
	) => Uint8Array;
	encode_rgba_literal_delta_gif: (
		rgbaStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delay: number,
		loopCount: number,
	) => Uint8Array;
	encode_rgba_literal_delta_gif_with_options: (
		rgbaStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
		alphaThreshold: number,
	) => Uint8Array;
	encode_rgba_gif_advanced: (
		rgbaStream: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
		deltas: boolean,
		alphaThreshold: number,
		quantization: number,
		paletteMode: number,
	) => Uint8Array;
	encode_rgba_gif_advanced_from_input: (
		length: number,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
		deltas: boolean,
		alphaThreshold: number,
		quantization: number,
		paletteMode: number,
	) => Uint8Array;
	encode_rgba_gif_advanced_scratch_from_input: (
		length: number,
		width: number,
		height: number,
		frameCount: number,
		paletteRgb: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
		deltas: boolean,
		alphaThreshold: number,
		quantization: number,
		paletteMode: number,
	) => number;
	quality_delay_scratch_reserve: (
		inputLength: number,
		delayCount: number,
	) => number;
	encode_rgba_quality_gif_scratch_from_input: (
		length: number,
		width: number,
		height: number,
		frameCount: number,
		delayCount: number,
		loopCount: number,
		alphaThreshold: number,
		independentFrames: boolean,
	) => number;
	encode_rgba_quality_gif_constant_delay_scratch_from_input: (
		length: number,
		width: number,
		height: number,
		frameCount: number,
		delay: number,
		loopCount: number,
		alphaThreshold: number,
		independentFrames: boolean,
	) => number;
	encode_rgba_quality_low_res_constant_delay_scratch_from_input: (
		length: number,
		width: number,
		height: number,
		frameCount: number,
		delay: number,
		loopCount: number,
		alphaThreshold: number,
		independentFrames: boolean,
	) => number;
	gif_output_scratch_ptr: () => number;
}

/** Exact exports shared by the full and encode-only Wasm artifacts. */
export type WasmEncodeCoreModule = Pick<
	WasmCoreModule,
	| "core_version"
	| "encode_indexed_literal_lzw_scratch"
	| "prepare_quality_encoder_code"
	| "indexed_lzw_input_scratch_reserve"
	| "encode_indexed_lzw_scratch_from_input"
	| "indexed_lzw_scratch_ptr"
	| "wasm_memory"
	| "encode_indexed_literal_gif"
	| "encode_indexed_literal_gif_scratch_from_input"
	| "encode_indexed_literal_gif_with_delays"
	| "encode_indexed_literal_delta_gif"
	| "encode_indexed_literal_delta_gif_with_delays"
	| "encode_rgba_literal_gif"
	| "encode_rgba_literal_gif_with_options"
	| "encode_rgba_literal_delta_gif"
	| "encode_rgba_literal_delta_gif_with_options"
	| "encode_rgba_gif_advanced"
	| "encode_rgba_gif_advanced_from_input"
	| "encode_rgba_gif_advanced_scratch_from_input"
	| "quality_delay_scratch_reserve"
	| "encode_rgba_quality_gif_scratch_from_input"
	| "encode_rgba_quality_gif_constant_delay_scratch_from_input"
	| "encode_rgba_quality_low_res_constant_delay_scratch_from_input"
	| "gif_output_scratch_ptr"
>;

/** Minimal ABI exported by the encode-only quality Wasm artifact. */
export interface WasmQualityCoreModule {
	prepare_quality_encoder_code: () => void;
	indexed_lzw_input_scratch_reserve: (length: number) => number;
	wasm_memory: () => WebAssembly.Memory;
	encode_rgba_quality_low_res_constant_delay_scratch_from_input: (
		length: number,
		width: number,
		height: number,
		frameCount: number,
		delay: number,
		loopCount: number,
		alphaThreshold: number,
		independentFrames: boolean,
	) => number;
	quality_delay_scratch_reserve: (
		inputLength: number,
		delayCount: number,
	) => number;
	encode_rgba_quality_gif_scratch_from_input: (
		length: number,
		width: number,
		height: number,
		frameCount: number,
		delayCount: number,
		loopCount: number,
		alphaThreshold: number,
		independentFrames: boolean,
	) => number;
	encode_rgba_quality_gif_constant_delay_scratch_from_input: (
		length: number,
		width: number,
		height: number,
		frameCount: number,
		delay: number,
		loopCount: number,
		alphaThreshold: number,
		independentFrames: boolean,
	) => number;
	gif_output_scratch_ptr: () => number;
}
