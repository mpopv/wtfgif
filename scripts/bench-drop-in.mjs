import { readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import {
	GifReader as WtfGifReader,
	GifWriter as WtfGifWriter,
} from "../dist/index.mjs";

const require = createRequire(import.meta.url);
const { GifReader: OmgGifReader, GifWriter: OmgGifWriter } = require("omggif");

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const gifsDir = join(root, "test", "gifs");
const iterations = Number(process.env.BENCH_ITERATIONS ?? 15);
const warmups = Number(process.env.BENCH_WARMUP_ITERATIONS ?? 3);
const targetSampleMs = Number(process.env.BENCH_TARGET_SAMPLE_MS ?? 10);
const benchmark = process.argv[2] ?? "all";
const outputKind =
	process.env.BENCH_OUTPUT ?? (benchmark === "encode" ? "typed" : "array");
const gifFilter = process.env.BENCH_GIF_FILTER;
const encodeColorCounts = (process.env.BENCH_COLOR_COUNTS ?? "256")
	.split(",")
	.map(Number);
let sink = 0;

if (!["all", "decode", "encode"].includes(benchmark)) {
	throw new Error("Usage: bench-drop-in.mjs [all|decode|encode]");
}
if (!["array", "typed"].includes(outputKind)) {
	throw new Error('BENCH_OUTPUT must be "array" or "typed"');
}
if (
	encodeColorCounts.some(
		(colorCount) =>
			!Number.isInteger(colorCount) ||
			colorCount < 2 ||
			colorCount > 256 ||
			(colorCount & (colorCount - 1)) !== 0,
	)
) {
	throw new Error(
		"BENCH_COLOR_COUNTS must contain powers of two from 2 to 256",
	);
}

function median(values) {
	const sorted = values.toSorted((left, right) => left - right);
	return sorted[Math.floor(sorted.length / 2)];
}

function geomean(values) {
	return Math.exp(
		values.reduce((total, value) => total + Math.log(value), 0) / values.length,
	);
}

function consume(result) {
	sink ^= result.checksum;
}

function measure(operation) {
	for (let iteration = 0; iteration < warmups; iteration++) {
		consume(operation());
	}

	const probeStarted = performance.now();
	consume(operation());
	const probeMs = Math.max(performance.now() - probeStarted, 0.001);
	const batchSize = Math.max(
		1,
		Math.min(1_000, Math.ceil(targetSampleMs / probeMs)),
	);
	const samples = [];

	for (let iteration = 0; iteration < iterations; iteration++) {
		const started = performance.now();
		for (let batch = 0; batch < batchSize; batch++) {
			consume(operation());
		}
		samples.push((performance.now() - started) / batchSize);
	}

	return { median: median(samples) };
}

function clearFrameRect(canvas, canvasWidth, info) {
	for (let row = info.y; row < info.y + info.height; row++) {
		canvas.fill(
			0,
			(row * canvasWidth + info.x) * 4,
			(row * canvasWidth + info.x + info.width) * 4,
		);
	}
}

function decodeAll(Reader, data) {
	const reader = new Reader(data);
	const frameBytes = reader.width * reader.height * 4;
	const canvas = new Uint8Array(frameBytes);
	const restore = new Uint8Array(frameBytes);
	const frames = new Uint8Array(frameBytes * reader.numFrames());

	for (let frame = 0; frame < reader.numFrames(); frame++) {
		const info = reader.frameInfo(frame);
		if (info.disposal === 3) {
			restore.set(canvas);
		}

		reader.decodeAndBlitFrameRGBA(frame, canvas);
		frames.set(canvas, frame * frameBytes);

		if (info.disposal === 2) {
			clearFrameRect(canvas, reader.width, info);
		} else if (info.disposal === 3) {
			canvas.set(restore);
		}
	}

	return {
		checksum:
			frames[0] ^
			frames[Math.floor(frames.length / 2)] ^
			frames[frames.length - 1],
		frameCount: reader.numFrames(),
		frames,
		height: reader.height,
		width: reader.width,
	};
}

function assertDecodedEqual(expected, actual, label) {
	if (
		expected.width !== actual.width ||
		expected.height !== actual.height ||
		expected.frameCount !== actual.frameCount ||
		expected.frames.length !== actual.frames.length
	) {
		throw new Error(`${label}: decoded shape differs`);
	}

	for (let index = 0; index < expected.frames.length; index++) {
		if (expected.frames[index] !== actual.frames[index]) {
			throw new Error(`${label}: decoded RGBA differs at byte ${index}`);
		}
	}
}

function makeIndexedFixture(colorCount) {
	const width = 128;
	const height = 128;
	const frameCount = 12;
	const palette = Array.from({ length: colorCount }, (_, index) => {
		const red = (index * 73) & 255;
		const green = (index * 151) & 255;
		const blue = (index * 199) & 255;
		return (red << 16) | (green << 8) | blue;
	});
	const frames = Array.from({ length: frameCount }, (_, frame) => {
		const pixels = new Uint8Array(width * height);
		for (let row = 0; row < height; row++) {
			for (let column = 0; column < width; column++) {
				pixels[row * width + column] =
					(column * 17 + row * 31 + frame * 13 + ((column * row) >> 3)) &
					(colorCount - 1);
			}
		}
		return pixels;
	});

	return {
		frameCount,
		frames,
		height,
		name: `${colorCount} colors, 128x128x12`,
		palette,
		width,
	};
}

function encodeAll(Writer, fixture) {
	const output =
		outputKind === "typed"
			? new Uint8Array(
					fixture.width * fixture.height * fixture.frameCount * 2 + 4096,
				)
			: [];
	const writer = new Writer(output, fixture.width, fixture.height, {
		loop: 0,
		palette: fixture.palette,
	});

	for (const frame of fixture.frames) {
		writer.addFrame(0, 0, fixture.width, fixture.height, frame, {
			delay: 2,
			disposal: 2,
		});
	}

	const length = writer.end();
	const bytes =
		output instanceof Uint8Array
			? output.subarray(0, length)
			: Uint8Array.from(output.slice(0, length));
	return {
		bytes,
		checksum: bytes[0] ^ bytes[Math.floor(bytes.length / 2)] ^ bytes.at(-1),
	};
}

function printHeader(title, firstColumn) {
	console.log(`\n${title}`);
	console.log(
		firstColumn.padEnd(42),
		"omggif ms".padStart(10),
		"wtfgif ms".padStart(10),
		"speedup".padStart(10),
	);
}

function printRow(name, omg, wtf) {
	console.log(
		name.padEnd(42),
		omg.median.toFixed(3).padStart(10),
		wtf.median.toFixed(3).padStart(10),
		`${(omg.median / wtf.median).toFixed(2)}x`.padStart(10),
	);
}

console.log(
	`Drop-in GifReader/GifWriter benchmark: ${iterations} samples, ${warmups} warmups, batched to about ${targetSampleMs} ms/sample`,
);

if (benchmark === "all" || benchmark === "decode") {
	const decodeRatios = [];
	printHeader("GifReader: GIF -> every composited RGBA frame", "fixture");
	for (const file of readdirSync(gifsDir)
		.filter((entry) => entry.endsWith(".gif"))
		.filter((entry) => !gifFilter || entry.includes(gifFilter))
		.toSorted()) {
		const data = readFileSync(join(gifsDir, file));
		const expected = decodeAll(OmgGifReader, data);
		const actual = decodeAll(WtfGifReader, data);
		assertDecodedEqual(expected, actual, file);

		const omg = measure(() => decodeAll(OmgGifReader, data));
		const wtf = measure(() => decodeAll(WtfGifReader, data));
		decodeRatios.push(omg.median / wtf.median);
		printRow(file, omg, wtf);
	}

	console.log(
		"decode summary".padEnd(42),
		"".padStart(10),
		"".padStart(10),
		`min ${Math.min(...decodeRatios).toFixed(2)}x / geo ${geomean(decodeRatios).toFixed(2)}x`.padStart(
			10,
		),
	);
}

if (benchmark === "all" || benchmark === "encode") {
	const encodeRatios = [];
	printHeader(
		`GifWriter: indexed frames -> GIF (${outputKind === "typed" ? "Uint8Array" : "number[]"} output)`,
		"fixture",
	);
	for (const colorCount of encodeColorCounts) {
		const fixture = makeIndexedFixture(colorCount);
		const omgOutput = encodeAll(OmgGifWriter, fixture);
		const wtfOutput = encodeAll(WtfGifWriter, fixture);
		assertDecodedEqual(
			decodeAll(OmgGifReader, omgOutput.bytes),
			decodeAll(OmgGifReader, wtfOutput.bytes),
			fixture.name,
		);

		const omg = measure(() => encodeAll(OmgGifWriter, fixture));
		const wtf = measure(() => encodeAll(WtfGifWriter, fixture));
		encodeRatios.push(omg.median / wtf.median);
		printRow(fixture.name, omg, wtf);
	}

	console.log(
		"encode summary".padEnd(42),
		"".padStart(10),
		"".padStart(10),
		`min ${Math.min(...encodeRatios).toFixed(2)}x / geo ${geomean(encodeRatios).toFixed(2)}x`.padStart(
			10,
		),
	);
}

console.log(`\nbenchmark sink: ${sink}`);
