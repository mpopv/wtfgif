import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const SHARP_DEFAULT_EFFORT = 7;
let wtfgif;
let ImageQ;
let GifWriter;
let gifenc;
let ModernGifEncoder;
let sharp;
let GifEncoder2;
let GifEncoder;
let gifski;
let vips;
let magick;
let ffmpeg;
// Color used for transparent pixels by encoders without alpha support.
const TRANSPARENT_KEY = 0x00ff01;

// Every implementation receives the same RGBA frames and alpha threshold,
// discovers its own global palette, and returns a complete GIF. Package
// loading happens in initialize(), outside the timed encode.
export const implementations = {
	wtfgif: {
		label: "wtfgif fastest",
		configuration:
			"global quality quantization, run-aware LZW, and changed-rectangle frames through the wtfgif/encode entry point",
		frameArrays: true,
		initialize: initializeWtfgif,
		encode: (value, alphaThreshold) => encodeWtfgif(value, alphaThreshold),
	},
	"wtfgif-smallest": {
		label: "wtfgif smallest",
		configuration:
			'wtfgif/encode with mode: "smallest": full-dictionary LZW where it is shorter than the run-aware stream',
		frameArrays: true,
		initialize: initializeWtfgif,
		encode: (value, alphaThreshold) =>
			encodeWtfgif(value, alphaThreshold, { mode: "smallest" }),
	},
	"wtfgif-independent": {
		label: "wtfgif (independent frames)",
		configuration:
			"wtfgif/encode with independentFrames: true, writing every frame as a full-canvas image",
		frameArrays: true,
		initialize: initializeWtfgif,
		encode: (value, alphaThreshold) =>
			encodeWtfgif(value, alphaThreshold, { independentFrames: true }),
	},
	"image-q-rgbquant+omggif": {
		label: "image-q + omggif",
		configuration:
			"global image-q rgbquant palette, nearest mapping, and omggif LZW",
		initialize: () => {
			ImageQ = require("image-q");
			({ GifWriter } = require("omggif"));
		},
		encode: encodeImageQOmggif,
	},
	gifenc: {
		label: "gifenc",
		configuration:
			"global gifenc rgb565 quantize over opaque pixels, applyPalette per frame, gifenc LZW",
		initialize: () => {
			gifenc = require("gifenc");
		},
		encode: encodeGifenc,
	},
	"modern-gif": {
		label: "modern-gif",
		configuration:
			"modern-gif Encoder with 255 colors on the main thread, alpha thresholded first",
		initialize: async () => {
			({ Encoder: ModernGifEncoder } = await import("modern-gif"));
		},
		encode: encodeModernGif,
	},
	...Object.fromEntries(
		[1, 4, 7, 10].map((effort) => [
			`sharp-effort${effort}`,
			{
				label: `sharp effort ${effort}`,
				configuration: `sharp (libvips + cgif + libimagequant) gif({ effort: ${effort}, dither: 0 }), alpha thresholded first`,
				initialize: initializeSharp,
				encode: (value, alphaThreshold) =>
					encodeSharp(value, alphaThreshold, { effort, dither: 0 }),
			},
		]),
	),
	"gif-encoder-2": {
		label: "gif-encoder-2",
		configuration:
			"gif-encoder-2 defaults (NeuQuant), transparent pixels as a color key",
		initialize: () => {
			GifEncoder2 = require("gif-encoder-2");
		},
		encode: (value, alphaThreshold) =>
			encodeNeuQuant(GifEncoder2, value, alphaThreshold, true),
	},
	gifencoder: {
		label: "gifencoder",
		configuration:
			"gifencoder defaults (NeuQuant), transparent pixels as a color key",
		initialize: () => {
			GifEncoder = require("gifencoder");
		},
		encode: (value, alphaThreshold) =>
			encodeNeuQuant(GifEncoder, value, alphaThreshold, false),
	},
	"gifski-wasm": {
		label: "gifski-wasm",
		configuration: "gifski-wasm defaults (quality 90), alpha thresholded first",
		initialize: initializeGifski,
		encode: encodeGifski,
	},
	"wasm-vips": {
		label: "wasm-vips effort 1",
		configuration:
			"wasm-vips (libvips + cgif + libimagequant in Wasm) gifsave effort 1, dither 0, alpha thresholded first",
		initialize: initializeVips,
		encode: encodeVips,
	},
	"magick-wasm": {
		label: "magick-wasm",
		configuration:
			"@imagemagick/magick-wasm GIF defaults with restore-to-background disposal, alpha thresholded first",
		initialize: initializeMagick,
		encode: encodeMagick,
	},
	"ffmpeg-wasm": {
		label: "ffmpeg.wasm",
		configuration:
			"@ffmpeg/core palettegen + paletteuse (one palette, default dithering), alpha thresholded first",
		initialize: initializeFfmpeg,
		encode: encodeFfmpeg,
	},
	"sharp-default": {
		label: "sharp defaults",
		configuration: `sharp gif() defaults (effort ${SHARP_DEFAULT_EFFORT}, dither 1.0), alpha thresholded first`,
		initialize: initializeSharp,
		encode: (value, alphaThreshold) => encodeSharp(value, alphaThreshold, {}),
	},
};

export async function initializeAdapter(implementation) {
	const adapter = implementations[implementation];
	if (!adapter) {
		throw new Error(`Unknown benchmark implementation: ${implementation}`);
	}
	return adapter.initialize();
}

async function initializeWtfgif() {
	wtfgif = await import("../../../dist/encode.mjs");
	await wtfgif.initializeWasmGlobally();
	return wtfgif.getWasmStatus();
}

async function initializeSharp() {
	sharp = require("sharp");
	// libvips registers savers and starts its thread pool on first use. Spend
	// that once on a fixed 2x2 animation, as wtfgif's initialization spends
	// its own preparation, so the timed encode is not charged for it.
	const pixels = Buffer.alloc(2 * 2 * 4 * 2, 255);
	await sharp(pixels, {
		raw: { width: 2, height: 4, channels: 4, pageHeight: 2 },
	})
		.gif({ delay: [10, 10], loop: 0 })
		.toBuffer();
	return { sharp: sharp.versions.sharp, vips: sharp.versions.vips };
}

function delayAt(delay, frame) {
	return typeof delay === "number" ? delay : delay[frame];
}

function frameDelays(value) {
	return Array.from({ length: value.frameCount }, (_, frame) =>
		delayAt(value.delay, frame),
	);
}

/** Binary alpha at the threshold, with transparent pixels cleared to zero. */
function thresholdAlpha(rgba, alphaThreshold, Output = Uint8Array) {
	const output = new Output(rgba.length);
	for (let offset = 0; offset < rgba.length; offset += 4) {
		if (rgba[offset + 3] < alphaThreshold) continue;
		output[offset] = rgba[offset];
		output[offset + 1] = rgba[offset + 1];
		output[offset + 2] = rgba[offset + 2];
		output[offset + 3] = 255;
	}
	return output;
}

export function encodeWtfgif(value, alphaThreshold, options = {}) {
	if (!wtfgif) throw new Error("Benchmark adapters have not been initialized");
	return wtfgif.encodeRgbaGifFrames({
		alphaThreshold,
		delay: value.delay,
		frameCount: value.frameCount,
		frames: value.frames ?? value.rgba,
		height: value.height,
		loop: 0,
		width: value.width,
		...options,
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

function encodeGifenc(value, alphaThreshold) {
	const { GIFEncoder, applyPalette, quantize } = gifenc;
	const { rgba, width, height, frameCount } = value;
	let opaqueCount = 0;
	for (let offset = 3; offset < rgba.length; offset += 4) {
		if (rgba[offset] >= alphaThreshold) opaqueCount += 1;
	}
	const opaque = new Uint8Array(Math.max(1, opaqueCount) * 4);
	let opaqueOffset = 0;
	for (let offset = 0; offset < rgba.length; offset += 4) {
		if (rgba[offset + 3] < alphaThreshold) continue;
		opaque[opaqueOffset] = rgba[offset];
		opaque[opaqueOffset + 1] = rgba[offset + 1];
		opaque[opaqueOffset + 2] = rgba[offset + 2];
		opaque[opaqueOffset + 3] = 255;
		opaqueOffset += 4;
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
	const frameBytes = width * height * 4;
	const gif = GIFEncoder({ initialCapacity: rgba.length });
	for (let frame = 0; frame < frameCount; frame += 1) {
		// Copy: gifenc reads `frame.buffer` from offset zero, and corpus RGBA
		// may be a Node Buffer whose slice() is a view into a shared pool.
		const frameRgba = new Uint8Array(
			rgba.subarray(frame * frameBytes, (frame + 1) * frameBytes),
		);
		const indexed = applyPalette(frameRgba, opaquePalette, "rgb565");
		let hasTransparency = false;
		for (let pixel = 0; pixel < width * height; pixel += 1) {
			if (frameRgba[pixel * 4 + 3] < alphaThreshold) {
				indexed[pixel] = transparentIndex;
				hasTransparency = true;
			}
		}
		gif.writeFrame(indexed, width, height, {
			delay: delayAt(value.delay, frame) * 10,
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

async function encodeModernGif(value, alphaThreshold) {
	const { width, height, frameCount } = value;
	const normalized = thresholdAlpha(
		value.rgba,
		alphaThreshold,
		Uint8ClampedArray,
	);
	const encoder = new ModernGifEncoder({
		backgroundColorIndex: 255,
		colorTableSize: 256,
		height,
		loopCount: 0,
		looped: true,
		maxColors: 255,
		premultipliedAlpha: false,
		width,
	});
	const frameBytes = width * height * 4;
	for (let frame = 0; frame < frameCount; frame += 1) {
		await encoder.encode({
			data: normalized.slice(frame * frameBytes, (frame + 1) * frameBytes),
			delay: delayAt(value.delay, frame) * 10,
			disposal: 2,
			height,
			width,
		});
	}
	return new Uint8Array(await encoder.flush("arrayBuffer"));
}

async function encodeSharp(value, alphaThreshold, options) {
	const { width, height, frameCount } = value;
	const pixels = thresholdAlpha(value.rgba, alphaThreshold);
	const output = await sharp(pixels, {
		raw: {
			width,
			height: height * frameCount,
			channels: 4,
			pageHeight: height,
		},
	})
		.gif({
			...options,
			delay: frameDelays(value).map((delay) => delay * 10),
			loop: 0,
		})
		.toBuffer();
	return new Uint8Array(output.buffer, output.byteOffset, output.byteLength);
}

const WARMUP = {
	width: 2,
	height: 2,
	frameCount: 2,
	delay: 10,
	rgba: Uint8Array.from({ length: 2 * 2 * 4 * 2 }, (_, index) =>
		index % 4 === 3 ? 255 : (index * 37) & 255,
	),
};

function packageDirectory(specifier) {
	return dirname(fileURLToPath(import.meta.resolve(specifier)));
}

/** gif-encoder-2 and gifencoder share NeuQuant and a color-key API. */
function encodeNeuQuant(Encoder, value, alphaThreshold, setDelayFirst) {
	const { width, height, frameCount } = value;
	const keyed = thresholdAlpha(value.rgba, alphaThreshold);
	for (let offset = 0; offset < keyed.length; offset += 4) {
		if (keyed[offset + 3] === 0) {
			keyed[offset] = TRANSPARENT_KEY >> 16;
			keyed[offset + 1] = (TRANSPARENT_KEY >> 8) & 255;
			keyed[offset + 2] = TRANSPARENT_KEY & 255;
		}
		keyed[offset + 3] = 255;
	}
	const encoder = new Encoder(width, height);
	if (setDelayFirst) encoder.setDelay(delayAt(value.delay, 0) * 10);
	encoder.start();
	encoder.setRepeat(0);
	encoder.setTransparent(TRANSPARENT_KEY);
	const frameBytes = width * height * 4;
	for (let frame = 0; frame < frameCount; frame += 1) {
		encoder.setDelay(delayAt(value.delay, frame) * 10);
		encoder.addFrame(
			keyed.subarray(frame * frameBytes, (frame + 1) * frameBytes),
		);
	}
	encoder.finish();
	return new Uint8Array(encoder.out.getData());
}

async function initializeGifski() {
	gifski = await import("gifski-wasm");
	const wasm = join(
		packageDirectory("gifski-wasm"),
		"..",
		"pkg",
		"gifski_wasm_bg.wasm",
	);
	await gifski.init(await WebAssembly.compile(readFileSync(wasm)));
	await encodeGifski(WARMUP, 128);
}

function encodeGifski(value, alphaThreshold) {
	const { width, height, frameCount } = value;
	const pixels = thresholdAlpha(value.rgba, alphaThreshold);
	const frameBytes = width * height * 4;
	return gifski.encode({
		frames: Array.from({ length: frameCount }, (_, frame) =>
			pixels.subarray(frame * frameBytes, (frame + 1) * frameBytes),
		),
		width,
		height,
		frameDurations: frameDelays(value).map((delay) => delay * 10),
		repeat: 0,
	});
}

async function initializeVips() {
	vips = await require("wasm-vips")();
	encodeVips(WARMUP, 128);
	return { vips: vips.version() };
}

function encodeVips(value, alphaThreshold) {
	const { width, height, frameCount } = value;
	const pixels = thresholdAlpha(value.rgba, alphaThreshold);
	const raw = vips.Image.newFromMemory(
		pixels,
		width,
		height * frameCount,
		4,
		"uchar",
	);
	const image = raw.copy({ interpretation: "srgb" });
	image.setInt("page-height", height);
	image.setArrayInt(
		"delay",
		frameDelays(value).map((delay) => delay * 10),
	);
	image.setInt("loop", 0);
	const output = image.writeToBuffer(".gif", { effort: 1, dither: 0 });
	image.delete();
	raw.delete();
	return output;
}

async function initializeMagick() {
	magick = await import("@imagemagick/magick-wasm");
	const wasm = join(
		packageDirectory("@imagemagick/magick-wasm"),
		"x86",
		"magick.wasm",
	);
	await magick.initializeImageMagick(readFileSync(wasm));
	encodeMagick(WARMUP, 128);
	return { imageMagick: magick.Magick.imageMagickVersion };
}

function encodeMagick(value, alphaThreshold) {
	const {
		MagickFormat,
		MagickImage,
		MagickImageCollection,
		MagickReadSettings,
	} = magick;
	const { width, height, frameCount } = value;
	const pixels = thresholdAlpha(value.rgba, alphaThreshold);
	const frameBytes = width * height * 4;
	const collection = MagickImageCollection.create();
	const delays = frameDelays(value);
	for (let frame = 0; frame < frameCount; frame += 1) {
		const settings = new MagickReadSettings();
		settings.format = MagickFormat.Rgba;
		settings.width = width;
		settings.height = height;
		const image = MagickImage.create(
			pixels.subarray(frame * frameBytes, (frame + 1) * frameBytes),
			settings,
		);
		image.animationDelay = delays[frame];
		image.animationIterations = 0;
		image.gifDisposeMethod = magick.GifDisposeMethod.Background;
		collection.push(image);
	}
	let output;
	collection.write(MagickFormat.Gif, (data) => {
		output = new Uint8Array(data);
	});
	collection.dispose();
	return output;
}

async function initializeFfmpeg() {
	const directory = dirname(require.resolve("@ffmpeg/core"));
	// The Emscripten core expects a worker-style global and finds its Wasm
	// through it.
	globalThis.self ??= globalThis;
	globalThis.self.location ??= { href: `file://${directory}/` };
	const createCore = require("@ffmpeg/core");
	ffmpeg = await createCore({
		locateFile: (file) => join(directory, file),
		wasmBinary: readFileSync(join(directory, "ffmpeg-core.wasm")),
		print: () => {},
		printErr: () => {},
	});
	encodeFfmpeg(WARMUP, 128);
}

function encodeFfmpeg(value, alphaThreshold) {
	const delays = frameDelays(value);
	if (delays.some((delay) => delay !== delays[0])) {
		throw new Error("The ffmpeg.wasm adapter needs one delay for every frame");
	}
	ffmpeg.FS.writeFile("in.rgba", thresholdAlpha(value.rgba, alphaThreshold));
	const code = ffmpeg.exec(
		"-loglevel",
		"error",
		"-f",
		"rawvideo",
		"-pix_fmt",
		"rgba",
		"-s",
		`${value.width}x${value.height}`,
		"-framerate",
		String(100 / delays[0]),
		"-i",
		"in.rgba",
		"-filter_complex",
		"split[a][b];[a]palettegen=reserve_transparent=1[p];[b][p]paletteuse=alpha_threshold=128",
		"-loop",
		"0",
		"out.gif",
	);
	if (code !== 0) throw new Error(`ffmpeg.wasm exited with ${code}`);
	const output = ffmpeg.FS.readFile("out.gif");
	ffmpeg.FS.unlink("in.rgba");
	ffmpeg.FS.unlink("out.gif");
	ffmpeg.reset();
	return output;
}
