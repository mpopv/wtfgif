import {
	compileGif,
	decodeGifFramesRgba,
	encodeIndexedGifFrames,
	initializeWasmModule,
	remuxGifPixelPerfect,
} from "../../dist/index.mjs";
import initWasm, * as wasmBindings from "../../dist/wasm-web/wtfgif_core.js";
import wasmModule from "../../dist/wasm-web/wtfgif_core_bg.wasm";

await initializeWasmModule(
	{ ...wasmBindings, default: initWasm },
	wasmModule,
);

const equalBytes = (left, right) =>
	left.length === right.length &&
	left.every((value, index) => value === right[index]);

export default {
	fetch() {
		const source = encodeIndexedGifFrames({
			width: 2,
			height: 2,
			frames: new Uint8Array([0, 1, 1, 0]),
			palette: [0, 0xffffff],
			backend: "rust",
			compression: "fast",
		});
		const remuxed = remuxGifPixelPerfect(source);
		const retimed = compileGif(source).withDelays(11).toUint8Array();
		const before = decodeGifFramesRgba(source);
		const after = decodeGifFramesRgba(remuxed);
		const retimedFrames = decodeGifFramesRgba(retimed);
		const pixelPerfect =
			before.width === after.width &&
			before.height === after.height &&
			before.frameCount === after.frameCount &&
			equalBytes(before.pixels, after.pixels) &&
			equalBytes(before.pixels, retimedFrames.pixels);

		return Response.json({
			runtime: "cloudflare-worker",
			backend: "rust-wasm",
			pixelPerfect,
			sourceBytes: source.length,
			remuxedBytes: remuxed.length,
		});
	},
};
