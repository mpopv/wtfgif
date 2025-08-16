import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { beforeAll, describe, expect, test } from 'vitest';
import { GifReader as OmgGifReader } from 'omggif';
import { GifReader as WtfGifReader, initializeWasmGlobally } from '../src/index';

const gifsDir = join(__dirname, 'gifs');
const gifFiles = readdirSync(gifsDir).filter(f => f.endsWith('.gif'));

beforeAll(async () => {
  try {
    await initializeWasmGlobally();
  } catch (err) {
    console.warn('Wasm initialization failed, continuing with JS fallback:', err);
  }
});

describe('Optimized path parity', () => {
  for (const file of gifFiles) {
    test(`${file} wasm vs baseline`, async () => {
      const gif = readFileSync(join(gifsDir, file));
      const omg = new OmgGifReader(gif);
      const wtf = new WtfGifReader(gif);

      const pixelCount = wtf.width * wtf.height * 4;
      const frameCount = Math.min(wtf.numFrames(), 2);

      for (let frameIdx = 0; frameIdx < frameCount; frameIdx++) {
        const omgPixels = new Uint8Array(pixelCount);
        omg.decodeAndBlitFrameRGBA(frameIdx, omgPixels);

        const baseline = new Uint8Array(pixelCount);
        wtf.decodeAndBlitFrameRGBA(frameIdx, baseline);

        const wasm32 = await wtf.framePixelsWasm(frameIdx);
        const wasmBytes = new Uint8Array(wasm32.buffer, wasm32.byteOffset, wasm32.byteLength);

        const mismatch = wasmBytes.findIndex((v, i) => v !== baseline[i]);
        if (mismatch !== -1) {
          console.error(`Divergence in ${file} frame ${frameIdx} at byte ${mismatch}`);
        }

        expect(wasmBytes).toStrictEqual(baseline);
        expect(wasmBytes).toStrictEqual(omgPixels);
      }

      wtf.returnToPool();
    });
  }
});
