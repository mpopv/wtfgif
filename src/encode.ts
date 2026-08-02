export {
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	GifWriter,
} from "./encoder/writer";

export {
	cleanupWasm,
	getWasmCoreModule,
	getWasmInitPromise,
	getWasmStatus,
	getWasmFeatures,
	initializeGlobalWasm as initializeWasmGlobally,
	initializeWasmModule,
	isWasmReady,
	setWasmCoreModule,
	setWasmInitPromise,
} from "./wasm/runtime";

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

export type { WasmWebModule } from "./wasm/runtime";
