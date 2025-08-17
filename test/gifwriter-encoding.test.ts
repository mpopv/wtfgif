import { describe, expect, test } from "vitest";
import { GifReader, GifWriter } from "../src/index";

// Unit tests focused on GifWriter's encoding-related utilities

describe("GifWriter encoding utilities", () => {
  test("allows adding frames after calling end", () => {
    const palette = [0x000000, 0xffffff];
    const buf = new Uint8Array(100);
    const writer = new GifWriter(buf, 1, 1, { palette });
    const frame1 = new Uint8Array([0]);
    const frame2 = new Uint8Array([1]);

    writer.addFrame(0, 0, 1, 1, frame1);
    const len1 = writer.end();

    // Save prefix without trailer to ensure it remains untouched
    const prefix = buf.slice(0, len1 - 1);

    // Adding another frame after end() should remove the trailer and append data
    writer.addFrame(0, 0, 1, 1, frame2);
    const len2 = writer.end();
    expect(len2).toBeGreaterThan(len1);
    expect(buf.slice(0, len1 - 1)).toStrictEqual(prefix);

    // Decode to verify both frames exist and have expected colors
    const gif = buf.slice(0, len2);
    const reader = new GifReader(gif);
    expect(reader.numFrames()).toBe(2);

    const out = new Uint8Array(4);
    reader.decodeAndBlitFrameRGBA(0, out);
    expect(Array.from(out)).toStrictEqual([0, 0, 0, 255]);
    reader.decodeAndBlitFrameRGBA(1, out);
    expect(Array.from(out)).toStrictEqual([255, 255, 255, 255]);
  });

  test("supports swapping output buffer and position", () => {
    const palette = [0x000000, 0xffffff];
    const frame = new Uint8Array([0]);

    // Reference encoding for comparison
    const refBuf = new Uint8Array(50);
    const refWriter = new GifWriter(refBuf, 1, 1, { palette });
    refWriter.addFrame(0, 0, 1, 1, frame);
    const refLen = refWriter.end();
    const refGif = refBuf.slice(0, refLen);

    // Create writer and swap buffer before encoding frame
    const buf1 = new Uint8Array(50);
    const writer = new GifWriter(buf1, 1, 1, { palette });
    const headerLen = writer.getOutputBufferPosition();
    expect(headerLen).toBeGreaterThan(0);

    const buf2 = new Uint8Array(50);
    // Preserve header into new buffer
    buf2.set(buf1.slice(0, headerLen));
    writer.setOutputBuffer(buf2);
    writer.setOutputBufferPosition(headerLen);

    writer.addFrame(0, 0, 1, 1, frame);
    const len = writer.end();

    expect(writer.getOutputBuffer()).toBe(buf2);
    expect(writer.getOutputBufferPosition()).toBe(len);
    expect(buf2.slice(0, len)).toStrictEqual(refGif);
  });
});
