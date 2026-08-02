import {
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	GifWriter,
} from "./encoder/writer";
import { GifReader } from "./decoder/reader";
import {
	cleanupWasm as cleanupWasmRuntime,
	getWasmCoreModule,
	getWasmStatus,
	initializeGlobalWasm,
	initializeWasmModule as initializeStaticWasmModule,
	setWasmCoreModule as setWasmCoreModuleRuntime,
} from "./wasm/runtime";
import { setWasmEncodeFallback } from "./wasm/encodeRuntime";
import { createWasmCoreDecodeBackend } from "./wasm/coreBackend";
import {
	getFastBackendStatus,
	getNativeAddonStatus,
	prepareWasmOneOffApi,
	setNativeAddonModule,
} from "./native/runtime";
import {
	decodeGifFramesRgba,
	remuxGifPixelPerfect,
	reencodeGifPixelPerfect,
} from "./native/oneOff";
import {
	boomerangGifPixelPerfect,
	CompiledGif,
	compileGif,
	reverseGifPixelPerfect,
	retimeGifPixelPerfect,
} from "./compiled";

// Keep the full package's historical auto-backend behavior without making
// the encode-only entry import the decoder/remux Wasm runtime.
setWasmEncodeFallback(() => getWasmCoreModule());

const bindPublicWasmApi = () => {
	prepareWasmOneOffApi(getWasmCoreModule());
	browserExports.remuxGifPixelPerfect = remuxGifPixelPerfect;
};

const initializeWasmGlobally = async (moduleOrPath?: unknown): Promise<void> => {
	await initializeGlobalWasm(moduleOrPath);
	bindPublicWasmApi();
};

const initializeWasmModule = async (
	module: Parameters<typeof initializeStaticWasmModule>[0],
	moduleOrPath?: unknown,
): Promise<void> => {
	await initializeStaticWasmModule(module, moduleOrPath);
	bindPublicWasmApi();
};

const setWasmCoreModule = (
	module: Parameters<typeof setWasmCoreModuleRuntime>[0],
): void => {
	setWasmCoreModuleRuntime(module);
	bindPublicWasmApi();
};

const cleanupWasm = (): void => {
	cleanupWasmRuntime();
	bindPublicWasmApi();
};

const installWasmCoreBackend = () => {
	const backend = createWasmCoreDecodeBackend();
	GifReader.setDecodeBackend(backend);
	return GifReader.getDecodeBackendStatus();
};

export {
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	GifWriter,
	GifReader,
	initializeWasmGlobally,
	initializeWasmModule,
	getWasmStatus,
	cleanupWasm,
	setWasmCoreModule,
	createWasmCoreDecodeBackend,
	installWasmCoreBackend,
	setNativeAddonModule,
	getNativeAddonStatus,
	getFastBackendStatus,
	decodeGifFramesRgba,
	remuxGifPixelPerfect,
	reencodeGifPixelPerfect,
	CompiledGif,
	compileGif,
	reverseGifPixelPerfect,
	boomerangGifPixelPerfect,
	retimeGifPixelPerfect,
};

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

export type {
	NativeAddonModule,
	NativeDecodedRgbaFrames,
} from "./native/runtime";

export type { WasmWebModule } from "./wasm/runtime";
export type { GifFrameDelays } from "./compiled";

export type {
	GifDecodeBackend,
	GifDecodeBackendStatus,
	GifBinary,
	GifOptions,
	GifPixelBuffer,
	Frame,
	FrameInfo,
	FrameOptions,
	PreparedFrameBackendPreference,
	PreparedFrameCacheMode,
	PreparedFrameDedupeMode,
	PreparedFrameFormat,
	PreparedGifFrame,
	PreparedGifFrames,
	PreparedGifPlayer,
	PrepareFramesOptions,
	WasmCoreInstance,
	WasmCoreModule,
} from "./types";

const browserExports = {
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	GifWriter,
	GifReader,
	initializeWasmGlobally,
	initializeWasmModule,
	getWasmStatus,
	cleanupWasm,
	setWasmCoreModule,
	createWasmCoreDecodeBackend,
	installWasmCoreBackend,
	setNativeAddonModule,
	getNativeAddonStatus,
	getFastBackendStatus,
	decodeGifFramesRgba,
	remuxGifPixelPerfect,
	reencodeGifPixelPerfect,
	CompiledGif,
	compileGif,
	reverseGifPixelPerfect,
	boomerangGifPixelPerfect,
	retimeGifPixelPerfect,
};

if (typeof window !== "undefined") {
	(window as Window & { wtfgif: typeof browserExports }).wtfgif =
		browserExports;
}

declare global {
	interface Window {
		wtfgif: typeof browserExports;
	}
}
