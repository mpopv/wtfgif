const fs = require('fs');
const { performance } = require('perf_hooks');
const { GifReader } = require('./wtfgif');

function createNonTransparentVersion(reader) {
  // Create a mock version where we simulate no transparency
  // by temporarily modifying frame info
  const originalFrames = [];
  
  for (let i = 0; i < reader.numFrames(); i++) {
    const frame = reader.frameInfo(i);
    originalFrames.push(frame.transparent_index);
    frame.transparent_index = null; // Force non-transparent
  }
  
  return () => {
    // Restore original transparency
    for (let i = 0; i < reader.numFrames(); i++) {
      const frame = reader.frameInfo(i);
      frame.transparent_index = originalFrames[i];
    }
  };
}

function benchmarkTransparencyPaths(filename) {
  console.log(`\nBenchmarking transparency paths for ${filename}:`);
  
  try {
    const data = fs.readFileSync(`test/gifs/${filename}`);
    const reader = new GifReader(data);
    
    console.log(`  Dimensions: ${reader.width}x${reader.height}`);
    console.log(`  Frames: ${reader.numFrames()}`);
    
    const pixels = new Uint8Array(reader.width * reader.height * 4);
    const framesToTest = Math.min(reader.numFrames(), 20);
    const iterations = 5;
    
    // Test with transparency (original)
    console.log('\n  Testing WITH transparency checks:');
    const transparentTimes = [];
    for (let run = 0; run < iterations; run++) {
      const startTime = performance.now();
      for (let i = 0; i < framesToTest; i++) {
        reader.decodeAndBlitFrameRGBA(i, pixels);
      }
      const endTime = performance.now();
      transparentTimes.push(endTime - startTime);
    }
    
    // Test without transparency (forced fast path)
    console.log('  Testing WITHOUT transparency (fast path):');
    const restoreTransparency = createNonTransparentVersion(reader);
    
    const nonTransparentTimes = [];
    for (let run = 0; run < iterations; run++) {
      const startTime = performance.now();
      for (let i = 0; i < framesToTest; i++) {
        reader.decodeAndBlitFrameRGBA(i, pixels);
      }
      const endTime = performance.now();
      nonTransparentTimes.push(endTime - startTime);
    }
    
    restoreTransparency(); // Restore original state
    
    // Calculate results
    const avgTransparent = transparentTimes.reduce((a, b) => a + b) / transparentTimes.length;
    const avgNonTransparent = nonTransparentTimes.reduce((a, b) => a + b) / nonTransparentTimes.length;
    const improvement = ((avgTransparent - avgNonTransparent) / avgTransparent) * 100;
    
    console.log(`\n  Results for ${framesToTest} frames:`);
    console.log(`    With transparency checks: ${avgTransparent.toFixed(2)}ms`);
    console.log(`    Without transparency (fast): ${avgNonTransparent.toFixed(2)}ms`);
    console.log(`    Performance improvement: ${improvement.toFixed(1)}%`);
    console.log(`    Per-frame savings: ${((avgTransparent - avgNonTransparent) / framesToTest).toFixed(3)}ms`);
    
    if (improvement > 0) {
      console.log(`    ✅ Fast path is ${improvement.toFixed(1)}% faster!`);
    }
    
    return improvement;
    
  } catch (error) {
    console.log(`  ERROR: ${error.message}`);
    return 0;
  }
}

console.log('Comparing transparency vs non-transparency decode paths...');
console.log('This simulates the benefit of the branch elimination optimization.');

const testFiles = [
  'Dramatic Chipmunk GIF.gif',
  'GIGACHAD-4x.gif',
  'catJAM-3x.gif',
  'excuseme.gif'
];

const improvements = [];
for (const filename of testFiles) {
  const improvement = benchmarkTransparencyPaths(filename);
  improvements.push(improvement);
}

const avgImprovement = improvements.reduce((a, b) => a + b) / improvements.length;
console.log(`\n🎯 Overall Results:`);
console.log(`Average performance improvement: ${avgImprovement.toFixed(1)}%`);
console.log(`\n💡 Key optimization benefits:`);
console.log(`- Eliminated conditional branch in tight inner loop`);
console.log(`- Better CPU instruction pipelining and branch prediction`);
console.log(`- Reduced code path complexity for common non-transparent case`);
console.log(`- Direct memory writes without transparency checks`);