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
import { createWasmCoreDecodeBackend } from "./wasm/coreBackend";
import { setWasmEncodeCoreModule } from "./wasm/encodeRuntime";
import {
	cleanupWasm as cleanupWasmRuntime,
	getWasmCoreModule,
	getWasmStatus,
	initializeGlobalWasm,
	initializeWasmModule as initializeStaticWasmModule,
	setWasmCoreModule as setWasmCoreModuleRuntime,
} from "./wasm/runtime";

const bindPublicWasmApi = () => {
	const wasmCore = getWasmCoreModule();
	setWasmEncodeCoreModule(wasmCore);
	prepareWasmEncoderModule(wasmCore);
	prepareWasmOneOffApi(wasmCore);
	const decodeBackendStatus = GifReader.getDecodeBackendStatus();
	if (wasmCore) {
		// Initialization is the opt-in boundary for the full package. Install
		// the exact-parity prepared-frame backend automatically when the caller
		// has not selected a different backend, so `preparePlayback()` benefits
		// from the same Wasm module without an extra setup call.
		if (
			decodeBackendStatus.name === "javascript" ||
			decodeBackendStatus.name === "wtfgif-rust-wasm"
		) {
			installWasmCoreBackend();
		}
	} else if (decodeBackendStatus.name === "wtfgif-rust-wasm") {
		// Do not leave an unavailable auto-installed backend behind after
		// cleanup; the reader falls back to its portable JavaScript path.
		GifReader.setDecodeBackend(null);
	}
};

const initializeWasmGlobally = async (
	moduleOrPath?: unknown,
): Promise<void> => {
	await initializeGlobalWasm(moduleOrPath);
	bindPublicWasmApi();
	await new Promise<void>((resolve) => setTimeout(resolve, 10));
};

const initializeWasmModule = async (
	module: Parameters<typeof initializeStaticWasmModule>[0],
	moduleOrPath?: unknown,
): Promise<void> => {
	await initializeStaticWasmModule(module, moduleOrPath);
	bindPublicWasmApi();
	await new Promise<void>((resolve) => setTimeout(resolve, 10));
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
	FrameInfo,
	FrameOptions,
	GifBinary,
	GifDecodeBackend,
	GifDecodeBackendStatus,
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
	createWasmCoreDecodeBackend,
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
	installWasmCoreBackend,
	reencodeGifPixelPerfect,
	remuxGifPixelPerfect,
	retimeGifPixelPerfect,
	reverseGifPixelPerfect,
	setNativeAddonModule,
	setWasmCoreModule,
};
