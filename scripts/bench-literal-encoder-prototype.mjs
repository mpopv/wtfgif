import { createRequire } from "node:module";
import { performance } from "node:perf_hooks";

const require = createRequire(import.meta.url);
const { GifReader, GifWriter } = require("omggif");
const wasm = require("../crates/wtfgif-core/pkg/wtfgif_core.js");
const iterations = Number(process.env.BENCH_ITERATIONS ?? 15);
const warmups = Number(process.env.BENCH_WARMUP_ITERATIONS ?? 0);
let sink = 0;

function makeFixture(name, width, height, frameCount, colorCount) {
	const palette = Array.from({ length: colorCount }, (_, index) => {
		const red = (index * 73) & 255;
		const green = (index * 151) & 255;
		const blue = (index * 199) & 255;
		return (red << 16) | (green << 8) | blue;
	});
	const framePixels = width * height;
	const frames = new Uint8Array(framePixels * frameCount);
	for (let frame = 0; frame < frameCount; frame++) {
		for (let y = 0; y < height; y++) {
			for (let x = 0; x < width; x++) {
				frames[frame * framePixels + y * width + x] =
					(x * 17 + y * 31 + frame * 13 + ((x * y) >> 3)) & (colorCount - 1);
			}
		}
	}
	return { name, width, height, frameCount, palette, frames };
}

function encodeOmggif(fixture) {
	const output = new Uint8Array(fixture.frames.length * 2 + 4096);
	const writer = new GifWriter(output, fixture.width, fixture.height, {
		palette: fixture.palette,
		loop: 0,
	});
	const framePixels = fixture.width * fixture.height;
	for (let frame = 0; frame < fixture.frameCount; frame++) {
		writer.addFrame(
			0,
			0,
			fixture.width,
			fixture.height,
			fixture.frames.subarray(frame * framePixels, (frame + 1) * framePixels),
			{ delay: 2 },
		);
	}
	return output.slice(0, writer.end());
}

function encodeWasm(fixture, literal) {
	const fn = literal
		? wasm.encode_indexed_literal_gif
		: wasm.encode_indexed_gif;
	return fn(
		fixture.frames,
		fixture.width,
		fixture.height,
		fixture.frameCount,
		Uint32Array.from(fixture.palette),
		2,
		0,
	);
}

function rgbaFixture(fixture) {
	const rgba = new Uint8Array(fixture.frames.length * 4);
	for (let pixel = 0; pixel < fixture.frames.length; pixel++) {
		const color = fixture.palette[fixture.frames[pixel]];
		const offset = pixel * 4;
		rgba[offset] = (color >> 16) & 255;
		rgba[offset + 1] = (color >> 8) & 255;
		rgba[offset + 2] = color & 255;
		rgba[offset + 3] = 255;
	}
	return { ...fixture, rgba };
}

function encodeRgbaOmggif(fixture) {
	const indexed = new Uint8Array(fixture.rgba.length / 4);
	const colorToIndex = new Map(
		fixture.palette.map((color, index) => [color, index]),
	);
	for (let pixel = 0; pixel < indexed.length; pixel++) {
		const offset = pixel * 4;
		const color =
			(fixture.rgba[offset] << 16) |
			(fixture.rgba[offset + 1] << 8) |
			fixture.rgba[offset + 2];
		indexed[pixel] = colorToIndex.get(color);
	}
	return encodeOmggif({ ...fixture, frames: indexed });
}

function encodeRgbaWasm(fixture, literal) {
	const fn = literal ? wasm.encode_rgba_literal_gif : wasm.encode_rgba_gif;
	return literal
		? fn(
				fixture.rgba,
				fixture.width,
				fixture.height,
				fixture.frameCount,
				Uint32Array.from(fixture.palette),
				2,
				0,
			)
		: fn(
				fixture.rgba,
				fixture.width,
				fixture.height,
				fixture.frameCount,
				Uint32Array.from(fixture.palette),
				2,
				0,
				false,
			);
}

function assertPixelPerfect(encoded, fixture, label) {
	const reader = new GifReader(encoded);
	if (
		reader.width !== fixture.width ||
		reader.height !== fixture.height ||
		reader.numFrames() !== fixture.frameCount
	) {
		throw new Error(
			`${fixture.name}/${label}: dimensions or frame count differ`,
		);
	}
	const framePixels = fixture.width * fixture.height;
	const rgba = new Uint8Array(framePixels * 4);
	for (let frame = 0; frame < fixture.frameCount; frame++) {
		rgba.fill(0);
		reader.decodeAndBlitFrameRGBA(frame, rgba);
		const expected = fixture.frames.subarray(
			frame * framePixels,
			(frame + 1) * framePixels,
		);
		for (let pixel = 0; pixel < framePixels; pixel++) {
			const color = fixture.palette[expected[pixel]];
			const offset = pixel * 4;
			if (
				rgba[offset] !== ((color >> 16) & 255) ||
				rgba[offset + 1] !== ((color >> 8) & 255) ||
				rgba[offset + 2] !== (color & 255) ||
				rgba[offset + 3] !== 255
			) {
				throw new Error(
					`${fixture.name}/${label}: frame ${frame}, pixel ${pixel} differs`,
				);
			}
		}
	}
}

function median(values) {
	return values.toSorted((a, b) => a - b)[Math.floor(values.length / 2)];
}

function measure(fn) {
	for (let iteration = 0; iteration < warmups; iteration++) {
		const result = fn();
		sink ^= result[result.length - 1] ?? 0;
	}
	const samples = [];
	for (let iteration = 0; iteration < iterations; iteration++) {
		const start = performance.now();
		const result = fn();
		samples.push(performance.now() - start);
		sink ^= result[result.length - 1] ?? 0;
	}
	return median(samples);
}

const fixtures = [
	makeFixture("4 colors, 128x128x12", 128, 128, 12, 4),
	makeFixture("16 colors, 128x128x12", 128, 128, 12, 16),
	makeFixture("256 colors, 128x128x12", 128, 128, 12, 256),
	makeFixture("256 colors, 512x512x4", 512, 512, 4, 256),
];

console.log(
	"fixture\tomggif ms\tstandard ms\tliteral ms\tliteral speedup\tstandard bytes\tliteral bytes\tsize ratio",
);
for (const fixture of fixtures) {
	const omgOutput = encodeOmggif(fixture);
	const standardOutput = encodeWasm(fixture, false);
	const literalOutput = encodeWasm(fixture, true);
	assertPixelPerfect(omgOutput, fixture, "omggif");
	assertPixelPerfect(standardOutput, fixture, "standard");
	assertPixelPerfect(literalOutput, fixture, "literal");

	const omgMs = measure(() => encodeOmggif(fixture));
	const standardMs = measure(() => encodeWasm(fixture, false));
	const literalMs = measure(() => encodeWasm(fixture, true));
	console.log(
		[
			fixture.name,
			omgMs.toFixed(3),
			standardMs.toFixed(3),
			literalMs.toFixed(3),
			`${(omgMs / literalMs).toFixed(2)}x`,
			standardOutput.length,
			literalOutput.length,
			`${(literalOutput.length / standardOutput.length).toFixed(2)}x`,
		].join("\t"),
	);
}

console.log(
	"\nRGBA fixture\tomggif ms\tstandard ms\tliteral ms\tliteral speedup\tliteral bytes",
);
for (const source of fixtures) {
	const fixture = rgbaFixture(source);
	const omgOutput = encodeRgbaOmggif(fixture);
	const standardOutput = encodeRgbaWasm(fixture, false);
	const literalOutput = encodeRgbaWasm(fixture, true);
	assertPixelPerfect(omgOutput, fixture, "RGBA omggif");
	assertPixelPerfect(standardOutput, fixture, "RGBA standard");
	assertPixelPerfect(literalOutput, fixture, "RGBA literal");
	const omgMs = measure(() => encodeRgbaOmggif(fixture));
	const standardMs = measure(() => encodeRgbaWasm(fixture, false));
	const literalMs = measure(() => encodeRgbaWasm(fixture, true));
	console.log(
		[
			fixture.name,
			omgMs.toFixed(3),
			standardMs.toFixed(3),
			literalMs.toFixed(3),
			`${(omgMs / literalMs).toFixed(2)}x`,
			literalOutput.length,
		].join("\t"),
	);
}
console.log(`sink=${sink}`);
