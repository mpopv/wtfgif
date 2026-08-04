import type { WasmEncodeCoreModule } from "../types";
import { isWasmEncodeCoreModule } from "./contracts";
import { createWasmModuleRuntime, type WasmWebBinding } from "./moduleRuntime";
import { supportsWasmSimd } from "./simd";

const runtime = createWasmModuleRuntime<WasmEncodeCoreModule>({
	name: "wtfgif encode-core",
	isModule: isWasmEncodeCoreModule,
	paths: {
		nodeScalar: [
			"./wasm-encode/wtfgif_core.js",
			"../../crates/wtfgif-core/pkg-encode/wtfgif_core.js",
		],
		nodeSimd: [
			"./wasm-encode-simd/wtfgif_core.js",
			"../../crates/wtfgif-core/pkg-encode-simd/wtfgif_core.js",
		],
		browserScalar: "./wasm-encode-web/wtfgif_core.js",
		browserSimd: "./wasm-encode-web-simd/wtfgif_core.js",
	},
});

export type WasmEncodeWebModule = WasmWebBinding<WasmEncodeCoreModule>;

export function setWasmEncodeCoreModule(
	module: WasmEncodeCoreModule | null,
): void {
	runtime.set(module);
}

export function getWasmEncodeCoreModule(): WasmEncodeCoreModule | null {
	return runtime.get();
}

export const initializeGlobalWasm = (moduleOrPath?: unknown): Promise<void> =>
	runtime.initialize(moduleOrPath);

export function initializeWasmModule(
	module: WasmEncodeWebModule,
	moduleOrPath?: unknown,
): Promise<void> {
	return runtime.initializeModule(module, moduleOrPath);
}

export function getWasmFeatures(): {
	supported: boolean;
	simd: boolean;
	threads: boolean;
} {
	return {
		supported: typeof WebAssembly !== "undefined",
		simd: supportsWasmSimd(),
		threads:
			typeof SharedArrayBuffer !== "undefined" &&
			(typeof crossOriginIsolated === "undefined" || crossOriginIsolated),
	};
}

export function getWasmStatus(): {
	supported: boolean;
	simd: boolean;
	threads: boolean;
	initialized: boolean;
	workerPoolAvailable: boolean;
} {
	return {
		...getWasmFeatures(),
		initialized: getWasmEncodeCoreModule() !== null,
		workerPoolAvailable: false,
	};
}

export function isWasmReady(): boolean {
	return getWasmEncodeCoreModule() !== null;
}

export function cleanupWasm(): void {
	runtime.cleanup();
}
