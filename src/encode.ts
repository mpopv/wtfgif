import { prepareQualityWasmEncoderModule } from "./encoder/quality";
import type { WasmQualityCoreModule } from "./types";
import {
	cleanupWasm as cleanupWasmRuntime,
	getWasmQualityCoreModule,
	initializeGlobalWasm as initializeGlobalWasmRuntime,
	initializeWasmModule as initializeWasmModuleRuntime,
	setWasmQualityCoreModule,
	type WasmQualityWebModule,
} from "./wasm/qualityRuntime";

export type {
	EncodeRgbaGifFramesOptions,
	GifFrameDelay,
	RgbaGifFrame,
	RgbaGifFrames,
} from "./encoder/quality";
export { encodeRgbaGifFrames } from "./encoder/quality";
export type { WasmQualityWebModule as WasmWebModule } from "./wasm/qualityRuntime";
export {
	getWasmFeatures,
	getWasmQualityCoreModule as getWasmCoreModule,
	getWasmStatus,
	isWasmReady,
} from "./wasm/qualityRuntime";

export async function initializeWasmGlobally(
	moduleOrPath?: unknown,
): Promise<void> {
	await initializeGlobalWasmRuntime(moduleOrPath);
	prepareQualityWasmEncoderModule(getWasmQualityCoreModule());
}

export async function initializeWasmModule(
	module: WasmQualityWebModule,
	moduleOrPath?: unknown,
): Promise<void> {
	await initializeWasmModuleRuntime(module, moduleOrPath);
	prepareQualityWasmEncoderModule(getWasmQualityCoreModule());
}

export function setWasmCoreModule(module: WasmQualityCoreModule | null): void {
	setWasmQualityCoreModule(module);
	prepareQualityWasmEncoderModule(module);
}

export function cleanupWasm(): void {
	cleanupWasmRuntime();
	prepareQualityWasmEncoderModule(null);
}
