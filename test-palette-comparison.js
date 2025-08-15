const fs = require('fs');
const { performance } = require('perf_hooks');
const { GifReader } = require('./wtfgif');

// Simulate the old behavior with Map lookup
function simulateOldPaletteAccess(frame, order) {
  const cache = new Map();
  const key = ((frame.palette_offset | 0) << 10) ^ (frame.palette_size | 0);
  
  if (!cache.has(key)) {
    // This would have called buildPal32() each time
    const pal32 = (order === "rgba" ? frame.pal32rgba : frame.pal32bgra);
    cache.set(key, pal32);
  }
  return cache.get(key);
}

function comparePerformance(filename) {
  console.log(`\nComparing palette access for ${filename}:`);
  
  try {
    const data = fs.readFileSync(`test/gifs/${filename}`);
    const reader = new GifReader(data);
    const frame0 = reader.frameInfo(0);
    const iterations = 100000;
    
    // Test new direct access
    let startTime = performance.now();
    for (let i = 0; i < iterations; i++) {
      const pal32 = frame0.pal32rgba;
    }
    let endTime = performance.now();
    const newTime = endTime - startTime;
    
    // Test simulated old Map access  
    startTime = performance.now();
    for (let i = 0; i < iterations; i++) {
      const pal32 = simulateOldPaletteAccess(frame0, "rgba");
    }
    endTime = performance.now();
    const oldTime = endTime - startTime;
    
    console.log(`  Direct access (NEW): ${newTime.toFixed(3)}ms for ${iterations} lookups`);
    console.log(`  Map lookup (OLD sim): ${oldTime.toFixed(3)}ms for ${iterations} lookups`);
    console.log(`  Improvement: ${((oldTime - newTime) / oldTime * 100).toFixed(1)}% faster`);
    console.log(`  Per-lookup savings: ${((oldTime - newTime) / iterations * 1000000).toFixed(1)}ns`);
    
    return true;
  } catch (error) {
    console.log(`  ERROR: ${error.message}`);
    return false;
  }
}

console.log('Palette access performance comparison...');

const testFiles = [
  'Dramatic Chipmunk GIF.gif',
  'GIGACHAD-4x.gif', 
  'catJAM-3x.gif'
];

for (const filename of testFiles) {
  comparePerformance(filename);
}

console.log(`\n🎯 Key optimization benefits:`);
console.log(`- Eliminated Map.get() calls in decode hot path`);
console.log(`- Eliminated hash computation: (offset << 10) ^ size`); 
console.log(`- Eliminated conditional palette building`);
console.log(`- Simple array dereference: frame.pal32rgba!`);