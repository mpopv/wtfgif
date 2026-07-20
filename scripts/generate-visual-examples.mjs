import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { encodeRgbaGifFrames, setWasmCoreModule } from "../dist/index.mjs";

const require = createRequire(import.meta.url);
const { GifReader } = require("omggif");
const wasm = require("../crates/wtfgif-core/pkg/wtfgif_core.js");
setWasmCoreModule(wasm);

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const sourcePath = join(
	root,
	"test",
	"gifs",
	"Disappear Homer Simpson GIF.gif",
);
const outputDir = join(root, "artifacts", "visual-examples");
const reencodedPath = join(outputDir, "homer-fast-pixel-perfect.gif");
mkdirSync(outputDir, { recursive: true });

function clearFrameRect(canvas, canvasWidth, info) {
	for (let y = info.y; y < info.y + info.height; y++) {
		canvas.fill(
			0,
			(y * canvasWidth + info.x) * 4,
			(y * canvasWidth + info.x + info.width) * 4,
		);
	}
}

function decodeComposited(data) {
	const reader = new GifReader(data);
	const frameBytes = reader.width * reader.height * 4;
	const canvas = new Uint8Array(frameBytes);
	const restore = new Uint8Array(frameBytes);
	const frames = new Uint8Array(frameBytes * reader.numFrames());
	const delays = new Uint16Array(reader.numFrames());
	for (let frame = 0; frame < reader.numFrames(); frame++) {
		const info = reader.frameInfo(frame);
		delays[frame] = info.delay;
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
		width: reader.width,
		height: reader.height,
		frameCount: reader.numFrames(),
		loop: reader.loopCount(),
		delays,
		frames,
	};
}

function exactPalette(rgba) {
	const colors = new Set();
	for (let offset = 0; offset < rgba.length; offset += 4) {
		if (rgba[offset + 3] !== 255) {
			throw new Error("Visual proof fixture is not fully opaque.");
		}
		colors.add(
			(rgba[offset] << 16) | (rgba[offset + 1] << 8) | rgba[offset + 2],
		);
	}
	if (colors.size > 256) {
		throw new Error(`Visual proof fixture has ${colors.size} colors.`);
	}
	return [...colors];
}

const sourceData = readFileSync(sourcePath);
const source = decodeComposited(sourceData);
const palette = exactPalette(source.frames);
const reencoded = encodeRgbaGifFrames({
	width: source.width,
	height: source.height,
	frameCount: source.frameCount,
	frames: source.frames,
	palette,
	delay: source.delays,
	loop: source.loop,
	compression: "fast",
	backend: "native",
});
const decoded = decodeComposited(reencoded);
if (
	decoded.width !== source.width ||
	decoded.height !== source.height ||
	decoded.frameCount !== source.frameCount ||
	decoded.frames.length !== source.frames.length
) {
	throw new Error("Reencoded visual proof dimensions differ.");
}
for (let index = 0; index < source.frames.length; index++) {
	if (decoded.frames[index] !== source.frames[index]) {
		throw new Error(`Reencoded visual proof differs at RGBA byte ${index}.`);
	}
}

writeFileSync(reencodedPath, reencoded);
console.log(
	JSON.stringify(
		{
			sourcePath,
			reencodedPath,
			width: source.width,
			height: source.height,
			frames: source.frameCount,
			colors: palette.length,
			sourceBytes: sourceData.length,
			reencodedBytes: reencoded.length,
			rgbaBytesCompared: source.frames.length,
			differingBytes: 0,
		},
		null,
		2,
	),
);
