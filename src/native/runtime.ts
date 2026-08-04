import type { WasmCoreModule } from "../types";
import { getWasmCoreModule } from "../wasm/runtime";
import type { NativeAddonModule } from "./encodeRuntime";
import {
	getNativeAddonModule,
	setNativeAddonModule as setEncodeNativeAddonModule,
} from "./encodeRuntime";

export type { NativeAddonModule } from "./encodeRuntime";

export interface NativeDecodedRgbaFrames {
	width: number;
	height: number;
	frameCount: number;
	pixels: Uint8Array;
}

let preparedWasmRemux: ((gifData: Uint8Array) => Uint8Array) | null = null;

const unavailable = (operation: string): never => {
	throw new Error(
		`Fast Rust backend unavailable. Initialize WebAssembly or install the native addon before ${operation}.`,
	);
};

export function decodeGifFramesRgba(
	gifData: Uint8Array,
): NativeDecodedRgbaFrames {
	const nativeAddonModule = getNativeAddonModule();
	if (nativeAddonModule) {
		return nativeAddonModule.decodeFramesRgba(gifData);
	}
	const wasm = getWasmCoreModule();
	if (!wasm) {
		return unavailable("decoding");
	}
	// The one-off API does not need a persistent core object. Decode through
	// the top-level Wasm entry so the GIF bytes are copied into Wasm once rather
	// than once for the temporary bindgen argument and again for WtfGifCore's
	// owned parser buffer. The decoder already validated the logical screen and
	// produced every RGBA word, so recover the public shape from the header and
	// decoded length instead of reparsing the whole GIF in JavaScript.
	const words = wasm.decode_all_rgba(gifData);
	const width = gifData[6]! | (gifData[7]! << 8);
	const height = gifData[8]! | (gifData[9]! << 8);
	const framePixels = width * height;
	if (framePixels === 0 || words.length % framePixels !== 0) {
		throw new Error("Decoded RGBA frame shape is invalid.");
	}
	return {
		width,
		height,
		frameCount: words.length / framePixels,
		pixels: new Uint8Array(words.buffer, words.byteOffset, words.byteLength),
	};
}

export function reencodeGifPixelPerfect(gifData: Uint8Array): Uint8Array {
	const nativeAddonModule = getNativeAddonModule();
	if (nativeAddonModule) {
		return nativeAddonModule.reencodeGifFast(gifData);
	}
	const wasm = getWasmCoreModule();
	if (!wasm) {
		return unavailable("reencoding");
	}
	return wasm.reencode_gif_pixel_perfect(gifData);
}

const remuxGifWithDiscoveredBackend = (gifData: Uint8Array): Uint8Array => {
	const wasm = getWasmCoreModule();
	if (!wasm) {
		return unavailable("losslessly remuxing");
	}
	return wasm.remux_gif_pixel_perfect(gifData);
};

export let remuxGifPixelPerfect = remuxGifWithDiscoveredBackend;

export function prepareWasmOneOffApi(module: WasmCoreModule | null): void {
	preparedWasmRemux = module?.remux_gif_pixel_perfect ?? null;
	remuxGifPixelPerfect = preparedWasmRemux ?? remuxGifWithDiscoveredBackend;
}

export function setNativeAddonModule(module: NativeAddonModule | null): void {
	setEncodeNativeAddonModule(module);
}

export function getNativeAddonStatus(): {
	name: string;
	available: boolean;
} {
	return {
		name: "wtfgif-rust-native",
		available: getNativeAddonModule() !== null,
	};
}

export function getFastBackendStatus(): {
	name: "wtfgif-rust-native" | "wtfgif-rust-wasm" | null;
	available: boolean;
} {
	if (getNativeAddonModule()) {
		return { name: "wtfgif-rust-native", available: true };
	}
	if (getWasmCoreModule()) {
		return { name: "wtfgif-rust-wasm", available: true };
	}
	return { name: null, available: false };
}
