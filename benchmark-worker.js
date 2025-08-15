importScripts('node_modules/omggif/omggif.js');
importScripts('wtfgif-browser.js');

// Preserve original omggif constructors before wtfgif overwrites them
const omggif = { GifReader: self.GifReader, GifWriter: self.GifWriter };

self.onmessage = (event) => {
  const { gifData, library, iterations, testId } = event.data;
  try {
    const data = new Uint8Array(gifData);
    const reader = library === 'omggif'
      ? new omggif.GifReader(data)
      : new self.wtfgif.GifReader(data);

    const pixels = new Uint8Array(reader.width * reader.height * 4);
    const numFrames = reader.numFrames();

    // JIT warm-up
    for (let w = 0; w < 2; w++) {
      for (let f = 0; f < numFrames; f++) {
        reader.decodeAndBlitFrameRGBA(f, pixels);
      }
    }

    const times = [];
    for (let i = 0; i < iterations; i++) {
      const start = performance.now();
      for (let f = 0; f < numFrames; f++) {
        reader.decodeAndBlitFrameRGBA(f, pixels);
      }
      times.push(performance.now() - start);
    }

    const result = {
      mean: times.reduce((a, b) => a + b) / times.length,
      min: Math.min(...times),
      max: Math.max(...times),
      times
    };

    self.postMessage({ success: true, result, library, testId });
  } catch (error) {
    self.postMessage({ success: false, error: error.message, library, testId });
  }
};
