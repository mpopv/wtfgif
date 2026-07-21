import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import { encodeRgbaGifFrames, initializeWasmGlobally } from "../dist/index.mjs";

const require = createRequire(import.meta.url);
const ImageQ = require("image-q");
const { GifReader, GifWriter } = require("omggif");
const root = dirname(dirname(fileURLToPath(import.meta.url)));
const iterations = Number(process.env.BENCH_ITERATIONS ?? 15);
const warmups = Number(process.env.BENCH_WARMUP_ITERATIONS ?? 4);
const alphaThreshold = Math.trunc(255 * 0.7);
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

function median(values) {
	const sorted = values.toSorted((left, right) => left - right);
	return sorted[Math.floor(sorted.length / 2)];
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

function fixed332Palette() {
	return Array.from({ length: 256 }, (_, index) => {
		const red = Math.floor((((index >> 5) & 7) * 255 + 3) / 7);
		const green = Math.floor((((index >> 2) & 7) * 255 + 3) / 7);
		const blue = Math.floor(((index & 3) * 255 + 1) / 3);
		return (red << 16) | (green << 8) | blue;
	});
}

function quantizeRgb332(rgba) {
	let hasTransparency = false;
	for (let offset = 3; offset < rgba.length; offset += 4) {
		if (rgba[offset] < alphaThreshold) {
			hasTransparency = true;
			break;
		}
	}
	const indexed = new Uint8Array(rgba.length / 4);
	for (
		let source = 0, target = 0;
		source < rgba.length;
		source += 4, target += 1
	) {
		if (hasTransparency && rgba[source + 3] < alphaThreshold)
			indexed[target] = 255;
		else {
			const index =
				(rgba[source] & 0xe0) |
				((rgba[source + 1] >> 3) & 0x1c) |
				(rgba[source + 2] >> 6);
			indexed[target] = hasTransparency ? Math.min(index, 254) : index;
		}
	}
	const palette = fixed332Palette();
	if (hasTransparency) palette[255] = 0;
	return {
		indexed,
		palette,
		transparentIndex: hasTransparency ? 255 : undefined,
	};
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

function encodeOmggifTurbo(fixture) {
	return encodeOmggifIndexed(fixture, quantizeRgb332(fixture.rgba));
}

function encodeWtfgif(fixture, profile) {
	return encodeRgbaGifFrames({
		alphaThreshold,
		backend: "wasm",
		compression: profile === "turbo" ? "fast" : "balanced",
		delay: 10,
		frameCount: fixture.frameCount,
		frames: fixture.rgba,
		height: fixture.height,
		loop: 0,
		paletteMode: "global",
		quantization: profile === "turbo" ? "fast" : "quality",
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

function assertEqual(left, right, label) {
	if (left.length !== right.length)
		throw new Error(`${label}: decoded lengths differ`);
	for (let index = 0; index < left.length; index += 1) {
		if (left[index] !== right[index])
			throw new Error(`${label}: decoded RGBA differs at byte ${index}`);
	}
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

const fixtures = [realImageFixture()];
if (process.env.BENCH_RGBA_INCLUDE_STRESS === "1") {
	fixtures.push(makePhotoLikeStressFixture(512, 512, 10));
}

console.log(
	`Arbitrary RGBA encoder benchmark: initialized Wasm, ${iterations} samples, ${warmups} warmups`,
);
console.log(
	"fixture\tprofile\timplementation\tmedian ms\tspeedup\tbytes\tPSNR dB",
);
for (const fixture of fixtures) {
	for (const profile of ["quality", "turbo"]) {
		const omgRun =
			profile === "quality"
				? () => encodeOmggifQuality(fixture)
				: () => encodeOmggifTurbo(fixture);
		const wtfRun = () => encodeWtfgif(fixture, profile);
		const omgOutput = omgRun();
		const wtfOutput = wtfRun();
		if (profile === "turbo")
			assertEqual(
				decode(omgOutput),
				decode(wtfOutput),
				`${fixture.name}/turbo`,
			);
		const omgMs = measure(omgRun);
		const wtfMs = measure(wtfRun);
		for (const [implementation, milliseconds, output, speedup] of [
			[
				profile === "quality" ? "image-q + omggif" : "RGB332 + omggif",
				omgMs,
				omgOutput,
				1,
			],
			["wtfgif", wtfMs, wtfOutput, omgMs / wtfMs],
		]) {
			console.log(
				[
					fixture.name,
					profile,
					implementation,
					milliseconds.toFixed(3),
					`${speedup.toFixed(2)}x`,
					output.length,
					quality(output, fixture).toFixed(2),
				].join("\t"),
			);
		}
	}
}
console.log(`sink=${sink}`);
