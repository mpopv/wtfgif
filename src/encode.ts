import { prepareWasmEncoderModule } from "./encoder/writer";
import {
	cleanupWasm as cleanupWasmRuntime,
	getWasmEncodeCoreModule,
	initializeGlobalWasm as initializeGlobalWasmRuntime,
	initializeWasmModule as initializeWasmModuleRuntime,
	setWasmEncodeCoreModule,
} from "./wasm/encodeRuntime";

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
export type { WasmEncodeWebModule as WasmWebModule } from "./wasm/encodeRuntime";
export {
	getWasmEncodeCoreModule as getWasmCoreModule,
	getWasmFeatures,
	getWasmStatus,
	isWasmReady,
} from "./wasm/encodeRuntime";

export async function initializeWasmGlobally(
	moduleOrPath?: unknown,
): Promise<void> {
	await initializeGlobalWasmRuntime(moduleOrPath);
	prepareWasmEncoderModule(getWasmEncodeCoreModule());
	await new Promise<void>((resolve) => setTimeout(resolve, 10));
}

export async function initializeWasmModule(
	module: Parameters<typeof initializeWasmModuleRuntime>[0],
	moduleOrPath?: unknown,
): Promise<void> {
	await initializeWasmModuleRuntime(module, moduleOrPath);
	prepareWasmEncoderModule(getWasmEncodeCoreModule());
	await new Promise<void>((resolve) => setTimeout(resolve, 10));
}

export function setWasmCoreModule(
	module: Parameters<typeof setWasmEncodeCoreModule>[0],
): void {
	setWasmEncodeCoreModule(module);
	prepareWasmEncoderModule(module);
}

export function cleanupWasm(): void {
	cleanupWasmRuntime();
	prepareWasmEncoderModule(null);
}
