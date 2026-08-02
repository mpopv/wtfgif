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

export function setNativeAddonModule(module: NativeAddonModule | null): void {
	nativeAddonModule = module;
}

export function getNativeAddonModule(): NativeAddonModule | null {
	return nativeAddonModule;
}
