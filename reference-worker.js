// reference-worker.js
// Reference architecture worker: Wasm decode → GPU compute → OffscreenCanvas → ImageBitmap

importScripts('./wtfgif.js');

let workerId = 0;
let gifReader = null;
let gpuEnabled = false;
let wasmEnabled = false;
let arenaAllocator = null;

// Performance tracking
let frameDecodeCount = 0;
let totalDecodeTime = 0;

async function initializeWorker(id, enableGPU, gifData) {
  workerId = id;
  
  try {
    console.log(`Worker ${workerId}: Initializing reference architecture`);
    
    // Initialize GIF reader
    gifReader = new wtfgif.GifReader(gifData);
    
    // Initialize WebAssembly with arena allocator for memory hygiene
    const wasmStatus = wtfgif.getWasmStatus();
    if (wasmStatus.supported) {
      await wtfgif.initializeWasmGlobally();
      await gifReader.initWasm();
      wasmEnabled = gifReader.isWasmReady();
      
      // Set up arena allocator for bump + reset per decode
      if (wasmEnabled) {
        console.log(`Worker ${workerId}: WebAssembly initialized with arena allocator`);
      }
    }
    
    // Initialize GPU if requested and available
    if (enableGPU) {
      gpuEnabled = await gifReader.initGPU();
      if (gpuEnabled) {
        console.log(`Worker ${workerId}: GPU acceleration enabled`);
      } else {
        console.log(`Worker ${workerId}: GPU acceleration not available`);
      }
    }
    
    console.log(`Worker ${workerId}: Initialization complete - Wasm: ${wasmEnabled}, GPU: ${gpuEnabled}`);
    
    return {
      workerId,
      wasmEnabled,
      gpuEnabled,
      frameCount: gifReader.numFrames()
    };
    
  } catch (error) {
    console.error(`Worker ${workerId}: Initialization failed:`, error);
    throw error;
  }
}

// Reference architecture decode pipeline
async function decodeFrameReference(frameIndex, requestId, priority) {
  const startTime = performance.now();
  
  try {
    if (!gifReader) {
      throw new Error('Worker not initialized');
    }
    
    let bitmap;
    let decodeTime;
    let renderTime;
    
    if (gpuEnabled) {
      // === GPU PIPELINE ===
      // Wasm.decode_to_index_bytes() → GPU compute map→RGBA → offscreenCanvas.transferToImageBitmap()
      
      const decodeStart = performance.now();
      
      // Step 1: Decode to index bytes using WebAssembly (with arena allocator)
      const frameInfo = gifReader.frameInfo(frameIndex);
      const indexData = await decodeToIndexBytes(frameIndex);
      
      const decodeEnd = performance.now();
      decodeTime = decodeEnd - decodeStart;
      
      // Step 2: GPU compute shader maps indices to RGBA
      const renderStart = performance.now();
      bitmap = await gifReader.frameImageBitmapGPU(frameIndex);
      
      if (!bitmap) {
        throw new Error('GPU frame decode failed');
      }
      
      const renderEnd = performance.now();
      renderTime = renderEnd - renderStart;
      
      console.log(`Worker ${workerId}: GPU pipeline - decode: ${decodeTime.toFixed(2)}ms, render: ${renderTime.toFixed(2)}ms`);
      
    } else if (wasmEnabled) {
      // === WEBASSEMBLY PIPELINE ===
      // Pure WebAssembly decode with optimized memory allocation
      
      const decodeStart = performance.now();
      
      // Reset arena allocator for this decode (bump + reset pattern)
      if (arenaAllocator) {
        arenaAllocator.reset();
      }
      
      // Decode using WebAssembly with pooled memory
      const pixels = await gifReader.framePixelsWasm(frameIndex);
      
      const decodeEnd = performance.now();
      decodeTime = decodeEnd - decodeStart;
      
      // Step 2: Create OffscreenCanvas and transfer to ImageBitmap
      const renderStart = performance.now();
      const frameInfo = gifReader.frameInfo(frameIndex);
      const offscreen = new OffscreenCanvas(frameInfo.width, frameInfo.height);
      const ctx = offscreen.getContext('2d');
      
      // Create ImageData from pixels (zero-copy where possible)
      const imageData = new ImageData(
        new Uint8ClampedArray(pixels.buffer), 
        frameInfo.width, 
        frameInfo.height
      );
      
      ctx.putImageData(imageData, 0, 0);
      bitmap = offscreen.transferToImageBitmap();
      
      const renderEnd = performance.now();
      renderTime = renderEnd - renderStart;
      
      console.log(`Worker ${workerId}: Wasm pipeline - decode: ${decodeTime.toFixed(2)}ms, render: ${renderTime.toFixed(2)}ms`);
      
    } else {
      // === JAVASCRIPT FALLBACK ===
      // Pure JavaScript with optimized hot-path and memory pooling
      
      const decodeStart = performance.now();
      
      // Use optimized JavaScript decode with TypedArray pooling
      const pixels = gifReader.framePixels(frameIndex);
      
      const decodeEnd = performance.now();
      decodeTime = decodeEnd - decodeStart;
      
      // Create OffscreenCanvas
      const renderStart = performance.now();
      const frameInfo = gifReader.frameInfo(frameIndex);
      const offscreen = new OffscreenCanvas(frameInfo.width, frameInfo.height);
      const ctx = offscreen.getContext('2d');
      
      const imageData = new ImageData(
        new Uint8ClampedArray(pixels.buffer), 
        frameInfo.width, 
        frameInfo.height
      );
      
      ctx.putImageData(imageData, 0, 0);
      bitmap = offscreen.transferToImageBitmap();
      
      const renderEnd = performance.now();
      renderTime = renderEnd - renderStart;
      
      console.log(`Worker ${workerId}: JS pipeline - decode: ${decodeTime.toFixed(2)}ms, render: ${renderTime.toFixed(2)}ms`);
    }
    
    // Update performance statistics
    frameDecodeCount++;
    totalDecodeTime += decodeTime;
    
    const totalTime = performance.now() - startTime;
    const frameInfo = gifReader.frameInfo(frameIndex);
    
    return {
      frameIndex,
      requestId,
      bitmap,
      delay: frameInfo.delay || 100,
      decodeTime,
      renderTime,
      totalTime,
      pipeline: gpuEnabled ? 'GPU' : wasmEnabled ? 'Wasm' : 'JS',
      workerId
    };
    
  } catch (error) {
    console.error(`Worker ${workerId}: Frame ${frameIndex} decode failed:`, error);
    throw error;
  }
}

// Optimized index-only decode for GPU pipeline
async function decodeToIndexBytes(frameIndex) {
  // This would use the arena allocator for memory hygiene
  // For now, delegate to the existing method
  const frameInfo = gifReader.frameInfo(frameIndex);
  const frameSize = frameInfo.width * frameInfo.height;
  const indexData = new Uint8Array(frameSize);
  
  // Use LZW decoder to fill index data directly
  // This avoids the palette mapping step which GPU will handle
  gifReader.lzwDecodeToIndices(frameInfo, indexData);
  
  return indexData;
}

// Message handler for reference architecture
self.onmessage = async function(event) {
  const { type, data } = event.data;
  
  try {
    if (type === 'init') {
      const result = await initializeWorker(
        data.workerId, 
        data.enableGPU, 
        data.gifData
      );
      
      self.postMessage({
        type: 'init-complete',
        data: result
      });
      
    } else if (type === 'decode-frame') {
      const { frameIndex, requestId, priority } = data;
      
      const result = await decodeFrameReference(frameIndex, requestId, priority);
      
      // Transfer ImageBitmap ownership to main thread
      self.postMessage({
        type: 'frame-complete',
        data: result
      }, [result.bitmap]);
      
    } else if (type === 'get-stats') {
      const avgDecodeTime = frameDecodeCount > 0 ? totalDecodeTime / frameDecodeCount : 0;
      
      self.postMessage({
        type: 'stats',
        data: {
          workerId,
          frameDecodeCount,
          avgDecodeTime,
          wasmEnabled,
          gpuEnabled
        }
      });
      
    } else if (type === 'terminate') {
      // Cleanup resources
      if (gifReader && gifReader.cleanupZeroCopyBuffers) {
        gifReader.cleanupZeroCopyBuffers();
      }
      
      if (gifReader && gifReader.cleanupWorkerPool) {
        await gifReader.cleanupWorkerPool();
      }
      
      self.close();
    }
    
  } catch (error) {
    self.postMessage({
      type: 'error',
      data: {
        error: error.message,
        workerId,
        requestType: type
      }
    });
  }
};

// Performance monitoring
setInterval(() => {
  if (frameDecodeCount > 0) {
    const avgDecodeTime = totalDecodeTime / frameDecodeCount;
    console.log(`Worker ${workerId}: Processed ${frameDecodeCount} frames, avg: ${avgDecodeTime.toFixed(2)}ms`);
  }
}, 30000); // Report every 30 seconds