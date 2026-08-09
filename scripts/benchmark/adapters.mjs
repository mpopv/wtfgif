import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
let wtfgif;
let ImageQ;
let GifWriter;

export async function initializeAdapter(implementation) {
	if (implementation === "wtfgif") {
		wtfgif = await import("../../dist/encode.mjs");
		await wtfgif.initializeWasmGlobally();
		return wtfgif.getWasmStatus();
	}
	if (implementation === "image-q-rgbquant+omggif") {
		ImageQ = require("image-q");
		({ GifWriter } = require("omggif"));
		return undefined;
	}
	throw new Error(`Unknown benchmark implementation: ${implementation}`);
}

function delayAt(delay, frame) {
	return typeof delay === "number" ? delay : delay[frame];
}

export function encodeWtfgif(value, alphaThreshold) {
	if (!wtfgif) throw new Error("Benchmark adapters have not been initialized");
	return wtfgif.encodeRgbaGifFrames({
		alphaThreshold,
		delay: value.delay,
		frameCount: value.frameCount,
		frames: value.frames ?? value.rgba,
		height: value.height,
		loop: 0,
		width: value.width,
	});
}

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

function quantizeImageQGlobal(rgba, alphaThreshold) {
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

export function encodeImageQOmggif(value, alphaThreshold) {
	if (!ImageQ || !GifWriter) {
		throw new Error("Benchmark adapter has not been initialized");
	}
	const quantized = quantizeImageQGlobal(value.rgba, alphaThreshold);
	const output = new Uint8Array(
		quantized.indexed.length * 2 + value.frameCount * 1024 + 8192,
	);
	const writer = new GifWriter(output, value.width, value.height, {
		loop: 0,
		palette: quantized.palette,
	});
	const framePixels = value.width * value.height;
	for (let frameIndex = 0; frameIndex < value.frameCount; frameIndex += 1) {
		const frame = quantized.indexed.subarray(
			frameIndex * framePixels,
			(frameIndex + 1) * framePixels,
		);
		writer.addFrame(0, 0, value.width, value.height, frame, {
			delay: delayAt(value.delay, frameIndex),
			disposal: 2,
			...(quantized.transparentIndex === undefined
				? {}
				: { transparent: quantized.transparentIndex }),
		});
	}
	return output.slice(0, writer.end());
}
