import { createRequire } from "node:module";
import sharp from "sharp";
import * as wtfgif from "../dist/index.mjs";
import {
	ALPHA_THRESHOLD,
	loadBenchmarkCorpus,
	sha256,
} from "./benchmark/corpus.mjs";
import {
	decodeCompositedGif,
	validateAndMeasure,
} from "./benchmark/metrics.mjs";

const require = createRequire(import.meta.url);
const scalarWasm = require("../crates/wtfgif-core/pkg/wtfgif_core.js");
const simdWasm = require("../crates/wtfgif-core/pkg-simd/wtfgif_core.js");
const forbiddenExports = [
	"encode_indexed_lzw",
	"encode_indexed_gif",
	"encode_indexed_delta_gif",
	"encode_rgba_gif",
];

function encode(value) {
	return wtfgif.encodeRgbaGifFrames({
		alphaThreshold: ALPHA_THRESHOLD,
		delay: value.delay,
		delta: value.delta,
		frameCount: value.frameCount,
		frames: value.rgba,
		height: value.height,
		loop: 0,
		width: value.width,
	});
}

function assertBytesEqual(actual, expected, label) {
	if (actual.length !== expected.length) {
		throw new Error(
			`${label}: byte lengths differ (${actual.length} versus ${expected.length})`,
		);
	}
	for (let index = 0; index < actual.length; index += 1) {
		if (actual[index] !== expected[index]) {
			throw new Error(
				`${label}: byte ${index} differs (${actual[index]} versus ${expected[index]})`,
			);
		}
	}
}

async function validateSharp(bytes, value, expectedPixels) {
	const image = sharp(bytes, { animated: true, pages: -1 });
	const metadata = await image.metadata();
	const { data, info } = await image
		.ensureAlpha()
		.raw()
		.toBuffer({ resolveWithObject: true });
	if (
		info.width !== value.width ||
		info.pageHeight !== value.height ||
		info.pages !== value.frameCount
	) {
		throw new Error(
			`${value.id}: Sharp/libvips decoded the wrong animation shape`,
		);
	}
	if (metadata.delay?.length !== value.frameCount) {
		throw new Error(`${value.id}: Sharp/libvips decoded the wrong delay count`);
	}
	for (let frame = 0; frame < value.frameCount; frame += 1) {
		const expectedDelay =
			(typeof value.delay === "number" ? value.delay : value.delay[frame]) * 10;
		if (metadata.delay[frame] !== expectedDelay) {
			throw new Error(
				`${value.id}: Sharp/libvips decoded delay ${metadata.delay[frame]}ms instead of ${expectedDelay}ms`,
			);
		}
	}
	assertBytesEqual(
		data,
		expectedPixels,
		`${value.id} Sharp/libvips versus omggif compositing`,
	);
}

for (const module of [scalarWasm, simdWasm]) {
	for (const name of forbiddenExports) {
		if (name in module)
			throw new Error(`obsolete dictionary encoder export remains: ${name}`);
	}
}

const corpus = loadBenchmarkCorpus();
const encodedByVariant = new Map();
for (const [variant, module] of [
	["scalar", scalarWasm],
	["simd", simdWasm],
]) {
	wtfgif.setWasmCoreModule(module);
	for (const value of corpus) {
		const bytes = encode(value);
		validateAndMeasure(bytes, value, ALPHA_THRESHOLD);
		const decoded = decodeCompositedGif(bytes);
		await validateSharp(bytes, value, decoded.pixels);
		encodedByVariant.set(`${variant}:${value.id}`, bytes);
	}
}

for (const value of corpus) {
	const scalar = encodedByVariant.get(`scalar:${value.id}`);
	const simd = encodedByVariant.get(`simd:${value.id}`);
	assertBytesEqual(simd, scalar, `${value.id} SIMD versus scalar output`);
}

console.log(
	`Independent decoder conformance passed: ${corpus.length} fixtures x scalar/SIMD through omggif and Sharp/libvips (${sha256(Buffer.concat([...encodedByVariant.values()].map((bytes) => Buffer.from(bytes)))).slice(0, 16)})`,
);
