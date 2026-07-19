import { describe, test, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { GifWriter as OmgGifWriter } from "omggif";
import { GifReader, GifWriter } from "../src/index";

const gifsDir = join(__dirname, "gifs");
const sampleGif = readFileSync(join(gifsDir, "partyparrot.gif"));

describe("GifReader error handling", () => {
  test("invalid header", () => {
    const bad = new Uint8Array([0, 1, 2, 3]);
    expect(() => new GifReader(bad)).toThrow(/Invalid GIF/);
  });

  test("out of bounds frame index", () => {
    const reader = new GifReader(sampleGif);
    const small = new ArrayBuffer(1);
    expect(() => reader.frameInfo(999)).toThrow(/Frame index out of range/);
    expect(() => reader.decodeFrameToTransferableRGBA(999)).toThrow(
      /Frame index out of range/
    );
    expect(() => reader.decodeFrameIntoBuffer(0, small)).toThrow(
      /Buffer too small/
    );
  });
});

describe("GifWriter error handling", () => {
  test("invalid palette size", () => {
    const buf = new Uint8Array(10);
    const bigPalette = new Array(300).fill(0);
    expect(
      () => new GifWriter(buf, 1, 1, { palette: bigPalette as number[] })
    ).toThrow(/Invalid palette size/);
    expect(() => new GifWriter(buf, 1, 1, { palette: [] as number[] })).toThrow(
      /Invalid palette size/
    );
  });

  test("invalid dimensions", () => {
    const buf = new Uint8Array(10);
    expect(() => new GifWriter(buf, 0, 1)).toThrow(/Width\/Height invalid/);
  });

  test("background index validation", () => {
    const buf = new Uint8Array(10);
    const palette = [0x000000, 0xffffff];
    expect(() => new GifWriter(buf, 1, 1, { palette, background: 2 })).toThrow(
      /Background index out of range/
    );
    expect(
      () => new GifWriter(buf, 1, 1, { palette, background: 0 })
    ).not.toThrow();
  });

  test("loop count invalid", () => {
    const buf = new Uint8Array(10);
    const palette = [0x000000, 0xffffff];
    expect(() => new GifWriter(buf, 1, 1, { palette, loop: 70000 })).toThrow(
      /Loop count invalid/
    );
  });

  test("addFrame palette requirement", () => {
    const buf = new Uint8Array(100);
    const writer = new GifWriter(buf, 1, 1);
    const pixels = new Uint8Array([0]);
    expect(() => writer.addFrame(0, 0, 1, 1, pixels)).toThrow(
      /Must supply either a local or global palette/
    );
  });

  test("addFrame parameter validation", () => {
    const buf = new Uint8Array(100);
    const palette = [0x000000, 0xffffff];
    const writer = new GifWriter(buf, 2, 2, { palette });
    const pixels = new Uint8Array([0, 1, 2, 3]);
    expect(() => writer.addFrame(-1, 0, 1, 1, pixels)).toThrow(/x\/y invalid/);
    expect(() => writer.addFrame(0, 0, 0, 1, pixels)).toThrow(
      /Width\/Height invalid/
    );
    expect(() =>
      writer.addFrame(0, 0, 2, 2, new Uint8Array([0, 1, 2]))
    ).toThrow(/Not enough pixels/);
    expect(() => writer.addFrame(0, 0, 2, 2, pixels, { disposal: 5 })).toThrow(
      /Disposal out of range/
    );
    expect(() =>
      writer.addFrame(0, 0, 2, 2, pixels, { transparent: 5 })
    ).toThrow(/Transparent color index out of range/);
  });

  test("out-of-range pixel indices", () => {
    const palette = [0x000000, 0xffffff];
    const frame = new Uint8Array([2]);
    const bufWtf = new Uint8Array(10);
    const wtfWriter = new GifWriter(bufWtf, 1, 1, { palette });
    expect(() => wtfWriter.addFrame(0, 0, 1, 1, frame)).toThrow(
      /Pixel index out of range/
    );
    const bufOmg = new Uint8Array(10);
    const omgWriter = new OmgGifWriter(bufOmg, 1, 1, { palette });
    expect(() =>
      omgWriter.addFrame(0, 0, 1, 1, Array.from(frame))
    ).not.toThrow();
  });
});
