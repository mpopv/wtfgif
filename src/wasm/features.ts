import { supportsWasmSimd } from "./simd";

export interface WasmFeatures {
	supported: boolean;
	simd: boolean;
	threads: boolean;
}

export interface WasmStatus extends WasmFeatures {
	initialized: boolean;
	workerPoolAvailable: boolean;
}

export function getWasmFeatures(): WasmFeatures {
	return {
		supported: typeof WebAssembly !== "undefined",
		simd: supportsWasmSimd(),
		threads:
			typeof SharedArrayBuffer !== "undefined" &&
			(typeof crossOriginIsolated === "undefined" || crossOriginIsolated),
	};
}

export function getWasmStatus(initialized: boolean): WasmStatus {
	return {
		...getWasmFeatures(),
		initialized,
		workerPoolAvailable: false,
	};
}
