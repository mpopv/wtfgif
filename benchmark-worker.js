// Web Worker for GIF benchmarking
importScripts('node_modules/omggif/omggif.js');
importScripts('wtfgif-browser.js');

// Store omggif constructors before they get overwritten
const omggif = { GifReader: self.GifReader, GifWriter: self.GifWriter };

self.onmessage = function(e) {
    const { gifData, library, iterations, testId } = e.data;
    
    try {
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
            library,
            testId
        });
    }
};