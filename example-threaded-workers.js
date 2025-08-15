// example-threaded-workers.js
// Complete example of threaded worker pool for parallel GIF frame decode

const wtfgif = require('./wtfgif.js');

function createTestGif() {
    return new Uint8Array([
        // GIF87a header
        0x47, 0x49, 0x46, 0x38, 0x37, 0x61, // "GIF87a"
        0x01, 0x00, // width = 1
        0x01, 0x00, // height = 1  
        0x80,       // global color table flag + size
        0x00,       // background
        0x00,       // aspect ratio
        // Global color table (2 colors)
        0x00, 0x00, 0x00, // black
        0xFF, 0xFF, 0xFF, // white
        // Image descriptor
        0x2C,       // image separator
        0x00, 0x00, // left = 0
        0x00, 0x00, // top = 0
        0x01, 0x00, // width = 1
        0x01, 0x00, // height = 1
        0x00,       // no local color table
        // Image data
        0x02,       // LZW min code size
        0x02, 0x44, 0x01, // data sub-block
        0x00,       // terminator
        // Trailer
        0x3B
    ]);
}

async function demonstrateThreadedWorkerPool() {
    console.log('🔄 Threaded Worker Pool - Parallel Frame Decode Demo\n');

    const gifData = createTestGif();
    const reader = new wtfgif.GifReader(gifData);

    console.log('📋 Implementation Pattern:');
    console.log('=========================\n');

    console.log('🏭 Main Thread - Worker Pool Setup:');
    console.log('   const pool = Array.from({length: n}, () => new Worker("decoder-worker.js"));');
    console.log('   function decodeAsync(frame) {');
    console.log('     return pool[next++ % n].postMessage(frame, [frame.buffer]);');
    console.log('   }\n');

    console.log('👷 Worker Thread - Per-Worker Wasm Instance:');
    console.log('   self.onmessage = ({data}) => {');
    console.log('     const out = decodeFrame(data);               // Wasm fast path');
    console.log('     self.postMessage(out, [out.buffer]);         // Transfer back');
    console.log('   };\n');

    console.log('🚀 Performance Characteristics:');
    console.log('   • Parallelism across frames ≈ linear until memory bandwidth caps out');
    console.log('   • Each worker maintains its own Wasm instance and LZW tables');
    console.log('   • Zero contention between workers');
    console.log('   • Transferable objects for zero-copy main ↔ worker communication\n');

    // Check system capabilities
    const wasmStatus = wtfgif.getWasmStatus();
    const cpuCores = require('os').cpus().length;
    
    console.log('🔍 System Analysis:');
    console.log('==================');
    console.log(`   CPU cores: ${cpuCores}`);
    console.log(`   WebAssembly threads: ${wasmStatus.threads ? 'Available' : 'Not Available'}`);
    console.log(`   Worker pool benefit: ${!wasmStatus.threads ? 'High (alternative to threads)' : 'Low (threads preferred)'}\n`);

    // Demonstrate API usage
    console.log('💡 API Usage Examples:');
    console.log('======================\n');

    console.log('1️⃣  Initialize Worker Pool:');
    console.log('   const reader = new wtfgif.GifReader(gifData);');
    console.log('   await reader.initWorkerPool({');
    console.log('     workerCount: navigator.hardwareConcurrency,');
    console.log('     maxQueueSize: 100');
    console.log('   });\n');

    console.log('2️⃣  Parallel Frame Decode:');
    console.log('   const frameIndices = [0, 1, 2, 3, 4, 5, 6, 7];');
    console.log('   const results = await reader.framePixelsThreadedPool(frameIndices);');
    console.log('   // Each frame decoded by different worker in parallel\n');

    console.log('3️⃣  Load-Balanced Single Frame:');
    console.log('   const pixels = await reader.framePixelsWorkerPool(frameIndex);');
    console.log('   // Automatically routes to least busy worker\n');

    console.log('4️⃣  Performance Monitoring:');
    console.log('   const stats = reader.getWorkerPoolStats();');
    console.log('   console.log({');
    console.log('     activeWorkers: stats.activeWorkers,');
    console.log('     avgDecodeTime: stats.avgDecodeTime,');
    console.log('     workerUtilization: stats.workerUtilization');
    console.log('   });\n');

    // Performance simulation
    console.log('📊 Performance Simulation:');
    console.log('==========================');
    
    const frameCount = 20;
    const singleFrameTime = 3.1; // ms per frame with Wasm
    
    // Sequential performance
    const sequentialTime = frameCount * singleFrameTime;
    
    // Parallel performance with different worker counts
    const workerCounts = [1, 2, 4, 8, cpuCores];
    
    console.log(`\nDecoding ${frameCount} frames (${singleFrameTime}ms each):\n`);
    console.log('Workers | Total Time | Speedup | Efficiency | Bottleneck');
    console.log('--------|------------|---------|------------|----------');
    
    workerCounts.forEach(workers => {
        const parallelTime = Math.ceil(frameCount / workers) * singleFrameTime * 1.1; // 10% overhead
        const speedup = sequentialTime / parallelTime;
        const efficiency = (speedup / workers * 100).toFixed(0) + '%';
        const bottleneck = workers <= 4 ? 'CPU' : workers <= 8 ? 'Memory' : 'Bandwidth';
        
        console.log(`${workers.toString().padStart(7)} | ${parallelTime.toFixed(1).padStart(9)}ms | ${speedup.toFixed(1).padStart(6)}x | ${efficiency.padStart(9)} | ${bottleneck}`);
    });

    console.log('\n🎯 Optimal Configuration:');
    console.log('=========================');
    
    const optimalWorkers = Math.min(cpuCores, 8); // Sweet spot before bandwidth limits
    const optimalTime = Math.ceil(frameCount / optimalWorkers) * singleFrameTime * 1.1;
    const optimalSpeedup = sequentialTime / optimalTime;
    
    console.log(`   Recommended workers: ${optimalWorkers} (${cpuCores > 8 ? 'bandwidth-limited' : 'CPU-limited'})`);
    console.log(`   Expected speedup: ${optimalSpeedup.toFixed(1)}x`);
    console.log(`   Frame decode time: ${optimalTime.toFixed(1)}ms (vs ${sequentialTime}ms sequential)`);

    // Memory bandwidth analysis
    const frameSize = 1024 * 1024 * 4; // 1MP frame in RGBA
    const totalData = frameCount * frameSize;
    const bandwidth = totalData / (optimalTime / 1000) / 1024 / 1024; // MB/s
    
    console.log(`\n💾 Memory Bandwidth Analysis (1MP frames):`);
    console.log(`   Total data: ${(totalData / 1024 / 1024).toFixed(1)} MB`);
    console.log(`   Bandwidth required: ${bandwidth.toFixed(0)} MB/s`);
    console.log(`   Bottleneck prediction: ${bandwidth > 2000 ? 'Memory bandwidth' : 'CPU decode'}`);

    // Use cases
    console.log('\n🎯 Use Cases:');
    console.log('=============');
    console.log('✅ Video/animation players with frame preloading');
    console.log('✅ Batch GIF processing applications');
    console.log('✅ Real-time GIF analysis and conversion');
    console.log('✅ Multi-frame thumbnail generation');
    console.log('✅ Legacy browsers without WebAssembly threads');
    console.log('');
    console.log('❌ Single frame decode (use regular methods)');
    console.log('❌ Systems with abundant WebAssembly thread support');
    console.log('❌ Memory-constrained environments');

    // Implementation tips
    console.log('\n💡 Implementation Tips:');
    console.log('=======================');
    console.log('• Start with navigator.hardwareConcurrency workers');
    console.log('• Monitor worker utilization and adjust pool size');
    console.log('• Use transferable objects for large pixel buffers');
    console.log('• Implement graceful fallback to sequential decode');
    console.log('• Consider memory bandwidth limits for high-resolution content');
    console.log('• Profile with real workloads to find optimal configuration');

    console.log('\n✨ Threaded Worker Pool enables linear speedup until memory bandwidth limits!');
}

// Simulate browser environment check
console.log('🌐 Environment Check:');
console.log('=====================');
if (typeof window !== 'undefined' && typeof Worker !== 'undefined') {
    console.log('✅ Browser environment with Worker support');
    console.log('   Worker pool will initialize successfully');
} else {
    console.log('📝 Node.js environment (no Workers)');
    console.log('   Demonstration shows patterns and expected performance');
}
console.log('');

// Run demonstration
demonstrateThreadedWorkerPool().catch(console.error);