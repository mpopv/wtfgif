// decoder-worker.js
// Per-worker Wasm instance with dedicated LZW tables for parallel frame decode

// Import the main wtfgif module
importScripts('./wtfgif.js');

let wasmInitialized = false;
let workerInstance = null;
let workerId = 0;

// Initialize per-worker Wasm instance and LZW tables
async function initializeWorker(id) {
  workerId = id;
  
  try {
    // Initialize WebAssembly for this worker
    await wtfgif.initializeWasmGlobally();
    
    // Each worker gets its own instance to avoid contention
    const wasmStatus = wtfgif.getWasmStatus();
    if (wasmStatus.supported) {
      wasmInitialized = true;
      console.log(`Worker ${workerId}: WebAssembly initialized`);
    } else {
      console.log(`Worker ${workerId}: WebAssembly not available, using JavaScript fallback`);
    }
    
    // Pre-warm with a test decode to ensure everything is ready
    const testGifData = createMinimalTestGif();
    workerInstance = new wtfgif.GifReader(testGifData);
    
    // Test decode to ensure worker is ready
    if (wasmInitialized) {
      await workerInstance.initWasm();
    }
    
    console.log(`Worker ${workerId}: Ready for frame decode`);
    
  } catch (error) {
    console.error(`Worker ${workerId}: Initialization failed:`, error);
    wasmInitialized = false;
  }
}

// Fast frame decode using per-worker Wasm instance
function decodeFrame(frameData) {
  const { gifData, frameIndex, requestId } = frameData;
  
  try {
    // Create reader instance for this GIF
    const reader = new wtfgif.GifReader(gifData);
    
    let pixels;
    let decodeTime;
    const startTime = performance.now();
    
    if (wasmInitialized && reader.isWasmReady()) {
      // Use fast Wasm path with dedicated worker tables
      pixels = reader.framePixelsWasm(frameIndex);
      decodeTime = performance.now() - startTime;
    } else {
      // Fallback to JavaScript with optimized LZW tables
      pixels = reader.framePixels(frameIndex);
      decodeTime = performance.now() - startTime;
    }
    
    // Get frame timing information
    const frameInfo = reader.frameInfo(frameIndex);
    const delay = frameInfo.delay || 100;
    
    return {
      requestId,
      frameIndex,
      pixels: pixels.buffer, // Transfer ownership
      delay,
      decodeTime,
      workerId,
      wasmUsed: wasmInitialized && reader.isWasmReady()
    };
    
  } catch (error) {
    return {
      requestId,
      frameIndex,
      error: error.message,
      workerId
    };
  }
}

// Message handler for frame decode requests
self.onmessage = async function(event) {
  const { data, type } = event.data;
  
  if (type === 'init') {
    // Initialize this worker with a unique ID
    await initializeWorker(data.workerId);
    self.postMessage({ 
      type: 'init-complete', 
      workerId, 
      wasmAvailable: wasmInitialized 
    });
    return;
  }
  
  if (type === 'decode') {
    // Decode frame using per-worker Wasm instance
    const result = decodeFrame(data);
    
    if (result.pixels) {
      // Transfer ownership of pixel buffer back to main thread
      self.postMessage({ 
        type: 'decode-complete', 
        data: result 
      }, [result.pixels]);
    } else {
      // Error case - no transferable objects
      self.postMessage({ 
        type: 'decode-error', 
        data: result 
      });
    }
    return;
  }
  
  if (type === 'terminate') {
    // Cleanup worker resources
    if (workerInstance) {
      workerInstance.cleanupZeroCopyBuffers?.();
    }
    self.close();
    return;
  }
};

// Helper function to create minimal test GIF for worker validation
function createMinimalTestGif() {
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

// Performance monitoring
let frameCount = 0;
let totalDecodeTime = 0;

setInterval(() => {
  if (frameCount > 0) {
    const avgDecodeTime = totalDecodeTime / frameCount;
    console.log(`Worker ${workerId}: Processed ${frameCount} frames, avg decode time: ${avgDecodeTime.toFixed(2)}ms`);
  }
}, 10000); // Report every 10 seconds