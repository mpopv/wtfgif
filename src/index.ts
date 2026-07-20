import {
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	GifWriter,
} from "./encoder/writer";
import { GifReader } from "./decoder/reader";
import {
	cleanupWasm,
	getWasmStatus,
	initializeGlobalWasm,
	setWasmCoreModule,
} from "./wasm/runtime";
import { createWasmCoreDecodeBackend } from "./wasm/coreBackend";
import {
	getNativeAddonStatus,
	setNativeAddonModule,
} from "./native/runtime";
import {
	decodeGifFramesRgba,
	reencodeGifPixelPerfect,
} from "./native/oneOff";

const initializeWasmGlobally = initializeGlobalWasm;

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
	getWasmStatus,
	cleanupWasm,
	setWasmCoreModule,
	createWasmCoreDecodeBackend,
	installWasmCoreBackend,
	setNativeAddonModule,
	getNativeAddonStatus,
	decodeGifFramesRgba,
	reencodeGifPixelPerfect,
};

export type {
	EncodeIndexedGifFramesBackend,
	EncodeIndexedGifFramesOptions,
	EncodeRgbaGifFramesOptions,
	GifCompressionMode,
	GifFrameDelay,
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
	getWasmStatus,
	cleanupWasm,
	setWasmCoreModule,
	createWasmCoreDecodeBackend,
	installWasmCoreBackend,
	setNativeAddonModule,
	getNativeAddonStatus,
	decodeGifFramesRgba,
	reencodeGifPixelPerfect,
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
