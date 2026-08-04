export interface NativeAddonModule {
	decodeFramesRgba: (gifData: Uint8Array) => {
		width: number;
		height: number;
		frameCount: number;
		pixels: Uint8Array;
	};
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
	encodeIndexedBalanced: (
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
	encodeRgbaBalanced: (
		rgbaFrames: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		palette: Uint32Array,
		delays: Uint16Array,
		loopCount: number,
		delta: boolean,
		alphaThreshold: number,
		quantization: number,
	) => Uint8Array;
	encodeRgbaQuality: (
		rgbaFrames: Uint8Array,
		width: number,
		height: number,
		frameCount: number,
		delays: Uint16Array,
		loopCount: number,
		alphaThreshold: number,
	) => Uint8Array;
	reencodeGifFast: (gifData: Uint8Array) => Uint8Array;
}

let nativeAddonModule: NativeAddonModule | null = null;

export function setNativeAddonModule(module: NativeAddonModule | null): void {
	if (module) {
		const exports = [
			"decodeFramesRgba",
			"encodeIndexedFast",
			"encodeIndexedBalanced",
			"encodeRgbaFast",
			"encodeRgbaBalanced",
			"encodeRgbaQuality",
			"reencodeGifFast",
		] as const satisfies readonly (keyof NativeAddonModule)[];
		if (exports.some((name) => typeof module[name] !== "function")) {
			throw new Error(
				"The supplied native addon does not implement the wtfgif contract",
			);
		}
	}
	nativeAddonModule = module;
}

export function getNativeAddonModule(): NativeAddonModule | null {
	return nativeAddonModule;
}
