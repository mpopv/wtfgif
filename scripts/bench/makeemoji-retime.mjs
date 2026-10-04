import { readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { performance } from "node:perf_hooks";
import { GifReader, GifWriter } from "omggif";
import { root } from "../lib/paths.mjs";
import { geometricMean, median } from "./lib/metrics.mjs";

/*
 * Benchmarks wtfgif's pixel-perfect timing edit against the current MakeEmoji
 * export path:
 *
 *   already-decoded RGBA frames -> image-q per-frame quantization -> GifWriter
 *
 * This deliberately excludes MakeEmoji's initial preview decode, making the
 * comparison conservative. It reports both:
 *
 *   one-shot: retimeGifPixelPerfect(source, delays)
 *   compiled: compiled.withDelays(delays).toUint8Array()
 *
 * Usage:
 *   npm run build
 *   npm run bench:makeemoji-retime
 *
 * Environment:
 *   BENCH_ITERATIONS=3
 *   BENCH_WARMUP_ITERATIONS=1
 *   BENCH_FILTER=partyparrot
 */

const gifsDir = join(root, "test", "gifs");
const require = createRequire(import.meta.url);
const ImageQ = require("image-q");

const wtfgif = await import("../../dist/index.mjs");
if (typeof wtfgif.compileGif !== "function") {
	throw new Error(
		"dist/index.mjs does not export compileGif(). Build the current wtfgif source first.",
	);
}
if (typeof wtfgif.retimeGifPixelPerfect !== "function") {
	throw new Error(
		"dist/index.mjs does not export retimeGifPixelPerfect(). Build the current wtfgif source first.",
	);
}

const iterations = Number(process.env.BENCH_ITERATIONS ?? 3);
const warmups = Number(process.env.BENCH_WARMUP_ITERATIONS ?? 1);
const fixtureFilter = process.env.BENCH_FILTER ?? "";

if (!Number.isInteger(iterations) || iterations < 1) {
	throw new Error("BENCH_ITERATIONS must be a positive integer");
}
if (!Number.isInteger(warmups) || warmups < 0) {
	throw new Error("BENCH_WARMUP_ITERATIONS must be a non-negative integer");
}

const transparentSentinelCandidates = [
	0x000000, 0x010101, 0x020202, 0x080808, 0x101010, 0x202020, 0xffffff,
	0xfefefe, 0x808080, 0x00ffb4, 0x00ff00, 0x00ffff, 0xffff00, 0xff8000,
	0xa000ff, 0xff00ff,
];

function pickTransparentSentinel(imageData, width, height) {
	const usedColors = new Set();
	const total = width * height * 4;
	for (let offset = 0; offset < total; offset += 4) {
		if (imageData[offset + 3] === 0) continue;
		usedColors.add(
			(imageData[offset] << 16) |
				(imageData[offset + 1] << 8) |
				imageData[offset + 2],
		);
	}
	for (const candidate of transparentSentinelCandidates) {
		if (!usedColors.has(candidate)) return candidate;
	}
	let fallback = 0;
	while (usedColors.has(fallback) && fallback < 0xffffff) fallback += 1;
	return fallback;
}

function pointsToRgb(points) {
	return points.map((point) => (point.r << 16) | (point.g << 8) | point.b);
}

function indexPixelsWithPalette(pixels, palette) {
	const paletteIndex = new Map();
	for (let index = 0; index < palette.length; index += 1) {
		paletteIndex.set(palette[index], index);
	}
	return Uint8Array.from(pixels.map((pixel) => paletteIndex.get(pixel) ?? 0));
}

function padPaletteToGifSize(palette) {
	const nextPalette = [...palette];
	if (nextPalette.length < 2) nextPalette.push(0x000000);
	while (
		nextPalette.length < 256 &&
		(nextPalette.length & (nextPalette.length - 1)) !== 0
	) {
		nextPalette.push(0x000000);
	}
	return nextPalette;
}

// Kept byte-for-byte equivalent in behavior to MakeEmoji's current
// animationGallery.ts quantizeGifFrame() image-q path.
function quantizeGifFrame(imageData, width, height) {
	const transparencyCutoff = Math.trunc(255 * 0.7);
	const pixelCount = width * height;
	const transparentMask = new Uint8Array(pixelCount);
	let transparentCount = 0;
	let opaqueCount = 0;

	for (let index = 0; index < pixelCount; index += 1) {
		if (imageData[index * 4 + 3] < transparencyCutoff) {
			transparentMask[index] = 1;
			transparentCount += 1;
		} else {
			opaqueCount += 1;
		}
	}

	if (opaqueCount === 0) {
		const sentinel = pickTransparentSentinel(imageData, width, height);
		const transparencyIndex = 1;
		const pixels = new Uint8Array(pixelCount);
		pixels.fill(transparencyIndex);
		return {
			palette: padPaletteToGifSize([0x000000, sentinel]),
			pixels,
			transparencyIndex,
		};
	}

	const pointContainer = ImageQ.utils.PointContainer.fromUint8Array(
		(() => {
			const opaqueRgba = new Uint8Array(opaqueCount * 4);
			let opaqueOffset = 0;
			for (let index = 0; index < pixelCount; index += 1) {
				if (transparentMask[index] === 1) continue;
				const sourceOffset = index * 4;
				const targetOffset = opaqueOffset * 4;
				opaqueRgba[targetOffset] = imageData[sourceOffset];
				opaqueRgba[targetOffset + 1] = imageData[sourceOffset + 1];
				opaqueRgba[targetOffset + 2] = imageData[sourceOffset + 2];
				opaqueRgba[targetOffset + 3] = 0xff;
				opaqueOffset += 1;
			}
			return opaqueRgba;
		})(),
		opaqueCount,
		1,
	);
	const palette = ImageQ.buildPaletteSync([pointContainer], {
		paletteQuantization: "rgbquant",
		colors: transparentCount > 0 ? 255 : 256,
	});
	const quantizedPoints = ImageQ.applyPaletteSync(pointContainer, palette, {
		imageQuantization: "nearest",
	});
	const paletteRgbArray = pointsToRgb(
		palette.getPointContainer().getPointArray(),
	).sort((left, right) => left - right);
	const indexedOpaque = indexPixelsWithPalette(
		pointsToRgb(quantizedPoints.getPointArray()),
		paletteRgbArray,
	);

	if (transparentCount === 0) {
		return {
			palette: padPaletteToGifSize(paletteRgbArray),
			pixels: indexedOpaque,
			transparencyIndex: undefined,
		};
	}

	const transparencyIndex = paletteRgbArray.length;
	const pixels = new Uint8Array(pixelCount);
	let opaqueIndex = 0;
	for (let pixelIndex = 0; pixelIndex < pixelCount; pixelIndex += 1) {
		if (transparentMask[pixelIndex] === 1) {
			pixels[pixelIndex] = transparencyIndex;
		} else {
			pixels[pixelIndex] = indexedOpaque[opaqueIndex] ?? 0;
			opaqueIndex += 1;
		}
	}
	return {
		palette: padPaletteToGifSize([
			...paletteRgbArray,
			pickTransparentSentinel(imageData, width, height),
		]),
		pixels,
		transparencyIndex,
	};
}

function clearFrameRect(canvas, canvasWidth, info) {
	for (let row = info.y; row < info.y + info.height; row += 1) {
		canvas.fill(
			0,
			(row * canvasWidth + info.x) * 4,
			(row * canvasWidth + info.x + info.width) * 4,
		);
	}
}

function decodeForTiming(data) {
	const reader = new GifReader(data);
	const frameBytes = reader.width * reader.height * 4;
	const canvas = new Uint8ClampedArray(frameBytes);
	const restore = new Uint8ClampedArray(frameBytes);
	const frames = [];
	const delays = [];

	for (let frame = 0; frame < reader.numFrames(); frame += 1) {
		const info = reader.frameInfo(frame);
		if (info.disposal === 3) restore.set(canvas);
		reader.decodeAndBlitFrameRGBA(frame, canvas);
		frames.push(new Uint8ClampedArray(canvas));
		delays.push(info.delay);
		if (info.disposal === 2) {
			clearFrameRect(canvas, reader.width, info);
		} else if (info.disposal === 3) {
			canvas.set(restore);
		}
	}

	return {
		width: reader.width,
		height: reader.height,
		frames,
		delays,
	};
}

function makeEditedDelays(delays) {
	return delays.map((delay, index) => {
		const normalized = delay <= 0 ? 10 : Math.max(2, delay);
		return Math.min(0xffff, normalized + 1 + (index % 7));
	});
}

function makeEmojiExport(decoded, delays) {
	const buffer = [];
	const writer = new GifWriter(buffer, decoded.width, decoded.height, {
		loop: 0,
	});
	for (let frame = 0; frame < decoded.frames.length; frame += 1) {
		const quantized = quantizeGifFrame(
			decoded.frames[frame],
			decoded.width,
			decoded.height,
		);
		writer.addFrame(0, 0, decoded.width, decoded.height, quantized.pixels, {
			palette: quantized.palette,
			delay: delays[frame],
			transparent: quantized.transparencyIndex,
			disposal: 2,
		});
	}
	writer.end();
	return Uint8Array.from(buffer);
}

function arraysEqual(left, right) {
	if (left.length !== right.length) return false;
	for (let index = 0; index < left.length; index += 1) {
		if (left[index] !== right[index]) return false;
	}
	return true;
}

function assertRgbaParity(expected, actual, label) {
	if (
		actual.width !== expected.width ||
		actual.height !== expected.height ||
		actual.frames.length !== expected.frames.length
	) {
		throw new Error(`${label}: dimensions or frame count changed`);
	}
	for (let frame = 0; frame < expected.frames.length; frame += 1) {
		if (!arraysEqual(expected.frames[frame], actual.frames[frame])) {
			throw new Error(`${label}: composited RGBA frame ${frame} changed`);
		}
	}
}

function assertDelayParity(actual, expected, label) {
	if (!arraysEqual(actual.delays, expected)) {
		throw new Error(
			`${label}: delays differ (${actual.delays.join(",")} vs ${expected.join(",")})`,
		);
	}
}

let sink = 0;
function consume(bytes) {
	sink ^= bytes.length ^ (bytes[bytes.length - 1] ?? 0);
}

function measure(operation) {
	for (let iteration = 0; iteration < warmups; iteration += 1) {
		consume(operation());
	}
	const samples = [];
	let output;
	for (let iteration = 0; iteration < iterations; iteration += 1) {
		const started = performance.now();
		output = operation();
		samples.push(performance.now() - started);
		consume(output);
	}
	return { medianMs: median(samples), output };
}

function formatMs(value) {
	if (value < 0.001) return `${(value * 1_000_000).toFixed(0)}ns`;
	if (value < 1) return `${(value * 1_000).toFixed(1)}µs`;
	return `${value.toFixed(2)}ms`;
}

const fixtures = readdirSync(gifsDir)
	.filter((file) => file.endsWith(".gif"))
	.filter((file) => file.includes(fixtureFilter))
	.toSorted();

if (fixtures.length === 0) {
	throw new Error(`No GIF fixtures matched BENCH_FILTER=${fixtureFilter}`);
}

console.log(
	[
		"MakeEmoji timing-export benchmark",
		`${iterations} measured iteration(s)`,
		`${warmups} warmup(s)`,
		"initial preview decode is outside every timed path",
		"compiled GIF construction is outside the compiled-edit path",
	].join(" | "),
);

const rows = [];
for (const file of fixtures) {
	const source = new Uint8Array(readFileSync(join(gifsDir, file)));
	const decoded = decodeForTiming(source);
	const editedDelays = makeEditedDelays(decoded.delays);
	const compiled = wtfgif.compileGif(source);

	const makeEmoji = measure(() => makeEmojiExport(decoded, editedDelays));
	const oneShot = measure(() =>
		wtfgif.retimeGifPixelPerfect(source, editedDelays),
	);
	const compiledEdit = measure(() =>
		compiled.withDelays(editedDelays).toUint8Array(),
	);

	const oneShotDecoded = decodeForTiming(oneShot.output);
	const compiledDecoded = decodeForTiming(compiledEdit.output);
	assertRgbaParity(decoded, oneShotDecoded, `${file}/one-shot`);
	assertRgbaParity(decoded, compiledDecoded, `${file}/compiled`);
	assertDelayParity(oneShotDecoded, editedDelays, `${file}/one-shot`);
	assertDelayParity(compiledDecoded, editedDelays, `${file}/compiled`);

	const makeEmojiDecoded = decodeForTiming(makeEmoji.output);
	assertDelayParity(makeEmojiDecoded, editedDelays, `${file}/MakeEmoji`);
	const makeEmojiPixelPerfect = (() => {
		try {
			assertRgbaParity(decoded, makeEmojiDecoded, `${file}/MakeEmoji`);
			return true;
		} catch {
			return false;
		}
	})();

	rows.push({
		file,
		shape: `${decoded.width}x${decoded.height}x${decoded.frames.length}`,
		makeEmojiMs: makeEmoji.medianMs,
		oneShotMs: oneShot.medianMs,
		compiledMs: compiledEdit.medianMs,
		oneShotSpeedup: makeEmoji.medianMs / oneShot.medianMs,
		compiledSpeedup: makeEmoji.medianMs / compiledEdit.medianMs,
		byteRatio: compiledEdit.output.length / makeEmoji.output.length,
		makeEmojiPixelPerfect,
	});
}

console.log();
console.log(
	[
		"fixture".padEnd(38),
		"shape".padStart(15),
		"MakeEmoji".padStart(11),
		"one-shot".padStart(11),
		"faster".padStart(10),
		"compiled".padStart(11),
		"faster".padStart(10),
		"wtf/old".padStart(9),
		"wtf parity".padStart(12),
		"old exact".padStart(10),
	].join(" "),
);
for (const row of rows) {
	console.log(
		[
			row.file.padEnd(38),
			row.shape.padStart(15),
			formatMs(row.makeEmojiMs).padStart(11),
			formatMs(row.oneShotMs).padStart(11),
			`${row.oneShotSpeedup.toFixed(1)}x`.padStart(10),
			formatMs(row.compiledMs).padStart(11),
			`${row.compiledSpeedup.toFixed(1)}x`.padStart(10),
			`${row.byteRatio.toFixed(2)}x`.padStart(9),
			"RGBA+delay".padStart(12),
			(row.makeEmojiPixelPerfect ? "yes" : "no").padStart(10),
		].join(" "),
	);
}

console.log();
console.log(
	`one-shot: min ${Math.min(...rows.map((row) => row.oneShotSpeedup)).toFixed(1)}x, ` +
		`geomean ${geometricMean(rows.map((row) => row.oneShotSpeedup)).toFixed(1)}x`,
);
console.log(
	`compiled: min ${Math.min(...rows.map((row) => row.compiledSpeedup)).toFixed(1)}x, ` +
		`geomean ${geometricMean(rows.map((row) => row.compiledSpeedup)).toFixed(1)}x`,
);
console.log(
	"wtfgif parity: exact composited RGBA and exact edited delays on every fixture",
);
console.log(
	`MakeEmoji current export pixel-perfect on ${rows.filter((row) => row.makeEmojiPixelPerfect).length}/${rows.length} fixture(s)`,
);

const structuralRows = [];
for (const file of fixtures) {
	const source = new Uint8Array(readFileSync(join(gifsDir, file)));
	const compiled = wtfgif.compileGif(source);
	if (!compiled.canReorderFrames) continue;

	const decoded = decodeForTiming(source);
	const reversedFrames = decoded.frames.slice().reverse();
	const reversedDelays = decoded.delays.slice().reverse();
	const boomerangFrames = [...decoded.frames, ...reversedFrames];
	const boomerangDelays = [...decoded.delays, ...reversedDelays];

	const makeEmojiReverse = measure(() =>
		makeEmojiExport({ ...decoded, frames: reversedFrames }, reversedDelays),
	);
	const wtfgifReverse = measure(() => compiled.reverseFrames());
	const makeEmojiBoomerang = measure(() =>
		makeEmojiExport({ ...decoded, frames: boomerangFrames }, boomerangDelays),
	);
	const wtfgifBoomerang = measure(() => compiled.boomerangFrames());

	const decodedReverse = decodeForTiming(wtfgifReverse.output);
	const decodedBoomerang = decodeForTiming(wtfgifBoomerang.output);
	assertRgbaParity(
		{ ...decoded, frames: reversedFrames },
		decodedReverse,
		`${file}/reverse`,
	);
	assertDelayParity(decodedReverse, reversedDelays, `${file}/reverse`);
	assertRgbaParity(
		{ ...decoded, frames: boomerangFrames },
		decodedBoomerang,
		`${file}/boomerang`,
	);
	assertDelayParity(decodedBoomerang, boomerangDelays, `${file}/boomerang`);

	structuralRows.push({
		file,
		shape: `${decoded.width}x${decoded.height}x${decoded.frames.length}`,
		reverseSpeedup: makeEmojiReverse.medianMs / wtfgifReverse.medianMs,
		boomerangSpeedup: makeEmojiBoomerang.medianMs / wtfgifBoomerang.medianMs,
		reverseMs: wtfgifReverse.medianMs,
		boomerangMs: wtfgifBoomerang.medianMs,
	});
}

if (structuralRows.length > 0) {
	console.log();
	console.log("Proven-safe structural frame reuse");
	console.log(
		[
			"fixture".padEnd(38),
			"shape".padStart(15),
			"reverse".padStart(11),
			"faster".padStart(10),
			"boomerang".padStart(11),
			"faster".padStart(10),
		].join(" "),
	);
	for (const row of structuralRows) {
		console.log(
			[
				row.file.padEnd(38),
				row.shape.padStart(15),
				formatMs(row.reverseMs).padStart(11),
				`${row.reverseSpeedup.toFixed(1)}x`.padStart(10),
				formatMs(row.boomerangMs).padStart(11),
				`${row.boomerangSpeedup.toFixed(1)}x`.padStart(10),
			].join(" "),
		);
	}
	console.log(
		`reverse: min ${Math.min(...structuralRows.map((row) => row.reverseSpeedup)).toFixed(1)}x, ` +
			`geomean ${geometricMean(structuralRows.map((row) => row.reverseSpeedup)).toFixed(1)}x`,
	);
	console.log(
		`boomerang: min ${Math.min(...structuralRows.map((row) => row.boomerangSpeedup)).toFixed(1)}x, ` +
			`geomean ${geometricMean(structuralRows.map((row) => row.boomerangSpeedup)).toFixed(1)}x`,
	);
	console.log(
		"Structural parity: exact mapped composited RGBA and delays on every accepted fixture",
	);
}
console.log(`sink=${sink}`);
