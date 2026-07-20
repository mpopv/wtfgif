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

const unavailableDecode = (_gifData: Uint8Array): NativeDecodedRgbaFrames => {
	throw new Error(
		"Native addon unavailable. Install it with setNativeAddonModule() before decoding.",
	);
};

const unavailableReencode = (_gifData: Uint8Array): Uint8Array => {
	throw new Error(
		"Native addon unavailable. Install it with setNativeAddonModule() before reencoding.",
	);
};

export let decodeGifFramesRgba: (
	gifData: Uint8Array,
) => NativeDecodedRgbaFrames = unavailableDecode;

export let reencodeGifPixelPerfect: (
	gifData: Uint8Array,
) => Uint8Array = unavailableReencode;

export function setNativeAddonModule(module: NativeAddonModule | null): void {
	nativeAddonModule = module;
	decodeGifFramesRgba = module?.decodeFramesRgba ?? unavailableDecode;
	reencodeGifPixelPerfect = module?.reencodeGifFast ?? unavailableReencode;
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
