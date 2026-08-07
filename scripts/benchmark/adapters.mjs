import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const ImageQ = require("image-q");
const { GifWriter } = require("omggif");

let wtfgif;

export async function initializeAdapters() {
	wtfgif = await import("../../dist/index.mjs");
	await wtfgif.initializeWasmGlobally();
	return wtfgif.getWasmStatus();
}

function delayAt(delay, frame) {
	return typeof delay === "number" ? delay : delay[frame];
}

export function encodeWtfgif(value, alphaThreshold) {
	if (!wtfgif) throw new Error("Benchmark adapters have not been initialized");
	return wtfgif.encodeRgbaGifFrames({
		alphaThreshold,
		delay: value.delay,
		delta: value.delta,
		frameCount: value.frameCount,
		frames: value.rgba,
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

function changedRect(previous, current, width, height) {
	let left = width;
	let top = height;
	let right = -1;
	let bottom = -1;
	for (let y = 0; y < height; y += 1) {
		for (let x = 0; x < width; x += 1) {
			const index = y * width + x;
			if (previous[index] === current[index]) continue;
			left = Math.min(left, x);
			top = Math.min(top, y);
			right = Math.max(right, x);
			bottom = Math.max(bottom, y);
		}
	}
	return right < left
		? null
		: { x: left, y: top, width: right - left + 1, height: bottom - top + 1 };
}

function copyRect(frame, canvasWidth, rect) {
	const output = new Uint8Array(rect.width * rect.height);
	for (let row = 0; row < rect.height; row += 1) {
		const source = (rect.y + row) * canvasWidth + rect.x;
		output.set(frame.subarray(source, source + rect.width), row * rect.width);
	}
	return output;
}

export function encodeImageQOmggif(value, alphaThreshold) {
	const quantized = quantizeImageQGlobal(value.rgba, alphaThreshold);
	const output = new Uint8Array(
		quantized.indexed.length * 2 + value.frameCount * 1024 + 8192,
	);
	const writer = new GifWriter(output, value.width, value.height, {
		loop: 0,
		palette: quantized.palette,
	});
	const framePixels = value.width * value.height;
	let previous = null;
	for (let frameIndex = 0; frameIndex < value.frameCount; frameIndex += 1) {
		const frame = quantized.indexed.subarray(
			frameIndex * framePixels,
			(frameIndex + 1) * framePixels,
		);
		let x = 0;
		let y = 0;
		let width = value.width;
		let height = value.height;
		let pixels = frame;
		if (value.delta && previous) {
			const rect = changedRect(previous, frame, value.width, value.height);
			if (rect) {
				({ x, y, width, height } = rect);
				pixels = copyRect(frame, value.width, rect);
			} else {
				width = 1;
				height = 1;
				pixels = frame.subarray(0, 1);
			}
		}
		writer.addFrame(x, y, width, height, pixels, {
			delay: delayAt(value.delay, frameIndex),
			disposal: value.delta ? 0 : 2,
			...(quantized.transparentIndex === undefined
				? {}
				: { transparent: quantized.transparentIndex }),
		});
		previous = frame;
	}
	return output.slice(0, writer.end());
}
