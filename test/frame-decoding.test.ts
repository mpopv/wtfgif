import { describe, expect, test } from "vitest";
import { GifReader, GifWriter } from "../src/index";

describe("GifReader frame decoding", () => {
	function makeDisposalGif(): Uint8Array {
		const palette = [0x000000, 0xff0000, 0x00ff00, 0x0000ff];
		const buf = new Uint8Array(100);
		const writer = new GifWriter(buf, 2, 2, { palette });

		writer.addFrame(0, 0, 2, 2, new Uint8Array([1, 1, 1, 1]));
		writer.addFrame(0, 0, 2, 2, new Uint8Array([2, 0, 2, 0]), {
			transparent: 0,
			disposal: 2,
		});
		writer.addFrame(1, 0, 1, 2, new Uint8Array([3, 3]));

		return buf.slice(0, writer.end());
	}

	test("decodes frames with transparency and disposal", () => {
		const palette = [0x000000, 0xff0000, 0x00ff00, 0x0000ff];
		const buf = new Uint8Array(100);
		const writer = new GifWriter(buf, 2, 2, { palette });

		// Frame 0: solid red
		writer.addFrame(0, 0, 2, 2, new Uint8Array([1, 1, 1, 1]));

		// Frame 1: green left column, transparent right column
		writer.addFrame(0, 0, 2, 2, new Uint8Array([2, 0, 2, 0]), {
			transparent: 0,
			disposal: 2,
		});

		// Frame 2: blue stripe on the right
		writer.addFrame(1, 0, 1, 2, new Uint8Array([3, 3]));

		const len = writer.end();
		const gif = buf.slice(0, len);
		const reader = new GifReader(gif);

		const expected0 = [
			255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
		];
		const p0 = new Uint8Array(16);
		reader.decodeAndBlitFrameRGBA(0, p0);
		expect(Array.from(p0)).toStrictEqual(expected0);

		const expected1 = [0, 255, 0, 255, 0, 0, 0, 0, 0, 255, 0, 255, 0, 0, 0, 0];
		const p1 = new Uint8Array(16);
		reader.decodeAndBlitFrameRGBA(1, p1);
		expect(Array.from(p1)).toStrictEqual(expected1);

		const expected2 = [0, 0, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 255, 255];
		const p2 = new Uint8Array(16);
		reader.decodeAndBlitFrameRGBA(2, p2);
		expect(Array.from(p2)).toStrictEqual(expected2);
	});

	test("decodeFrameIntoBuffer supports BGRA output", () => {
		const palette = [0x000000, 0xff0000];
		const buf = new Uint8Array(50);
		const writer = new GifWriter(buf, 1, 1, { palette });
		writer.addFrame(0, 0, 1, 1, new Uint8Array([1]));
		const len = writer.end();
		const gif = buf.slice(0, len);
		const reader = new GifReader(gif);

		const ab = new ArrayBuffer(4);
		reader.decodeFrameIntoBuffer(0, ab, "bgra");
		expect(Array.from(new Uint8Array(ab))).toStrictEqual([0, 0, 255, 255]);
	});

	test("transparent pixels leave existing destination pixels unchanged", () => {
		const palette = [0x000000, 0xff0000, 0x00ff00];
		const buf = new Uint8Array(100);
		const writer = new GifWriter(buf, 2, 1, { palette });
		writer.addFrame(0, 0, 2, 1, new Uint8Array([0, 2]), {
			transparent: 0,
		});
		const len = writer.end();
		const reader = new GifReader(buf.slice(0, len));
		const pixels = new Uint8Array([9, 8, 7, 6, 1, 2, 3, 4]);

		reader.decodeAndBlitFrameRGBA(0, pixels);

		expect(Array.from(pixels)).toStrictEqual([9, 8, 7, 6, 0, 255, 0, 255]);
	});

	test("preparePlayback returns composited frames with disposal applied", () => {
		const reader = new GifReader(makeDisposalGif());
		const prepared = reader.preparePlayback();
		const pixels = new Uint8Array(16);

		prepared.copyFrame(0, pixels);
		expect(Array.from(pixels)).toStrictEqual([
			255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
		]);

		prepared.copyFrame(1, pixels);
		expect(Array.from(pixels)).toStrictEqual([
			0, 255, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 255, 0, 0, 255,
		]);

		prepared.copyFrame(2, pixels);
		expect(Array.from(pixels)).toStrictEqual([
			0, 0, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 255, 255,
		]);
	});

	test("decodeAndBlitCompositedFrameRGBA uses the prepared playback cache", () => {
		const reader = new GifReader(makeDisposalGif());
		const pixels = new Uint8Array(16);
		const activePreparedFrames = (
			reader as unknown as { activePreparedFrames: Set<unknown> }
		).activePreparedFrames;

		reader.decodeAndBlitCompositedFrameRGBA(1, pixels);

		expect(Array.from(pixels)).toStrictEqual([
			0, 255, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 255, 0, 0, 255,
		]);
		expect(activePreparedFrames.size).toBe(0);
	});

	test("preparePlayback handles restore-to-previous disposal", () => {
		const palette = [0x000000, 0xff0000, 0x00ff00, 0x0000ff];
		const buf = new Uint8Array(100);
		const writer = new GifWriter(buf, 2, 2, { palette });
		writer.addFrame(0, 0, 2, 2, new Uint8Array([1, 1, 1, 1]), {
			disposal: 1,
		});
		writer.addFrame(0, 0, 1, 2, new Uint8Array([2, 2]), {
			disposal: 3,
		});
		writer.addFrame(1, 0, 1, 2, new Uint8Array([3, 3]));
		const reader = new GifReader(buf.slice(0, writer.end()));
		const prepared = reader.preparePlayback();
		const pixels = new Uint8Array(16);

		prepared.copyFrame(2, pixels);

		expect(Array.from(pixels)).toStrictEqual([
			255, 0, 0, 255, 0, 0, 255, 255, 255, 0, 0, 255, 0, 0, 255, 255,
		]);
	});

	test("preparePlayback deduplicates identical composited frames", () => {
		const palette = [0x000000, 0xff0000];
		const buf = new Uint8Array(100);
		const writer = new GifWriter(buf, 2, 2, { palette });
		writer.addFrame(0, 0, 2, 2, new Uint8Array([1, 1, 1, 1]));
		writer.addFrame(0, 0, 2, 2, new Uint8Array([1, 1, 1, 1]));
		const reader = new GifReader(buf.slice(0, writer.end()));
		const prepared = reader.preparePlayback();

		expect(prepared.frames[0]?.pixels).toBe(prepared.frames[1]?.pixels);
		expect(prepared.byteLength).toBe(16);
	});

	test("preparePlayback can disable composited frame dedupe", () => {
		const palette = [0x000000, 0xff0000];
		const buf = new Uint8Array(100);
		const writer = new GifWriter(buf, 2, 2, { palette });
		writer.addFrame(0, 0, 2, 2, new Uint8Array([1, 1, 1, 1]));
		writer.addFrame(0, 0, 2, 2, new Uint8Array([1, 1, 1, 1]));
		const reader = new GifReader(buf.slice(0, writer.end()));
		const prepared = reader.preparePlayback({ dedupe: "none" });

		expect(prepared.frames[0]?.pixels).not.toBe(prepared.frames[1]?.pixels);
		expect(prepared.byteLength).toBe(32);
	});

	test("preparePlayback exposes zero-copy frame views", () => {
		const reader = new GifReader(makeDisposalGif());
		const prepared = reader.preparePlayback();

		expect(prepared.getFramePixels(0)?.length).toBe(4);
		expect(prepared.getFrameBytes(0)?.byteLength).toBe(16);
		expect(prepared.getFramePixels(99)).toBeUndefined();
		expect(prepared.getFrameBytes(99)).toBeUndefined();
	});

	test("dispose clears prepared frame caches", () => {
		const reader = new GifReader(makeDisposalGif());
		const prepared = reader.preparePlayback();

		reader.dispose();

		expect(prepared.frames).toHaveLength(0);
		expect(() => prepared.copyFrame(0, new Uint8Array(16))).toThrow(
			"Frame index out of range.",
		);
		expect(reader.preparePlayback()).not.toBe(prepared);
	});

	test("prepared player draws sequential frames with deltas", () => {
		const reader = new GifReader(makeDisposalGif());
		const prepared = reader.preparePlayback({ deltas: true });
		const frame1 = prepared.getFrame(1);
		const target = new Uint8Array(16);
		const player = prepared.createPlayer(target);

		expect(frame1?.changedX).toBe(0);
		expect(frame1?.changedY).toBe(0);
		expect(frame1?.changedWidth).toBe(1);
		expect(frame1?.changedHeight).toBe(2);
		expect(frame1?.changedPixels?.length).toBe(2);

		player.next();
		expect(player.currentIndex).toBe(0);
		expect(Array.from(target)).toStrictEqual([
			255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
		]);

		player.next();
		expect(player.currentIndex).toBe(1);
		expect(Array.from(target)).toStrictEqual([
			0, 255, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 255, 0, 0, 255,
		]);

		player.drawFrame(2);
		expect(player.currentIndex).toBe(2);
		expect(Array.from(target)).toStrictEqual([
			0, 0, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 255, 255,
		]);

		player.reset();
		expect(player.currentIndex).toBe(-1);
		expect(Array.from(target)).toStrictEqual(new Array(16).fill(0));
	});

	test("prepared player falls back to full copy for non-sequential seeks", () => {
		const reader = new GifReader(makeDisposalGif());
		const prepared = reader.preparePlayback({ deltas: true });
		const target = new Uint8Array(16);
		const player = prepared.createPlayer(target);

		player.drawFrame(2);

		expect(Array.from(target)).toStrictEqual([
			0, 0, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 255, 255,
		]);
	});

	test("prepareFrames respects maxBytes", () => {
		const reader = new GifReader(makeDisposalGif());

		expect(() => reader.preparePlayback({ maxBytes: 15 })).toThrow(/maxBytes/);
	});

	test("prepareFrames can cache sparse transparent pixels", () => {
		const reader = new GifReader(makeDisposalGif());
		const prepared = reader.prepareFrames({
			composited: false,
			cache: "sparse-rgba",
			frameIndices: [1],
		});
		const frame = prepared.getFrame(1);
		const pixels = new Uint8Array([
			9, 8, 7, 6, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
		]);

		expect(
			(frame?.spans?.length ?? 0) + (frame?.positions?.length ?? 0),
		).toBeGreaterThan(0);
		prepared.copyFrame(1, pixels);

		expect(Array.from(pixels)).toStrictEqual([
			0, 255, 0, 255, 1, 2, 3, 4, 0, 255, 0, 255, 9, 10, 11, 12,
		]);
	});

	test("prepareFrames uses spans for long transparent runs", () => {
		const palette = [0x000000, 0xff0000, 0x00ff00, 0x0000ff];
		const buf = new Uint8Array(100);
		const writer = new GifWriter(buf, 8, 1, { palette });
		writer.addFrame(0, 0, 8, 1, new Uint8Array([1, 1, 1, 1, 0, 0, 0, 0]), {
			transparent: 0,
		});
		const reader = new GifReader(buf.slice(0, writer.end()));

		const prepared = reader.prepareFrames({
			composited: false,
			cache: "sparse-rgba",
		});

		expect(prepared.getFrame(0)?.spans?.length).toBe(3);
	});

	test("Wasm backend status is unavailable by default", () => {
		GifReader.setDecodeBackend(null);

		expect(GifReader.getDecodeBackendStatus()).toStrictEqual({
			name: "javascript",
			available: false,
		});
	});
});
