import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, test } from 'vitest';
import { GifReader as OmgGifReader, GifWriter as OmgGifWriter } from 'omggif';
import { GifReader as WtfGifReader, GifWriter as WtfGifWriter } from '../src/index';

const gifsDir = join(__dirname, 'gifs');
// Use a representative subset to keep test runtime reasonable
const gifFiles = [
  '18d30677-d255-4cc9-9933-c8d35306c1d5.gif',
  'party_blob.gif',
  'partyparrot.gif',
  'excuseme.gif',
].filter(f => readdirSync(gifsDir).includes(f));

describe('GifReader parity with omggif', () => {
  for (const file of gifFiles) {
    test(file, () => {
      const gif = readFileSync(join(gifsDir, file));
      const omg = new OmgGifReader(gif);
      const wtf = new WtfGifReader(gif);
      expect(wtf.width).toBe(omg.width);
      expect(wtf.height).toBe(omg.height);
      expect(wtf.numFrames()).toBe(omg.numFrames());

      const fields = ['x','y','width','height','has_local_palette','palette_offset','palette_size','data_offset','data_length','transparent_index','interlaced','delay','disposal'] as const;
      const numFrames = omg.numFrames();
      const len = wtf.width * wtf.height * 4;
      for (let i = 0; i < numFrames; i++) {
        const omgInfo = omg.frameInfo(i);
        const wtfInfo = wtf.frameInfo(i);
        const filteredWtf = Object.fromEntries(fields.map(f => [f, (wtfInfo as any)[f]]));
        expect(filteredWtf).toStrictEqual(omgInfo);

        // Exercise decoding APIs for coverage
        const bufRGBA = wtf.decodeFrameToTransferableRGBA(i);
        expect(bufRGBA.byteLength).toBe(len);
        const bufBGRA = wtf.decodeFrameToTransferableBGRA(i);
        expect(bufBGRA.byteLength).toBe(len);
        const abR = new ArrayBuffer(len);
        wtf.decodeFrameIntoBuffer(i, abR, 'rgba');
        const abB = new ArrayBuffer(len);
        wtf.decodeFrameIntoBuffer(i, abB, 'bgra');
        wtf.decodeAndBlitFrameRGBA(i, new Uint8Array(len));
        wtf.decodeAndBlitFrameBGRA(i, new Uint8Array(len));
      }

      expect(wtf.loopCount()).toBe(omg.loopCount());
      wtf.returnToPool();
    });
  }

  test('pooling API', () => {
    const gif = readFileSync(join(gifsDir, gifFiles[0]));
    const pooled = WtfGifReader.createPooled(gif);
    expect(() => pooled.returnToPool()).not.toThrow();
    const unpooled = WtfGifReader.createUnpooled(gif);
    expect(() => unpooled.dispose()).not.toThrow();
  });
});

describe('GifWriter parity with omggif', () => {
  test('encodes identical bytes', () => {
    const width = 2;
    const height = 2;
    const palette = [0x000000, 0xffffff];
    const frame = new Uint8Array([0, 1, 1, 0]);
    const bufOmg = new Uint8Array(1000);
    const bufWtf = new Uint8Array(1000);
    const omgWriter = new OmgGifWriter(bufOmg, width, height, { palette });
    omgWriter.addFrame(0, 0, width, height, frame);
    const omgLen = omgWriter.end();
    const wtfWriter = new WtfGifWriter(bufWtf, width, height, { palette });
    wtfWriter.addFrame(0, 0, width, height, frame);
    const wtfLen = wtfWriter.end();
    expect(wtfLen).toBe(omgLen);
    const omgGif = bufOmg.slice(0, omgLen);
    const wtfGif = bufWtf.slice(0, wtfLen);
    expect(wtfGif).toStrictEqual(omgGif);

    const omgReader = new OmgGifReader(omgGif);
    const wtfReader = new WtfGifReader(wtfGif);
    const len = width * height * 4;
    const omgPixels = new Uint8Array(len);
    const wtfPixels = new Uint8Array(len);
    omgReader.decodeAndBlitFrameRGBA(0, omgPixels);
    wtfReader.decodeAndBlitFrameRGBA(0, wtfPixels);
    expect(wtfPixels).toStrictEqual(omgPixels);
  });

  test('local palette support', () => {
    const width = 2;
    const height = 2;
    const palette = [0x000000, 0xffffff];
    const frame = new Uint8Array([0, 1, 1, 0]);
    const bufOmg = new Uint8Array(1000);
    const bufWtf = new Uint8Array(1000);
    const omgWriter = new OmgGifWriter(bufOmg, width, height);
    omgWriter.addFrame(0, 0, width, height, frame, { palette });
    const omgLen = omgWriter.end();
    const wtfWriter = new WtfGifWriter(bufWtf, width, height);
    wtfWriter.addFrame(0, 0, width, height, frame, { palette });
    const wtfLen = wtfWriter.end();
    expect(wtfLen).toBe(omgLen);
    expect(bufWtf.slice(0, wtfLen)).toStrictEqual(bufOmg.slice(0, omgLen));
  });
});

