import { createRequire } from "node:module";
import { performance } from "node:perf_hooks";
import {
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	initializeWasmGlobally,
} from "../dist/index.mjs";

const require = createRequire(import.meta.url);
const { GifReader, GifWriter } = require("omggif");
const iterations = Number(process.env.BENCH_ITERATIONS ?? 25);
const warmups = Number(process.env.BENCH_WARMUP_ITERATIONS ?? 8);
let sink = 0;

await initializeWasmGlobally();

function makePhotoLikeFixture(width, height, frameCount) {
	const rgba = new Uint8Array(width * height * frameCount * 4);
	let output = 0;
	for (let frame = 0; frame < frameCount; frame++) {
		for (let y = 0; y < height; y++) {
			for (let x = 0; x < width; x++) {
				const texture =
					((x * 13 + y * 17 + frame * 29) ^
						((x * y + frame * 101) >>> 2)) &
					31;
				rgba[output++] =
					(Math.round((x * 255) / Math.max(1, width - 1)) +
						frame * 19 +
						texture) &
					255;
				rgba[output++] =
					(Math.round((y * 255) / Math.max(1, height - 1)) +
						frame * 31 +
						(texture << 1)) &
					255;
				rgba[output++] =
					(Math.round(
						((x + y) * 127) / Math.max(1, width + height - 2),
					) +
						frame * 47 +
						texture * 3) &
					255;
				rgba[output++] = 255;
			}
		}
	}
	return {
		name: `${frameCount} x ${width}x${height}`,
		width,
		height,
		frameCount,
		rgba,
	};
}

function fixed332Palette() {
	return Array.from({ length: 256 }, (_, index) => {
		const red = Math.floor(((((index >> 5) & 7) * 255 + 3) / 7));
		const green = Math.floor(((((index >> 2) & 7) * 255 + 3) / 7));
		const blue = Math.floor((((index & 3) * 255 + 1) / 3));
		return (red << 16) | (green << 8) | blue;
	});
}

function mapRgb332(rgba) {
	const indexed = new Uint8Array(rgba.length / 4);
	for (let source = 0, output = 0; source < rgba.length; source += 4) {
		indexed[output++] =
			(rgba[source] & 0xe0) |
			((rgba[source + 1] >> 3) & 0x1c) |
			(rgba[source + 2] >> 6);
	}
	return indexed;
}

function encodeOmggifIndexed(fixture, indexed, palette) {
	const output = new Uint8Array(indexed.length * 2 + 4096);
	const writer = new GifWriter(output, fixture.width, fixture.height, {
		palette,
		loop: 0,
	});
	const framePixels = fixture.width * fixture.height;
	for (let frame = 0; frame < fixture.frameCount; frame++) {
		writer.addFrame(
			0,
			0,
			fixture.width,
			fixture.height,
			indexed.subarray(
				frame * framePixels,
				(frame + 1) * framePixels,
			),
			{ delay: 10 },
		);
	}
	return output.slice(0, writer.end());
}

function measure(operation) {
	for (let iteration = 0; iteration < warmups; iteration++) {
		const result = operation();
		sink ^= result[result.length - 1] ?? 0;
	}
	const samples = [];
	for (let iteration = 0; iteration < iterations; iteration++) {
		const start = performance.now();
		const result = operation();
		samples.push(performance.now() - start);
		sink ^= result[result.length - 1] ?? 0;
	}
	samples.sort((left, right) => left - right);
	return samples[Math.floor(samples.length / 2)];
}

function quality(encoded, fixture) {
	const reader = new GifReader(encoded);
	if (
		reader.width !== fixture.width ||
		reader.height !== fixture.height ||
		reader.numFrames() !== fixture.frameCount
	) {
		throw new Error(`${fixture.name}: encoded dimensions or frame count differ`);
	}
	const frameBytes = fixture.width * fixture.height * 4;
	const decoded = new Uint8Array(frameBytes);
	let squaredError = 0;
	let sampleCount = 0;
	for (let frame = 0; frame < fixture.frameCount; frame++) {
		decoded.fill(0);
		reader.decodeAndBlitFrameRGBA(frame, decoded);
		const sourceStart = frame * frameBytes;
		for (let offset = 0; offset < frameBytes; offset += 4) {
			for (let channel = 0; channel < 3; channel++) {
				const difference =
					fixture.rgba[sourceStart + offset + channel] -
					decoded[offset + channel];
				squaredError += difference * difference;
				sampleCount++;
			}
		}
	}
	const rmse = Math.sqrt(squaredError / sampleCount);
	return {
		rmse,
		psnr: 20 * Math.log10(255 / rmse),
	};
}

const fixtures = [
	makePhotoLikeFixture(128, 128, 12),
	makePhotoLikeFixture(512, 512, 10),
];
const palette = fixed332Palette();

console.log(
	"fixture\toperation\tmedian ms\tspeedup vs omggif pipeline\tbytes\tPSNR dB",
);
for (const fixture of fixtures) {
	const indexed = mapRgb332(fixture.rgba);
	const omggifPipeline = () => {
		const mapped = mapRgb332(fixture.rgba);
		return encodeOmggifIndexed(fixture, mapped, palette);
	};
	const operations = [
		{
			name: "omggif + RGB332 mapping",
			run: omggifPipeline,
			comparable: true,
		},
		{
			name: "wtfgif RGBA fast/global",
			run: () =>
				encodeRgbaGifFrames({
					width: fixture.width,
					height: fixture.height,
					frameCount: fixture.frameCount,
					frames: fixture.rgba,
					delay: 10,
					loop: 0,
					compression: "fast",
					quantization: "fast",
					paletteMode: "global",
					backend: "wasm",
				}),
			comparable: true,
		},
		{
			name: "wtfgif RGBA quality/global",
			run: () =>
				encodeRgbaGifFrames({
					width: fixture.width,
					height: fixture.height,
					frameCount: fixture.frameCount,
					frames: fixture.rgba,
					delay: 10,
					loop: 0,
					compression: "fast",
					quantization: "quality",
					paletteMode: "global",
					backend: "wasm",
				}),
			comparable: false,
		},
		{
			name: "wtfgif RGBA quality/local",
			run: () =>
				encodeRgbaGifFrames({
					width: fixture.width,
					height: fixture.height,
					frameCount: fixture.frameCount,
					frames: fixture.rgba,
					delay: 10,
					loop: 0,
					compression: "fast",
					quantization: "quality",
					paletteMode: "local",
					backend: "wasm",
				}),
			comparable: false,
		},
		{
			name: "wtfgif RGBA legacy balanced",
			run: () =>
				encodeRgbaGifFrames({
					width: fixture.width,
					height: fixture.height,
					frameCount: fixture.frameCount,
					frames: fixture.rgba,
					delay: 10,
					loop: 0,
					compression: "balanced",
					backend: "wasm",
				}),
			comparable: true,
		},
		{
			name: "wtfgif indexed fast lower bound",
			run: () =>
				encodeIndexedGifFrames({
					width: fixture.width,
					height: fixture.height,
					frameCount: fixture.frameCount,
					frames: indexed,
					palette,
					delay: 10,
					loop: 0,
					compression: "fast",
					backend: "wasm",
				}),
			comparable: false,
		},
	];
	const baselineMs = measure(omggifPipeline);
	for (const operation of operations) {
		const encoded = operation.run();
		const medianMs =
			operation.name === "omggif + RGB332 mapping"
				? baselineMs
				: measure(operation.run);
		const measuredQuality = quality(encoded, fixture);
		console.log(
			[
				fixture.name,
				operation.name,
				medianMs.toFixed(3),
				operation.comparable
					? `${(baselineMs / medianMs).toFixed(2)}x`
					: "n/a",
				encoded.length,
				measuredQuality.psnr.toFixed(2),
			].join("\t"),
		);
	}
}
console.log(`sink=${sink}`);
