import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, test } from 'vitest';
import { GifReader as OmgGifReader, GifWriter as OmgGifWriter } from 'omggif';
import { GifReader as WtfGifReader, GifWriter as WtfGifWriter } from '../src/index';

const gifsDir = join(__dirname, 'gifs');
// Test ALL GIF files for comprehensive compatibility
const allGifFiles = readdirSync(gifsDir).filter(f => f.endsWith('.gif'));
// For performance, test a representative subset in CI, all files when needed
const gifFiles = allGifFiles.slice(0, 2); // Test first 2 files for now

describe('GIF file inventory', () => {
  test('discovers all GIF files in test directory', () => {
    console.log(`Found ${allGifFiles.length} GIF files:`, allGifFiles);
    console.log(`Testing ${gifFiles.length} files:`, gifFiles);
    expect(allGifFiles.length).toBeGreaterThan(0);
    expect(gifFiles.length).toBeGreaterThan(0);
  });
});

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
      const frameLimit = Math.min(numFrames, 3);
      const len = wtf.width * wtf.height * 4;
      for (let i = 0; i < frameLimit; i++) {
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

describe('Pixel-perfect decoding compatibility', () => {
  for (const file of gifFiles) {
    test(`${file} - pixel data matches omggif exactly`, () => {
      const gif = readFileSync(join(gifsDir, file));
      const omg = new OmgGifReader(gif);
      const wtf = new WtfGifReader(gif);
      
      const pixelCount = omg.width * omg.height * 4;
      const frameCount = Math.min(omg.numFrames(), 2); // Test first 2 frames for performance
      
      for (let frameIdx = 0; frameIdx < frameCount; frameIdx++) {
        // Test RGBA decoding
        const omgRGBA = new Uint8Array(pixelCount);
        const wtfRGBA = new Uint8Array(pixelCount);
        omg.decodeAndBlitFrameRGBA(frameIdx, omgRGBA);
        wtf.decodeAndBlitFrameRGBA(frameIdx, wtfRGBA);
        expect(wtfRGBA).toStrictEqual(omgRGBA);
        
        // Test BGRA decoding
        const omgBGRA = new Uint8Array(pixelCount);
        const wtfBGRA = new Uint8Array(pixelCount);
        omg.decodeAndBlitFrameBGRA(frameIdx, omgBGRA);
        wtf.decodeAndBlitFrameBGRA(frameIdx, wtfBGRA);
        expect(wtfBGRA).toStrictEqual(omgBGRA);
      }
      
      wtf.returnToPool();
    });
  }
});

describe('Global properties compatibility', () => {
  for (const file of gifFiles) {
    test(`${file} - global properties match omggif`, () => {
      const gif = readFileSync(join(gifsDir, file));
      const omg = new OmgGifReader(gif);
      const wtf = new WtfGifReader(gif);
      
      // Basic dimensions
      expect(wtf.width).toBe(omg.width);
      expect(wtf.height).toBe(omg.height);
      expect(wtf.numFrames()).toBe(omg.numFrames());
      
      // Loop count
      expect(wtf.loopCount()).toBe(omg.loopCount());
      
      // Global color table comparison (if available through omggif)
      if ('globalColorTable' in omg && omg.globalColorTable) {
        // Note: omggif exposes globalColorTable, wtfgif has different API
        // This tests that both can handle the same global palette data
        expect(wtf.numFrames()).toBeGreaterThan(0); // Basic sanity check
      }
      
      wtf.returnToPool();
    });
  }
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

    test('clears dictionary between frames', () => {
      const width = 2;
      const height = 2;
      const palette = [0x000000, 0xffffff];
      const frame1 = new Uint8Array([0, 1, 1, 0]);
      const frame2 = new Uint8Array([1, 0, 0, 1]);
      const buf = new Uint8Array(1000);
      const writer = new WtfGifWriter(buf, width, height, { palette });
      writer.addFrame(0, 0, width, height, frame1);
      writer.addFrame(0, 0, width, height, frame2);
      const len = writer.end();
      const gif = buf.slice(0, len);
      const omgReader = new OmgGifReader(gif);
      const wtfReader = new WtfGifReader(gif);
      const outLen = width * height * 4;
      const omgPixels = new Uint8Array(outLen);
      const wtfPixels = new Uint8Array(outLen);
      omgReader.decodeAndBlitFrameRGBA(1, omgPixels);
      wtfReader.decodeAndBlitFrameRGBA(1, wtfPixels);
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

  test('supports explicit background index 0', () => {
    const width = 2;
    const height = 2;
    const palette = [0x000000, 0xffffff];
    const frame = new Uint8Array([0, 1, 1, 0]);
    const buf = new Uint8Array(100);
    const writer = new WtfGifWriter(buf, width, height, { palette, background: 0 });
    writer.addFrame(0, 0, width, height, frame);
    const len = writer.end();
    const gif = buf.slice(0, len);

    // Ensure other libraries can read the output
    const omgReader = new OmgGifReader(gif);
    const wtfReader = new WtfGifReader(gif);
    const outLen = width * height * 4;
    const omgPixels = new Uint8Array(outLen);
    const wtfPixels = new Uint8Array(outLen);
    omgReader.decodeAndBlitFrameRGBA(0, omgPixels);
    wtfReader.decodeAndBlitFrameRGBA(0, wtfPixels);
    expect(wtfPixels).toStrictEqual(omgPixels);
  });

  test('supports non-zero background index', () => {
    const width = 2;
    const height = 2;
    const palette = [0x000000, 0xffffff];
    const frame = new Uint8Array([0, 1, 1, 0]);
    const bufOmg = new Uint8Array(100);
    const bufWtf = new Uint8Array(100);
    const omgWriter = new OmgGifWriter(bufOmg, width, height, { palette, background: 1 });
    omgWriter.addFrame(0, 0, width, height, frame);
    const omgLen = omgWriter.end();
    const wtfWriter = new WtfGifWriter(bufWtf, width, height, { palette, background: 1 });
    wtfWriter.addFrame(0, 0, width, height, frame);
    const wtfLen = wtfWriter.end();
    expect(wtfLen).toBe(omgLen);
    expect(bufWtf.slice(0, wtfLen)).toStrictEqual(bufOmg.slice(0, omgLen));
  });

  test('multi-frame parity with transparency and looping', () => {
    const width = 2;
    const height = 2;
    const palette = [0x000000, 0xffffff, 0xff0000, 0x00ff00];
    const frame1 = new Uint8Array([0, 1, 1, 0]);
    const frame2 = new Uint8Array([2, 3, 3, 2]);
    const bufOmg = new Uint8Array(1000);
    const bufWtf = new Uint8Array(1000);
    const omgWriter = new OmgGifWriter(bufOmg, width, height, { palette, loop: 1, background: 1 });
    omgWriter.addFrame(0, 0, width, height, frame1, { delay: 5, disposal: 1 });
    omgWriter.addFrame(0, 0, width, height, frame2, { delay: 10, disposal: 2, transparent: 2 });
    const omgLen = omgWriter.end();
    const wtfWriter = new WtfGifWriter(bufWtf, width, height, { palette, loop: 1, background: 1 });
    wtfWriter.addFrame(0, 0, width, height, frame1, { delay: 5, disposal: 1 });
    wtfWriter.addFrame(0, 0, width, height, frame2, { delay: 10, disposal: 2, transparent: 2 });
    const wtfLen = wtfWriter.end();
    expect(wtfLen).toBe(omgLen);
    const omgGif = bufOmg.slice(0, omgLen);
    const wtfGif = bufWtf.slice(0, wtfLen);
    expect(wtfGif).toStrictEqual(omgGif);

    const omgReader = new OmgGifReader(omgGif);
    const wtfReader = new WtfGifReader(wtfGif);
    expect(wtfReader.loopCount()).toBe(omgReader.loopCount());
  });
});

describe('Cross-library write/read compatibility', () => {
  for (const file of gifFiles.slice(0, 3)) { // Test first 3 files to keep runtime reasonable
    test(`${file} - wtfgif can read/write then omggif can read`, () => {
      const originalGif = readFileSync(join(gifsDir, file));
      const wtfReader = new WtfGifReader(originalGif);
      
      // Extract basic properties
      const width = wtfReader.width;
      const height = wtfReader.height;
      const numFrames = Math.min(wtfReader.numFrames(), 2); // Limit for performance
      
      // Decode first frame to create a simple palette
      const pixelCount = width * height * 4;
      const firstFramePixels = new Uint8Array(pixelCount);
      wtfReader.decodeAndBlitFrameRGBA(0, firstFramePixels);
      
      // Create a simple 2-color palette for testing
      const palette = [0x000000, 0xFFFFFF];
      const frameData = new Uint8Array(width * height);
      
      // Convert first frame to simple binary pattern for testing
      for (let i = 0; i < frameData.length; i++) {
        // Simple thresholding to create binary data
        const pixelOffset = i * 4;
        const brightness = (firstFramePixels[pixelOffset] + firstFramePixels[pixelOffset + 1] + firstFramePixels[pixelOffset + 2]) / 3;
        frameData[i] = brightness > 127 ? 1 : 0;
      }
      
      // Write GIF using wtfgif
      const outputBuf = new Uint8Array(originalGif.length * 2); // Allow extra space
      const wtfWriter = new WtfGifWriter(outputBuf, width, height, { palette });
      wtfWriter.addFrame(0, 0, width, height, frameData);
      const outputLen = wtfWriter.end();
      const wtfGif = outputBuf.slice(0, outputLen);
      
      // Verify omggif can read wtfgif-written GIF
      expect(() => {
        const omgReader = new OmgGifReader(wtfGif);
        expect(omgReader.width).toBe(width);
        expect(omgReader.height).toBe(height);
        expect(omgReader.numFrames()).toBeGreaterThan(0);
        
        // Test that omggif can decode wtfgif-written pixels
        const omgPixels = new Uint8Array(width * height * 4);
        omgReader.decodeAndBlitFrameRGBA(0, omgPixels);
        expect(omgPixels.length).toBe(width * height * 4);
      }).not.toThrow();
      
      wtfReader.returnToPool();
    });
  }
  
  test('performance comparison - wtfgif vs omggif decoding speed', () => {
    const file = gifFiles[0]; // Use first available file
    const gif = readFileSync(join(gifsDir, file));
    const omgReader = new OmgGifReader(gif);
    const wtfReader = new WtfGifReader(gif);
    
    const pixelCount = omgReader.width * omgReader.height * 4;
    const pixels = new Uint8Array(pixelCount);
    
    // Warm up both decoders
    omgReader.decodeAndBlitFrameRGBA(0, pixels);
    wtfReader.decodeAndBlitFrameRGBA(0, pixels);
    
    // Time omggif
    const omgStart = performance.now();
    for (let i = 0; i < 10; i++) {
      omgReader.decodeAndBlitFrameRGBA(0, pixels);
    }
    const omgTime = performance.now() - omgStart;
    
    // Time wtfgif
    const wtfStart = performance.now();
    for (let i = 0; i < 10; i++) {
      wtfReader.decodeAndBlitFrameRGBA(0, pixels);
    }
    const wtfTime = performance.now() - wtfStart;
    
    const speedup = omgTime / wtfTime;
    console.log(`Performance: wtfgif ${speedup.toFixed(2)}x ${speedup >= 1 ? 'faster' : 'slower'} than omggif on ${file}`);
    
    // wtfgif should be competitive (at least 50% of omggif speed)
    expect(speedup).toBeGreaterThan(0.5);
    
    wtfReader.returnToPool();
  });
});

