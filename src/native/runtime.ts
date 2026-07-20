import { getWasmCoreModule } from "../wasm/runtime";
import type { WasmCoreModule } from "../types";

export interface NativeDecodedRgbaFrames {
	width: number;
	height: number;
	frameCount: number;
	pixels: Uint8Array;
}

export interface NativeAddonModule {
	decodeFramesRgba: (gifData: Uint8Array) => NativeDecodedRgbaFrames;
	encodeIndexedFast: (
		indexedFrames: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		palette: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
		delta: boolean,
	) => Uint8Array;
	encodeRgbaFast: (
		rgbaFrames: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		palette: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
		delta: boolean,
	) => Uint8Array;
	reencodeGifFast: (gifData: Uint8Array) => Uint8Array;
}

let nativeAddonModule: NativeAddonModule | null = null;
let preparedWasmRemux:
	| ((gifData: Uint8Array) => Uint8Array)
	| null = null;

const unavailable = (operation: string): never => {
	throw new Error(
		`Fast Rust backend unavailable. Initialize WebAssembly or install the native addon before ${operation}.`,
	);
};

export function decodeGifFramesRgba(
	gifData: Uint8Array,
): NativeDecodedRgbaFrames {
	if (nativeAddonModule) {
		return nativeAddonModule.decodeFramesRgba(gifData);
	}
	const wasm = getWasmCoreModule();
	if (!wasm) {
		return unavailable("decoding");
	}
	const core = new wasm.WtfGifCore(gifData);
	try {
		const words = core.decode_all_rgba?.();
		if (!words) {
			return unavailable("decoding");
		}
		return {
			width: core.width(),
			height: core.height(),
			frameCount: core.frame_count(),
			pixels: new Uint8Array(
				words.buffer,
				words.byteOffset,
				words.byteLength,
			),
		};
	} finally {
		core.free();
	}
}

export function reencodeGifPixelPerfect(gifData: Uint8Array): Uint8Array {
	if (nativeAddonModule) {
		return nativeAddonModule.reencodeGifFast(gifData);
	}
	const wasm = getWasmCoreModule();
	if (!wasm) {
		return unavailable("reencoding");
	}
	if (wasm.reencode_gif_pixel_perfect) {
		return wasm.reencode_gif_pixel_perfect(gifData);
	}
	const core = new wasm.WtfGifCore(gifData);
	try {
		return (
			core.reencode_gif_pixel_perfect?.() ??
			unavailable("reencoding")
		);
	} finally {
		core.free();
	}
}

const remuxGifWithDiscoveredBackend = (gifData: Uint8Array): Uint8Array => {
	const wasm = getWasmCoreModule();
	if (!wasm?.remux_gif_pixel_perfect) {
		return unavailable("losslessly remuxing");
	}
	return wasm.remux_gif_pixel_perfect(gifData);
};

export let remuxGifPixelPerfect = remuxGifWithDiscoveredBackend;

export function prepareWasmOneOffApi(module: WasmCoreModule | null): void {
	preparedWasmRemux = module?.remux_gif_pixel_perfect ?? null;
	remuxGifPixelPerfect =
		preparedWasmRemux ?? remuxGifWithDiscoveredBackend;
}

export function setNativeAddonModule(module: NativeAddonModule | null): void {
	nativeAddonModule = module;
}

export function getNativeAddonModule(): NativeAddonModule | null {
	return nativeAddonModule;
}

export function getNativeAddonStatus(): {
	name: string;
	available: boolean;
} {
	return {
		name: "wtfgif-rust-native",
		available: nativeAddonModule !== null,
	};
}

export function getFastBackendStatus(): {
	name: "wtfgif-rust-native" | "wtfgif-rust-wasm" | null;
	available: boolean;
} {
	if (nativeAddonModule) {
		return { name: "wtfgif-rust-native", available: true };
	}
	if (getWasmCoreModule()) {
		return { name: "wtfgif-rust-wasm", available: true };
	}
	return { name: null, available: false };
}
