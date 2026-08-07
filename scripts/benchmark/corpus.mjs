import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const { GifReader } = require("omggif");
const root = dirname(dirname(dirname(fileURLToPath(import.meta.url))));

export const ALPHA_THRESHOLD = 179;

export function sha256(bytes) {
	return createHash("sha256").update(bytes).digest("hex");
}

function fixture({
	id,
	label,
	category,
	width,
	height,
	frameCount,
	rgba,
	delay = 10,
	delta = false,
	source,
}) {
	if (rgba.length !== width * height * frameCount * 4) {
		throw new Error(`${id}: RGBA byte length does not match its dimensions`);
	}
	return {
		id,
		label,
		category,
		width,
		height,
		frameCount,
		rgba,
		delay,
		delta,
		source,
	};
}

function generatedFixture({
	id,
	label,
	category,
	width,
	height,
	frameCount,
	draw,
	delta,
}) {
	const rgba = new Uint8Array(width * height * frameCount * 4);
	let offset = 0;
	for (let frame = 0; frame < frameCount; frame += 1) {
		for (let y = 0; y < height; y += 1) {
			for (let x = 0; x < width; x += 1) {
				const pixel = draw(x, y, frame);
				rgba[offset++] = pixel[0];
				rgba[offset++] = pixel[1];
				rgba[offset++] = pixel[2];
				rgba[offset++] = pixel[3];
			}
		}
	}
	return fixture({
		id,
		label,
		category,
		width,
		height,
		frameCount,
		rgba,
		delta,
		source: { kind: "deterministic-generator", version: 1 },
	});
}

function makePixelArt() {
	const palette = [
		[12, 16, 32],
		[245, 196, 48],
		[239, 71, 111],
		[17, 138, 178],
		[6, 214, 160],
		[255, 255, 255],
	];
	return generatedFixture({
		id: "pixel-art",
		label: "Pixel art",
		category: "flat-pixel-art",
		width: 64,
		height: 64,
		frameCount: 12,
		draw(x, y, frame) {
			const checker = ((x >> 3) + (y >> 3)) & 1;
			const spriteX = (frame * 4) % 48;
			const inSprite = x >= spriteX && x < spriteX + 16 && y >= 24 && y < 40;
			const color = inSprite
				? palette[2 + ((x + y + frame) & 3)]
				: palette[checker];
			return [...color, 255];
		},
	});
}

function makeGradient() {
	return generatedFixture({
		id: "gradient",
		label: "Smooth gradients",
		category: "gradients",
		width: 128,
		height: 128,
		frameCount: 8,
		draw(x, y, frame) {
			return [
				Math.round((x * 255) / 127),
				Math.round((y * 255) / 127),
				(x + y + frame * 19) & 255,
				255,
			];
		},
	});
}

function xorshift32(value) {
	value ^= value << 13;
	value ^= value >>> 17;
	value ^= value << 5;
	return value >>> 0;
}

function makeNoise() {
	return generatedFixture({
		id: "noise",
		label: "Deterministic noise",
		category: "random-noise",
		width: 128,
		height: 128,
		frameCount: 8,
		draw(x, y, frame) {
			let value = xorshift32(0x9e3779b9 ^ x ^ (y << 10) ^ (frame << 20));
			const red = value & 255;
			value = xorshift32(value);
			const green = value & 255;
			value = xorshift32(value);
			return [red, green, value & 255, 255];
		},
	});
}

function makeTransparency() {
	return generatedFixture({
		id: "transparency",
		label: "Transparent edges",
		category: "transparency-and-semitransparency",
		width: 128,
		height: 128,
		frameCount: 8,
		draw(x, y, frame) {
			const centerX = 40 + frame * 7;
			const centerY = 64;
			const distance = Math.hypot(x - centerX, y - centerY);
			const alpha =
				distance < 29
					? 255
					: distance < 34
						? Math.round((34 - distance) * 51)
						: 0;
			return [(x * 3 + frame * 17) & 255, (y * 5) & 255, 220, alpha];
		},
	});
}

function makeDisjointPalettes() {
	return generatedFixture({
		id: "disjoint-palettes",
		label: "Disjoint frame palettes",
		category: "cross-frame-color-distribution",
		width: 128,
		height: 128,
		frameCount: 8,
		draw(x, y, frame) {
			const band = ((x >> 3) + (y >> 3)) & 15;
			return [
				(frame * 31 + band * 13) & 255,
				(frame * 67 + band * 7) & 255,
				(frame * 101 + band * 17) & 255,
				255,
			];
		},
	});
}

function makeNearlyStatic() {
	return generatedFixture({
		id: "nearly-static",
		label: "Nearly static animation",
		category: "changed-rectangle",
		width: 128,
		height: 128,
		frameCount: 12,
		delta: true,
		draw(x, y, frame) {
			const moving = x >= frame * 8 && x < frame * 8 + 12 && y >= 56 && y < 68;
			const grid = x % 16 === 0 || y % 16 === 0;
			return moving
				? [255, 80, 40, 255]
				: grid
					? [40, 50, 70, 255]
					: [17, 24, 39, 255];
		},
	});
}

function makeTiny() {
	return generatedFixture({
		id: "tiny",
		label: "Tiny animation",
		category: "tiny",
		width: 16,
		height: 16,
		frameCount: 6,
		draw(x, y, frame) {
			return [(x * 17) & 255, (y * 17) & 255, (frame * 43) & 255, 255];
		},
	});
}

function makeLarge() {
	return generatedFixture({
		id: "large",
		label: "One-megapixel animation",
		category: "large-total-pixel-count",
		width: 512,
		height: 512,
		frameCount: 4,
		draw(x, y, frame) {
			const texture = ((x * 13 + y * 17 + frame * 29) ^ ((x * y) >>> 3)) & 31;
			return [
				(x + frame * 19 + texture) & 255,
				(y + texture * 2) & 255,
				(x + y + texture * 3) & 255,
				255,
			];
		},
	});
}

function makeStress() {
	return generatedFixture({
		id: "stress-3mp",
		label: "Three-megapixel stress animation",
		category: "multi-megapixel-stress",
		width: 1024,
		height: 1024,
		frameCount: 3,
		draw(x, y, frame) {
			const texture = ((x * 11 + y * 23 + frame * 47) ^ ((x * y) >>> 4)) & 63;
			return [
				(x + texture) & 255,
				(y + frame * 41) & 255,
				(x + y + texture) & 255,
				255,
			];
		},
	});
}

function makeMakeEmojiFixture() {
	const path = join(root, "test", "rgba", "makeemoji-128x128x8.rgba");
	const rgba = readFileSync(path);
	return fixture({
		id: "makeemoji-real",
		label: "MakeEmoji production sample",
		category: "real-small-images",
		width: 128,
		height: 128,
		frameCount: 8,
		rgba,
		source: {
			kind: "repository-fixture",
			path: relative(root, path),
			sha256: sha256(rgba),
		},
	});
}

function clearRect(canvas, screenWidth, info) {
	for (let y = info.y; y < info.y + info.height; y += 1) {
		canvas.fill(
			0,
			(y * screenWidth + info.x) * 4,
			(y * screenWidth + info.x + info.width) * 4,
		);
	}
}

function resizeNearest(source, sourceWidth, sourceHeight, width, height) {
	const output = new Uint8Array(width * height * 4);
	for (let y = 0; y < height; y += 1) {
		const sourceY = Math.min(
			sourceHeight - 1,
			Math.floor((y * sourceHeight) / height),
		);
		for (let x = 0; x < width; x += 1) {
			const sourceX = Math.min(
				sourceWidth - 1,
				Math.floor((x * sourceWidth) / width),
			);
			const from = (sourceY * sourceWidth + sourceX) * 4;
			output.set(source.subarray(from, from + 4), (y * width + x) * 4);
		}
	}
	return output;
}

function makePhotographicFixture() {
	const path = join(root, "test", "gifs", "Dramatic Chipmunk GIF.gif");
	const bytes = readFileSync(path);
	const reader = new GifReader(bytes);
	const frameCount = Math.min(8, reader.numFrames());
	const width = 128;
	const height = 96;
	const frameBytes = width * height * 4;
	const rgba = new Uint8Array(frameBytes * frameCount);
	const canvas = new Uint8Array(reader.width * reader.height * 4);
	const delays = new Uint16Array(frameCount);
	let previousInfo = null;
	let previousRestore = null;
	for (let frame = 0; frame < frameCount; frame += 1) {
		if (previousInfo?.disposal === 2)
			clearRect(canvas, reader.width, previousInfo);
		else if (previousInfo?.disposal === 3 && previousRestore)
			canvas.set(previousRestore);
		const info = reader.frameInfo(frame);
		const restore = info.disposal === 3 ? canvas.slice() : null;
		reader.decodeAndBlitFrameRGBA(frame, canvas);
		rgba.set(
			resizeNearest(canvas, reader.width, reader.height, width, height),
			frame * frameBytes,
		);
		delays[frame] = info.delay;
		previousInfo = info;
		previousRestore = restore;
	}
	return fixture({
		id: "photographic",
		label: "Photographic animation",
		category: "real-photographic-content",
		width,
		height,
		frameCount,
		rgba,
		delay: delays,
		source: {
			kind: "repository-regression-gif",
			path: relative(root, path),
			sha256: sha256(bytes),
			transform: "first 8 composited frames, nearest-neighbor resize to 128x96",
		},
	});
}

export function loadBenchmarkCorpus({ includeStress = false } = {}) {
	const corpus = [
		makeMakeEmojiFixture(),
		makePhotographicFixture(),
		makePixelArt(),
		makeGradient(),
		makeNoise(),
		makeTransparency(),
		makeDisjointPalettes(),
		makeNearlyStatic(),
		makeTiny(),
		makeLarge(),
	];
	if (includeStress) corpus.push(makeStress());
	return corpus;
}

export function corpusManifestEntry(value) {
	return {
		id: value.id,
		label: value.label,
		category: value.category,
		width: value.width,
		height: value.height,
		frameCount: value.frameCount,
		pixelCount: value.width * value.height * value.frameCount,
		delta: value.delta,
		rgbaSha256: sha256(value.rgba),
		source: value.source,
	};
}
