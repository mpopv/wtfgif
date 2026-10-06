import { applyPalette, GIFEncoder, quantize } from "gifenc";
import ImageQ from "image-q";
import { Encoder as ModernGifEncoder } from "modern-gif";
import { GifReader, GifWriter } from "omggif";
import { encodeRgbaGifFrames, initializeWasmGlobally } from "/dist/encode.mjs";

const WIDTH = 128;
const HEIGHT = 128;
const FRAME_COUNT = 8;
const FRAME_PIXELS = WIDTH * HEIGHT;
const FRAME_BYTES = FRAME_PIXELS * 4;
const ALPHA_THRESHOLD = 179;
const DELAY_CENTISECONDS = 10;
const DELAY_MILLISECONDS = DELAY_CENTISECONDS * 10;
const TRANSPARENT_KEY = 0x00ff01;

const parameters = new URLSearchParams(location.search);
const implementation = parameters.get("implementation");
const token = parameters.get("token");
const includeOutput = parameters.get("includeOutput") === "1";

if (!implementation || !token) {
	throw new Error("Missing implementation or result token");
}
if (!globalThis.crossOriginIsolated) {
	throw new Error("Benchmark page is not cross-origin isolated");
}

const fixtureResponse = await fetch("/fixture.rgba");
if (!fixtureResponse.ok) {
	throw new Error(
		`Could not load benchmark fixture: ${fixtureResponse.status}`,
	);
}
const source = new Uint8ClampedArray(await fixtureResponse.arrayBuffer());
if (source.length !== FRAME_BYTES * FRAME_COUNT) {
	throw new Error(`Unexpected fixture byte length: ${source.length}`);
}
const sourceFrames = Array.from({ length: FRAME_COUNT }, (_, frame) =>
	source.slice(frame * FRAME_BYTES, (frame + 1) * FRAME_BYTES),
);

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

function quantizeImageQGlobal(rgba) {
	const pixelCount = rgba.length / 4;
	const transparent = new Uint8Array(pixelCount);
	let opaqueCount = 0;
	for (let pixel = 0; pixel < pixelCount; pixel += 1) {
		if (rgba[pixel * 4 + 3] < ALPHA_THRESHOLD) transparent[pixel] = 1;
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
		const sourceOffset = pixel * 4;
		const targetOffset = opaqueOffset * 4;
		opaqueRgba[targetOffset] = rgba[sourceOffset];
		opaqueRgba[targetOffset + 1] = rgba[sourceOffset + 1];
		opaqueRgba[targetOffset + 2] = rgba[sourceOffset + 2];
		opaqueRgba[targetOffset + 3] = 255;
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
		if (transparent[pixel]) {
			indexed[pixel] = transparentIndex;
		} else {
			indexed[pixel] =
				colorToIndex.get(pointColor(quantizedColors[opaqueOffset])) ?? 0;
			opaqueOffset += 1;
		}
	}
	return { indexed, palette: padPalette(palette), transparentIndex };
}

function encodeOmggif() {
	const quantized = quantizeImageQGlobal(source);
	const output = new Uint8Array(
		quantized.indexed.length * 2 + FRAME_COUNT * 1024 + 8192,
	);
	const writer = new GifWriter(output, WIDTH, HEIGHT, {
		loop: 0,
		palette: quantized.palette,
	});
	for (let frame = 0; frame < FRAME_COUNT; frame += 1) {
		writer.addFrame(
			0,
			0,
			WIDTH,
			HEIGHT,
			quantized.indexed.subarray(
				frame * FRAME_PIXELS,
				(frame + 1) * FRAME_PIXELS,
			),
			{
				delay: DELAY_CENTISECONDS,
				disposal: 2,
				transparent: quantized.transparentIndex,
			},
		);
	}
	return output.slice(0, writer.end());
}

function encodeGifenc() {
	let opaqueCount = 0;
	for (let offset = 3; offset < source.length; offset += 4) {
		if (source[offset] >= ALPHA_THRESHOLD) opaqueCount += 1;
	}
	const opaque = new Uint8Array(opaqueCount * 4);
	let outputOffset = 0;
	for (let offset = 0; offset < source.length; offset += 4) {
		if (source[offset + 3] < ALPHA_THRESHOLD) continue;
		opaque[outputOffset] = source[offset];
		opaque[outputOffset + 1] = source[offset + 1];
		opaque[outputOffset + 2] = source[offset + 2];
		opaque[outputOffset + 3] = 255;
		outputOffset += 4;
	}

	const opaquePalette = quantize(opaque, 255, { format: "rgb565" });
	const palette = [...opaquePalette, [0, 0, 0]];
	const transparentIndex = palette.length - 1;
	while (
		palette.length < 256 &&
		(palette.length & (palette.length - 1)) !== 0
	) {
		palette.push([0, 0, 0]);
	}

	const gif = GIFEncoder({ initialCapacity: source.length });
	for (let frame = 0; frame < FRAME_COUNT; frame += 1) {
		const rgba = source.slice(frame * FRAME_BYTES, (frame + 1) * FRAME_BYTES);
		const indexed = applyPalette(rgba, opaquePalette, "rgb565");
		let hasTransparency = false;
		for (let pixel = 0; pixel < FRAME_PIXELS; pixel += 1) {
			if (rgba[pixel * 4 + 3] < ALPHA_THRESHOLD) {
				indexed[pixel] = transparentIndex;
				hasTransparency = true;
			}
		}
		gif.writeFrame(indexed, WIDTH, HEIGHT, {
			delay: DELAY_MILLISECONDS,
			dispose: 2,
			palette: frame === 0 ? palette : undefined,
			repeat: 0,
			transparent: hasTransparency,
			transparentIndex,
		});
	}
	gif.finish();
	return gif.bytes();
}

function normalizeAlpha(rgba) {
	const normalized = new Uint8ClampedArray(rgba);
	for (let offset = 0; offset < normalized.length; offset += 4) {
		if (normalized[offset + 3] < ALPHA_THRESHOLD) {
			normalized[offset] = 0;
			normalized[offset + 1] = 0;
			normalized[offset + 2] = 0;
			normalized[offset + 3] = 0;
		} else {
			normalized[offset + 3] = 255;
		}
	}
	return normalized;
}

async function encodeModernGif() {
	const normalized = normalizeAlpha(source);
	const encoder = new ModernGifEncoder({
		backgroundColorIndex: 255,
		colorTableSize: 256,
		height: HEIGHT,
		loopCount: 0,
		looped: true,
		maxColors: 255,
		premultipliedAlpha: false,
		width: WIDTH,
	});
	for (let frame = 0; frame < FRAME_COUNT; frame += 1) {
		await encoder.encode({
			data: normalized.slice(frame * FRAME_BYTES, (frame + 1) * FRAME_BYTES),
			delay: DELAY_MILLISECONDS,
			disposal: 2,
			height: HEIGHT,
			width: WIDTH,
		});
	}
	return new Uint8Array(await encoder.flush("arrayBuffer"));
}

function normalizeForColorKey(rgba) {
	const keyed = new Uint8ClampedArray(rgba);
	for (let offset = 0; offset < keyed.length; offset += 4) {
		if (keyed[offset + 3] < ALPHA_THRESHOLD) {
			keyed[offset] = (TRANSPARENT_KEY >>> 16) & 255;
			keyed[offset + 1] = (TRANSPARENT_KEY >>> 8) & 255;
			keyed[offset + 2] = TRANSPARENT_KEY & 255;
		}
		keyed[offset + 3] = 255;
	}
	return keyed;
}

function encodeGifJs(GifClass, workerScript) {
	const keyed = normalizeForColorKey(source);
	const gif = new GifClass({
		height: HEIGHT,
		quality: 1,
		repeat: 0,
		transparent: TRANSPARENT_KEY,
		workers: 2,
		workerScript,
		width: WIDTH,
	});
	for (let frame = 0; frame < FRAME_COUNT; frame += 1) {
		gif.addFrame(
			new ImageData(
				keyed.subarray(frame * FRAME_BYTES, (frame + 1) * FRAME_BYTES),
				WIDTH,
				HEIGHT,
			),
			{ delay: DELAY_MILLISECONDS },
		);
	}
	return new Promise((resolve) => {
		gif.on("finished", (_blob, bytes) => {
			resolve({
				bytes: new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength),
				cleanup: () => {
					for (const worker of [...gif.freeWorkers, ...gif.activeWorkers]) {
						worker.terminate();
					}
				},
			});
		});
		gif.render();
	});
}

function encodeWtfgif(mode) {
	return encodeRgbaGifFrames({
		alphaThreshold: ALPHA_THRESHOLD,
		delay: DELAY_CENTISECONDS,
		frameCount: FRAME_COUNT,
		frames: sourceFrames,
		height: HEIGHT,
		loop: 0,
		mode,
		width: WIDTH,
	});
}

const operations = {
	"gif.js": () =>
		encodeGifJs(globalThis.GifJsOriginal, "/vendor/gif.worker.js"),
	"gif.js.optimized": () =>
		encodeGifJs(globalThis.GifJsOptimized, "/vendor/gif.optimized.worker.js"),
	gifenc: encodeGifenc,
	modernGif: encodeModernGif,
	omggif: encodeOmggif,
	wtfgif: () => encodeWtfgif("fastest"),
	wtfgifSmallest: () => encodeWtfgif("smallest"),
};

function validate(bytes) {
	const reader = new GifReader(bytes);
	if (
		reader.width !== WIDTH ||
		reader.height !== HEIGHT ||
		reader.numFrames() !== FRAME_COUNT
	) {
		throw new Error(
			`${implementation} returned ${reader.width}x${reader.height} with ${reader.numFrames()} frames`,
		);
	}
	let squaredError = 0;
	let sampleCount = 0;
	let alphaMatches = 0;
	let pixelCount = 0;
	const canvas = new Uint8Array(FRAME_BYTES);
	const restore = new Uint8Array(FRAME_BYTES);
	const delays = [];
	for (let frame = 0; frame < FRAME_COUNT; frame += 1) {
		const info = reader.frameInfo(frame);
		if (info.disposal === 3) restore.set(canvas);
		reader.decodeAndBlitFrameRGBA(frame, canvas);
		delays.push(info.delay);
		const sourceFrameOffset = frame * FRAME_BYTES;
		for (let offset = 0; offset < FRAME_BYTES; offset += 4) {
			const sourceOpaque =
				source[sourceFrameOffset + offset + 3] >= ALPHA_THRESHOLD;
			const decodedOpaque = canvas[offset + 3] >= 128;
			if (sourceOpaque === decodedOpaque) alphaMatches += 1;
			pixelCount += 1;
			if (!sourceOpaque) continue;
			for (let channel = 0; channel < 3; channel += 1) {
				const difference =
					source[sourceFrameOffset + offset + channel] -
					canvas[offset + channel];
				squaredError += difference * difference;
				sampleCount += 1;
			}
		}
		if (info.disposal === 2) {
			for (let y = info.y; y < info.y + info.height; y += 1) {
				canvas.fill(
					0,
					(y * WIDTH + info.x) * 4,
					(y * WIDTH + info.x + info.width) * 4,
				);
			}
		} else if (info.disposal === 3) {
			canvas.set(restore);
		}
	}
	return {
		alphaAccuracyPercent: (alphaMatches / pixelCount) * 100,
		delays,
		psnrDb:
			squaredError === 0
				? Number.POSITIVE_INFINITY
				: 20 * Math.log10(255 / Math.sqrt(squaredError / sampleCount)),
	};
}

async function report(result) {
	await fetch("/result", {
		body: JSON.stringify({ token, ...result }),
		headers: { "content-type": "application/json" },
		method: "POST",
	});
}

function base64(bytes) {
	let binary = "";
	for (let offset = 0; offset < bytes.length; offset += 32_768) {
		binary += String.fromCharCode(...bytes.subarray(offset, offset + 32_768));
	}
	return btoa(binary);
}

try {
	if (implementation.startsWith("wtfgif")) {
		await initializeWasmGlobally();
	}
	const cacheEviction = new Uint8Array(64 * 1024 * 1024);
	cacheEviction.fill(1);
	await new Promise((resolve) => requestAnimationFrame(() => resolve()));
	const operation = operations[implementation];
	if (!operation) throw new Error(`Unknown implementation: ${implementation}`);

	const started = performance.now();
	const encoded = await operation();
	const elapsedMs = performance.now() - started;
	const bytes = encoded.bytes ?? encoded;
	const cleanup = encoded.cleanup;
	const validation = validate(bytes);
	cleanup?.();

	await report({
		bytes: bytes.length,
		cacheEvictionSink: cacheEviction[cacheEviction.length - 1],
		elapsedMs,
		...(includeOutput ? { outputBase64: base64(bytes) } : {}),
		...validation,
	});
} catch (error) {
	await report({
		error: error instanceof Error ? error.message : String(error),
		stack: error instanceof Error ? error.stack : undefined,
	});
}
