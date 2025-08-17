import { GifEncoderStream, GifDecoderStream } from "../src";
import { once } from "events";
import { test, expect } from "vitest";

// Simple streaming roundtrip test to ensure the streaming interfaces work in
// basic scenarios. A tiny 2x2 gif is encoded, streamed through the encoder
// and then decoded back via the decoder stream.

test("streaming encode/decode", async () => {
  const encoder = new GifEncoderStream(2, 2, { palette: [0x000000, 0xffffff] });
  encoder.addFrame(0, 0, 2, 2, new Uint8Array([0, 1, 1, 0]));
  encoder.finish();

  const chunks: Buffer[] = [];
  for await (const chunk of encoder) {
    chunks.push(chunk as Buffer);
  }

  const decoder = new GifDecoderStream();
  const decodedPromise = once(decoder as any, "decoded");
  for (const chunk of chunks) {
    decoder.write(chunk);
  }
  decoder.end();
  const [reader] = await decodedPromise as any;

  expect(reader.width).toBe(2);
  expect(reader.height).toBe(2);
});
