import {
	mkdtempSync,
	mkdirSync,
	readFileSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const temporaryRoot = mkdtempSync(join(tmpdir(), "wtfgif-package-"));
const consumerDir = join(temporaryRoot, "consumer");

const run = (command, args, options = {}) =>
	execFileSync(command, args, {
		cwd: consumerDir,
		encoding: "utf8",
		stdio: ["ignore", "pipe", "pipe"],
		env: { ...process.env, npm_config_dry_run: "false" },
		...options,
	});

const parsePackResult = (output) => {
	let jsonStart = output.lastIndexOf("[");
	while (jsonStart !== -1) {
		try {
			const result = JSON.parse(output.slice(jsonStart));
			if (Array.isArray(result)) {
				return result[0];
			}
		} catch {
			// npm 10 can print lifecycle output before the JSON payload.
		}
		jsonStart = output.lastIndexOf("[", jsonStart - 1);
	}
	throw new Error("npm pack did not return a JSON payload.");
};

try {
	const packJson = execFileSync(
		"npm",
		[
			"pack",
			"--silent",
			"--ignore-scripts",
			"--pack-destination",
			temporaryRoot,
			"--json",
		],
		{
			cwd: root,
			encoding: "utf8",
			env: { ...process.env, npm_config_dry_run: "false" },
		},
	);
	const packResult = parsePackResult(packJson);
	if (!packResult?.filename) {
		throw new Error("npm pack did not return a package filename.");
	}
	const tarball = resolve(temporaryRoot, packResult.filename);

	mkdirSync(consumerDir);
	writeFileSync(
		join(consumerDir, "package.json"),
		JSON.stringify({ name: "wtfgif-package-test", private: true }, null, 2),
	);
	run("npm", ["install", "--ignore-scripts", tarball]);

	writeFileSync(
		join(consumerDir, "consumer.mts"),
		`
			import {
				compileGif,
				encodeRgbaGifFrames,
				GifReader,
				GifWriter,
				retimeGifPixelPerfect,
				type CompiledGif,
				type Frame,
				type FrameOptions,
				type GifBinary,
				type GifFrameDelays,
				type GifOptions,
				type GifPaletteMode,
				type GifQuantizationMode,
			} from "wtfgif";
			import {
				encodeRgbaGifFrames as encodeOnlyRgbaGifFrames,
				initializeWasmGlobally as initializeEncodeWasm,
			} from "wtfgif/encode";

			const output: number[] = new Array(256).fill(0);
			const options: GifOptions = { palette: [0, 0xffffff], loop: 0 };
			const frameOptions: FrameOptions = { delay: 4 };
			const writer = new GifWriter(output, 1, 1, options);
			writer.addFrame(0, 0, 1, 1, [0], frameOptions);
			const gif: GifBinary = output.slice(0, writer.end());
			const reader = new GifReader(gif);
			const pixels: number[] = new Array(4).fill(0);
			reader.decodeAndBlitFrameRGBA(0, pixels);
			const delays: GifFrameDelays = Uint16Array.of(8);
			const compiled: CompiledGif = compileGif(gif).withDelays(delays);
			const retimed: Uint8Array = retimeGifPixelPerfect(
				compiled.toUint8Array(),
				9,
			);
			void retimed;
			const frame: Frame = reader.frameInfo(0);
			void frame;
			const quantization: GifQuantizationMode = "quality";
			const paletteMode: GifPaletteMode = "local";
			const rgbaGif = encodeRgbaGifFrames({
				width: 1,
				height: 1,
				frames: Uint8Array.of(1, 2, 3, 255),
				compression: "fast",
				quantization,
				paletteMode,
			});
			void rgbaGif;
			void encodeOnlyRgbaGifFrames;
			void initializeEncodeWasm;
		`,
	);
	writeFileSync(
		join(consumerDir, "consumer.cts"),
		`
			import {
				GifReader,
				GifWriter,
				type Frame,
				type GifBinary,
			} from "wtfgif";

			const output: GifBinary = new Array(256).fill(0);
			const writer = new GifWriter(output, 1, 1, {
				palette: [0, 0xffffff],
			});
			writer.addFrame(0, 0, 1, 1, [1]);
			const reader = new GifReader(output);
			const frame: Frame = reader.frameInfo(0);
			void frame;
		`,
	);
	writeFileSync(
		join(consumerDir, "tsconfig.json"),
		JSON.stringify(
			{
				compilerOptions: {
					module: "NodeNext",
					moduleResolution: "NodeNext",
					noEmit: true,
					strict: true,
					target: "ES2020",
				},
				include: ["consumer.mts", "consumer.cts"],
			},
			null,
			2,
		),
	);
	run(process.execPath, [
		join(root, "node_modules", "typescript", "bin", "tsc"),
		"-p",
		"tsconfig.json",
	]);

	const commonJsResult = JSON.parse(
		run("node", [
			"-e",
			`
				const wtfgif = require("wtfgif");
				const status = wtfgif.installWasmCoreBackend();
				const gif = wtfgif.encodeIndexedGifFrames({
					width: 2,
					height: 2,
					frames: new Uint8Array([0, 1, 1, 0]),
					palette: [0, 0xffffff],
					backend: "rust",
				});
				const retimed = wtfgif
					.compileGif(gif)
					.withDelays(Uint16Array.of(13))
					.toUint8Array();
				const boomerang = wtfgif.boomerangGifPixelPerfect(retimed);
				const reader = new wtfgif.GifReader(gif);
				const retimedReader = new wtfgif.GifReader(retimed);
				const boomerangReader = new wtfgif.GifReader(boomerang);
				const rgba = new Uint8Array(300 * 4);
				for (let pixel = 0; pixel < 300; pixel++) {
					const offset = pixel * 4;
					rgba[offset] = pixel & 255;
					rgba[offset + 1] = (pixel >> 8) & 255;
					rgba[offset + 2] = (pixel * 17) & 255;
					rgba[offset + 3] = 255;
				}
				const arbitrary = wtfgif.encodeRgbaGifFrames({
					width: 300,
					height: 1,
					frames: rgba,
					compression: "fast",
					quantization: "quality",
					paletteMode: "local",
					backend: "wasm",
				});
				const arbitraryReader = new wtfgif.GifReader(arbitrary);
				const result = {
					available: status.available,
					frames: reader.numFrames(),
					retimedDelay: retimedReader.frameInfo(0).delay,
					boomerangFrames: boomerangReader.numFrames(),
					arbitraryFrames: arbitraryReader.numFrames(),
					hasCompiledApi:
						typeof wtfgif.compileGif === "function" &&
						typeof wtfgif.retimeGifPixelPerfect === "function" &&
						typeof wtfgif.reverseGifPixelPerfect === "function" &&
						typeof wtfgif.boomerangGifPixelPerfect === "function",
					hasCompatibilityApi:
						typeof wtfgif.initializeWasmGlobally === "function" &&
						typeof wtfgif.getWasmStatus === "function" &&
						typeof wtfgif.cleanupWasm === "function",
				};
				reader.dispose();
				retimedReader.dispose();
				boomerangReader.dispose();
				arbitraryReader.dispose();
				process.stdout.write(JSON.stringify(result));
			`,
		]),
	);
	if (
		!commonJsResult.available ||
		commonJsResult.frames !== 1 ||
		commonJsResult.retimedDelay !== 13 ||
		commonJsResult.boomerangFrames !== 2 ||
		commonJsResult.arbitraryFrames !== 1 ||
		!commonJsResult.hasCompiledApi ||
		!commonJsResult.hasCompatibilityApi
	) {
		throw new Error(
			`CommonJS package validation failed: ${JSON.stringify(commonJsResult)}`,
		);
	}

	const omggifDropInResult = JSON.parse(
		run("node", [
			"-e",
			`
				const { GifReader, GifWriter } = require("wtfgif");
				const output = new Array(256).fill(0);
				const writer = new GifWriter(output, 2, 2, {
					loop: 0,
					palette: [0, 0xffffff],
				});
				writer.addFrame(0, 0, 2, 2, [0, 1, 1, 0], {
					delay: 7,
					disposal: 1,
				});
				const gif = output.slice(0, writer.end());
				const reader = new GifReader(gif);
				const pixels = new Array(16).fill(0);
				reader.decodeAndBlitFrameRGBA(0, pixels);
				process.stdout.write(JSON.stringify({
					width: reader.width,
					height: reader.height,
					frames: reader.numFrames(),
					loop: reader.loopCount(),
					delay: reader.frameInfo(0).delay,
					pixels,
				}));
			`,
		]),
	);
	if (
		omggifDropInResult.width !== 2 ||
		omggifDropInResult.height !== 2 ||
		omggifDropInResult.frames !== 1 ||
		omggifDropInResult.loop !== 0 ||
		omggifDropInResult.delay !== 7 ||
		omggifDropInResult.pixels.join(",") !==
			"0,0,0,255,255,255,255,255,255,255,255,255,0,0,0,255"
	) {
		throw new Error(
			`omggif-style package validation failed: ${JSON.stringify(omggifDropInResult)}`,
		);
	}

	const encodeOnlyResult = JSON.parse(
		run("node", [
			"-e",
			`
				(async () => {
					const encode = require("wtfgif/encode");
					await encode.initializeWasmGlobally();
					const gif = encode.encodeRgbaGifFrames({
						width: 2,
						height: 1,
						frames: Uint8Array.of(
							255, 0, 0, 255,
							0, 0, 255, 255,
						),
						frameCount: 1,
						delay: 7,
						loop: 0,
					});
					process.stdout.write(JSON.stringify({
						initialized: encode.getWasmStatus().initialized,
						bytes: gif.length,
					}));
				})();
			`,
		]),
	);
	if (!encodeOnlyResult.initialized || encodeOnlyResult.bytes <= 0) {
		throw new Error(
			`Encode-only package validation failed: ${JSON.stringify(encodeOnlyResult)}`,
		);
	}

	const esmResult = JSON.parse(
		run("node", [
			"--input-type=module",
			"-e",
			`
				globalThis.window = {};
				const wtfgif = await import("wtfgif");
				const status = wtfgif.installWasmCoreBackend();
				const gif = wtfgif.encodeIndexedGifFrames({
					width: 2,
					height: 2,
					frames: new Uint8Array([0, 1, 1, 0]),
					palette: [0, 0xffffff],
					backend: "native",
				});
				const reader = new wtfgif.GifReader(gif);
				const result = {
					available: status.available,
					frames: reader.numFrames(),
					browserGlobal:
						globalThis.window.wtfgif?.GifReader === wtfgif.GifReader &&
						globalThis.window.wtfgif?.compileGif === wtfgif.compileGif,
				};
				reader.dispose();
				process.stdout.write(JSON.stringify(result));
			`,
		]),
	);
	if (
		!esmResult.available ||
		esmResult.frames !== 1 ||
		!esmResult.browserGlobal
	) {
		throw new Error(`ESM package validation failed: ${JSON.stringify(esmResult)}`);
	}

	const browserWasmResult = JSON.parse(
		run("node", [
			"--conditions=browser",
			"--input-type=module",
			"-e",
			`
				import { readFileSync } from "node:fs";
				import { fileURLToPath } from "node:url";
				const wtfgif = await import("wtfgif");
				const encodeOnly = await import("wtfgif/encode");
				wtfgif.cleanupWasm();
				const wasmUrl = import.meta.resolve("wtfgif/wasm-web/wasm");
				const wasmBytes = readFileSync(fileURLToPath(wasmUrl));
				await wtfgif.initializeWasmGlobally(wasmBytes);
				const encodeWasmUrl = import.meta.resolve("wtfgif/wasm-encode/wasm");
				const encodeWasmBytes = readFileSync(fileURLToPath(encodeWasmUrl));
				await encodeOnly.initializeWasmGlobally(encodeWasmBytes);
				const status = wtfgif.installWasmCoreBackend();
				const source = wtfgif.encodeIndexedGifFrames({
					width: 2,
					height: 2,
					frames: new Uint8Array([0, 1, 1, 0]),
					palette: [0, 0xffffff],
					backend: "rust",
					compression: "fast",
				});
				const remuxed = wtfgif.remuxGifPixelPerfect(source);
				const before = wtfgif.decodeGifFramesRgba(source);
				const after = wtfgif.decodeGifFramesRgba(remuxed);
				const rgba = new Uint8Array(300 * 4);
				for (let pixel = 0; pixel < 300; pixel++) {
					const offset = pixel * 4;
					rgba[offset] = pixel & 255;
					rgba[offset + 1] = (pixel >> 8) & 255;
					rgba[offset + 2] = (pixel * 17) & 255;
					rgba[offset + 3] = 255;
				}
				const arbitrary = wtfgif.decodeGifFramesRgba(
					wtfgif.encodeRgbaGifFrames({
						width: 300,
						height: 1,
						frames: rgba,
						compression: "fast",
						quantization: "quality",
						paletteMode: "local",
						backend: "wasm",
					}),
				);
				const encodeOnlyGif = encodeOnly.encodeRgbaGifFrames({
					width: 2,
					height: 1,
					frames: Uint8Array.of(
						255, 0, 0, 255,
						0, 0, 255, 255,
					),
					frameCount: 1,
					delay: 7,
					loop: 0,
				});
				process.stdout.write(JSON.stringify({
					available: status.available,
					initialized: wtfgif.getWasmStatus().initialized,
					pixelPerfect:
						before.width === after.width &&
						before.height === after.height &&
						before.frameCount === after.frameCount &&
						before.pixels.every(
							(value, index) => value === after.pixels[index],
						),
					arbitraryRgba:
						arbitrary.width === 300 &&
						arbitrary.height === 1 &&
						arbitrary.frameCount === 1,
					encodeOnlyInitialized: encodeOnly.getWasmStatus().initialized,
					encodeOnlyBytes: encodeOnlyGif.length,
				}));
			`,
		]),
	);
	if (
		!browserWasmResult.available ||
		!browserWasmResult.initialized ||
		!browserWasmResult.pixelPerfect ||
		!browserWasmResult.arbitraryRgba ||
		!browserWasmResult.encodeOnlyInitialized ||
		browserWasmResult.encodeOnlyBytes <= 0
	) {
		throw new Error(
			`Browser Wasm package validation failed: ${JSON.stringify(browserWasmResult)}`,
		);
	}

	const webWrapper = readFileSync(
		join(root, "dist", "wasm-web", "wtfgif_core.js"),
		"utf8",
	);
	if (
		webWrapper.includes('from "node:') ||
		webWrapper.includes("require(") ||
		webWrapper.includes("node:fs")
	) {
		throw new Error("Browser Wasm wrapper contains a Node-only dependency.");
	}

	console.log(
		"Packaged omggif-style CommonJS, Node ESM, browser global, and browser Wasm validation passed.",
	);
} finally {
	rmSync(temporaryRoot, { recursive: true, force: true });
}
