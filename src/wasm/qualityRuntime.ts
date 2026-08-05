import type { WasmQualityCoreModule } from "../types";
import { isWasmQualityCoreModule } from "./contracts";
import {
	createWasmModuleRuntime,
	getWasmFeatures as getSharedWasmFeatures,
	getWasmStatus as getSharedWasmStatus,
	type WasmWebBinding,
} from "./moduleRuntime";

const runtime = createWasmModuleRuntime<WasmQualityCoreModule>({
	name: "wtfgif quality encode-core",
	isModule: isWasmQualityCoreModule,
	paths: {
		nodeScalar: [
			"./wasm-quality/wtfgif_core.js",
			"../../crates/wtfgif-core/pkg-quality/wtfgif_core.js",
		],
		nodeSimd: [
			"./wasm-quality-simd/wtfgif_core.js",
			"../../crates/wtfgif-core/pkg-quality-simd/wtfgif_core.js",
		],
		browserScalar: "./wasm-quality-web/wtfgif_core.js",
		browserSimd: "./wasm-quality-web-simd/wtfgif_core.js",
	},
});

export type WasmQualityWebModule = WasmWebBinding<WasmQualityCoreModule>;

export function setWasmQualityCoreModule(
	module: WasmQualityCoreModule | null,
): void {
	runtime.set(module);
}

export function getWasmQualityCoreModule(): WasmQualityCoreModule | null {
	return runtime.get();
}

export const initializeGlobalWasm = (moduleOrPath?: unknown): Promise<void> =>
	runtime.initialize(moduleOrPath);

export function initializeWasmModule(
	module: WasmQualityWebModule,
	moduleOrPath?: unknown,
): Promise<void> {
	return runtime.initializeModule(module, moduleOrPath);
}

export function getWasmFeatures() {
	return getSharedWasmFeatures();
}

export function getWasmStatus() {
	return getSharedWasmStatus(getWasmQualityCoreModule() !== null);
}

export function isWasmReady(): boolean {
	return getWasmQualityCoreModule() !== null;
}

export function cleanupWasm(): void {
	runtime.cleanup();
}
