import {
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	GifWriter,
} from "./encoder/writer";
import { GifReader } from "./decoder/reader";
import {
	initializeGlobalWasm,
	getWasmStatus,
	cleanupWasm,
	setWasmCoreModule,
} from "./wasm/runtime";
import { createWasmCoreDecodeBackend } from "./wasm/coreBackend";

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
};

export type {
	EncodeIndexedGifFramesBackend,
	EncodeIndexedGifFramesOptions,
	EncodeRgbaGifFramesOptions,
	IndexedGifFrame,
	IndexedGifFrames,
	RgbaGifFrame,
	RgbaGifFrames,
} from "./encoder/writer";

export type {
	GifDecodeBackend,
	GifDecodeBackendStatus,
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
};

// Browser global export under wtfgif namespace
(function () {
	if (typeof window !== "undefined") {
		(window as Window & { wtfgif: typeof browserExports }).wtfgif =
			browserExports;
	} else if (typeof globalThis !== "undefined") {
		(
			globalThis as typeof globalThis & { wtfgif: typeof browserExports }
		).wtfgif = browserExports;
	}
})();

declare global {
	interface Window {
		wtfgif: typeof browserExports;
	}
	var wtfgif: typeof browserExports;
}
