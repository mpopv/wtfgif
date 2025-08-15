"use strict";
// wasmDecoder.ts
// Full WebAssembly GIF decoder with SIMD + threading support
Object.defineProperty(exports, "__esModule", { value: true });
exports.createWasmGifDecoder = createWasmGifDecoder;
exports.createWasmWorkerPool = createWasmWorkerPool;
exports.isWasmSupported = isWasmSupported;
exports.isWasmSIMDSupported = isWasmSIMDSupported;
exports.isWasmThreadsSupported = isWasmThreadsSupported;
class WasmDecoderInstance {
    constructor(instance, memory) {
        this.instance = instance;
        const exports = instance.exports;
        this.decoder = {
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
        // Initialize heap
        this.decoder.init_heap();
    }
    getDecoder() {
        return this.decoder;
    }
    updateViews() {
        // Update views after memory growth
        this.decoder.heapU8 = new Uint8Array(this.decoder.memory.buffer);
        this.decoder.heapU32 = new Uint32Array(this.decoder.memory.buffer);
    }
    mallocCopy(data) {
        const ptr = this.decoder.wasm_malloc(data.length);
        if (ptr === 0)
            return 0;
        this.decoder.heapU8.set(data, ptr);
        return ptr;
    }
    free(ptr) {
        if (ptr !== 0) {
            this.decoder.wasm_free(ptr);
        }
    }
    decodeFrame(gifData, frameIndex) {
        // Estimate output size (will be corrected by actual GIF dimensions)
        const estimatedSize = 512 * 512; // Default estimate
        const gifPtr = this.mallocCopy(gifData);
        if (gifPtr === 0) {
            console.error('Failed to allocate memory for GIF data');
            return null;
        }
        const outPtr = this.decoder.wasm_malloc(estimatedSize * 4);
        if (outPtr === 0) {
            this.free(gifPtr);
            console.error('Failed to allocate memory for output');
            return null;
        }
        try {
            const result = this.decoder.decode_rgba(gifPtr, gifData.length, frameIndex, outPtr >>> 2, // Convert to u32 offset
            estimatedSize);
            if (result > 1000) {
                // Error code returned
                console.error(`Wasm decode error: ${result}`);
                return null;
            }
            // Copy result (in production, you'd get actual dimensions from decoder)
            const pixels = new Uint32Array(estimatedSize);
            pixels.set(this.decoder.heapU32.subarray(outPtr >>> 2, (outPtr >>> 2) + estimatedSize));
            return { pixels, delay: result };
        }
        finally {
            this.free(gifPtr);
            this.free(outPtr);
        }
    }
}
class WorkerPoolManager {
    constructor(numWorkers = navigator.hardwareConcurrency || 4) {
        this.numWorkers = numWorkers;
        this.workers = [];
        this.activeJobs = 0;
        this.completedJobs = 0;
        this.totalDecodeTime = 0;
        this.jobQueue = [];
        this.initializeWorkers();
    }
    initializeWorkers() {
        for (let i = 0; i < this.numWorkers; i++) {
            const worker = new Worker('wasmWorker.js');
            worker.onmessage = this.handleWorkerMessage.bind(this);
            worker.onerror = this.handleWorkerError.bind(this);
            this.workers.push(worker);
        }
    }
    handleWorkerMessage(event) {
        const { jobId, result, error, decodeTime } = event.data;
        if (error) {
            console.error('Worker error:', error);
            return;
        }
        this.activeJobs--;
        this.completedJobs++;
        this.totalDecodeTime += decodeTime || 0;
        // Find and resolve corresponding job
        const jobIndex = this.jobQueue.findIndex(job => job.data.jobId === jobId);
        if (jobIndex >= 0) {
            const job = this.jobQueue.splice(jobIndex, 1)[0];
            job.resolve(result);
        }
        // Process next queued job if any
        this.processNextJob();
    }
    handleWorkerError(error) {
        console.error('Worker error:', error);
    }
    processNextJob() {
        if (this.jobQueue.length === 0 || this.activeJobs >= this.numWorkers) {
            return;
        }
        const job = this.jobQueue[0];
        const availableWorker = this.workers[this.activeJobs];
        if (availableWorker) {
            availableWorker.postMessage(job.data);
            this.activeJobs++;
        }
    }
    async decode(gifData, frameIndex) {
        return new Promise((resolve, reject) => {
            const jobId = Math.random().toString(36);
            this.jobQueue.push({
                resolve,
                reject,
                data: {
                    jobId,
                    type: 'decode',
                    gifData: gifData.buffer.slice(gifData.byteOffset, gifData.byteOffset + gifData.byteLength),
                    frameIndex,
                },
            });
            this.processNextJob();
        });
    }
    async decodeParallel(gifData, frameIndices) {
        const promises = frameIndices.map(frameIndex => this.decode(gifData, frameIndex));
        return Promise.all(promises);
    }
    terminate() {
        this.workers.forEach(worker => worker.terminate());
        this.workers = [];
        this.jobQueue = [];
    }
    getStats() {
        return {
            activeWorkers: this.activeJobs,
            completedJobs: this.completedJobs,
            avgDecodeTime: this.completedJobs > 0 ? this.totalDecodeTime / this.completedJobs : 0,
        };
    }
}
// Main decoder factory
async function createWasmGifDecoder(wasmPath = './wtfgif-full.wasm') {
    try {
        // Create shared memory for threading support
        const memory = new WebAssembly.Memory({
            initial: 64, // 64 pages = 4MB
            maximum: 256, // 256 pages = 16MB
            shared: true // Enable SharedArrayBuffer for threading
        });
        const imports = {
            env: {
                memory,
                // Add any required imports here
                abort: () => {
                    throw new Error('Wasm module aborted');
                },
            }
        };
        const { instance } = await WebAssembly.instantiateStreaming(fetch(wasmPath), imports);
        const wasmInstance = new WasmDecoderInstance(instance, memory);
        return wasmInstance.getDecoder();
    }
    catch (error) {
        console.error('Failed to load WebAssembly module:', error);
        return null;
    }
}
// Worker pool factory
async function createWasmWorkerPool(wasmPath = './wtfgif-full.wasm', numWorkers) {
    return new WorkerPoolManager(numWorkers);
}
// Feature detection
function isWasmSupported() {
    return typeof WebAssembly === 'object' &&
        typeof WebAssembly.instantiate === 'function';
}
function isWasmSIMDSupported() {
    // Feature detection for SIMD support
    try {
        return WebAssembly.validate(new Uint8Array([
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01,
            0x60, 0x00, 0x01, 0x7b, 0x03, 0x02, 0x01, 0x00, 0x0a, 0x0a, 0x01,
            0x08, 0x00, 0xfd, 0x0f, 0xfd, 0x62, 0x0b
        ]));
    }
    catch {
        return false;
    }
}
function isWasmThreadsSupported() {
    return typeof SharedArrayBuffer !== 'undefined' &&
        typeof Atomics !== 'undefined';
}
