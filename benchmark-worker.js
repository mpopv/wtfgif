// Web Worker for GIF benchmarking and memory optimization testing
importScripts('node_modules/omggif/omggif.js');
importScripts('wtfgif-browser.js');

// Store omggif constructors before they get overwritten
const omggif = { GifReader: self.GifReader, GifWriter: self.GifWriter };

self.onmessage = (event) => {
  const messageData = event.data;
  const { type, data } = messageData;
  
  // Handle legacy format (direct data without type wrapper)
  if (!type && messageData.gifData && messageData.library) {
    handleLegacyBenchmark(messageData);
    return;
  }
  
  if (type === 'transferable_benchmark') {
    try {
      const { gifBuffer, iterations, frameCount } = data;
      
      // Create reader from transferable buffer
      const reader = new self.wtfgif.GifReader(new Uint8Array(gifBuffer));
      
      const results = [];
      const startTime = performance.now();
      
      for (let run = 0; run < iterations; run++) {
        const runStartTime = performance.now();
        
        for (let frameIdx = 0; frameIdx < Math.min(frameCount, reader.numFrames()); frameIdx++) {
          // Test transferable buffer methods
          const rgbaBuffer = reader.decodeFrameToTransferableRGBA(frameIdx);
          const bgraBuffer = reader.decodeFrameToTransferableBGRA(frameIdx);
          
          // Send back with transferable ownership
          self.postMessage({
            type: 'frame_result',
            frameIdx,
            run,
            rgbaSize: rgbaBuffer.byteLength,
            bgraSize: bgraBuffer.byteLength
          }, [rgbaBuffer, bgraBuffer]);
        }
        
        const runEndTime = performance.now();
        results.push(runEndTime - runStartTime);
      }
      
      const totalTime = performance.now() - startTime;
      
      self.postMessage({
        type: 'benchmark_complete',
        results: {
          iterations,
          frameCount: Math.min(frameCount, reader.numFrames()),
          totalTime,
          avgTime: results.reduce((a, b) => a + b) / results.length,
          minTime: Math.min(...results),
          maxTime: Math.max(...results),
          runTimes: results
        }
      });
      
    } catch (error) {
      self.postMessage({
        type: 'error',
        message: error.message,
        stack: error.stack
      });
    }
    
  } else if (type === 'pool_benchmark') {
    try {
      const { gifBuffer, iterations } = data;
      
      // Test object pooling with repeated reader creation
      const poolResults = [];
      
      for (let i = 0; i < iterations; i++) {
        const startTime = performance.now();
        
        // Create pooled reader
        const reader = self.wtfgif.GifReader.createPooled(new Uint8Array(gifBuffer));
        
        // Decode first frame to test tables
        const buffer = reader.decodeFrameToTransferableRGBA(0);
        
        // Return to pool
        reader.returnToPool();
        
        const endTime = performance.now();
        poolResults.push(endTime - startTime);
        
        // Transfer the buffer back
        self.postMessage({
          type: 'pool_frame',
          iteration: i,
          bufferSize: buffer.byteLength
        }, [buffer]);
      }
      
      self.postMessage({
        type: 'pool_complete',
        results: {
          iterations,
          avgTime: poolResults.reduce((a, b) => a + b) / poolResults.length,
          minTime: Math.min(...poolResults),
          times: poolResults
        }
      });
      
    } catch (error) {
      self.postMessage({
        type: 'error', 
        message: error.message,
        stack: error.stack
      });
    }
    
  }
};

function handleLegacyBenchmark(messageData) {
  try {
    const { gifData, library, iterations, testId } = messageData;
    
    // Create reader once and reuse
    const data = new Uint8Array(gifData);
    const reader = library === 'omggif' 
      ? new omggif.GifReader(data)
      : new self.wtfgif.GifReader(data);
    
    // Pre-allocate reusable buffer
    const pixels = new Uint8Array(reader.width * reader.height * 4);
    const numFrames = reader.numFrames();
    
    // Warm up - 2 unmeasured runs to let JIT specialize
    for (let w = 0; w < 2; w++) {
      for (let frame = 0; frame < numFrames; frame++) {
        reader.decodeAndBlitFrameRGBA(frame, pixels);
      }
    }
    
    const times = [];
    
    // Now measure only decode performance
    for (let i = 0; i < iterations; i++) {
      const start = performance.now();
      
      for (let frame = 0; frame < numFrames; frame++) {
        reader.decodeAndBlitFrameRGBA(frame, pixels);
      }
      
      times.push(performance.now() - start);
    }
    
    const result = {
      mean: times.reduce((a, b) => a + b) / times.length,
      min: Math.min(...times),
      max: Math.max(...times),
      times: times
    };
    
    self.postMessage({
      success: true,
      result,
      library,
      testId
    });
    
  } catch (error) {
    self.postMessage({
      success: false,
      error: error.message,
      library: messageData.library,
      testId: messageData.testId
    });
  }
}