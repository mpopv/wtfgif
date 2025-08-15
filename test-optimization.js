const fs = require('fs');
const { performance } = require('perf_hooks');
const { GifReader } = require('./wtfgif');

function testGif(filename) {
  console.log(`\nTesting ${filename}:`);
  
  try {
    const data = fs.readFileSync(`test/gifs/${filename}`);
    const reader = new GifReader(data);
    
    console.log(`  Dimensions: ${reader.width}x${reader.height}`);
    console.log(`  Frames: ${reader.numFrames()}`);
    
    // Test frame parsing (should have the new fields)
    const frame0 = reader.frameInfo(0);
    console.log(`  Frame 0 has flattened codes: ${frame0.codes ? 'YES' : 'NO'}`);
    console.log(`  Frame 0 min_code_size: ${frame0.min_code_size}`);
    console.log(`  Frame 0 codes length: ${frame0.codes ? frame0.codes.length : 'N/A'}`);
    console.log(`  Frame 0 has prebuilt RGBA palette: ${frame0.pal32rgba ? 'YES' : 'NO'}`);
    console.log(`  Frame 0 has prebuilt BGRA palette: ${frame0.pal32bgra ? 'YES' : 'NO'}`);
    
    // Test decoding performance 
    const pixels = new Uint8Array(reader.width * reader.height * 4);
    const startTime = performance.now();
    
    // Decode all frames
    for (let i = 0; i < Math.min(reader.numFrames(), 10); i++) {
      reader.decodeAndBlitFrameRGBA(i, pixels);
    }
    
    const endTime = performance.now();
    console.log(`  Decoded ${Math.min(reader.numFrames(), 10)} frames in ${(endTime - startTime).toFixed(2)}ms`);
    
    return true;
  } catch (error) {
    console.log(`  ERROR: ${error.message}`);
    return false;
  }
}

console.log('Testing LZW sub-block flattening optimization...');

// Test a few different GIF files
const testFiles = [
  'Clap-1x.gif',
  'excuseme.gif',
  'partyparrot.gif',
  'party_blob.gif'
];

let passed = 0;
for (const filename of testFiles) {
  if (testGif(filename)) {
    passed++;
  }
}

console.log(`\nSummary: ${passed}/${testFiles.length} tests passed`);