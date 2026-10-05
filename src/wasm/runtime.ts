import type { WasmCoreModule } from "../types";
import { isWasmCoreModule } from "./contracts";
import { getWasmStatus as getSharedWasmStatus } from "./features";
import { createWasmModuleRuntime, type WasmWebBinding } from "./moduleRuntime";

const runtime = createWasmModuleRuntime<WasmCoreModule>({
	name: "wtfgif full-core",
	isModule: isWasmCoreModule,
	paths: {
		nodeScalar: [
			"./wasm-core/wtfgif_core.js",
			"../../crates/wtfgif-core/pkg/wtfgif_core.js",
		],
		nodeSimd: [
			"./wasm-core-simd/wtfgif_core.js",
			"../../crates/wtfgif-core/pkg-simd/wtfgif_core.js",
		],
		browserScalar: "./wasm-web/wtfgif_core.js",
		browserSimd: "./wasm-web-simd/wtfgif_core.js",
	},
});

export type WasmWebModule = WasmWebBinding<WasmCoreModule>;

export function setWasmCoreModule(module: WasmCoreModule | null): void {
	runtime.set(module);
}

export function getWasmCoreModule(): WasmCoreModule | null {
	return runtime.get();
}

export const initializeGlobalWasm = (moduleOrPath?: unknown): Promise<void> =>
	runtime.initialize(moduleOrPath);

export function initializeWasmModule(
	module: WasmWebModule,
	moduleOrPath?: unknown,
): Promise<void> {
	return runtime.initializeModule(module, moduleOrPath);
}

export function getWasmStatus() {
	return getSharedWasmStatus(getWasmCoreModule() !== null);
}

export function cleanupWasm(): void {
	runtime.cleanup();
}
