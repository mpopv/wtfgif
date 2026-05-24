import { describe, expect, test } from "vitest";
import { GifReader, GifWriter } from "../src/index";

describe("GifReader frame decoding", () => {
  test("decodes frames with transparency and disposal", () => {
    const palette = [0x000000, 0xff0000, 0x00ff00, 0x0000ff];
    const buf = new Uint8Array(100);
    const writer = new GifWriter(buf, 2, 2, { palette });

    // Frame 0: solid red
    writer.addFrame(0, 0, 2, 2, new Uint8Array([1, 1, 1, 1]));

    // Frame 1: green left column, transparent right column
    writer.addFrame(
      0,
      0,
      2,
      2,
      new Uint8Array([2, 0, 2, 0]),
      { transparent: 0, disposal: 2 }
    );

    // Frame 2: blue stripe on the right
    writer.addFrame(1, 0, 1, 2, new Uint8Array([3, 3]));

    const len = writer.end();
    const gif = buf.slice(0, len);
    const reader = new GifReader(gif);

    const expected0 = [
      255, 0, 0, 255,
      255, 0, 0, 255,
      255, 0, 0, 255,
      255, 0, 0, 255,
    ];
    const p0 = new Uint8Array(16);
    reader.decodeAndBlitFrameRGBA(0, p0);
    expect(Array.from(p0)).toStrictEqual(expected0);

    const expected1 = [
      0, 255, 0, 255,
      0, 0, 0, 0,
      0, 255, 0, 255,
      0, 0, 0, 0,
    ];
    const p1 = new Uint8Array(16);
    reader.decodeAndBlitFrameRGBA(1, p1);
    expect(Array.from(p1)).toStrictEqual(expected1);

    const expected2 = [
      0, 0, 0, 0,
      0, 0, 255, 255,
      0, 0, 0, 0,
      0, 0, 255, 255,
    ];
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
    expect(Array.from(new Uint8Array(ab))).toStrictEqual([
      0, 0, 255, 255,
    ]);
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
    const pixels = new Uint8Array([
      9, 8, 7, 6,
      1, 2, 3, 4,
    ]);

    reader.decodeAndBlitFrameRGBA(0, pixels);

    expect(Array.from(pixels)).toStrictEqual([
      9, 8, 7, 6,
      0, 255, 0, 255,
    ]);
  });
});
