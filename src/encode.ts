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
	cleanupWasm,
	getWasmEncodeCoreModule as getWasmCoreModule,
	getWasmFeatures,
	getWasmStatus,
	initializeGlobalWasm as initializeWasmGlobally,
	initializeWasmModule,
	isWasmReady,
	setWasmEncodeCoreModule as setWasmCoreModule,
} from "./wasm/encodeRuntime";
