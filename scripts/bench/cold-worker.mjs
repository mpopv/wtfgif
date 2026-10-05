import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import {
	decodeGifFramesRgba,
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	initializeWasmGlobally,
	reencodeGifPixelPerfect,
	remuxGifPixelPerfect,
	setNativeAddonModule,
	setWasmCoreModule,
} from "../../dist/index.mjs";

if (process.env.WTFGIF_BENCH_FRESH_PROCESS !== "1") {
	throw new Error("cold-worker.mjs must be launched by cold.mjs");
}

const require = createRequire(import.meta.url);
const { GifReader, GifWriter } = require("omggif");
if (process.env.WTFGIF_BENCH_BACKEND !== "wasm") {
	setNativeAddonModule(
		require(
			process.env.WTFGIF_NATIVE_ADDON ??
				fileURLToPath(
					new URL("../../native/build/wtfgif_native.node", import.meta.url),
				),
		),
	);
}
const wasmCore = require("../../crates/wtfgif-core/pkg/wtfgif_core.js");
if (process.env.WTFGIF_PREPARE_WASM_AT_PAGE_LOAD === "1") {
	await initializeWasmGlobally(wasmCore);
} else {
	setWasmCoreModule(wasmCore);
}

const [operation, implementation, fixtureArgument] = process.argv.slice(2);

function readGifFixture(path) {
	const bytes = readFileSync(path);
	return process.env.WTFGIF_BENCH_BACKEND === "wasm"
		? new Uint8Array(bytes)
		: bytes;
}

function clearFrameRect(canvas, canvasWidth, info) {
	for (let y = info.y; y < info.y + info.height; y++) {
		canvas.fill(
			0,
			(y * canvasWidth + info.x) * 4,
			(y * canvasWidth + info.x + info.width) * 4,
		);
	}
}

function decodeAllOmggif(data) {
	const reader = new GifReader(data);
	const frameBytes = reader.width * reader.height * 4;
	const canvas = new Uint8Array(frameBytes);
	const restore = new Uint8Array(frameBytes);
	const pixels = new Uint8Array(frameBytes * reader.numFrames());
	for (let frame = 0; frame < reader.numFrames(); frame++) {
		const info = reader.frameInfo(frame);
		if (info.disposal === 3) {
			restore.set(canvas);
		}
		reader.decodeAndBlitFrameRGBA(frame, canvas);
		pixels.set(canvas, frame * frameBytes);
		if (info.disposal === 2) {
			clearFrameRect(canvas, reader.width, info);
		} else if (info.disposal === 3) {
			canvas.set(restore);
		}
	}
	return {
		width: reader.width,
		height: reader.height,
		frameCount: reader.numFrames(),
		pixels,
	};
}

function decodeNative(data) {
	return decodeGifFramesRgba(data);
}

function reencodeOmggif(data) {
	const reader = new GifReader(data);
	let totalFramePixels = 0;
	for (let frame = 0; frame < reader.numFrames(); frame++) {
		const info = reader.frameInfo(frame);
		totalFramePixels += info.width * info.height;
	}
	const output = new Uint8Array(totalFramePixels * 2 + data.length + 4096);
	const writer = new GifWriter(output, reader.width, reader.height, {
		loop: reader.loopCount(),
	});
	const rgba = new Uint8Array(reader.width * reader.height * 4);
	const paletteCache = new Map();
	for (let frame = 0; frame < reader.numFrames(); frame++) {
		const info = reader.frameInfo(frame);
		const paletteKey = `${info.palette_offset}:${info.palette_size}`;
		let paletteInfo = paletteCache.get(paletteKey);
		if (!paletteInfo) {
			const palette = new Array(info.palette_size);
			const colorToIndex = new Map();
			for (let index = 0; index < info.palette_size; index++) {
				const offset = info.palette_offset + index * 3;
				const color =
					(data[offset] << 16) | (data[offset + 1] << 8) | data[offset + 2];
				palette[index] = color;
				if (!colorToIndex.has(color)) {
					colorToIndex.set(color, index);
				}
			}
			paletteInfo = { palette, colorToIndex };
			paletteCache.set(paletteKey, paletteInfo);
		}

		rgba.fill(0);
		reader.decodeAndBlitFrameRGBA(frame, rgba);
		const indices = new Uint8Array(info.width * info.height);
		let outputIndex = 0;
		for (let y = 0; y < info.height; y++) {
			let offset = ((info.y + y) * reader.width + info.x) * 4;
			for (let x = 0; x < info.width; x++) {
				if (rgba[offset + 3] === 0 && info.transparent_index !== null) {
					indices[outputIndex++] = info.transparent_index;
				} else {
					const color =
						(rgba[offset] << 16) | (rgba[offset + 1] << 8) | rgba[offset + 2];
					const index = paletteInfo.colorToIndex.get(color);
					if (index === undefined) {
						throw new Error("Decoded color is absent from the frame palette");
					}
					indices[outputIndex++] = index;
				}
				offset += 4;
			}
		}
		writer.addFrame(info.x, info.y, info.width, info.height, indices, {
			palette: paletteInfo.palette,
			delay: info.delay,
			disposal: info.disposal,
			transparent: info.transparent_index,
		});
	}
	return output.slice(0, writer.end());
}

function flatten(frames) {
	const output = new Uint8Array(
		frames.reduce((total, frame) => total + frame.length, 0),
	);
	let offset = 0;
	for (const frame of frames) {
		output.set(frame, offset);
		offset += frame.length;
	}
	return output;
}

function makeIndexedFixture(colorCount, delta) {
	const width = 128;
	const height = 128;
	const frameCount = delta ? 24 : 12;
	const palette = Array.from({ length: colorCount }, (_, index) => {
		const red = (index * 73) & 255;
		const green = (index * 151) & 255;
		const blue = (index * 199) & 255;
		return (red << 16) | (green << 8) | blue;
	});
	const frames = [];
	for (let frame = 0; frame < frameCount; frame++) {
		const pixels = new Uint8Array(width * height);
		if (delta) {
			const startX = (frame * 5) % (width - 16);
			const startY = (frame * 3) % (height - 16);
			for (let y = 0; y < 16; y++) {
				for (let x = 0; x < 16; x++) {
					pixels[(startY + y) * width + startX + x] =
						1 + (frame % (colorCount - 1));
				}
			}
		} else {
			for (let y = 0; y < height; y++) {
				for (let x = 0; x < width; x++) {
					pixels[y * width + x] =
						(x * 17 + y * 31 + frame * 13 + ((x * y) >> 3)) & (colorCount - 1);
				}
			}
		}
		frames.push(pixels);
	}
	return {
		width,
		height,
		frameCount,
		palette,
		frames,
		flatFrames: flatten(frames),
	};
}

function toRgbaFixture(indexed) {
	const frames = indexed.frames.map((source) => {
		const rgba = new Uint8Array(source.length * 4);
		for (let pixel = 0; pixel < source.length; pixel++) {
			const color = indexed.palette[source[pixel]];
			const offset = pixel * 4;
			rgba[offset] = (color >> 16) & 255;
			rgba[offset + 1] = (color >> 8) & 255;
			rgba[offset + 2] = color & 255;
			rgba[offset + 3] = 255;
		}
		return rgba;
	});
	return { ...indexed, frames, flatFrames: flatten(frames) };
}

function encodeIndexedOmggif(fixture) {
	const output = new Uint8Array(fixture.flatFrames.length * 2 + 4096);
	const writer = new GifWriter(output, fixture.width, fixture.height, {
		palette: fixture.palette,
		loop: 0,
	});
	for (const frame of fixture.frames) {
		writer.addFrame(0, 0, fixture.width, fixture.height, frame, { delay: 2 });
	}
	return output.slice(0, writer.end());
}

function encodeIndexedNative(fixture, delta) {
	return encodeIndexedGifFrames({
		width: fixture.width,
		height: fixture.height,
		frameCount: fixture.frameCount,
		frames: fixture.flatFrames,
		palette: fixture.palette,
		delay: 2,
		loop: 0,
		backend: "native-addon",
		delta,
	});
}

function indexRgba(fixture) {
	const colorToIndex = new Map(
		fixture.palette.map((color, index) => [color, index]),
	);
	const indexed = new Uint8Array(fixture.flatFrames.length / 4);
	for (let pixel = 0; pixel < indexed.length; pixel++) {
		const offset = pixel * 4;
		const color =
			(fixture.flatFrames[offset] << 16) |
			(fixture.flatFrames[offset + 1] << 8) |
			fixture.flatFrames[offset + 2];
		indexed[pixel] = colorToIndex.get(color);
	}
	return {
		...fixture,
		frames: Array.from({ length: fixture.frameCount }, (_, frame) =>
			indexed.subarray(
				frame * fixture.width * fixture.height,
				(frame + 1) * fixture.width * fixture.height,
			),
		),
		flatFrames: indexed,
	};
}

function encodeRgbaNative(fixture, delta) {
	return encodeRgbaGifFrames({
		width: fixture.width,
		height: fixture.height,
		frameCount: fixture.frameCount,
		frames: fixture.flatFrames,
		palette: fixture.palette,
		delay: 2,
		loop: 0,
		backend: "native-addon",
		delta,
	});
}

function signature(decoded) {
	const hash = createHash("sha256").update(decoded.pixels).digest("hex");
	return `${decoded.width}x${decoded.height}x${decoded.frameCount}:${hash}`;
}

let operationResult;
let started;
let elapsedMs;

if (operation === "decode") {
	const data = readGifFixture(fixtureArgument);
	started = performance.now();
	operationResult =
		implementation === "omggif" ? decodeAllOmggif(data) : decodeNative(data);
	elapsedMs = performance.now() - started;
	process.stdout.write(
		JSON.stringify({
			elapsedMs,
			signature: signature(operationResult),
			outputBytes: operationResult.pixels.length,
		}),
	);
} else if (operation === "reencode") {
	const data = readGifFixture(fixtureArgument);
	started = performance.now();
	operationResult =
		implementation === "omggif"
			? reencodeOmggif(data)
			: process.env.WTFGIF_REENCODE_MODE === "remux"
				? remuxGifPixelPerfect(data)
				: reencodeGifPixelPerfect(data);
	elapsedMs = performance.now() - started;
	const sourceSignature = signature(decodeAllOmggif(data));
	process.stdout.write(
		JSON.stringify({
			elapsedMs,
			signature: signature(decodeAllOmggif(operationResult)),
			sourceSignature,
			outputBytes: operationResult.length,
		}),
	);
} else if (operation === "encode-indexed" || operation === "encode-rgba") {
	const [colorCountText, deltaText] = fixtureArgument.split(":");
	const delta = deltaText === "delta";
	const indexedFixture = makeIndexedFixture(Number(colorCountText), delta);
	const fixture =
		operation === "encode-rgba"
			? toRgbaFixture(indexedFixture)
			: indexedFixture;
	started = performance.now();
	if (implementation === "omggif") {
		operationResult =
			operation === "encode-rgba"
				? encodeIndexedOmggif(indexRgba(fixture))
				: encodeIndexedOmggif(fixture);
	} else {
		operationResult =
			operation === "encode-rgba"
				? encodeRgbaNative(fixture, delta)
				: encodeIndexedNative(fixture, delta);
	}
	elapsedMs = performance.now() - started;
	process.stdout.write(
		JSON.stringify({
			elapsedMs,
			signature: signature(decodeAllOmggif(operationResult)),
			outputBytes: operationResult.length,
		}),
	);
} else {
	throw new Error(`Unknown cold benchmark operation: ${operation}`);
}
