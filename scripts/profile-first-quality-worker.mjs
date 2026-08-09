import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { performance } from "node:perf_hooks";
import { loadBenchmarkCorpus } from "./benchmark/corpus.mjs";

const fixtureId = process.env.PROFILE_FIXTURE ?? "makeemoji-real";
const fixture = loadBenchmarkCorpus().find((value) => value.id === fixtureId);
if (!fixture) throw new Error(`Unknown PROFILE_FIXTURE: ${fixtureId}`);
const repeat = Number(process.env.PROFILE_REPEAT ?? 1);
if (!Number.isInteger(repeat) || repeat < 1) {
	throw new Error("PROFILE_REPEAT must be a positive integer");
}
let { rgba } = fixture;
const { width, height, frameCount } = fixture;
if (process.env.PROFILE_TRANSPARENT_PERCENT !== undefined) {
	const transparentPercent = Number(process.env.PROFILE_TRANSPARENT_PERCENT);
	if (
		!Number.isFinite(transparentPercent) ||
		transparentPercent < 0 ||
		transparentPercent > 100
	) {
		throw new Error("PROFILE_TRANSPARENT_PERCENT must be between 0 and 100");
	}
	rgba = rgba.slice();
	const transparentThreshold = Math.round(transparentPercent * 10_000);
	for (let pixel = 0; pixel < rgba.length / 4; pixel += 1) {
		const sample = (Math.imul(pixel, 2_654_435_761) >>> 0) % 1_000_000;
		rgba[pixel * 4 + 3] = sample < transparentThreshold ? 0 : 255;
	}
}
const wasmBytes = readFileSync(
	process.env.PROFILE_WASM_PATH ??
		new URL(
			"../crates/wtfgif-core/pkg-quality-simd/wtfgif_core_bg.wasm",
			import.meta.url,
		),
);
const moduleStarted = performance.now();
const module = new WebAssembly.Module(wasmBytes);
const moduleCompleted = performance.now();
let wasm;
const imports = {
	"./wtfgif_core_bg.js": {
		__wbindgen_init_externref_table() {
			const table = wasm.__wbindgen_externrefs;
			const offset = table.grow(4);
			table.set(0, undefined);
			table.set(offset, undefined);
			table.set(offset + 1, null);
			table.set(offset + 2, true);
			table.set(offset + 3, false);
		},
	},
};
wasm = new WebAssembly.Instance(module, imports).exports;
const instanceCompleted = performance.now();
wasm.__wbindgen_start();
wasm.prepare_quality_encoder_code?.();
const startCompleted = performance.now();
wasm.indexed_lzw_input_scratch_reserve(
	Number(process.env.PROFILE_INITIAL_INPUT_CAPACITY ?? 4 * 1024 * 1024),
);
const tierStarted = performance.now();
if (process.env.PROFILE_JS_TIER === "1") {
	const largeTierPixelCount = 1001 * 1000;
	const largeTierPixels = new Uint32Array(
		wasm.memory.buffer,
		wasm.indexed_lzw_input_scratch_reserve(largeTierPixelCount * 4),
		largeTierPixelCount,
	);
	for (let pixel = 0; pixel < largeTierPixels.length; pixel += 1) {
		const cell = (pixel * 32429) & 32767;
		const red = ((cell >> 10) << 3) | 4;
		const green = (((cell >> 5) & 31) << 3) | 4;
		const blue = ((cell & 31) << 3) | 4;
		largeTierPixels[pixel] = 0xff000000 | (blue << 16) | (green << 8) | red;
	}
	wasm.encode_rgba_quality_gif_constant_delay_scratch_from_input(
		largeTierPixelCount * 4,
		1001,
		1000,
		1,
		0,
		0,
		179,
	);
	const tierPixelCount = 128 * 128 * 8;
	const tierPointer = wasm.indexed_lzw_input_scratch_reserve(
		tierPixelCount * 4,
	);
	const tierPixels = new Uint32Array(
		wasm.memory.buffer,
		tierPointer,
		tierPixelCount,
	);
	if (process.env.PROFILE_SPLIT_TIER !== "0") {
		const splitTierIterations = Number(
			process.env.PROFILE_SPLIT_TIER_ITERATIONS ?? 1,
		);
		for (let pixel = 0; pixel < tierPixels.length; pixel += 1) {
			const pair = pixel >> 1;
			const split = pair % 4 === 2 ? pixel & 1 : 0;
			const cell = (pair * 73 + split) & 2047;
			const red = ((cell >> 8) << 4) | 8;
			const green = (((cell >> 4) & 15) << 4) | 8;
			const blue = ((cell & 15) << 4) | 8;
			tierPixels[pixel] = 0xff000000 | (blue << 16) | (green << 8) | red;
		}
		for (let iteration = 0; iteration < splitTierIterations; iteration += 1) {
			wasm.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
				tierPixelCount * 4,
				128,
				128,
				8,
				0,
				0,
				179,
			);
		}
	}
	for (let pixel = 0; pixel < tierPixels.length; pixel += 1) {
		if (pixel % 31 === 0) {
			tierPixels[pixel] = 0;
			continue;
		}
		const cell = (pixel * 4051) & 2047;
		const red = ((cell >> 8) << 4) | 8;
		const green = (((cell >> 4) & 15) << 4) | 8;
		const blue = ((cell & 15) << 4) | 8;
		tierPixels[pixel] = 0xff000000 | (blue << 16) | (green << 8) | red;
	}
	const mixedTierIterations = Number(
		process.env.PROFILE_MIXED_TIER_ITERATIONS ?? 1,
	);
	for (let iteration = 0; iteration < mixedTierIterations; iteration += 1) {
		wasm.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
			tierPixelCount * 4,
			128,
			128,
			8,
			0,
			0,
			179,
		);
	}
	const gridPixelCount = 8 * 12 * 16;
	for (let pixel = 0; pixel < gridPixelCount; pixel += 1) {
		const red = ((pixel % 8) << 5) | 16;
		const green = (((Math.floor(pixel / 8) % 12) << 4) | 8) & 255;
		const blue = ((Math.floor(pixel / (8 * 12)) << 4) | 8) & 255;
		tierPixels[pixel] = 0xff000000 | (blue << 16) | (green << 8) | red;
	}
	for (let iteration = 0; iteration < 3; iteration += 1) {
		wasm.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
			gridPixelCount * 4,
			32,
			16,
			3,
			0,
			0,
			179,
		);
	}
	const directPixelCount = 32 * 16 * 2;
	const directShades = [2, 8, 14];
	for (let pixel = 0; pixel < directPixelCount; pixel += 1) {
		const cell = (pixel * 73) & 127;
		const shade = directShades[Math.floor(pixel / 128) % 3];
		const red = ((cell >> 4) << 4) | shade;
		const green = (((cell >> 2) & 3) << 4) | shade;
		const blue = ((cell & 3) << 4) | shade;
		tierPixels[pixel] = 0xff000000 | (blue << 16) | (green << 8) | red;
	}
	for (let iteration = 0; iteration < 3; iteration += 1) {
		wasm.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
			directPixelCount * 4,
			32,
			16,
			2,
			0,
			0,
			179,
		);
	}
	const singleMergePixelCount = 32 * 16 * 2;
	for (let pixel = 0; pixel < singleMergePixelCount; pixel += 1) {
		const cell = pixel & 255;
		const red = ((cell >> 5) << 5) | 16;
		const green = (((cell >> 2) & 7) << 5) | 16;
		const blue = ((cell & 3) << 6) | 32;
		const alpha = pixel < 768 ? 255 : 0;
		tierPixels[pixel] = (alpha << 24) | (blue << 16) | (green << 8) | red;
	}
	for (let iteration = 0; iteration < 3; iteration += 1) {
		wasm.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
			singleMergePixelCount * 4,
			32,
			16,
			2,
			0,
			0,
			179,
		);
	}
	const exactPixelCount = 64 * 64 * 12;
	const exactPalette = [
		0xff20100c, 0xff30c4f5, 0xff6f47ef, 0xffb28a11, 0xffa0d606, 0xffffffff,
		0xff5634a2, 0xff18b070,
	];
	for (let pixel = 0; pixel < exactPixelCount; pixel += 1) {
		const x = pixel & 63;
		const y = (pixel >> 6) & 63;
		const frame = pixel >> 12;
		tierPixels[pixel] = exactPalette[((x >> 3) + (y >> 3) + frame * 3) & 7];
	}
	for (let iteration = 0; iteration < 4; iteration += 1) {
		wasm.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
			exactPixelCount * 4,
			64,
			64,
			12,
			0,
			0,
			179,
		);
	}
	const stablePixelCount = 128 * 128 * 12;
	for (let pixel = 0; pixel < stablePixelCount; pixel += 1) {
		const x = pixel & 127;
		const y = (pixel >> 7) & 127;
		const frame = pixel >> 14;
		tierPixels[pixel] = exactPalette[((x >> 4) + (y >> 4) + frame) % 3];
	}
	wasm.encode_rgba_quality_low_res_constant_delay_scratch_from_input(
		stablePixelCount * 4,
		128,
		128,
		12,
		0,
		0,
		179,
	);
}
const tierCompleted = performance.now();

const evictionBytes = Number(process.env.PROFILE_CACHE_EVICTION_BYTES ?? 0);
if (evictionBytes > 0) {
	const eviction = new Uint8Array(evictionBytes);
	eviction.fill(1);
	await new Promise((resolve) => setTimeout(resolve, 0));
}

const started = performance.now();
const inputPointer = wasm.indexed_lzw_input_scratch_reserve(rgba.length);
const reserved = performance.now();
new Uint8Array(wasm.memory.buffer, inputPointer, rgba.length).set(rgba);
const copiedInput = performance.now();
const encode =
	rgba.length / 4 > 1_000_000 || process.env.PROFILE_GENERAL === "1"
		? wasm.encode_rgba_quality_gif_constant_delay_scratch_from_input
		: wasm.encode_rgba_quality_low_res_constant_delay_scratch_from_input;
let outputLength = 0;
for (let iteration = 0; iteration < repeat; iteration += 1) {
	outputLength = encode(rgba.length, width, height, frameCount, 10, 0, 179);
}
const encoded = performance.now();
const output = new Uint8Array(
	new Uint8Array(
		wasm.memory.buffer,
		wasm.gif_output_scratch_ptr(),
		outputLength,
	),
);
const copiedOutput = performance.now();

process.stdout.write(
	JSON.stringify({
		repeat,
		moduleMs: moduleCompleted - moduleStarted,
		instanceMs: instanceCompleted - moduleCompleted,
		startMs: startCompleted - instanceCompleted,
		tierMs: tierCompleted - tierStarted,
		reserveMs: reserved - started,
		inputCopyMs: copiedInput - reserved,
		wasmMs: encoded - copiedInput,
		outputCopyMs: copiedOutput - encoded,
		totalMs: copiedOutput - started,
		outputBytes: output.length,
		outputSha256: createHash("sha256").update(output).digest("hex"),
	}),
);
