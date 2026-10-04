import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { performance } from "node:perf_hooks";
import { root } from "../lib/paths.mjs";
import { median } from "./lib/metrics.mjs";

const encoderEntry =
	process.env.BENCH_WTFFIG_ENTRY === "root"
		? "../../dist/index.mjs"
		: "../../dist/encode.mjs";
const { encodeRgbaGifFrames, initializeWasmGlobally } = await import(
	encoderEntry
);

const require = createRequire(import.meta.url);
const ImageQ = require("image-q");
const { GifReader, GifWriter } = require("omggif");
const iterations = Number(process.env.BENCH_ITERATIONS ?? 200);
const warmups = Number(process.env.BENCH_WARMUP_ITERATIONS ?? 30);
const alphaThreshold = Math.trunc(
	Number(process.env.BENCH_ALPHA_THRESHOLD ?? 179),
);
if (
	!Number.isInteger(alphaThreshold) ||
	alphaThreshold < 0 ||
	alphaThreshold > 255
) {
	throw new Error(
		"BENCH_ALPHA_THRESHOLD must be an integer from 0 through 255",
	);
}
let sink = 0;

await initializeWasmGlobally();

function realImageFixture() {
	const width = 128;
	const height = 128;
	const frameCount = 8;
	const rgba = readFileSync(
		join(root, "test", "rgba", "makeemoji-128x128x8.rgba"),
	);
	if (rgba.length !== width * height * frameCount * 4) {
		throw new Error("Real-image RGBA fixture has the wrong byte length");
	}
	return {
		frameCount,
		height,
		name: "8 real MakeEmoji images, 128x128",
		rgba,
		width,
	};
}

function makePhotoLikeStressFixture(width, height, frameCount) {
	const rgba = new Uint8Array(width * height * frameCount * 4);
	let output = 0;
	for (let frame = 0; frame < frameCount; frame += 1) {
		for (let y = 0; y < height; y += 1) {
			for (let x = 0; x < width; x += 1) {
				const texture =
					((x * 13 + y * 17 + frame * 29) ^ ((x * y + frame * 101) >>> 2)) & 31;
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
					(Math.round(((x + y) * 127) / Math.max(1, width + height - 2)) +
						frame * 47 +
						texture * 3) &
					255;
				rgba[output++] = 255;
			}
		}
	}
	return {
		frameCount,
		height,
		name: `${frameCount} synthetic stress frames, ${width}x${height}`,
		rgba,
		width,
	};
}

function measure(operation) {
	for (let iteration = 0; iteration < warmups; iteration += 1) {
		const result = operation();
		sink ^= result[result.length - 1] ?? 0;
	}
	const samples = [];
	for (let iteration = 0; iteration < iterations; iteration += 1) {
		const started = performance.now();
		const result = operation();
		samples.push(performance.now() - started);
		sink ^= result[result.length - 1] ?? 0;
	}
	return median(samples);
}

function pointColor(point) {
	return (point.r << 16) | (point.g << 8) | point.b;
}

function padPalette(palette) {
	const padded = [...palette];
	if (padded.length < 2) padded.push(0);
	while (padded.length < 256 && (padded.length & (padded.length - 1)) !== 0)
		padded.push(0);
	return padded;
}

function quantizeImageQGlobal(rgba) {
	const pixelCount = rgba.length / 4;
	const transparent = new Uint8Array(pixelCount);
	let opaqueCount = 0;
	for (let pixel = 0; pixel < pixelCount; pixel += 1) {
		if (rgba[pixel * 4 + 3] < alphaThreshold) transparent[pixel] = 1;
		else opaqueCount += 1;
	}
	if (opaqueCount === 0) {
		return {
			indexed: new Uint8Array(pixelCount).fill(1),
			palette: [0, 0],
			transparentIndex: 1,
		};
	}
	const opaqueRgba = new Uint8Array(opaqueCount * 4);
	let opaqueOffset = 0;
	for (let pixel = 0; pixel < pixelCount; pixel += 1) {
		if (transparent[pixel]) continue;
		const source = pixel * 4;
		const target = opaqueOffset * 4;
		opaqueRgba[target] = rgba[source];
		opaqueRgba[target + 1] = rgba[source + 1];
		opaqueRgba[target + 2] = rgba[source + 2];
		opaqueRgba[target + 3] = 255;
		opaqueOffset += 1;
	}
	const points = ImageQ.utils.PointContainer.fromUint8Array(
		opaqueRgba,
		opaqueCount,
		1,
	);
	const paletteObject = ImageQ.buildPaletteSync([points], {
		paletteQuantization: "rgbquant",
		colors: opaqueCount === pixelCount ? 256 : 255,
	});
	const quantized = ImageQ.applyPaletteSync(points, paletteObject, {
		imageQuantization: "nearest",
	});
	const palette = paletteObject
		.getPointContainer()
		.getPointArray()
		.map(pointColor);
	const colorToIndex = new Map(palette.map((color, index) => [color, index]));
	const quantizedColors = quantized.getPointArray();
	const transparentIndex =
		opaqueCount === pixelCount ? undefined : palette.length;
	if (transparentIndex !== undefined) palette.push(0);
	const indexed = new Uint8Array(pixelCount);
	opaqueOffset = 0;
	for (let pixel = 0; pixel < pixelCount; pixel += 1) {
		if (transparent[pixel]) indexed[pixel] = transparentIndex;
		else {
			indexed[pixel] =
				colorToIndex.get(pointColor(quantizedColors[opaqueOffset])) ?? 0;
			opaqueOffset += 1;
		}
	}
	return { indexed, palette: padPalette(palette), transparentIndex };
}

function encodeOmggifIndexed(fixture, quantized) {
	const output = new Uint8Array(
		quantized.indexed.length * 2 + fixture.frameCount * 1024 + 8192,
	);
	const writer = new GifWriter(output, fixture.width, fixture.height, {
		loop: 0,
		palette: quantized.palette,
	});
	const framePixels = fixture.width * fixture.height;
	for (let frame = 0; frame < fixture.frameCount; frame += 1) {
		writer.addFrame(
			0,
			0,
			fixture.width,
			fixture.height,
			quantized.indexed.subarray(
				frame * framePixels,
				(frame + 1) * framePixels,
			),
			{
				delay: 10,
				disposal: 2,
				transparent: quantized.transparentIndex,
			},
		);
	}
	return output.slice(0, writer.end());
}

function encodeOmggifQuality(fixture) {
	return encodeOmggifIndexed(fixture, quantizeImageQGlobal(fixture.rgba));
}

function encodeWtfgif(fixture) {
	return encodeRgbaGifFrames({
		alphaThreshold,
		delay: 10,
		frameCount: fixture.frameCount,
		frames: fixture.rgba,
		height: fixture.height,
		loop: 0,
		width: fixture.width,
	});
}

function decode(encoded) {
	const reader = new GifReader(encoded);
	const frameBytes = reader.width * reader.height * 4;
	const pixels = new Uint8Array(frameBytes * reader.numFrames());
	const frame = new Uint8Array(frameBytes);
	for (let index = 0; index < reader.numFrames(); index += 1) {
		frame.fill(0);
		reader.decodeAndBlitFrameRGBA(index, frame);
		pixels.set(frame, index * frameBytes);
	}
	return pixels;
}

function quality(encoded, fixture) {
	const decoded = decode(encoded);
	let squaredError = 0;
	let sampleCount = 0;
	for (let offset = 0; offset < fixture.rgba.length; offset += 4) {
		if (fixture.rgba[offset + 3] < alphaThreshold) continue;
		for (let channel = 0; channel < 3; channel += 1) {
			const difference =
				fixture.rgba[offset + channel] - decoded[offset + channel];
			squaredError += difference * difference;
			sampleCount += 1;
		}
	}
	if (squaredError === 0 || sampleCount === 0) return Number.POSITIVE_INFINITY;
	return 20 * Math.log10(255 / Math.sqrt(squaredError / sampleCount));
}

const fixtureMode = process.env.BENCH_RGBA_FIXTURE ?? "real";
if (
	fixtureMode !== "real" &&
	fixtureMode !== "stress" &&
	fixtureMode !== "all"
) {
	throw new Error("BENCH_RGBA_FIXTURE must be real, stress, or all");
}
const fixtures = fixtureMode === "stress" ? [] : [realImageFixture()];
if (
	fixtureMode === "stress" ||
	fixtureMode === "all" ||
	process.env.BENCH_RGBA_INCLUDE_STRESS === "1"
) {
	fixtures.push(makePhotoLikeStressFixture(512, 512, 10));
}

console.log(
	`Arbitrary RGBA encoder benchmark: initialized Wasm, alpha threshold ${alphaThreshold}, ${iterations} samples, ${warmups} warmups`,
);
console.log("fixture\timplementation\tmedian ms\tspeedup\tbytes\tPSNR dB");
for (const fixture of fixtures) {
	const omgRun = () => encodeOmggifQuality(fixture);
	const wtfRun = () => encodeWtfgif(fixture);
	const omgOutput = omgRun();
	const wtfOutput = wtfRun();
	const omgMs = measure(omgRun);
	const wtfMs = measure(wtfRun);
	for (const [implementation, milliseconds, output, speedup] of [
		["image-q + omggif", omgMs, omgOutput, 1],
		["wtfgif", wtfMs, wtfOutput, omgMs / wtfMs],
	]) {
		console.log(
			[
				fixture.name,
				implementation,
				milliseconds.toFixed(3),
				`${speedup.toFixed(2)}x`,
				output.length,
				quality(output, fixture).toFixed(2),
			].join("\t"),
		);
	}
}
console.log(`sink=${sink}`);
