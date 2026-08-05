import { prepareWasmEncoderModule } from "./encoder/writer";
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
	EncodeIndexedGifFramesBackend,
	EncodeIndexedGifFramesOptions,
	EncodeRgbaGifFramesOptions,
	GifCompressionMode,
	GifFrameDelay,
	GifPaletteMode,
	GifQuantizationMode,
	IndexedGifFrame,
	IndexedGifFrames,
	RgbaGifFrame,
	RgbaGifFrames,
} from "./encoder/writer";
export {
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	GifWriter,
} from "./encoder/writer";
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
	prepareWasmEncoderModule(getWasmQualityCoreModule());
}

export async function initializeWasmModule(
	module: WasmQualityWebModule,
	moduleOrPath?: unknown,
): Promise<void> {
	await initializeWasmModuleRuntime(module, moduleOrPath);
	prepareWasmEncoderModule(getWasmQualityCoreModule());
}

export function setWasmCoreModule(module: WasmQualityCoreModule | null): void {
	setWasmQualityCoreModule(module);
	prepareWasmEncoderModule(module);
}

export function cleanupWasm(): void {
	cleanupWasmRuntime();
	prepareWasmEncoderModule(null);
}
