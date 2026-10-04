import { GifReader } from "omggif";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import {
	cleanupWasm,
	encodeRgbaGifFrames,
	initializeWasmGlobally,
} from "../src/encode";
import { compileGif } from "../src/index";

const WIDTH = 48;
const HEIGHT = 32;
const FRAME_BYTES = WIDTH * HEIGHT * 4;

beforeAll(async () => {
	await initializeWasmGlobally();
});

afterAll(() => {
	cleanupWasm();
});

/** Composite every frame the way browsers do, honoring disposal. */
function compositeFrames(gif: Uint8Array): Uint8Array[] {
	const reader = new GifReader(gif);
	const canvas = new Uint8Array(reader.width * reader.height * 4);
	const frames: Uint8Array[] = [];
	let previous: ReturnType<GifReader["frameInfo"]> | null = null;
	for (let frame = 0; frame < reader.numFrames(); frame += 1) {
		if (previous?.disposal === 2) {
			for (let y = previous.y; y < previous.y + previous.height; y += 1) {
				const start = (y * reader.width + previous.x) * 4;
				canvas.fill(0, start, start + previous.width * 4);
			}
		}
		reader.decodeAndBlitFrameRGBA(frame, canvas);
		frames.push(canvas.slice());
		previous = reader.frameInfo(frame);
	}
	return frames;
}

/**
 * A moving square over either an opaque grid or a fully transparent field.
 * (An opaque grid on a transparent field cannot be differenced: clearing the
 * square's old position restores the whole canvas, so every later frame
 * redraws the grid.)
 */
function movingSquare(frameCount: number, transparentBackground: boolean) {
	const frames: Uint8Array[] = [];
	for (let frame = 0; frame < frameCount; frame += 1) {
		const rgba = new Uint8Array(FRAME_BYTES);
		for (let y = 0; y < HEIGHT; y += 1) {
			for (let x = 0; x < WIDTH; x += 1) {
				const offset = (y * WIDTH + x) * 4;
				const inSquare =
					x >= frame * 4 && x < frame * 4 + 8 && y >= 10 && y < 18;
				const grid = !transparentBackground && (x % 8 === 0 || y % 8 === 0);
				const color = inSquare
					? [240, 80, 40, 255]
					: grid
						? [40, 60, 90, 255]
						: [10, 20, 30, transparentBackground ? 0 : 255];
				rgba.set(color, offset);
			}
		}
		frames.push(rgba);
	}
	return frames;
}

describe("compact quality output", () => {
	test.each([
		["opaque", false],
		["transparent", true],
	] as const)(
		"differenced %s frames composite like independent frames",
		(_name, transparentBackground) => {
			const frames = movingSquare(8, transparentBackground);
			const options = {
				width: WIDTH,
				height: HEIGHT,
				frames,
				delay: 5,
				loop: 0,
			};
			const differenced = encodeRgbaGifFrames(options);
			const independent = encodeRgbaGifFrames({
				...options,
				independentFrames: true,
			});
			expect(compositeFrames(differenced)).toStrictEqual(
				compositeFrames(independent),
			);
			expect(differenced.length).toBeLessThan(independent.length);
		},
	);

	test("independent frames stay full-canvas and reorderable", () => {
		const frames = movingSquare(6, false);
		const options = { width: WIDTH, height: HEIGHT, frames, delay: 5, loop: 0 };
		const independent = encodeRgbaGifFrames({
			...options,
			independentFrames: true,
		});
		const reader = new GifReader(independent);
		for (let frame = 0; frame < reader.numFrames(); frame += 1) {
			const info = reader.frameInfo(frame);
			expect([info.x, info.y, info.width, info.height]).toEqual([
				0,
				0,
				WIDTH,
				HEIGHT,
			]);
		}
		const compiled = compileGif(independent);
		expect(compiled.canReorderFrames).toBe(true);
		expect(compositeFrames(compiled.reverseFrames())).toStrictEqual(
			compositeFrames(independent).reverse(),
		);

		// Differenced frames depend on the previous canvas, so reordering
		// them without decoding is refused rather than rendered wrongly.
		expect(compileGif(encodeRgbaGifFrames(options)).canReorderFrames).toBe(
			false,
		);
	});

	test("a frame that clears opaque pixels restores the canvas first", () => {
		const opaque = new Uint8Array(FRAME_BYTES).fill(200);
		const cleared = opaque.slice();
		for (let offset = 3; offset < cleared.length; offset += 4) {
			if (offset % 32 === 3) cleared[offset] = 0;
		}
		const gif = encodeRgbaGifFrames({
			width: WIDTH,
			height: HEIGHT,
			frames: [opaque, cleared, opaque],
			delay: 5,
			loop: 0,
		});
		const composited = compositeFrames(gif);
		expect(composited[1]![3]).toBe(0);
		expect(composited[1]![7]).toBe(255);
		expect(composited[2]![3]).toBe(255);
	});
});
