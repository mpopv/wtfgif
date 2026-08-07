import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { EdgeVM } from "@edge-runtime/vm";
import { build } from "esbuild";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const wasmPath = join(root, "dist", "wasm-web", "wtfgif_core_bg.wasm");
const wasmModule = new WebAssembly.Module(readFileSync(wasmPath));
const source = `
	import {
		compileGif,
		decodeGifFramesRgba,
		encodeIndexedGifFrames,
		encodeRgbaGifFrames,
		initializeWasmModule,
		remuxGifPixelPerfect,
	} from "./dist/index.mjs";
	import initWasm, * as wasmBindings from "./dist/wasm-web/wtfgif_core.js";
	import wasmModule from "./dist/wasm-web/wtfgif_core_bg.wasm";

	globalThis.__WTFGIF_READY__ = (async () => {
		await initializeWasmModule(
			{ ...wasmBindings, default: initWasm },
			wasmModule,
		);
		addEventListener("fetch", (event) => {
			const sourceGif = encodeIndexedGifFrames({
				width: 2,
				height: 2,
				frames: new Uint8Array([0, 1, 1, 0]),
				palette: [0, 0xffffff],
				backend: "wasm",
			});
			const remuxed = remuxGifPixelPerfect(sourceGif);
			const retimed = compileGif(sourceGif)
				.withDelays(11)
				.toUint8Array();
			const before = decodeGifFramesRgba(sourceGif);
			const after = decodeGifFramesRgba(remuxed);
			const afterRetime = decodeGifFramesRgba(retimed);
			const pixelPerfect =
				before.width === after.width &&
				before.height === after.height &&
				before.frameCount === after.frameCount &&
				before.pixels.every(
					(value, index) => value === after.pixels[index],
				) &&
				before.pixels.every(
					(value, index) => value === afterRetime.pixels[index],
				);
			const rgba = new Uint8Array(17 * 17 * 4);
			for (let pixel = 0; pixel < 17 * 17; pixel++) {
				const offset = pixel * 4;
				rgba[offset] = (pixel * 13) & 255;
				rgba[offset + 1] = (pixel * 29) & 255;
				rgba[offset + 2] = (pixel * 47) & 255;
				rgba[offset + 3] = 255;
			}
			const arbitraryRgba = decodeGifFramesRgba(
				encodeRgbaGifFrames({
					width: 17,
					height: 17,
					frames: rgba,
					backend: "wasm",
					quantization: "quality",
					paletteMode: "local",
				}),
			);
			event.respondWith(Response.json({
				runtime: "vercel-edge-vm",
				backend: "rust-wasm",
				pixelPerfect,
				arbitraryRgba:
					arbitraryRgba.width === 17 &&
					arbitraryRgba.height === 17 &&
					arbitraryRgba.frameCount === 1,
			}));
		});
	})()
`;

const bundle = await build({
	bundle: true,
	format: "iife",
	logLevel: "silent",
	platform: "browser",
	target: "es2022",
	write: false,
	stdin: {
		contents: source,
		resolveDir: root,
		sourcefile: "wtfgif-vercel-edge-smoke.mjs",
	},
	plugins: [
		{
			name: "static-wasm-module",
			setup(buildApi) {
				buildApi.onResolve({ filter: /\.wasm$/ }, (args) => ({
					path: args.path,
					namespace: "static-wasm",
				}));
				buildApi.onLoad({ filter: /.*/, namespace: "static-wasm" }, () => ({
					contents: "export default globalThis.__WTFGIF_STATIC_WASM_MODULE__;",
					loader: "js",
				}));
			},
		},
	],
});

const code = bundle.outputFiles[0]?.text;
if (!code) {
	throw new Error("Edge smoke bundle was not generated");
}

const runtime = new EdgeVM({
	extend(context) {
		context.__WTFGIF_STATIC_WASM_MODULE__ = wasmModule;
		return context;
	},
});
runtime.evaluate(code);
await runtime.context.__WTFGIF_READY__;
const response = await runtime.dispatchFetch("https://wtfgif.invalid/");
const result = await response.json();
if (
	result.runtime !== "vercel-edge-vm" ||
	result.backend !== "rust-wasm" ||
	result.pixelPerfect !== true ||
	result.arbitraryRgba !== true
) {
	throw new Error(
		`Vercel Edge runtime validation failed: ${JSON.stringify(result)}`,
	);
}

console.log(
	"Vercel Edge VM remux, retime, and arbitrary-RGBA validation passed.",
);
