// wasmWorker.js
// WebAssembly Worker for full LZW decoder with SharedArrayBuffer support

let wasmModule = null;
let wasmDecoder = null;

// Worker message handler
self.onmessage = async function(event) {
    const { data } = event;
    
    try {
        // Handle different message types
        if (data.type === 'init') {
            await initializeWasm(data.wasmPath);
            self.postMessage({ type: 'init-complete' });
            return;
        }
        
        if (data.type === 'decode') {
            const result = await decodeFrame(data);
            self.postMessage({
                type: 'decode-complete',
                jobId: data.jobId,
                result,
                decodeTime: result.decodeTime
            });
            return;
        }
        
        if (data.type === 'terminate') {
            self.close();
            return;
        }
        
        // Fallback for legacy messages without type
        if (!data.type && data.gifData && data.jobId) {
            const result = await decodeFrame(data);
            self.postMessage({
                jobId: data.jobId,
                result: {
                    pixels: result.pixels,
                    delay: result.delay
                },
                decodeTime: result.decodeTime
            });
            return;
        }
        
    } catch (error) {
        self.postMessage({
            type: 'error',
            jobId: data.jobId,
            error: error.message,
            stack: error.stack
        });
    }
};

async function initializeWasm(wasmPath = './wtfgif-full.wasm') {
    try {
        // Create shared memory for threading
        const memory = new WebAssembly.Memory({
            initial: 64,    // 4MB
            maximum: 256,   // 16MB
            shared: isSharedArrayBufferSupported()
        });
        
        const imports = {
            env: {
                memory,
                abort: () => {
                    throw new Error('Wasm module aborted');
                },
                // Threading primitives (if supported)
                __wbindgen_thread_destroy: () => {},
                __wbindgen_start: () => {},
            }
        };
        
        // Load and instantiate WebAssembly module
        const response = await fetch(wasmPath);
        const wasmArrayBuffer = await response.arrayBuffer();
        const { instance } = await WebAssembly.instantiate(wasmArrayBuffer, imports);
        
        wasmModule = instance;
        
        // Extract exports and create decoder interface
        const exports = instance.exports;
        wasmDecoder = {
            memory,
            decode_rgba: exports.decode_rgba,
            decode_rgba_threaded: exports.decode_rgba_threaded,
            init_heap: exports.init_heap,
            reset_heap: exports.reset_heap,
            get_heap_usage: exports.get_heap_usage,
            test_simd: exports.test_simd,
            wasm_malloc: exports.wasm_malloc,
            wasm_free: exports.wasm_free,
            heapU8: new Uint8Array(memory.buffer),
            heapU32: new Uint32Array(memory.buffer),
        };
        
        // Initialize the Wasm heap
        wasmDecoder.init_heap();
        
        console.log('Wasm worker initialized successfully');
        
    } catch (error) {
        console.error('Failed to initialize Wasm worker:', error);
        throw error;
    }
}

async function decodeFrame(data) {
    const startTime = performance.now();
    
    if (!wasmDecoder) {
        // Auto-initialize if not done yet
        await initializeWasm();
    }
    
    const { gifData, frameIndex } = data;
    
    // Convert ArrayBuffer back to Uint8Array
    const gifBytes = new Uint8Array(gifData);
    
    // Estimate output size (512x512 default, will be corrected by decoder)
    const estimatedSize = 512 * 512;
    
    // Allocate memory in Wasm heap
    const gifPtr = mallocCopy(gifBytes);
    if (gifPtr === 0) {
        throw new Error('Failed to allocate memory for GIF data');
    }
    
    const outPtr = wasmDecoder.wasm_malloc(estimatedSize * 4);
    if (outPtr === 0) {
        wasmDecoder.wasm_free(gifPtr);
        throw new Error('Failed to allocate memory for output buffer');
    }
    
    try {
        // Update heap views in case memory grew
        updateHeapViews();
        
        // Call Wasm decoder
        const result = wasmDecoder.decode_rgba(
            gifPtr,
            gifBytes.length,
            frameIndex || 0,
            outPtr >>> 2, // Convert to u32 offset
            estimatedSize
        );
        
        // Check for error codes (> 1000 indicates error)
        if (result > 1000) {
            throw new Error(`Wasm decode error code: ${result}`);
        }
        
        // Copy output pixels (in production you'd get actual dimensions)
        const pixels = new Uint32Array(estimatedSize);
        const outArray = wasmDecoder.heapU32.subarray(
            outPtr >>> 2, 
            (outPtr >>> 2) + estimatedSize
        );
        pixels.set(outArray);
        
        const endTime = performance.now();
        
        return {
            pixels,
            delay: result, // Result contains delay in ms
            decodeTime: endTime - startTime,
            heapUsage: wasmDecoder.get_heap_usage()
        };
        
    } finally {
        // Free allocated memory
        wasmDecoder.wasm_free(gifPtr);
        wasmDecoder.wasm_free(outPtr);
    }
}

function mallocCopy(data) {
    if (!wasmDecoder) return 0;
    
    const ptr = wasmDecoder.wasm_malloc(data.length);
    if (ptr === 0) return 0;
    
    // Update views and copy data
    updateHeapViews();
    wasmDecoder.heapU8.set(data, ptr);
    
    return ptr;
}

function updateHeapViews() {
    if (!wasmDecoder) return;
    
    // Update typed array views after potential memory growth
    wasmDecoder.heapU8 = new Uint8Array(wasmDecoder.memory.buffer);
    wasmDecoder.heapU32 = new Uint32Array(wasmDecoder.memory.buffer);
}

function isSharedArrayBufferSupported() {
    return typeof SharedArrayBuffer !== 'undefined' && 
           typeof Atomics !== 'undefined' &&
           crossOriginIsolated;
}

// Feature detection and capabilities
function getWorkerCapabilities() {
    return {
        wasm: typeof WebAssembly !== 'undefined',
        simd: isWasmSIMDSupported(),
        threads: isSharedArrayBufferSupported(),
        transferable: typeof Transferable !== 'undefined'
    };
}

function isWasmSIMDSupported() {
    try {
        // Test SIMD support with minimal v128 instruction
        return WebAssembly.validate(new Uint8Array([
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01,
            0x60, 0x00, 0x01, 0x7b, 0x03, 0x02, 0x01, 0x00, 0x0a, 0x0a, 0x01,
            0x08, 0x00, 0xfd, 0x0f, 0xfd, 0x62, 0x0b
        ]));
    } catch {
        return false;
    }
}

// Handle uncaught errors
self.onerror = function(error) {
    console.error('Wasm worker error:', error);
    self.postMessage({
        type: 'error',
        error: error.message,
        filename: error.filename,
        lineno: error.lineno
    });
};

// Send capabilities on startup
self.postMessage({
    type: 'worker-ready',
    capabilities: getWorkerCapabilities()
});