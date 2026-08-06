import { readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import {
	decodeGifFramesRgba,
	encodeIndexedGifFrames,
	encodeRgbaGifFrames,
	setNativeAddonModule,
	setWasmCoreModule,
	GifReader as WtfGifReader,
} from "../dist/index.mjs";

const require = createRequire(import.meta.url);
const { GifReader: OmgGifReader, GifWriter: OmgGifWriter } = require("omggif");
const nativeAddon = require("../native/build/wtfgif_native.node");
setNativeAddonModule(nativeAddon);
const wasm = require("../crates/wtfgif-core/pkg/wtfgif_core.js");
setWasmCoreModule(wasm);

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const gifsDir = join(root, "test", "gifs");
const iterations = Number(process.env.BENCH_ITERATIONS ?? 15);
const warmups = Number(process.env.BENCH_WARMUP_ITERATIONS ?? 0);
let sink = 0;

function median(values) {
	return values.toSorted((a, b) => a - b)[Math.floor(values.length / 2)];
}

function consume(result) {
	sink ^= result.checksum ?? result.bytes?.[result.bytes.length - 1] ?? 0;
	result.dispose?.();
}

function measure(operation) {
	for (let iteration = 0; iteration < warmups; iteration++) {
		consume(operation());
	}
	const samples = [];
	for (let iteration = 0; iteration < iterations; iteration++) {
		const started = performance.now();
		const result = operation();
		samples.push(performance.now() - started);
		consume(result);
	}
	return { median: median(samples) };
}

function clearFrameRect(canvas, canvasWidth, info) {
	for (let y = info.y; y < info.y + info.height; y++) {
		canvas.fill(
			0,
			(y * canvasWidth + info.x) * 4,
			(y * canvasWidth + info.x + info.width) * 4,
		);
	}
}

function decodeAllOmggif(data) {
	const reader = new OmgGifReader(data);
	const frameBytes = reader.width * reader.height * 4;
	const canvas = new Uint8Array(frameBytes);
	const restore = new Uint8Array(frameBytes);
	const bytes = new Uint8Array(frameBytes * reader.numFrames());
	for (let frame = 0; frame < reader.numFrames(); frame++) {
		const info = reader.frameInfo(frame);
		if (info.disposal === 3) {
			restore.set(canvas);
		}
		reader.decodeAndBlitFrameRGBA(frame, canvas);
		bytes.set(canvas, frame * frameBytes);
		if (info.disposal === 2) {
			clearFrameRect(canvas, reader.width, info);
		} else if (info.disposal === 3) {
			canvas.set(restore);
		}
	}
	return {
		width: reader.width,
		height: reader.height,
		frameCount: reader.numFrames(),
		frameBytes,
		bytes,
		checksum: bytes[bytes.length - 1] ?? 0,
	};
}

function decodeAllWtfgif(data, backend) {
	const reader = new WtfGifReader(data);
	const prepared = reader.preparePlayback({
		backend,
		dedupe: "none",
		format: "rgba",
	});
	let checksum = prepared.frames.length;
	for (let frame = 0; frame < prepared.frames.length; frame++) {
		const bytes = prepared.getFrameBytes(frame);
		checksum ^= bytes?.[bytes.length - 1] ?? 0;
	}
	return {
		width: reader.width,
		height: reader.height,
		frameCount: reader.numFrames(),
		prepared,
		checksum,
		dispose() {
			prepared.dispose();
			reader.dispose();
		},
	};
}

function decodeAllNativeAddon(data) {
	const decoded = decodeGifFramesRgba(data);
	return {
		width: decoded.width,
		height: decoded.height,
		frameCount: decoded.frameCount,
		bytes: decoded.pixels,
		checksum: decoded.pixels[decoded.pixels.length - 1] ?? 0,
	};
}

function assertDecodeParity(data, file, backend) {
	const expected = decodeAllOmggif(data);
	const actual = decodeAllWtfgif(data, backend);
	try {
		if (
			actual.width !== expected.width ||
			actual.height !== expected.height ||
			actual.frameCount !== expected.frameCount
		) {
			throw new Error(`${file}/${backend}: decoded dimensions differ`);
		}
		for (let frame = 0; frame < expected.frameCount; frame++) {
			const expectedFrame = expected.bytes.subarray(
				frame * expected.frameBytes,
				(frame + 1) * expected.frameBytes,
			);
			const actualFrame = actual.prepared.getFrameBytes(frame);
			if (
				!actualFrame ||
				actualFrame.length !== expectedFrame.length ||
				actualFrame.some((byte, index) => byte !== expectedFrame[index])
			) {
				throw new Error(
					`${file}/${backend}: composited frame ${frame} differs`,
				);
			}
		}
	} finally {
		actual.dispose();
	}
}

function assertNativeAddonDecodeParity(data, file) {
	const expected = decodeAllOmggif(data);
	const actual = decodeAllNativeAddon(data);
	if (
		actual.width !== expected.width ||
		actual.height !== expected.height ||
		actual.frameCount !== expected.frameCount ||
		actual.bytes.length !== expected.bytes.length ||
		actual.bytes.some((byte, index) => byte !== expected.bytes[index])
	) {
		throw new Error(`${file}/native addon: decoded RGBA bytes differ`);
	}
}

function flatten(frames) {
	const frameLength = frames[0].length;
	const output = new Uint8Array(frameLength * frames.length);
	for (let frame = 0; frame < frames.length; frame++) {
		output.set(frames[frame], frame * frameLength);
	}
	return output;
}

function makeIndexedFixture(name, colorCount, delta = false) {
	const width = 128;
	const height = 128;
	const frameCount = delta ? 24 : 12;
	const palette = Array.from({ length: colorCount }, (_, index) => {
		const red = (index * 73) & 255;
		const green = (index * 151) & 255;
		const blue = (index * 199) & 255;
		return (red << 16) | (green << 8) | blue;
	});
	const frames = [];
	for (let frame = 0; frame < frameCount; frame++) {
		const pixels = new Uint8Array(width * height);
		if (delta) {
			const startX = (frame * 5) % (width - 16);
			const startY = (frame * 3) % (height - 16);
			for (let y = 0; y < 16; y++) {
				for (let x = 0; x < 16; x++) {
					pixels[(startY + y) * width + startX + x] =
						1 + (frame % (colorCount - 1));
				}
			}
		} else {
			for (let y = 0; y < height; y++) {
				for (let x = 0; x < width; x++) {
					pixels[y * width + x] =
						(x * 17 + y * 31 + frame * 13 + ((x * y) >> 3)) & (colorCount - 1);
				}
			}
		}
		frames.push(pixels);
	}
	return {
		name,
		width,
		height,
		frameCount,
		palette,
		frames,
		flatFrames: flatten(frames),
	};
}

function toRgbaFixture(indexed) {
	const frames = indexed.frames.map((source) => {
		const rgba = new Uint8Array(source.length * 4);
		for (let pixel = 0; pixel < source.length; pixel++) {
			const color = indexed.palette[source[pixel]];
			const offset = pixel * 4;
			rgba[offset] = (color >> 16) & 255;
			rgba[offset + 1] = (color >> 8) & 255;
			rgba[offset + 2] = color & 255;
			rgba[offset + 3] = 255;
		}
		return rgba;
	});
	return { ...indexed, frames, flatFrames: flatten(frames) };
}

function encodeIndexedOmggif(fixture) {
	const output = new Uint8Array(fixture.flatFrames.length * 2 + 4096);
	const writer = new OmgGifWriter(output, fixture.width, fixture.height, {
		palette: fixture.palette,
		loop: 0,
	});
	for (const frame of fixture.frames) {
		writer.addFrame(0, 0, fixture.width, fixture.height, frame, { delay: 2 });
	}
	return output.slice(0, writer.end());
}

function indexRgba(fixture) {
	const colorToIndex = new Map(
		fixture.palette.map((color, index) => [color, index]),
	);
	const indexed = new Uint8Array(fixture.flatFrames.length / 4);
	for (let pixel = 0; pixel < indexed.length; pixel++) {
		const offset = pixel * 4;
		const color =
			(fixture.flatFrames[offset] << 16) |
			(fixture.flatFrames[offset + 1] << 8) |
			fixture.flatFrames[offset + 2];
		indexed[pixel] = colorToIndex.get(color);
	}
	return {
		...fixture,
		frames: Array.from({ length: fixture.frameCount }, (_, frame) =>
			indexed.subarray(
				frame * fixture.width * fixture.height,
				(frame + 1) * fixture.width * fixture.height,
			),
		),
		flatFrames: indexed,
	};
}

function encodedResult(bytes) {
	return { bytes, checksum: bytes[bytes.length - 1] ?? 0 };
}

function encodeIndexedWtfgif(fixture, delta, compression) {
	return encodeIndexedGifFrames({
		width: fixture.width,
		height: fixture.height,
		frames: fixture.flatFrames,
		frameCount: fixture.frameCount,
		palette: fixture.palette,
		delay: 2,
		loop: 0,
		delta,
		compression,
		backend: "native-addon",
	});
}

function encodeRgbaWtfgif(fixture, delta, compression) {
	return encodeRgbaGifFrames({
		width: fixture.width,
		height: fixture.height,
		frames: fixture.flatFrames,
		frameCount: fixture.frameCount,
		palette: fixture.palette,
		delay: 2,
		loop: 0,
		delta,
		compression,
		backend: "native-addon",
	});
}

function assertEncodedParity(bytes, rgbaFixture, label) {
	const decoded = decodeAllOmggif(bytes);
	const expected = rgbaFixture.flatFrames;
	if (
		decoded.width !== rgbaFixture.width ||
		decoded.height !== rgbaFixture.height ||
		decoded.frameCount !== rgbaFixture.frameCount ||
		decoded.bytes.length !== expected.length
	) {
		throw new Error(`${label}: encoded dimensions differ`);
	}
	for (let index = 0; index < expected.length; index++) {
		if (decoded.bytes[index] !== expected[index]) {
			const frameBytes = rgbaFixture.width * rgbaFixture.height * 4;
			throw new Error(
				`${label}: frame ${Math.floor(index / frameBytes)}, byte ${index % frameBytes} differs`,
			);
		}
	}
}

function formatMs(value) {
	return value.toFixed(3).padStart(8);
}

function formatRatio(value) {
	return `${value.toFixed(2)}x`.padStart(8);
}

function printDecodeRows(rows) {
	console.log(
		"\nGIF -> all owned composited RGBA frames (fresh reader + output)",
	);
	console.log(
		"fixture".padEnd(38),
		"omggif".padStart(8),
		"wtf JS".padStart(8),
		"speedup".padStart(8),
		"wtf Wasm".padStart(8),
		"speedup".padStart(8),
		"wtf native".padStart(10),
		"speedup".padStart(8),
	);
	for (const row of rows) {
		console.log(
			row.file.padEnd(38),
			formatMs(row.omg.median),
			formatMs(row.js.median),
			formatRatio(row.omg.median / row.js.median),
			formatMs(row.wasm.median),
			formatRatio(row.omg.median / row.wasm.median),
			formatMs(row.native.median),
			formatRatio(row.omg.median / row.native.median),
		);
	}
}

function printEncodeRows(title, rows) {
	console.log(`\n${title}`);
	console.log(
		"fixture".padEnd(29),
		"omggif".padStart(8),
		"wtfgif".padStart(8),
		"speedup".padStart(8),
		"size".padStart(9),
	);
	for (const row of rows) {
		console.log(
			row.name.padEnd(29),
			formatMs(row.omg.median),
			formatMs(row.wtf.median),
			formatRatio(row.omg.median / row.wtf.median),
			`${row.wtfBytes}/${row.omgBytes}`.padStart(9),
		);
	}
}

console.log(
	`In-process diagnostic benchmark: ${iterations} samples, ${warmups} explicit warmups, no result reuse (use npm run bench for true fresh-process cold jobs)`,
);

const decodeRows = [];
for (const file of readdirSync(gifsDir)
	.filter((file) => file.endsWith(".gif"))
	.toSorted()) {
	const data = readFileSync(join(gifsDir, file));
	assertDecodeParity(data, file, "javascript");
	assertDecodeParity(data, file, "wasm");
	assertNativeAddonDecodeParity(data, file);
	decodeRows.push({
		file,
		omg: measure(() => decodeAllOmggif(data)),
		js: measure(() => decodeAllWtfgif(data, "javascript")),
		wasm: measure(() => decodeAllWtfgif(data, "wasm")),
		native: measure(() => decodeAllNativeAddon(data)),
	});
}
printDecodeRows(decodeRows);

const indexedFixtures = [
	makeIndexedFixture("4 colors", 4),
	makeIndexedFixture("16 colors", 16),
	makeIndexedFixture("256 colors", 256),
];
const rgbaFixtures = indexedFixtures.map(toRgbaFixture);
const deltaIndexed = makeIndexedFixture("4 colors, changed rects", 4, true);
const deltaRgba = toRgbaFixture(deltaIndexed);

const indexedRows = [];
for (const fixture of indexedFixtures) {
	const rgba = toRgbaFixture(fixture);
	const omgBytes = encodeIndexedOmggif(fixture);
	const wtfBytes = encodeIndexedWtfgif(fixture, false, "fast");
	assertEncodedParity(omgBytes, rgba, `${fixture.name}/omggif`);
	assertEncodedParity(wtfBytes, rgba, `${fixture.name}/wtfgif fast`);
	indexedRows.push({
		name: fixture.name,
		omg: measure(() => encodedResult(encodeIndexedOmggif(fixture))),
		wtf: measure(() =>
			encodedResult(encodeIndexedWtfgif(fixture, false, "fast")),
		),
		omgBytes: omgBytes.length,
		wtfBytes: wtfBytes.length,
	});
}
printEncodeRows(
	"Indexed frames -> GIF (pixel-perfect fast compression)",
	indexedRows,
);

const deltaOmgBytes = encodeIndexedOmggif(deltaIndexed);
const deltaWtfBytes = encodeIndexedWtfgif(deltaIndexed, true, "balanced");
assertEncodedParity(deltaOmgBytes, deltaRgba, "indexed delta/omggif");
assertEncodedParity(deltaWtfBytes, deltaRgba, "indexed delta/wtfgif");
printEncodeRows("Indexed changed rectangles -> GIF (pixel-perfect)", [
	{
		name: deltaIndexed.name,
		omg: measure(() => encodedResult(encodeIndexedOmggif(deltaIndexed))),
		wtf: measure(() =>
			encodedResult(encodeIndexedWtfgif(deltaIndexed, true, "balanced")),
		),
		omgBytes: deltaOmgBytes.length,
		wtfBytes: deltaWtfBytes.length,
	},
]);

const rgbaRows = [];
for (const fixture of rgbaFixtures) {
	const omgFixture = indexRgba(fixture);
	const omgBytes = encodeIndexedOmggif(omgFixture);
	const wtfBytes = encodeRgbaWtfgif(fixture, false, "fast");
	assertEncodedParity(omgBytes, fixture, `${fixture.name} RGBA/omggif`);
	assertEncodedParity(wtfBytes, fixture, `${fixture.name} RGBA/wtfgif fast`);
	rgbaRows.push({
		name: fixture.name,
		omg: measure(() => encodedResult(encodeIndexedOmggif(indexRgba(fixture)))),
		wtf: measure(() => encodedResult(encodeRgbaWtfgif(fixture, false, "fast"))),
		omgBytes: omgBytes.length,
		wtfBytes: wtfBytes.length,
	});
}
printEncodeRows(
	"RGBA frames -> GIF (pixel-perfect fast compression)",
	rgbaRows,
);

const deltaRgbaOmgFixture = indexRgba(deltaRgba);
const deltaRgbaOmgBytes = encodeIndexedOmggif(deltaRgbaOmgFixture);
const deltaRgbaWtfBytes = encodeRgbaWtfgif(deltaRgba, true, "balanced");
assertEncodedParity(deltaRgbaOmgBytes, deltaRgba, "RGBA delta/omggif");
assertEncodedParity(deltaRgbaWtfBytes, deltaRgba, "RGBA delta/wtfgif");
printEncodeRows("RGBA changed rectangles -> GIF (pixel-perfect)", [
	{
		name: deltaRgba.name,
		omg: measure(() =>
			encodedResult(encodeIndexedOmggif(indexRgba(deltaRgba))),
		),
		wtf: measure(() =>
			encodedResult(encodeRgbaWtfgif(deltaRgba, true, "balanced")),
		),
		omgBytes: deltaRgbaOmgBytes.length,
		wtfBytes: deltaRgbaWtfBytes.length,
	},
]);

console.log(`\nbenchmark sink: ${sink}`);
