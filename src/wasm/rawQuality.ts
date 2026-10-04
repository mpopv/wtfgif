import type { WasmQualityCoreModule } from "../types";
import { supportsWasmSimd } from "./simd";

type ProcessWithBuiltinModule = NodeJS.Process & {
	getBuiltinModule?: (
		id: string,
	) => { createRequire?: (filename: string | URL) => NodeRequire } | undefined;
};

const nodeRequire = (() => {
	const processWithBuiltins =
		typeof process === "undefined"
			? undefined
			: (process as ProcessWithBuiltinModule);
	const createRequire =
		processWithBuiltins?.getBuiltinModule?.("node:module")?.createRequire;
	if (createRequire) return createRequire(import.meta.url);
	return typeof require === "function" && typeof require.resolve === "function"
		? require
		: null;
})();

type RawQualityExports = WebAssembly.Exports & {
	memory: WebAssembly.Memory;
	__wbindgen_externrefs: WebAssembly.Table;
	__wbindgen_start: () => void;
	prepare_quality_encoder_code: () => void;
	indexed_lzw_input_scratch_reserve: (length: number) => number;
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
	gif_output_scratch_ptr: () => number;
};

function isMissing(error: unknown): boolean {
	if (!(error instanceof Error)) return false;
	const code = (error as Error & { code?: string }).code;
	return code === "MODULE_NOT_FOUND" || code === "ERR_MODULE_NOT_FOUND";
}

function wasmImports(getRaw: () => RawQualityExports | undefined) {
	return {
		"./wtfgif_core_bg.js": {
			__wbindgen_init_externref_table: () => {
				const table = getRaw()?.__wbindgen_externrefs;
				if (!table) throw new Error("Wasm externref table is unavailable.");
				const offset = table.grow(4);
				table.set(0, undefined);
				table.set(offset, undefined);
				table.set(offset + 1, null);
				table.set(offset + 2, true);
				table.set(offset + 3, false);
			},
		},
	};
}

function createRawQualityModule(raw: RawQualityExports): WasmQualityCoreModule {
	const memory = raw.memory;
	return {
		prepare_quality_encoder_code: raw.prepare_quality_encoder_code,
		indexed_lzw_input_scratch_reserve: raw.indexed_lzw_input_scratch_reserve,
		encode_rgba_quality_low_res_constant_delay_scratch_from_input:
			raw.encode_rgba_quality_low_res_constant_delay_scratch_from_input,
		quality_delay_scratch_reserve: raw.quality_delay_scratch_reserve,
		wasm_memory: () => memory,
		encode_rgba_quality_gif_scratch_from_input:
			raw.encode_rgba_quality_gif_scratch_from_input,
		encode_rgba_quality_gif_constant_delay_scratch_from_input:
			raw.encode_rgba_quality_gif_constant_delay_scratch_from_input,
		gif_output_scratch_ptr: raw.gif_output_scratch_ptr,
	};
}

function loadRawNode(path: string): WasmQualityCoreModule {
	const bytes = nodeRequire!("node:fs").readFileSync(path);
	let raw: RawQualityExports | undefined;
	raw = new WebAssembly.Instance(
		new WebAssembly.Module(bytes),
		wasmImports(() => raw),
	).exports as RawQualityExports;
	raw.__wbindgen_start();
	return createRawQualityModule(raw);
}

/**
 * Load the quality encoder directly from its Wasm binary in Node. This skips
 * the generated CommonJS glue's module/bootstrap work while keeping the same
 * validated public encoder and exact output.
 */
export function loadRawQualityNode(): WasmQualityCoreModule | undefined {
	if (!nodeRequire) return undefined;
	const candidates = supportsWasmSimd()
		? [
				"./wasm-quality-simd/wtfgif_core_bg.wasm",
				"./wasm-quality/wtfgif_core_bg.wasm",
				"../../crates/wtfgif-core/pkg-quality-simd/wtfgif_core_bg.wasm",
				"../../crates/wtfgif-core/pkg-quality/wtfgif_core_bg.wasm",
			]
		: [
				"./wasm-quality/wtfgif_core_bg.wasm",
				"../../crates/wtfgif-core/pkg-quality/wtfgif_core_bg.wasm",
			];
	for (const candidate of candidates) {
		try {
			return loadRawNode(nodeRequire.resolve(candidate));
		} catch (error) {
			if (!isMissing(error)) throw error;
		}
	}
	return undefined;
}
