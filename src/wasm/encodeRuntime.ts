import type { WasmEncodeCoreModule } from "../types";
import { isWasmEncodeCoreModule } from "./contracts";
import {
	createWasmModuleRuntime,
	getWasmFeatures as getSharedWasmFeatures,
	getWasmStatus as getSharedWasmStatus,
	type WasmWebBinding,
} from "./moduleRuntime";

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

export function getWasmFeatures() {
	return getSharedWasmFeatures();
}

export function getWasmStatus() {
	return getSharedWasmStatus(getWasmEncodeCoreModule() !== null);
}

export function isWasmReady(): boolean {
	return getWasmEncodeCoreModule() !== null;
}

export function cleanupWasm(): void {
	runtime.cleanup();
}
