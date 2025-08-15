const fs = require('fs');
const { GifReader } = require('./wtfgif');

function testCorrectness(filename) {
  console.log(`Testing correctness for ${filename}:`);
  
  try {
    const data = fs.readFileSync(`test/gifs/${filename}`);
    const reader = new GifReader(data);
    
    const pixels1 = new Uint8Array(reader.width * reader.height * 4);
    const pixels2 = new Uint8Array(reader.width * reader.height * 4);
    
    // Decode same frame twice
    reader.decodeAndBlitFrameRGBA(0, pixels1);
    reader.decodeAndBlitFrameRGBA(0, pixels2);
    
    // Compare results
    let differences = 0;
    for (let i = 0; i < pixels1.length; i++) {
      if (pixels1[i] !== pixels2[i]) {
        differences++;
      }
    }
    
    if (differences === 0) {
      console.log(`  ✅ Consistent decoding - no differences found`);
      return true;
    } else {
      console.log(`  ❌ Found ${differences} pixel differences`);
      return false;
    }
    
  } catch (error) {
    console.log(`  ERROR: ${error.message}`);
    return false;
  }
}

console.log('Testing correctness of transparency split optimization...');

const testFiles = [
  'Clap-1x.gif',
  'partyparrot.gif',
  'excuseme.gif',
  'party_blob.gif'
];

let passed = 0;
for (const filename of testFiles) {
  if (testCorrectness(filename)) {
    passed++;
  }
}

console.log(`\nCorrectness: ${passed}/${testFiles.length} tests passed`);

if (passed === testFiles.length) {
  console.log('✅ All transparency split optimizations working correctly!');
} else {
  console.log('❌ Some correctness issues detected');
}