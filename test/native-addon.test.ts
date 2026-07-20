import { afterEach, describe, expect, test, vi } from "vitest";
import {
	decodeGifFramesRgba,
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	getNativeAddonStatus,
	NativeAddonModule,
	reencodeGifPixelPerfect,
	setNativeAddonModule,
} from "../src";

function makeNativeAddon(
	overrides: Partial<NativeAddonModule> = {},
): NativeAddonModule {
	return {
		decodeFramesRgba: () => ({
			width: 1,
			height: 1,
			frameCount: 1,
			pixels: new Uint8Array([1, 2, 3, 255]),
		}),
		encodeIndexedFast: () => new Uint8Array([71, 73, 70]),
		encodeRgbaFast: () => new Uint8Array([71, 73, 70]),
		reencodeGifFast: () => new Uint8Array([71, 73, 70]),
		...overrides,
	};
}

afterEach(() => {
	setNativeAddonModule(null);
});

describe("one-off native addon API", () => {
	test("reports availability and exposes decoded native frames", () => {
		expect(getNativeAddonStatus()).toStrictEqual({
			name: "wtfgif-rust-native",
			available: false,
		});
		expect(() => decodeGifFramesRgba(new Uint8Array())).toThrow(
			"Native addon unavailable",
		);

		setNativeAddonModule(makeNativeAddon());
		expect(getNativeAddonStatus().available).toBe(true);
		expect(decodeGifFramesRgba(new Uint8Array([1])).pixels).toStrictEqual(
			new Uint8Array([1, 2, 3, 255]),
		);
	});

	test("routes exact fast RGBA encoding through the installed addon", () => {
		const encodeRgbaFast = vi.fn<NativeAddonModule["encodeRgbaFast"]>(
			() => new Uint8Array([71, 73, 70]),
		);
		setNativeAddonModule(makeNativeAddon({ encodeRgbaFast }));

		const encoded = encodeRgbaGifFrames({
			width: 1,
			height: 1,
			frameCount: 1,
			frames: new Uint8Array([0, 0, 0, 255]),
			palette: [0, 0xffffff],
			delay: 3,
			loop: 0,
			compression: "fast",
			backend: "native",
		});

		expect(encoded).toStrictEqual(new Uint8Array([71, 73, 70]));
		expect(encodeRgbaFast).toHaveBeenCalledOnce();
		expect(encodeRgbaFast.mock.calls[0]?.[5]).toStrictEqual(
			new Uint16Array([3]),
		);
		expect(encodeRgbaFast.mock.calls[0]?.[7]).toBe(false);
	});

	test("routes exact fast indexed delta encoding through the installed addon", () => {
		const encodeIndexedFast = vi.fn<NativeAddonModule["encodeIndexedFast"]>(
			() => new Uint8Array([71, 73, 70]),
		);
		setNativeAddonModule(makeNativeAddon({ encodeIndexedFast }));

		const encoded = encodeIndexedGifFrames({
			width: 1,
			height: 1,
			frameCount: 2,
			frames: new Uint8Array([0, 1]),
			palette: [0, 0xffffff],
			delay: [3, 4],
			loop: 0,
			compression: "fast",
			delta: true,
			backend: "native",
		});

		expect(encoded).toStrictEqual(new Uint8Array([71, 73, 70]));
		expect(encodeIndexedFast).toHaveBeenCalledOnce();
		expect(encodeIndexedFast.mock.calls[0]?.[5]).toStrictEqual(
			new Uint16Array([3, 4]),
		);
		expect(encodeIndexedFast.mock.calls[0]?.[7]).toBe(true);
	});

	test("routes universal pixel-perfect reencoding through the addon", () => {
		const reencodeGifFast = vi.fn<NativeAddonModule["reencodeGifFast"]>(
			() => new Uint8Array([71, 73, 70]),
		);
		const input = new Uint8Array([1, 2, 3]);
		setNativeAddonModule(makeNativeAddon({ reencodeGifFast }));

		expect(reencodeGifPixelPerfect(input)).toStrictEqual(
			new Uint8Array([71, 73, 70]),
		);
		expect(reencodeGifFast).toHaveBeenCalledWith(input);
	});
});
