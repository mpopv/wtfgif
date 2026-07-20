import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, test } from 'vitest';
import { GifReader as OmgGifReader } from 'omggif';
import { GifReader as WtfGifReader } from '../src/index';

const gifsDir = join(__dirname, 'gifs');
const gifFiles = readdirSync(gifsDir).filter(f => f.endsWith('.gif'));

describe('Optimized path parity', () => {
  for (const file of gifFiles) {
    test(`${file} prepared frames vs baseline`, () => {
      const gif = readFileSync(join(gifsDir, file));
      const omg = new OmgGifReader(gif);
      const wtf = new WtfGifReader(gif);
      const prepared = wtf.prepareFrames();

      const pixelCount = wtf.width * wtf.height * 4;
      const frameCount = Math.min(wtf.numFrames(), 2);

      for (let frameIdx = 0; frameIdx < frameCount; frameIdx++) {
        const omgPixels = new Uint8Array(pixelCount);
        omg.decodeAndBlitFrameRGBA(frameIdx, omgPixels);

        const baseline = new Uint8Array(pixelCount);
        wtf.decodeAndBlitFrameRGBA(frameIdx, baseline);

        const preparedPixels = new Uint8Array(pixelCount);
        prepared.copyFrame(frameIdx, preparedPixels);

        expect(preparedPixels).toStrictEqual(baseline);
        expect(preparedPixels).toStrictEqual(omgPixels);
      }

      prepared.dispose();
      wtf.returnToPool();
    }, 30000);
  }
});
