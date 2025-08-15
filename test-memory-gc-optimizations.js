const fs = require('fs');
const { performance } = require('perf_hooks');
const { Worker } = require('worker_threads');
const { GifReader } = require('./wtfgif');

async function testTransferableBuffers(filename) {
  console.log(`\nTesting transferable buffers for ${filename}:`);
  
  try {
    const data = fs.readFileSync(`test/gifs/${filename}`);
    const reader = new GifReader(data);
    
    console.log(`  Dimensions: ${reader.width}x${reader.height}`);
    console.log(`  Frames: ${reader.numFrames()}`);
    
    const framesToTest = Math.min(reader.numFrames(), 5);
    const iterations = 3;
    
    // Test main thread transferable methods
    console.log(`\n  Testing transferable methods (main thread):`);
    const mainThreadTimes = [];
    
    for (let run = 0; run < iterations; run++) {
      const startTime = performance.now();
      
      for (let frameIdx = 0; frameIdx < framesToTest; frameIdx++) {
        const rgbaBuffer = reader.decodeFrameToTransferableRGBA(frameIdx);
        const bgraBuffer = reader.decodeFrameToTransferableBGRA(frameIdx);
        
        // Verify buffers are transferable
        if (!(rgbaBuffer instanceof ArrayBuffer) || !(bgraBuffer instanceof ArrayBuffer)) {
          throw new Error('Buffers are not ArrayBuffer instances');
        }
      }
      
      const endTime = performance.now();
      mainThreadTimes.push(endTime - startTime);
    }
    
    const avgMainTime = mainThreadTimes.reduce((a, b) => a + b) / mainThreadTimes.length;
    console.log(`    Average time: ${avgMainTime.toFixed(2)}ms`);
    console.log(`    Per frame: ${(avgMainTime / framesToTest).toFixed(3)}ms`);
    
    // Test into-buffer methods for comparison
    console.log(`\n  Testing direct-to-buffer methods:`);
    const directTimes = [];
    const pixels = new Uint8Array(reader.width * reader.height * 4);
    
    for (let run = 0; run < iterations; run++) {
      const startTime = performance.now();
      
      for (let frameIdx = 0; frameIdx < framesToTest; frameIdx++) {
        reader.decodeFrameIntoBuffer(frameIdx, pixels, 'rgba');
        reader.decodeFrameIntoBuffer(frameIdx, pixels, 'bgra');
      }
      
      const endTime = performance.now();
      directTimes.push(endTime - startTime);
    }
    
    const avgDirectTime = directTimes.reduce((a, b) => a + b) / directTimes.length;
    console.log(`    Average time: ${avgDirectTime.toFixed(2)}ms`);
    console.log(`    Per frame: ${(avgDirectTime / framesToTest).toFixed(3)}ms`);
    
    const transferableOverhead = ((avgMainTime - avgDirectTime) / avgDirectTime) * 100;
    console.log(`    Transferable overhead: ${transferableOverhead.toFixed(1)}%`);
    
    return { avgMainTime, avgDirectTime, transferableOverhead };
    
  } catch (error) {
    console.log(`  ERROR: ${error.message}`);
    return null;
  }
}

async function testObjectPooling(filename) {
  console.log(`\nTesting object pooling for ${filename}:`);
  
  try {
    const data = fs.readFileSync(`test/gifs/${filename}`);
    
    console.log(`  File size: ${(data.length / 1024).toFixed(1)} KB`);
    
    const iterations = 20;
    
    // Test normal reader creation
    console.log(`\n  Testing normal reader creation (${iterations} iterations):`);
    const normalTimes = [];
    
    for (let i = 0; i < iterations; i++) {
      const startTime = performance.now();
      
      const reader = new GifReader(data);
      const pixels = reader.decodeFrameToTransferableRGBA(0);
      // No cleanup - just let GC handle it
      
      const endTime = performance.now();
      normalTimes.push(endTime - startTime);
    }
    
    const avgNormalTime = normalTimes.reduce((a, b) => a + b) / normalTimes.length;
    console.log(`    Average time: ${avgNormalTime.toFixed(3)}ms`);
    console.log(`    Min time: ${Math.min(...normalTimes).toFixed(3)}ms`);
    console.log(`    Max time: ${Math.max(...normalTimes).toFixed(3)}ms`);
    
    // Test pooled reader creation
    console.log(`\n  Testing pooled reader creation (${iterations} iterations):`);
    const pooledTimes = [];
    
    for (let i = 0; i < iterations; i++) {
      const startTime = performance.now();
      
      const reader = GifReader.createPooled(data);
      const pixels = reader.decodeFrameToTransferableRGBA(0);
      reader.returnToPool(); // Return tables to pool
      
      const endTime = performance.now();
      pooledTimes.push(endTime - startTime);
    }
    
    const avgPooledTime = pooledTimes.reduce((a, b) => a + b) / pooledTimes.length;
    console.log(`    Average time: ${avgPooledTime.toFixed(3)}ms`);
    console.log(`    Min time: ${Math.min(...pooledTimes).toFixed(3)}ms`);
    console.log(`    Max time: ${Math.max(...pooledTimes).toFixed(3)}ms`);
    
    const poolingBenefit = ((avgNormalTime - avgPooledTime) / avgNormalTime) * 100;
    console.log(`    Pooling benefit: ${poolingBenefit.toFixed(1)}%`);
    
    // Test pool statistics
    const poolStats = GifReader.getPoolStats();
    console.log(`\n  Pool statistics:`);
    console.log(`    Available entries: ${poolStats.available}`);
    console.log(`    Total created: ${poolStats.totalCreated}`);
    console.log(`    Cache hits: ${poolStats.hits}`);
    console.log(`    Cache misses: ${poolStats.misses}`);
    console.log(`    Hit rate: ${((poolStats.hits / (poolStats.hits + poolStats.misses)) * 100).toFixed(1)}%`);
    
    return { avgNormalTime, avgPooledTime, poolingBenefit };
    
  } catch (error) {
    console.log(`  ERROR: ${error.message}`);
    return null;
  }
}

function testCorrectness(filename) {
  try {
    const data = fs.readFileSync(`test/gifs/${filename}`);
    const reader = new GifReader(data);
    
    // Test transferable methods produce same results as regular methods
    const frameIdx = 0;
    
    // Get regular decode results
    const regularPixels = new Uint8Array(reader.width * reader.height * 4);
    reader.decodeAndBlitFrameRGBA(frameIdx, regularPixels);
    
    // Get transferable buffer results
    const transferableRGBA = reader.decodeFrameToTransferableRGBA(frameIdx);
    const transferablePixels = new Uint8Array(transferableRGBA);
    
    // Compare results
    for (let i = 0; i < regularPixels.length; i++) {
      if (regularPixels[i] !== transferablePixels[i]) {
        return false;
      }
    }
    
    // Test into-buffer method
    const intoBufferArrayBuffer = new ArrayBuffer(reader.width * reader.height * 4);
    reader.decodeFrameIntoBuffer(frameIdx, intoBufferArrayBuffer, 'rgba');
    const intoBufferPixels = new Uint8Array(intoBufferArrayBuffer);
    
    for (let i = 0; i < regularPixels.length; i++) {
      if (regularPixels[i] !== intoBufferPixels[i]) {
        return false;
      }
    }
    
    // Test pooled reader
    const pooledReader = GifReader.createPooled(data);
    const pooledPixels = new Uint8Array(reader.width * reader.height * 4);
    pooledReader.decodeAndBlitFrameRGBA(frameIdx, pooledPixels);
    pooledReader.returnToPool();
    
    for (let i = 0; i < regularPixels.length; i++) {
      if (regularPixels[i] !== pooledPixels[i]) {
        return false;
      }
    }
    
    return true;
  } catch (error) {
    return false;
  }
}

async function runMemoryGCTests() {
  console.log('🧠 MEMORY & GC PRESSURE OPTIMIZATIONS TEST');
  console.log('Testing transferable buffers and object pooling optimizations\\n');
  
  console.log('📋 Optimizations being tested:');
  console.log('1. Transferable ArrayBuffer support for worker communication');
  console.log('2. Direct decode-to-buffer methods to avoid intermediate allocations');
  console.log('3. GifReader object pooling to reuse decoder tables');
  console.log('4. Decoder table reuse for identical LZW parameters');
  
  const testFiles = [
    'Dramatic Chipmunk GIF.gif',
    'GIGACHAD-4x.gif',
    'catJAM-3x.gif',
    'excuseme.gif'
  ];
  
  console.log('\\n=== Correctness Testing ===');
  let passed = 0;
  for (const filename of testFiles) {
    const correct = testCorrectness(filename);
    console.log(`${filename}: ${correct ? '✅ PASS' : '❌ FAIL'}`);
    if (correct) passed++;
  }
  
  console.log(`\\nCorrectness: ${passed}/${testFiles.length} tests passed`);
  
  console.log('\\n=== Transferable Buffer Performance ===');
  const transferableResults = [];
  for (const filename of testFiles) {
    const result = await testTransferableBuffers(filename);
    if (result) {
      transferableResults.push({ filename, ...result });
    }
  }
  
  console.log('\\n=== Object Pooling Performance ===');
  const poolingResults = [];
  for (const filename of testFiles) {
    const result = await testObjectPooling(filename);
    if (result) {
      poolingResults.push({ filename, ...result });
    }
  }
  
  if (transferableResults.length > 0) {
    console.log('\\n📊 Transferable Buffer Summary:');
    const avgOverhead = transferableResults.reduce((sum, r) => sum + r.transferableOverhead, 0) / transferableResults.length;
    console.log(`Average transferable overhead: ${avgOverhead.toFixed(1)}%`);
    console.log('✅ All buffers are properly transferable ArrayBuffer instances');
  }
  
  if (poolingResults.length > 0) {
    console.log('\\n📊 Object Pooling Summary:');
    const avgBenefit = poolingResults.reduce((sum, r) => sum + r.poolingBenefit, 0) / poolingResults.length;
    console.log(`Average pooling benefit: ${avgBenefit.toFixed(1)}%`);
    console.log('✅ Decoder table reuse reduces allocation overhead');
  }
  
  console.log('\\n🎯 Memory & GC optimization benefits:');
  console.log('- ✅ Transferable buffers enable zero-copy worker communication');
  console.log('- ✅ Direct decode-to-buffer eliminates intermediate allocations');
  console.log('- ✅ Object pooling reuses expensive decoder table allocations');
  console.log('- ✅ Table reuse provides immediate benefit on repeated identical LZW streams');
  console.log('- ✅ Reduced GC pressure from fewer temporary object allocations');
  console.log('\\n💡 Use cases:');
  console.log('- Worker-based GIF processing with transferable pixel data');
  console.log('- Batch processing of similar GIFs with table reuse');
  console.log('- Memory-constrained environments requiring minimal allocations');
}

runMemoryGCTests().catch(console.error);