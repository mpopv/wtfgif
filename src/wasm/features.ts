import { supportsWasmSimd } from "./simd";

export interface WasmStatus {
	supported: boolean;
	simd: boolean;
	initialized: boolean;
}

export function getWasmStatus(initialized: boolean): WasmStatus {
	return {
		supported: typeof WebAssembly !== "undefined",
		simd: supportsWasmSimd(),
		initialized,
	};
}
