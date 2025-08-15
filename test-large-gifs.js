const fs = require('fs');
const { performance } = require('perf_hooks');
const { GifReader } = require('./wtfgif');

function testLargeGif(filename) {
  console.log(`\nTesting ${filename}:`);
  
  try {
    const data = fs.readFileSync(`test/gifs/${filename}`);
    const reader = new GifReader(data);
    
    console.log(`  Dimensions: ${reader.width}x${reader.height}`);
    console.log(`  Frames: ${reader.numFrames()}`);
    
    // Check sub-block savings
    let totalOriginalSize = 0;
    let totalFlattenedSize = 0;
    let totalSubBlockHeaders = 0;
    
    for (let i = 0; i < reader.numFrames(); i++) {
      const frame = reader.frameInfo(i);
      totalOriginalSize += frame.data_length;
      totalFlattenedSize += frame.codes.length;
      totalSubBlockHeaders += (frame.data_length - frame.codes.length - 1);
    }
    
    console.log(`  Total LZW data: ${totalOriginalSize} -> ${totalFlattenedSize + reader.numFrames()} bytes`);
    console.log(`  Sub-block headers removed: ${totalSubBlockHeaders} bytes`);
    
    // Performance test
    const pixels = new Uint8Array(reader.width * reader.height * 4);
    const framesToTest = Math.min(reader.numFrames(), 20);
    
    const startTime = performance.now();
    for (let i = 0; i < framesToTest; i++) {
      reader.decodeAndBlitFrameRGBA(i, pixels);
    }
    const endTime = performance.now();
    
    console.log(`  Decoded ${framesToTest} frames in ${(endTime - startTime).toFixed(2)}ms`);
    console.log(`  Average per frame: ${((endTime - startTime) / framesToTest).toFixed(3)}ms`);
    
    return true;
  } catch (error) {
    console.log(`  ERROR: ${error.message}`);
    return false;
  }
}

console.log('Testing optimization on larger GIF files...');

const largeFiles = [
  'GIGACHAD-4x.gif',
  'catJAM-3x.gif', 
  'excuseme.gif',
  'Proud Of You Yes GIF.gif'
];

let passed = 0;
for (const filename of largeFiles) {
  if (testLargeGif(filename)) {
    passed++;
  }
}

console.log(`\nSummary: ${passed}/${largeFiles.length} large GIFs tested successfully`);