import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { performance } from "node:perf_hooks";
import {
	createWasmCoreDecodeBackend,
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	GifReader as WtfGifReader,
	GifWriter as WtfGifWriter,
	setWasmCoreModule,
} from "../dist/index.mjs";

const require = createRequire(import.meta.url);
const { GifReader: OmgGifReader, GifWriter: OmgGifWriter } = require("omggif");
let wasmCoreModule = null;
try {
	wasmCoreModule = require("../crates/wtfgif-core/pkg/wtfgif_core.js");
	setWasmCoreModule(wasmCoreModule);
} catch {
	setWasmCoreModule(null);
}

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const gifsDir = join(root, "test", "gifs");

const iterations = Number(process.env.BENCH_ITERATIONS ?? 10);
const warmupIterations = Number(process.env.BENCH_WARMUP_ITERATIONS ?? 3);
const frameLimit = Number(process.env.BENCH_FRAME_LIMIT ?? 5);

function median(values) {
	const sorted = values.toSorted((a, b) => a - b);
	return sorted[Math.floor(sorted.length / 2)] ?? 0;
}

function percentile(values, p) {
	const sorted = values.toSorted((a, b) => a - b);
	const index = Math.min(sorted.length - 1, Math.ceil(sorted.length * p) - 1);
	return sorted[index] ?? 0;
}

function measure(fn) {
	for (let i = 0; i < warmupIterations; i++) {
		fn();
	}

	const samples = [];
	for (let i = 0; i < iterations; i++) {
		const start = performance.now();
		fn();
		samples.push(performance.now() - start);
	}

	return {
		median: median(samples),
		p95: percentile(samples, 0.95),
	};
}

function createDecodeTask(Reader, data, maxFrames) {
	const reader = new Reader(data);
	const frames = Math.min(reader.numFrames(), maxFrames);
	const pixels = new Uint8Array(reader.width * reader.height * 4);
	return {
		run() {
			for (let frame = 0; frame < frames; frame++) {
				pixels.fill(0);
				reader.decodeAndBlitFrameRGBA(frame, pixels);
			}
			return pixels[pixels.length - 1] ?? 0;
		},
		cleanup() {
			reader.returnToPool?.();
		},
	};
}

function createPreparedPlaybackTask(data, maxFrames) {
	const reader = new WtfGifReader(data);
	const frames = Math.min(reader.numFrames(), maxFrames);
	const prepared = reader.preparePlayback({
		format: "rgba",
		backend: "javascript",
	});
	const pixels = new Uint32Array(reader.width * reader.height);
	return {
		run() {
			for (let frame = 0; frame < frames; frame++) {
				prepared.copyFrame(frame, pixels);
			}
			return pixels[pixels.length - 1] ?? 0;
		},
		cleanup() {
			prepared.dispose();
			reader.returnToPool?.();
		},
	};
}

function createPreparedPlayerTask(data, maxFrames, backend = "javascript") {
	const reader = new WtfGifReader(data);
	const frames = Math.min(reader.numFrames(), maxFrames);
	const prepared = reader.preparePlayback({
		format: "rgba",
		deltas: true,
		backend,
	});
	const player = prepared.createPlayer();
	return {
		run() {
			for (let frame = 0; frame < frames; frame++) {
				player.drawFrame(frame);
			}
			return player.target[player.target.length - 1] ?? 0;
		},
		cleanup() {
			prepared.dispose();
			reader.returnToPool?.();
		},
	};
}

function preparePlaybackCold(
	data,
	maxFrames,
	backend = "javascript",
	deltas = false,
) {
	const reader = new WtfGifReader(data);
	const frames = Math.min(reader.numFrames(), maxFrames);
	const frameIndices = Array.from({ length: frames }, (_, index) => index);
	const prepared = reader.preparePlayback({
		format: "rgba",
		frameIndices,
		backend,
		deltas,
	});
	const pixels = prepared.getFramePixels(frames - 1);
	const value = pixels?.[pixels.length - 1] ?? 0;
	prepared.dispose();
	reader.returnToPool?.();
	return value;
}

function decodeCold(Reader, data, maxFrames) {
	const reader = new Reader(data);
	const frames = Math.min(reader.numFrames(), maxFrames);
	const pixels = new Uint8Array(reader.width * reader.height * 4);
	for (let frame = 0; frame < frames; frame++) {
		pixels.fill(0);
		reader.decodeAndBlitFrameRGBA(frame, pixels);
	}
	reader.returnToPool?.();
	return pixels[pixels.length - 1] ?? 0;
}

function framesFromGifRgba(Reader, data, maxFrames) {
	const reader = new Reader(data);
	const frames = Math.min(reader.numFrames(), maxFrames);
	const frameBytes = reader.width * reader.height * 4;
	const decodedFrames = new Array(frames);
	let checksum = 0;
	for (let frame = 0; frame < frames; frame++) {
		const pixels = new Uint8Array(frameBytes);
		reader.decodeAndBlitFrameRGBA(frame, pixels);
		decodedFrames[frame] = pixels;
		checksum ^= pixels[pixels.length - 1] ?? 0;
	}
	reader.returnToPool?.();
	return checksum ^ decodedFrames.length;
}

function verifyFrameParity(data, file) {
	const omg = new OmgGifReader(data);
	const wtf = new WtfGifReader(data);
	const frames = Math.min(omg.numFrames(), wtf.numFrames(), frameLimit);
	const length = omg.width * omg.height * 4;
	for (let frame = 0; frame < frames; frame++) {
		const omgPixels = new Uint8Array(length);
		const wtfPixels = new Uint8Array(length);
		omg.decodeAndBlitFrameRGBA(frame, omgPixels);
		wtf.decodeAndBlitFrameRGBA(frame, wtfPixels);
		for (let i = 0; i < length; i++) {
			if (omgPixels[i] !== wtfPixels[i]) {
				throw new Error(`${file} frame ${frame} differs at byte ${i}`);
			}
		}
	}
	wtf.returnToPool();
}

function makeSyntheticFrames() {
	const width = 128;
	const height = 128;
	const palette = [0x000000, 0xffffff, 0xff0000, 0x00ff00];
	const frames = [];
	for (let frame = 0; frame < 12; frame++) {
		const pixels = new Uint8Array(width * height);
		for (let y = 0; y < height; y++) {
			for (let x = 0; x < width; x++) {
				pixels[y * width + x] = (x + y + frame) & 3;
			}
		}
		frames.push(pixels);
	}
	return { width, height, palette, frames, flatFrames: flattenFrames(frames) };
}

function makeSyntheticDeltaFrames() {
	const width = 128;
	const height = 128;
	const palette = [0x000000, 0xffffff, 0xff0000, 0x00ff00];
	const frames = [];
	const pixels = new Uint8Array(width * height);
	for (let frame = 0; frame < 24; frame++) {
		pixels.fill(0);
		const startX = (frame * 5) % (width - 16);
		const startY = (frame * 3) % (height - 16);
		for (let y = 0; y < 16; y++) {
			for (let x = 0; x < 16; x++) {
				pixels[(startY + y) * width + startX + x] = 1 + (frame & 1);
			}
		}
		frames.push(pixels.slice());
	}
	return { width, height, palette, frames, flatFrames: flattenFrames(frames) };
}

function makeRgbaFrames(indexedFixture) {
	const frames = [];
	for (const indexedFrame of indexedFixture.frames) {
		const rgba = new Uint8Array(indexedFrame.length * 4);
		for (let i = 0; i < indexedFrame.length; i++) {
			const color = indexedFixture.palette[indexedFrame[i]] ?? 0;
			const dst = i * 4;
			rgba[dst] = (color >> 16) & 0xff;
			rgba[dst + 1] = (color >> 8) & 0xff;
			rgba[dst + 2] = color & 0xff;
			rgba[dst + 3] = 255;
		}
		frames.push(rgba);
	}
	return {
		width: indexedFixture.width,
		height: indexedFixture.height,
		palette: indexedFixture.palette,
		frames,
		flatFrames: flattenFrames(frames),
	};
}

function flattenFrames(frames) {
	const frameSize = frames[0]?.length ?? 0;
	const flatFrames = new Uint8Array(frameSize * frames.length);
	for (let i = 0; i < frames.length; i++) {
		flatFrames.set(frames[i], i * frameSize);
	}
	return flatFrames;
}

function encodeSynthetic(Writer, fixture) {
	const out = new Uint8Array(
		fixture.width * fixture.height * fixture.frames.length * 2 + 1024,
	);
	const writer = new Writer(out, fixture.width, fixture.height, {
		palette: fixture.palette,
		loop: 0,
	});
	for (const frame of fixture.frames) {
		writer.addFrame(0, 0, fixture.width, fixture.height, frame, {
			delay: 2,
		});
	}
	return writer.end();
}

function encodeSyntheticRgbaOmg(fixture) {
	const indexed = indexRgbaToPalette(fixture);
	return encodeSynthetic(OmgGifWriter, indexed);
}

function indexRgbaToPalette(fixture) {
	const colorToIndex = new Map();
	for (let i = 0; i < fixture.palette.length; i++) {
		colorToIndex.set(fixture.palette[i] ?? 0, i);
	}
	const frames = fixture.frames.map((rgba) => {
		const indexed = new Uint8Array(rgba.length >> 2);
		for (let offset = 0, pixel = 0; offset < rgba.length; offset += 4, pixel++) {
			const color =
				((rgba[offset] ?? 0) << 16) |
				((rgba[offset + 1] ?? 0) << 8) |
				(rgba[offset + 2] ?? 0);
			indexed[pixel] = colorToIndex.get(color) ?? 0;
		}
		return indexed;
	});
	return {
		width: fixture.width,
		height: fixture.height,
		palette: fixture.palette,
		frames,
		flatFrames: flattenFrames(frames),
	};
}

function encodeSyntheticDelta(Writer, fixture) {
	const out = new Uint8Array(
		fixture.width * fixture.height * fixture.frames.length * 2 + 1024,
	);
	const writer = new Writer(out, fixture.width, fixture.height, {
		palette: fixture.palette,
		loop: 0,
	});
	for (const frame of fixture.frames) {
		writer.addFrameDelta(frame, {
			delay: 2,
		});
	}
	return writer.end();
}

function encodeSyntheticWtf(fixture, useNative) {
	setWasmCoreModule(useNative ? wasmCoreModule : null);
	try {
		return encodeSynthetic(WtfGifWriter, fixture);
	} finally {
		setWasmCoreModule(wasmCoreModule);
	}
}

function encodeSyntheticDeltaWtf(fixture, useNative) {
	setWasmCoreModule(useNative ? wasmCoreModule : null);
	try {
		return encodeSyntheticDelta(WtfGifWriter, fixture);
	} finally {
		setWasmCoreModule(wasmCoreModule);
	}
}

function encodeSyntheticRgbaWtf(fixture, useNative, delta = false) {
	setWasmCoreModule(useNative ? wasmCoreModule : null);
	try {
		return encodeRgbaGifFrames({
			width: fixture.width,
			height: fixture.height,
			palette: fixture.palette,
			frames: fixture.flatFrames,
			delay: 2,
			loop: 0,
			backend: useNative ? "native" : "javascript",
			delta,
		}).length;
	} finally {
		setWasmCoreModule(wasmCoreModule);
	}
}

function encodeSyntheticBatchNativeWtf(fixture) {
	setWasmCoreModule(wasmCoreModule);
	try {
		return encodeIndexedGifFrames({
			width: fixture.width,
			height: fixture.height,
			palette: fixture.palette,
			frames: fixture.flatFrames,
			delay: 2,
			loop: 0,
			backend: "native",
		}).length;
	} finally {
		setWasmCoreModule(wasmCoreModule);
	}
}

function encodeSyntheticDeltaBatchNativeWtf(fixture) {
	setWasmCoreModule(wasmCoreModule);
	try {
		return encodeIndexedGifFrames({
			width: fixture.width,
			height: fixture.height,
			palette: fixture.palette,
			frames: fixture.flatFrames,
			delay: 2,
			loop: 0,
			backend: "native",
			delta: true,
		}).length;
	} finally {
		setWasmCoreModule(wasmCoreModule);
	}
}

function formatMs(value) {
	return value.toFixed(3).padStart(8);
}

function formatRatio(value) {
	return `${value.toFixed(2)}x`.padStart(7);
}

function printRows(title, rows) {
	console.log(`\n${title}`);
	console.log(
		"file".padEnd(38),
		"omggif".padStart(8),
		"wtfgif".padStart(8),
		"speedup".padStart(7),
		"p95".padStart(8),
	);
	for (const row of rows) {
		console.log(
			row.file.padEnd(38),
			formatMs(row.omg),
			formatMs(row.wtf),
			formatRatio(row.speedup),
			formatMs(row.wtfP95),
		);
	}
	const geomean = Math.exp(
		rows.reduce(
			(sum, row) => sum + Math.log(Math.max(row.speedup, Number.EPSILON)),
			0,
		) / rows.length,
	);
	console.log(
		"geomean".padEnd(38),
		"".padStart(8),
		"".padStart(8),
		formatRatio(geomean),
	);
}

const files = readdirSync(gifsDir)
	.filter((file) => file.endsWith(".gif"))
	.sort();

console.log(
	`wtfgif benchmark: iterations=${iterations}, warmup=${warmupIterations}, frameLimit=${frameLimit}`,
);

const parseRows = [];
const coldDecodeRows = [];
const framesFromGifRows = [];
const decodeRows = [];
const preparePlaybackRows = [];
const preparedPlaybackRows = [];
const preparedPlayerRows = [];
const nativePreparePlaybackRows = [];
const nativePreparePlaybackDeltaRows = [];
const nativePreparedPlayerRows = [];
const wasmCoreBackend = createWasmCoreDecodeBackend();
const wasmCoreAvailable = wasmCoreBackend.isAvailable();
if (wasmCoreAvailable) {
	WtfGifReader.setDecodeBackend(wasmCoreBackend);
}

for (const file of files) {
	const data = readFileSync(join(gifsDir, file));
	verifyFrameParity(data, file);

	const omgParse = measure(() => new OmgGifReader(data));
	const wtfParse = measure(() => {
		const reader = new WtfGifReader(data);
		reader.returnToPool();
	});
	parseRows.push({
		file,
		omg: omgParse.median,
		wtf: wtfParse.median,
		wtfP95: wtfParse.p95,
		speedup: omgParse.median / Math.max(wtfParse.median, Number.EPSILON),
	});

	const omgColdDecode = measure(() =>
		decodeCold(OmgGifReader, data, frameLimit),
	);
	const wtfColdDecode = measure(() =>
		decodeCold(WtfGifReader, data, frameLimit),
	);
	coldDecodeRows.push({
		file,
		omg: omgColdDecode.median,
		wtf: wtfColdDecode.median,
		wtfP95: wtfColdDecode.p95,
		speedup:
			omgColdDecode.median / Math.max(wtfColdDecode.median, Number.EPSILON),
	});

	const omgFramesFromGif = measure(() =>
		framesFromGifRgba(OmgGifReader, data, frameLimit),
	);
	const wtfFramesFromGif = measure(() =>
		framesFromGifRgba(WtfGifReader, data, frameLimit),
	);
	framesFromGifRows.push({
		file,
		omg: omgFramesFromGif.median,
		wtf: wtfFramesFromGif.median,
		wtfP95: wtfFramesFromGif.p95,
		speedup:
			omgFramesFromGif.median /
			Math.max(wtfFramesFromGif.median, Number.EPSILON),
	});

	const wtfPreparePlayback = measure(() =>
		preparePlaybackCold(data, frameLimit),
	);
	preparePlaybackRows.push({
		file,
		omg: omgColdDecode.median,
		wtf: wtfPreparePlayback.median,
		wtfP95: wtfPreparePlayback.p95,
		speedup:
			omgColdDecode.median /
			Math.max(wtfPreparePlayback.median, Number.EPSILON),
	});

	if (wasmCoreAvailable) {
		const nativePreparePlayback = measure(() =>
			preparePlaybackCold(data, frameLimit, "native"),
		);
		const nativePreparePlaybackDelta = measure(() =>
			preparePlaybackCold(data, frameLimit, "native", true),
		);
		nativePreparePlaybackRows.push({
			file,
			omg: omgColdDecode.median,
			wtf: nativePreparePlayback.median,
			wtfP95: nativePreparePlayback.p95,
			speedup:
				omgColdDecode.median /
				Math.max(nativePreparePlayback.median, Number.EPSILON),
		});
		nativePreparePlaybackDeltaRows.push({
			file,
			omg: omgColdDecode.median,
			wtf: nativePreparePlaybackDelta.median,
			wtfP95: nativePreparePlaybackDelta.p95,
			speedup:
				omgColdDecode.median /
				Math.max(nativePreparePlaybackDelta.median, Number.EPSILON),
		});
	}

	const omgDecodeTask = createDecodeTask(OmgGifReader, data, frameLimit);
	const wtfDecodeTask = createDecodeTask(WtfGifReader, data, frameLimit);
	const omgDecode = measure(omgDecodeTask.run);
	const wtfDecode = measure(wtfDecodeTask.run);
	omgDecodeTask.cleanup();
	wtfDecodeTask.cleanup();
	decodeRows.push({
		file,
		omg: omgDecode.median,
		wtf: wtfDecode.median,
		wtfP95: wtfDecode.p95,
		speedup: omgDecode.median / Math.max(wtfDecode.median, Number.EPSILON),
	});

	const wtfPreparedPlaybackTask = createPreparedPlaybackTask(data, frameLimit);
	const wtfPreparedPlayback = measure(wtfPreparedPlaybackTask.run);
	wtfPreparedPlaybackTask.cleanup();
	preparedPlaybackRows.push({
		file,
		omg: omgDecode.median,
		wtf: wtfPreparedPlayback.median,
		wtfP95: wtfPreparedPlayback.p95,
		speedup:
			omgDecode.median / Math.max(wtfPreparedPlayback.median, Number.EPSILON),
	});

	const wtfPreparedPlayerTask = createPreparedPlayerTask(data, frameLimit);
	const wtfPreparedPlayer = measure(wtfPreparedPlayerTask.run);
	wtfPreparedPlayerTask.cleanup();
	preparedPlayerRows.push({
		file,
		omg: omgDecode.median,
		wtf: wtfPreparedPlayer.median,
		wtfP95: wtfPreparedPlayer.p95,
		speedup:
			omgDecode.median / Math.max(wtfPreparedPlayer.median, Number.EPSILON),
	});

	if (wasmCoreAvailable) {
		const nativePreparedPlayerTask = createPreparedPlayerTask(
			data,
			frameLimit,
			"native",
		);
		const nativePreparedPlayer = measure(nativePreparedPlayerTask.run);
		nativePreparedPlayerTask.cleanup();
		nativePreparedPlayerRows.push({
			file,
			omg: omgDecode.median,
			wtf: nativePreparedPlayer.median,
			wtfP95: nativePreparedPlayer.p95,
			speedup:
				omgDecode.median /
				Math.max(nativePreparedPlayer.median, Number.EPSILON),
		});
	}
}

const synthetic = makeSyntheticFrames();
const rgbaSynthetic = makeRgbaFrames(synthetic);
const omgEncodedLength = encodeSynthetic(OmgGifWriter, synthetic);
const wtfJsEncodedLength = encodeSyntheticWtf(synthetic, false);
const wtfNativeEncodedLength = wasmCoreModule
	? encodeSyntheticWtf(synthetic, true)
	: wtfJsEncodedLength;
const wtfNativeBatchEncodedLength = wasmCoreModule
	? encodeSyntheticBatchNativeWtf(synthetic)
	: wtfNativeEncodedLength;
if (
	omgEncodedLength !== wtfJsEncodedLength ||
	omgEncodedLength !== wtfNativeEncodedLength ||
	omgEncodedLength !== wtfNativeBatchEncodedLength
) {
	throw new Error(
		`synthetic encode length mismatch: omggif=${omgEncodedLength}, wtfgif-js=${wtfJsEncodedLength}, wtfgif-native=${wtfNativeEncodedLength}, wtfgif-native-batch=${wtfNativeBatchEncodedLength}`,
	);
}

const omgEncode = measure(() => encodeSynthetic(OmgGifWriter, synthetic));
const wtfJsEncode = measure(() => encodeSyntheticWtf(synthetic, false));
const encodeRows = [
	{
		file: "synthetic-128x128x12",
		omg: omgEncode.median,
		wtf: wtfJsEncode.median,
		wtfP95: wtfJsEncode.p95,
		speedup: omgEncode.median / Math.max(wtfJsEncode.median, Number.EPSILON),
	},
];
const nativeEncodeRows = [];
const nativeBatchEncodeRows = [];
if (wasmCoreModule) {
	const wtfNativeEncode = measure(() => encodeSyntheticWtf(synthetic, true));
	nativeEncodeRows.push({
		file: "synthetic-128x128x12",
		omg: omgEncode.median,
		wtf: wtfNativeEncode.median,
		wtfP95: wtfNativeEncode.p95,
		speedup:
			omgEncode.median / Math.max(wtfNativeEncode.median, Number.EPSILON),
	});
	const wtfNativeBatchEncode = measure(() =>
		encodeSyntheticBatchNativeWtf(synthetic),
	);
	nativeBatchEncodeRows.push({
		file: "synthetic-128x128x12",
		omg: omgEncode.median,
		wtf: wtfNativeBatchEncode.median,
		wtfP95: wtfNativeBatchEncode.p95,
		speedup:
			omgEncode.median / Math.max(wtfNativeBatchEncode.median, Number.EPSILON),
	});
}

const omgRgbaEncode = measure(() => encodeSyntheticRgbaOmg(rgbaSynthetic));
const wtfRgbaJsEncode = measure(() =>
	encodeSyntheticRgbaWtf(rgbaSynthetic, false),
);
const rgbaEncodeRows = [
	{
		file: "synthetic-rgba-128x128x12",
		omg: omgRgbaEncode.median,
		wtf: wtfRgbaJsEncode.median,
		wtfP95: wtfRgbaJsEncode.p95,
		speedup:
			omgRgbaEncode.median / Math.max(wtfRgbaJsEncode.median, Number.EPSILON),
	},
];
const nativeRgbaEncodeRows = [];
if (wasmCoreModule) {
	const wtfRgbaNativeEncode = measure(() =>
		encodeSyntheticRgbaWtf(rgbaSynthetic, true),
	);
	nativeRgbaEncodeRows.push({
		file: "synthetic-rgba-128x128x12",
		omg: omgRgbaEncode.median,
		wtf: wtfRgbaNativeEncode.median,
		wtfP95: wtfRgbaNativeEncode.p95,
		speedup:
			omgRgbaEncode.median /
			Math.max(wtfRgbaNativeEncode.median, Number.EPSILON),
	});
}
const deltaSynthetic = makeSyntheticDeltaFrames();
const rgbaDeltaSynthetic = makeRgbaFrames(deltaSynthetic);
const omgDeltaEncode = measure(() =>
	encodeSynthetic(OmgGifWriter, deltaSynthetic),
);
const wtfDeltaJsEncode = measure(() =>
	encodeSyntheticDeltaWtf(deltaSynthetic, false),
);
const deltaEncodeRows = [
	{
		file: "synthetic-delta-128x128x24",
		omg: omgDeltaEncode.median,
		wtf: wtfDeltaJsEncode.median,
		wtfP95: wtfDeltaJsEncode.p95,
		speedup:
			omgDeltaEncode.median / Math.max(wtfDeltaJsEncode.median, Number.EPSILON),
	},
];
const nativeDeltaEncodeRows = [];
const nativeDeltaBatchEncodeRows = [];
if (wasmCoreModule) {
	const wtfDeltaNativeEncode = measure(() =>
		encodeSyntheticDeltaWtf(deltaSynthetic, true),
	);
	nativeDeltaEncodeRows.push({
		file: "synthetic-delta-128x128x24",
		omg: omgDeltaEncode.median,
		wtf: wtfDeltaNativeEncode.median,
		wtfP95: wtfDeltaNativeEncode.p95,
		speedup:
			omgDeltaEncode.median /
			Math.max(wtfDeltaNativeEncode.median, Number.EPSILON),
	});
	const wtfDeltaNativeBatchEncode = measure(() =>
		encodeSyntheticDeltaBatchNativeWtf(deltaSynthetic),
	);
	nativeDeltaBatchEncodeRows.push({
		file: "synthetic-delta-128x128x24",
		omg: omgDeltaEncode.median,
		wtf: wtfDeltaNativeBatchEncode.median,
		wtfP95: wtfDeltaNativeBatchEncode.p95,
		speedup:
			omgDeltaEncode.median /
			Math.max(wtfDeltaNativeBatchEncode.median, Number.EPSILON),
	});
}

const omgRgbaDeltaEncode = measure(() =>
	encodeSyntheticRgbaOmg(rgbaDeltaSynthetic),
);
const wtfRgbaDeltaJsEncode = measure(() =>
	encodeSyntheticRgbaWtf(rgbaDeltaSynthetic, false, true),
);
const rgbaDeltaEncodeRows = [
	{
		file: "synthetic-rgba-delta-128x128x24",
		omg: omgRgbaDeltaEncode.median,
		wtf: wtfRgbaDeltaJsEncode.median,
		wtfP95: wtfRgbaDeltaJsEncode.p95,
		speedup:
			omgRgbaDeltaEncode.median /
			Math.max(wtfRgbaDeltaJsEncode.median, Number.EPSILON),
	},
];
const nativeRgbaDeltaEncodeRows = [];
if (wasmCoreModule) {
	const wtfRgbaDeltaNativeEncode = measure(() =>
		encodeSyntheticRgbaWtf(rgbaDeltaSynthetic, true, true),
	);
	nativeRgbaDeltaEncodeRows.push({
		file: "synthetic-rgba-delta-128x128x24",
		omg: omgRgbaDeltaEncode.median,
		wtf: wtfRgbaDeltaNativeEncode.median,
		wtfP95: wtfRgbaDeltaNativeEncode.p95,
		speedup:
			omgRgbaDeltaEncode.median /
			Math.max(wtfRgbaDeltaNativeEncode.median, Number.EPSILON),
	});
}

printRows("Parse metadata", parseRows);
printRows("Cold parse + first decode RGBA", coldDecodeRows);
printRows("Frames from GIF RGBA", framesFromGifRows);
printRows("Cached decode RGBA", decodeRows);
printRows("Prepare playback build RGBA", preparePlaybackRows);
if (nativePreparePlaybackRows.length > 0) {
	printRows("Native prepare playback build RGBA", nativePreparePlaybackRows);
	printRows(
		"Native prepare playback build deltas RGBA",
		nativePreparePlaybackDeltaRows,
	);
}
printRows("Prepared playback copy RGBA", preparedPlaybackRows);
printRows("Prepared playback player RGBA", preparedPlayerRows);
if (nativePreparedPlayerRows.length > 0) {
	printRows("Native prepared playback player RGBA", nativePreparedPlayerRows);
}
printRows("Encode indexed frames JS", encodeRows);
if (nativeEncodeRows.length > 0) {
	printRows("Encode indexed frames Wasm writer", nativeEncodeRows);
	printRows("GIF from indexed frames Wasm", nativeBatchEncodeRows);
}
printRows("GIF from RGBA frames JS", rgbaEncodeRows);
if (nativeRgbaEncodeRows.length > 0) {
	printRows("GIF from RGBA frames Wasm", nativeRgbaEncodeRows);
}
printRows("Encode changed-rect frames JS", deltaEncodeRows);
if (nativeDeltaEncodeRows.length > 0) {
	printRows("Encode changed-rect frames Wasm writer", nativeDeltaEncodeRows);
	printRows("GIF from changed-rect frames Wasm", nativeDeltaBatchEncodeRows);
}
printRows("Encode RGBA changed-rect frames JS", rgbaDeltaEncodeRows);
if (nativeRgbaDeltaEncodeRows.length > 0) {
	printRows("GIF from RGBA changed-rect frames Wasm", nativeRgbaDeltaEncodeRows);
}
