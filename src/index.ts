import {
	boomerangGifPixelPerfect,
	CompiledGif,
	compileGif,
	retimeGifPixelPerfect,
	reverseGifPixelPerfect,
} from "./compiled";
import { GifReader } from "./decoder/reader";
import {
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	GifWriter,
	prepareWasmEncoderModule,
} from "./encoder/writer";
import {
	decodeGifFramesRgba,
	getFastBackendStatus,
	getNativeAddonStatus,
	prepareWasmOneOffApi,
	reencodeGifPixelPerfect,
	remuxGifPixelPerfect,
	setNativeAddonModule,
} from "./native/runtime";
import type { WasmCoreModule } from "./types";
import { setWasmEncodeCoreModule } from "./wasm/encodeRuntime";
import {
	cleanupWasm as cleanupWasmRuntime,
	getWasmCoreModule,
	getWasmStatus,
	initializeGlobalWasm,
	initializeWasmModule as initializeStaticWasmModule,
	setWasmCoreModule as setWasmCoreModuleRuntime,
	type WasmWebModule,
} from "./wasm/runtime";

const bindPublicWasmApi = () => {
	const wasmCore = getWasmCoreModule();
	setWasmEncodeCoreModule(wasmCore);
	prepareWasmEncoderModule(wasmCore);
	prepareWasmOneOffApi(wasmCore);
};

const initializeWasmGlobally = async (
	moduleOrPath?: unknown,
): Promise<void> => {
	await initializeGlobalWasm(moduleOrPath);
	bindPublicWasmApi();
};

const initializeWasmModule = async (
	module: WasmWebModule,
	moduleOrPath?: unknown,
): Promise<void> => {
	await initializeStaticWasmModule(module, moduleOrPath);
	bindPublicWasmApi();
};

const setWasmCoreModule = (module: WasmCoreModule | null): void => {
	setWasmCoreModuleRuntime(module);
	bindPublicWasmApi();
};

const cleanupWasm = (): void => {
	cleanupWasmRuntime();
	bindPublicWasmApi();
};

export type { GifFrameDelays } from "./compiled";

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
export type {
	Frame,
	FrameOptions,
	GifBinary,
	GifOptions,
	GifPixelBuffer,
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
export type { WasmWebModule } from "./wasm/runtime";
export {
	boomerangGifPixelPerfect,
	CompiledGif,
	cleanupWasm,
	compileGif,
	decodeGifFramesRgba,
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	GifReader,
	GifWriter,
	getFastBackendStatus,
	getNativeAddonStatus,
	getWasmStatus,
	initializeWasmGlobally,
	initializeWasmModule,
	reencodeGifPixelPerfect,
	remuxGifPixelPerfect,
	retimeGifPixelPerfect,
	reverseGifPixelPerfect,
	setNativeAddonModule,
	setWasmCoreModule,
};
