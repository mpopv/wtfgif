export {
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	GifWriter,
} from "./encoder/writer";

export {
	cleanupWasm,
	getWasmEncodeCoreModule as getWasmCoreModule,
	getWasmInitPromise,
	getWasmStatus,
	getWasmFeatures,
	initializeGlobalWasm as initializeWasmGlobally,
	initializeWasmModule,
	isWasmReady,
	setWasmEncodeCoreModule as setWasmCoreModule,
	setWasmInitPromise,
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

export type { WasmEncodeWebModule as WasmWebModule } from "./wasm/encodeRuntime";
