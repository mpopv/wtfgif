import { readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { performance } from "node:perf_hooks";
import {
	decodeGifFramesRgba,
	encodeRgbaGifFrames,
	reencodeGifPixelPerfect,
	setNativeAddonModule,
} from "../../dist/index.mjs";
import { median } from "../bench/lib/metrics.mjs";
import { root } from "../lib/paths.mjs";

const require = createRequire(import.meta.url);
const { GifReader, GifWriter } = require("omggif");
const native = require("../../native/build/wtfgif_native.node");
setNativeAddonModule(native);
const gifsDir = join(root, "test", "gifs");
const iterations = Number(process.env.BENCH_ITERATIONS ?? 15);
const warmups = Number(process.env.BENCH_WARMUP_ITERATIONS ?? 0);
const fixtureFilter = process.env.BENCH_FILTER ?? "";
const decodeNativeOnly = process.env.BENCH_DECODE_NATIVE_ONLY === "1";
let sink = 0;

function clearFrameRect(canvas, canvasWidth, info) {
	for (let y = info.y; y < info.y + info.height; y++) {
		canvas.fill(
			0,
			(y * canvasWidth + info.x) * 4,
			(y * canvasWidth + info.x + info.width) * 4,
		);
	}
}

function decodeAllWithOmggif(data) {
	const reader = new GifReader(data);
	const frameBytes = reader.width * reader.height * 4;
	const canvas = new Uint8Array(frameBytes);
	const output = new Uint8Array(frameBytes * reader.numFrames());
	const restore = new Uint8Array(frameBytes);
	for (let frame = 0; frame < reader.numFrames(); frame++) {
		const info = reader.frameInfo(frame);
		if (info.disposal === 3) {
			restore.set(canvas);
		}
		reader.decodeAndBlitFrameRGBA(frame, canvas);
		output.set(canvas, frame * frameBytes);
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
		pixels: output,
	};
}

function assertEqual(actual, expected, file) {
	for (const key of ["width", "height", "frameCount"]) {
		if (actual[key] !== expected[key]) {
			throw new Error(`${file}: ${key} differs`);
		}
	}
	if (
		actual.pixels.length !== expected.pixels.length ||
		actual.pixels.some((byte, index) => byte !== expected.pixels[index])
	) {
		throw new Error(`${file}: decoded RGBA bytes differ`);
	}
}

function measure(operation) {
	for (let iteration = 0; iteration < warmups; iteration++) {
		const result = operation();
		const bytes = result.pixels ?? result;
		sink ^= bytes[bytes.length - 1] ?? 0;
	}
	const samples = [];
	for (let iteration = 0; iteration < iterations; iteration++) {
		const start = performance.now();
		const result = operation();
		samples.push(performance.now() - start);
		const bytes = result.pixels ?? result;
		sink ^= bytes[bytes.length - 1] ?? 0;
	}
	return median(samples);
}

console.log(
	"fixture\tframes\tpixels\tomggif ms\tnative ms\tspeedup\tparse/decode/compose ms",
);
const gifFixtures = [];
for (const file of readdirSync(gifsDir)
	.filter((file) => file.endsWith(".gif"))
	.filter((file) => file.includes(fixtureFilter))
	.toSorted()) {
	const data = readFileSync(join(gifsDir, file));
	const expected = decodeAllWithOmggif(data);
	gifFixtures.push({ file, data, expected });
	const actual = decodeGifFramesRgba(data);
	assertEqual(actual, expected, file);
	const omgMs = decodeNativeOnly
		? Number.NaN
		: measure(() => decodeAllWithOmggif(data));
	const nativeMs = measure(() => decodeGifFramesRgba(data));
	console.log(
		[
			file,
			expected.frameCount,
			expected.width * expected.height * expected.frameCount,
			decodeNativeOnly ? "-" : omgMs.toFixed(3),
			nativeMs.toFixed(3),
			decodeNativeOnly ? "-" : `${(omgMs / nativeMs).toFixed(2)}x`,
			"-",
		].join("\t"),
	);
}
if (process.env.BENCH_DECODE_ONLY === "1") {
	console.log(`sink=${sink}`);
	process.exit(0);
}

function reencodeWithOmggif(data) {
	const reader = new GifReader(data);
	let totalFramePixels = 0;
	for (let frame = 0; frame < reader.numFrames(); frame++) {
		const info = reader.frameInfo(frame);
		totalFramePixels += info.width * info.height;
	}
	const output = new Uint8Array(totalFramePixels * 2 + data.length + 4096);
	const writer = new GifWriter(output, reader.width, reader.height, {
		loop: reader.loopCount(),
	});
	const rgba = new Uint8Array(reader.width * reader.height * 4);
	const paletteCache = new Map();
	for (let frame = 0; frame < reader.numFrames(); frame++) {
		const info = reader.frameInfo(frame);
		const paletteKey = `${info.palette_offset}:${info.palette_size}`;
		let paletteInfo = paletteCache.get(paletteKey);
		if (!paletteInfo) {
			const palette = new Array(info.palette_size);
			const colorToIndex = new Map();
			for (let index = 0; index < info.palette_size; index++) {
				const offset = info.palette_offset + index * 3;
				const color =
					(data[offset] << 16) | (data[offset + 1] << 8) | data[offset + 2];
				palette[index] = color;
				if (!colorToIndex.has(color)) {
					colorToIndex.set(color, index);
				}
			}
			paletteInfo = { palette, colorToIndex };
			paletteCache.set(paletteKey, paletteInfo);
		}

		rgba.fill(0);
		reader.decodeAndBlitFrameRGBA(frame, rgba);
		const indices = new Uint8Array(info.width * info.height);
		let outputIndex = 0;
		for (let y = 0; y < info.height; y++) {
			let offset = ((info.y + y) * reader.width + info.x) * 4;
			for (let x = 0; x < info.width; x++) {
				if (rgba[offset + 3] === 0 && info.transparent_index !== null) {
					indices[outputIndex++] = info.transparent_index;
				} else {
					const color =
						(rgba[offset] << 16) | (rgba[offset + 1] << 8) | rgba[offset + 2];
					const index = paletteInfo.colorToIndex.get(color);
					if (index === undefined) {
						throw new Error("omggif decoded a color outside its frame palette");
					}
					indices[outputIndex++] = index;
				}
				offset += 4;
			}
		}
		writer.addFrame(info.x, info.y, info.width, info.height, indices, {
			palette: paletteInfo.palette,
			delay: info.delay,
			disposal: info.disposal,
			transparent: info.transparent_index,
		});
	}
	return output.slice(0, writer.end());
}

console.log(
	"\nGIF -> freshly LZW-reencoded GIF (per-frame palettes; decoded-byte parity required)",
);
console.log("fixture\tomggif ms\tnative ms\tspeedup\tnative/source bytes");
for (const { file, data, expected } of gifFixtures) {
	const baseline = reencodeWithOmggif(data);
	const actual = reencodeGifPixelPerfect(data);
	assertEqual(
		decodeAllWithOmggif(baseline),
		expected,
		`${file}/omggif reencode`,
	);
	assertEqual(decodeAllWithOmggif(actual), expected, `${file}/native reencode`);
	const omgMs = measure(() => reencodeWithOmggif(data));
	const nativeMs = measure(() => reencodeGifPixelPerfect(data));
	console.log(
		[
			file,
			omgMs.toFixed(3),
			nativeMs.toFixed(3),
			`${(omgMs / nativeMs).toFixed(2)}x`,
			`${actual.length}/${data.length}`,
		].join("\t"),
	);
}

function makeRgbaFixture(colorCount) {
	const width = 128;
	const height = 128;
	const frameCount = 12;
	const palette = Uint32Array.from({ length: colorCount }, (_, index) => {
		const red = (index * 73) & 255;
		const green = (index * 151) & 255;
		const blue = (index * 199) & 255;
		return (red << 16) | (green << 8) | blue;
	});
	const pixelsPerFrame = width * height;
	const indexed = new Uint8Array(pixelsPerFrame * frameCount);
	const rgba = Buffer.allocUnsafe(indexed.length * 4);
	for (let frame = 0; frame < frameCount; frame++) {
		for (let y = 0; y < height; y++) {
			for (let x = 0; x < width; x++) {
				const pixel = frame * pixelsPerFrame + y * width + x;
				const index =
					(x * 17 + y * 31 + frame * 13 + ((x * y) >> 3)) & (colorCount - 1);
				const color = palette[index];
				indexed[pixel] = index;
				rgba[pixel * 4] = (color >> 16) & 255;
				rgba[pixel * 4 + 1] = (color >> 8) & 255;
				rgba[pixel * 4 + 2] = color & 255;
				rgba[pixel * 4 + 3] = 255;
			}
		}
	}
	return {
		width,
		height,
		frameCount,
		palette,
		delays: new Uint16Array(frameCount).fill(2),
		indexed,
		rgba,
	};
}

function encodeRgbaWithOmggif(fixture) {
	const colorToIndex = new Map(
		Array.from(fixture.palette, (color, index) => [color, index]),
	);
	const indexed = new Uint8Array(fixture.indexed.length);
	for (let pixel = 0; pixel < indexed.length; pixel++) {
		const offset = pixel * 4;
		const color =
			(fixture.rgba[offset] << 16) |
			(fixture.rgba[offset + 1] << 8) |
			fixture.rgba[offset + 2];
		indexed[pixel] = colorToIndex.get(color);
	}
	const output = new Uint8Array(indexed.length * 2 + 4096);
	const writer = new GifWriter(output, fixture.width, fixture.height, {
		palette: Array.from(fixture.palette),
		loop: 0,
	});
	const frameLength = fixture.width * fixture.height;
	for (let frame = 0; frame < fixture.frameCount; frame++) {
		writer.addFrame(
			0,
			0,
			fixture.width,
			fixture.height,
			indexed.subarray(frame * frameLength, (frame + 1) * frameLength),
			{ delay: 2 },
		);
	}
	return output.slice(0, writer.end());
}

function encodeRgbaNative(fixture) {
	return encodeRgbaGifFrames({
		width: fixture.width,
		height: fixture.height,
		frameCount: fixture.frameCount,
		frames: fixture.rgba,
		palette: Array.from(fixture.palette),
		delay: fixture.delays,
		loop: 0,
		backend: "native-addon",
	});
}

function assertEncodedParity(bytes, fixture, label) {
	const actual = decodeAllWithOmggif(bytes);
	if (
		actual.width !== fixture.width ||
		actual.height !== fixture.height ||
		actual.frameCount !== fixture.frameCount ||
		actual.pixels.length !== fixture.rgba.length ||
		actual.pixels.some((byte, index) => byte !== fixture.rgba[index])
	) {
		throw new Error(`${label}: decoded RGBA bytes differ`);
	}
}

console.log(
	"\nRGBA frames -> GIF (fresh exact native encoder; decoded-byte parity required)",
);
console.log("colors\tomggif ms\tnative ms\tspeedup\tnative/omggif bytes");
for (const colorCount of [4, 16, 256]) {
	const fixture = makeRgbaFixture(colorCount);
	const expected = encodeRgbaWithOmggif(fixture);
	const actual = encodeRgbaNative(fixture);
	assertEncodedParity(expected, fixture, `${colorCount} colors/omggif`);
	assertEncodedParity(actual, fixture, `${colorCount} colors/native`);
	const omgMs = measure(() => encodeRgbaWithOmggif(fixture));
	const nativeMs = measure(() => encodeRgbaNative(fixture));
	console.log(
		[
			colorCount,
			omgMs.toFixed(3),
			nativeMs.toFixed(3),
			`${(omgMs / nativeMs).toFixed(2)}x`,
			`${actual.length}/${expected.length}`,
		].join("\t"),
	);
}
console.log(`sink=${sink}`);
