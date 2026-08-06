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
	return typeof require === "function" ? require : null;
})();

type RawQualityExports = WebAssembly.Exports & {
	memory: WebAssembly.Memory;
	core_version: () => string;
	__wbindgen_add_to_stack_pointer: (delta: number) => number;
	__wbindgen_malloc: (size: number, align: number) => number;
	__wbindgen_free: (ptr: number, size: number, align: number) => void;
	indexed_lzw_input_scratch_reserve: (length: number) => number;
	encode_rgba_quality_gif_constant_delay_scratch_from_input: (
		retptr: number,
		length: number,
		width: number,
		height: number,
		frameCount: number,
		delay: number,
		loopCount: number,
		alphaThreshold: number,
	) => void;
	encode_rgba_quality_gif_scratch_from_input: (
		retptr: number,
		length: number,
		width: number,
		height: number,
		frameCount: number,
		delaysPtr: number,
		delaysLength: number,
		loopCount: number,
		alphaThreshold: number,
	) => void;
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
			__wbg___wbindgen_memory_fbc4c3e30b409f08: () => getRaw()?.memory,
			__wbindgen_cast_0000000000000001: (value: unknown) => value,
		},
	};
}

function createRawQualityModule(raw: RawQualityExports): WasmQualityCoreModule {
	const memory = raw.memory;
	const callResult = (invoke: (retptr: number) => void): number => {
		const retptr = raw.__wbindgen_add_to_stack_pointer(-16);
		try {
			invoke(retptr);
			const view = new DataView(memory.buffer);
			const value = view.getInt32(retptr, true);
			if (view.getInt32(retptr + 8, true) !== 0) {
				throw new Error("Wasm quality encoding failed.");
			}
			return value >>> 0;
		} finally {
			raw.__wbindgen_add_to_stack_pointer(16);
		}
	};
	return {
		core_version: raw.core_version,
		indexed_lzw_input_scratch_reserve: raw.indexed_lzw_input_scratch_reserve,
		wasm_memory: () => memory,
		encode_rgba_quality_gif_from_input: () => {
			throw new Error("Use scratch quality encoding.");
		},
		encode_rgba_quality_gif_scratch_from_input: (
			length: number,
			width: number,
			height: number,
			frameCount: number,
			delays: Uint16Array,
			loopCount: number,
			alphaThreshold: number,
		) => {
			const ptr = raw.__wbindgen_malloc(delays.length * 2, 2);
			new Uint16Array(memory.buffer, ptr, delays.length).set(delays);
			try {
				return callResult((retptr) =>
					raw.encode_rgba_quality_gif_scratch_from_input(
						retptr,
						length,
						width,
						height,
						frameCount,
						ptr,
						delays.length,
						loopCount,
						alphaThreshold,
					),
				);
			} finally {
				raw.__wbindgen_free(ptr, delays.length * 2, 2);
			}
		},
		encode_rgba_quality_gif_constant_delay_scratch_from_input: (
			length: number,
			width: number,
			height: number,
			frameCount: number,
			delay: number,
			loopCount: number,
			alphaThreshold: number,
		) =>
			callResult((retptr) =>
				raw.encode_rgba_quality_gif_constant_delay_scratch_from_input(
					retptr,
					length,
					width,
					height,
					frameCount,
					delay,
					loopCount,
					alphaThreshold,
				),
			),
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
