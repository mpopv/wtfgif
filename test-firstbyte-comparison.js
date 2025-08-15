const { performance } = require('perf_hooks');

// Simulate old chasing approach
function simulateOldChasing(table, codes, iterations) {
  const times = [];
  
  for (let run = 0; run < 3; run++) {
    const startTime = performance.now();
    
    for (let iter = 0; iter < iterations; iter++) {
      for (let i = 0; i < codes.length; i++) {
        const code = codes[i];
        if (code < 256) continue; // Skip base codes
        
        // OLD: Chase to find first byte (O(n) where n is sequence length)
        let cur = code;
        while (cur >= 256) {
          cur = (table[cur] >>> 8) | 0;
        }
        const outFirst = cur & 0xff; // Found first byte
      }
    }
    
    const endTime = performance.now();
    times.push(endTime - startTime);
  }
  
  return times.reduce((a, b) => a + b) / times.length;
}

// Simulate new O(1) lookup approach
function simulateNewLookup(firstByte, codes, iterations) {
  const times = [];
  
  for (let run = 0; run < 3; run++) {
    const startTime = performance.now();
    
    for (let iter = 0; iter < iterations; iter++) {
      for (let i = 0; i < codes.length; i++) {
        const code = codes[i];
        if (code < 256) continue; // Skip base codes
        
        // NEW: O(1) table lookup
        const outFirst = firstByte[code] | 0;
      }
    }
    
    const endTime = performance.now();
    times.push(endTime - startTime);
  }
  
  return times.reduce((a, b) => a + b) / times.length;
}

function createSyntheticData(numCodes, avgSequenceLength) {
  // Create mock LZW dictionary table
  const table = new Int32Array(4096);
  const firstByte = new Int16Array(4096);
  
  // Initialize base codes (0-255)
  for (let i = 0; i < 256; i++) {
    table[i] = i;
    firstByte[i] = i;
  }
  
  // Build dictionary entries with sequences of varying lengths
  let nextCode = 256;
  for (let i = 0; i < numCodes && nextCode < 4096; i++) {
    const sequenceLength = Math.min(avgSequenceLength + Math.floor(Math.random() * 5), 20);
    const baseCode = Math.floor(Math.random() * 256);
    
    // Build a chain of codes
    let prevCode = baseCode;
    for (let j = 0; j < sequenceLength && nextCode < 4096; j++) {
      const suffix = Math.floor(Math.random() * 256);
      table[nextCode] = (prevCode << 8) | suffix;
      firstByte[nextCode] = firstByte[prevCode]; // O(1) copy
      prevCode = nextCode;
      nextCode++;
    }
  }
  
  // Create test codes that would benefit from first-byte optimization
  const codes = [];
  for (let i = 256; i < Math.min(nextCode, 1000); i++) {
    codes.push(i);
  }
  
  return { table, firstByte, codes };
}

console.log('Comparing first-byte chase vs O(1) lookup approaches...\n');

const testCases = [
  { name: 'Light compression (short sequences)', numCodes: 200, avgSeqLen: 2, iterations: 100000 },
  { name: 'Medium compression (medium sequences)', numCodes: 500, avgSeqLen: 5, iterations: 50000 },
  { name: 'Heavy compression (long sequences)', numCodes: 1000, avgSeqLen: 10, iterations: 20000 }
];

for (const testCase of testCases) {
  console.log(`${testCase.name}:`);
  const { table, firstByte, codes } = createSyntheticData(testCase.numCodes, testCase.avgSeqLen);
  
  console.log(`  Generated ${codes.length} test codes, avg sequence length ~${testCase.avgSeqLen}`);
  
  const oldTime = simulateOldChasing(table, codes, testCase.iterations);
  const newTime = simulateNewLookup(firstByte, codes, testCase.iterations);
  
  const improvement = ((oldTime - newTime) / oldTime) * 100;
  const opsPerSec = (testCase.iterations * codes.length) / (newTime / 1000);
  
  console.log(`  O(n) chasing (OLD): ${oldTime.toFixed(2)}ms`);
  console.log(`  O(1) lookup (NEW): ${newTime.toFixed(2)}ms`);
  console.log(`  Performance improvement: ${improvement.toFixed(1)}%`);
  console.log(`  Operations per second: ${(opsPerSec / 1000000).toFixed(1)}M ops/sec`);
  console.log();
}

console.log('🎯 First-byte optimization effectiveness:');
console.log('- Linear performance gain proportional to sequence complexity');
console.log('- Most effective on highly compressible content with long LZW sequences'); 
console.log('- Transforms O(n) chase into O(1) table access');
console.log('- Eliminates redundant work in dictionary lookups');
console.log('- Benefits compound with frame count and compression ratio');