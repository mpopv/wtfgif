import { readFileSync } from "node:fs";
import { performance } from "node:perf_hooks";

if (process.env.WTFGIF_COLD_RGBA_WORKER !== "1") {
	throw new Error(
		"bench-cold-rgba-worker.mjs must be launched by bench-cold-rgba.mjs",
	);
}

const implementation = process.argv[2];
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

const fixtureName = process.env.BENCH_COLD_RGBA_FIXTURE ?? "real";

function makeStressFixture(width, height, frameCount) {
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
	return { width, height, frameCount, rgba };
}

const initializedFirst = process.env.BENCH_INITIALIZED_FIRST === "1";
let started = performance.now();
const fixture =
	fixtureName === "stress"
		? makeStressFixture(512, 512, 10)
		: {
				width: 128,
				height: 128,
				frameCount: 8,
				rgba: new Uint8Array(
					readFileSync(
						new URL("../test/rgba/makeemoji-128x128x8.rgba", import.meta.url),
					),
				),
			};
const { width, height, frameCount, rgba } = fixture;

function pointColor(point) {
	return (point.r << 16) | (point.g << 8) | point.b;
}

function padPalette(palette) {
	const padded = [...palette];
	if (padded.length < 2) padded.push(0);
	while (padded.length < 256 && (padded.length & (padded.length - 1)) !== 0) {
		padded.push(0);
	}
	return padded;
}

function quantizeImageQGlobal(ImageQ) {
	const pixelCount = rgba.length / 4;
	const transparent = new Uint8Array(pixelCount);
	let opaqueCount = 0;
	for (let pixel = 0; pixel < pixelCount; pixel += 1) {
		if (rgba[pixel * 4 + 3] < alphaThreshold) transparent[pixel] = 1;
		else opaqueCount += 1;
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

function encodeBaselineGif(GifWriter, quantized) {
	const output = new Uint8Array(quantized.indexed.length * 2 + 8 * 1024 + 8192);
	const writer = new GifWriter(output, width, height, {
		loop: 0,
		palette: quantized.palette,
	});
	const framePixels = width * height;
	for (let frame = 0; frame < frameCount; frame += 1) {
		writer.addFrame(
			0,
			0,
			width,
			height,
			quantized.indexed.subarray(
				frame * framePixels,
				(frame + 1) * framePixels,
			),
			{ delay: 10, disposal: 2, transparent: quantized.transparentIndex },
		);
	}
	return output.slice(0, writer.end());
}

let output;
if (implementation === "baseline") {
	const [{ GifWriter }, { default: ImageQ }] = await Promise.all([
		import("omggif"),
		import("image-q"),
	]);
	if (initializedFirst) started = performance.now();
	output = encodeBaselineGif(GifWriter, quantizeImageQGlobal(ImageQ));
} else if (implementation === "wtfgif") {
	const entry =
		process.env.BENCH_WTFFIG_ENTRY === "encode"
			? "../dist/encode.mjs"
			: "../dist/index.mjs";
	const { encodeRgbaGifFrames, initializeWasmGlobally } = await import(entry);
	await initializeWasmGlobally();
	if (initializedFirst) started = performance.now();
	output = encodeRgbaGifFrames({
		alphaThreshold,
		backend: "wasm",
		delay: 10,
		frameCount,
		frames: rgba,
		height,
		loop: 0,
		paletteMode: "global",
		quantization: "quality",
		width,
	});
} else {
	throw new Error(`Unknown implementation: ${implementation}`);
}

process.stdout.write(
	JSON.stringify({
		elapsedMs: performance.now() - started,
		outputBytes: output.length,
	}),
);
