const fs = require('fs');
const { performance } = require('perf_hooks');
const { GifReader } = require('./wtfgif');

console.log('Testing Dramatic Chipmunk GIF (large file optimization test)...');

const filename = 'Dramatic Chipmunk GIF.gif';
try {
  const data = fs.readFileSync(`test/gifs/${filename}`);
  const reader = new GifReader(data);
  
  console.log(`Dimensions: ${reader.width}x${reader.height}`);
  console.log(`Frames: ${reader.numFrames()}`);
  
  // Check first frame optimization
  const frame0 = reader.frameInfo(0);
  console.log(`Frame 0 flattened codes length: ${frame0.codes.length} bytes`);
  console.log(`Frame 0 original data_length: ${frame0.data_length} bytes`);
  console.log(`Savings from flattening: ${frame0.data_length - frame0.codes.length - 1} bytes (sub-block headers removed)`);
  
  // Performance test - decode all frames
  const pixels = new Uint8Array(reader.width * reader.height * 4);
  
  console.log('\nDecoding all frames...');
  const startTime = performance.now();
  
  for (let i = 0; i < reader.numFrames(); i++) {
    reader.decodeAndBlitFrameRGBA(i, pixels);
  }
  
  const endTime = performance.now();
  console.log(`Decoded ${reader.numFrames()} frames in ${(endTime - startTime).toFixed(2)}ms`);
  console.log(`Average per frame: ${((endTime - startTime) / reader.numFrames()).toFixed(3)}ms`);
  
} catch (error) {
  console.log(`ERROR: ${error.message}`);
}