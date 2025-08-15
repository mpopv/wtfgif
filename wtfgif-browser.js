"use strict";
var wtfgif = (() => {
  var __create = Object.create;
  var __defProp = Object.defineProperty;
  var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
  var __getOwnPropNames = Object.getOwnPropertyNames;
  var __getProtoOf = Object.getPrototypeOf;
  var __hasOwnProp = Object.prototype.hasOwnProperty;
  var __esm = (fn, res) => function __init() {
    return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
  };
  var __commonJS = (cb, mod) => function __require() {
    return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
  };
  var __export = (target, all) => {
    for (var name in all)
      __defProp(target, name, { get: all[name], enumerable: true });
  };
  var __copyProps = (to, from, except, desc) => {
    if (from && typeof from === "object" || typeof from === "function") {
      for (let key of __getOwnPropNames(from))
        if (!__hasOwnProp.call(to, key) && key !== except)
          __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
    }
    return to;
  };
  var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
    // If the importer is in node compatibility mode or this is not an ESM
    // file that has been converted to a CommonJS file using a Babel-
    // compatible transform (i.e. "__esModule" has not been set), then set
    // "default" to the CommonJS "module.exports" for node compatibility.
    isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
    mod
  ));
  var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

  // wasm-full/wasmDecoder.ts
  var wasmDecoder_exports = {};
  __export(wasmDecoder_exports, {
    createWasmGifDecoder: () => createWasmGifDecoder,
    createWasmWorkerPool: () => createWasmWorkerPool,
    isWasmSIMDSupported: () => isWasmSIMDSupported,
    isWasmSupported: () => isWasmSupported,
    isWasmThreadsSupported: () => isWasmThreadsSupported
  });
  async function createWasmGifDecoder(wasmPath = "./wtfgif-full.wasm") {
    try {
      const memory = new WebAssembly.Memory({
        initial: 64,
        // 64 pages = 4MB
        maximum: 256,
        // 256 pages = 16MB
        shared: true
        // Enable SharedArrayBuffer for threading
      });
      const imports = {
        env: {
          memory,
          // Add any required imports here
          abort: () => {
            throw new Error("Wasm module aborted");
          }
        }
      };
      const { instance } = await WebAssembly.instantiateStreaming(
        fetch(wasmPath),
        imports
      );
      const wasmInstance = new WasmDecoderInstance(instance, memory);
      return wasmInstance.getDecoder();
    } catch (error) {
      console.error("Failed to load WebAssembly module:", error);
      return null;
    }
  }
  async function createWasmWorkerPool(wasmPath = "./wtfgif-full.wasm", numWorkers) {
    return new WorkerPoolManager(numWorkers);
  }
  function isWasmSupported() {
    return typeof WebAssembly === "object" && typeof WebAssembly.instantiate === "function";
  }
  function isWasmSIMDSupported() {
    try {
      return WebAssembly.validate(new Uint8Array([
        0,
        97,
        115,
        109,
        1,
        0,
        0,
        0,
        1,
        5,
        1,
        96,
        0,
        1,
        123,
        3,
        2,
        1,
        0,
        10,
        10,
        1,
        8,
        0,
        253,
        15,
        253,
        98,
        11
      ]));
    } catch {
      return false;
    }
  }
  function isWasmThreadsSupported() {
    return typeof SharedArrayBuffer !== "undefined" && typeof Atomics !== "undefined";
  }
  var WasmDecoderInstance, WorkerPoolManager;
  var init_wasmDecoder = __esm({
    "wasm-full/wasmDecoder.ts"() {
      WasmDecoderInstance = class {
        instance;
        decoder;
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
            heapU32: new Uint32Array(memory.buffer)
          };
          this.decoder.init_heap();
        }
        getDecoder() {
          return this.decoder;
        }
        updateViews() {
          this.decoder.heapU8 = new Uint8Array(this.decoder.memory.buffer);
          this.decoder.heapU32 = new Uint32Array(this.decoder.memory.buffer);
        }
        mallocCopy(data) {
          const ptr = this.decoder.wasm_malloc(data.length);
          if (ptr === 0) return 0;
          this.decoder.heapU8.set(data, ptr);
          return ptr;
        }
        free(ptr) {
          if (ptr !== 0) {
            this.decoder.wasm_free(ptr);
          }
        }
        decodeFrame(gifData, frameIndex) {
          const estimatedSize = 512 * 512;
          const gifPtr = this.mallocCopy(gifData);
          if (gifPtr === 0) {
            console.error("Failed to allocate memory for GIF data");
            return null;
          }
          const outPtr = this.decoder.wasm_malloc(estimatedSize * 4);
          if (outPtr === 0) {
            this.free(gifPtr);
            console.error("Failed to allocate memory for output");
            return null;
          }
          try {
            const result = this.decoder.decode_rgba(
              gifPtr,
              gifData.length,
              frameIndex,
              outPtr >>> 2,
              // Convert to u32 offset
              estimatedSize
            );
            if (result > 1e3) {
              console.error(`Wasm decode error: ${result}`);
              return null;
            }
            const pixels = new Uint32Array(estimatedSize);
            pixels.set(this.decoder.heapU32.subarray(outPtr >>> 2, (outPtr >>> 2) + estimatedSize));
            return { pixels, delay: result };
          } finally {
            this.free(gifPtr);
            this.free(outPtr);
          }
        }
      };
      WorkerPoolManager = class {
        constructor(numWorkers = navigator.hardwareConcurrency || 4) {
          this.numWorkers = numWorkers;
          this.initializeWorkers();
        }
        workers = [];
        activeJobs = 0;
        completedJobs = 0;
        totalDecodeTime = 0;
        jobQueue = [];
        initializeWorkers() {
          for (let i = 0; i < this.numWorkers; i++) {
            const worker = new Worker("wasmWorker.js");
            worker.onmessage = this.handleWorkerMessage.bind(this);
            worker.onerror = this.handleWorkerError.bind(this);
            this.workers.push(worker);
          }
        }
        handleWorkerMessage(event) {
          const { jobId, result, error, decodeTime } = event.data;
          if (error) {
            console.error("Worker error:", error);
            return;
          }
          this.activeJobs--;
          this.completedJobs++;
          this.totalDecodeTime += decodeTime || 0;
          const jobIndex = this.jobQueue.findIndex((job) => job.data.jobId === jobId);
          if (jobIndex >= 0) {
            const job = this.jobQueue.splice(jobIndex, 1)[0];
            job.resolve(result);
          }
          this.processNextJob();
        }
        handleWorkerError(error) {
          console.error("Worker error:", error);
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
                type: "decode",
                gifData: gifData.buffer.slice(gifData.byteOffset, gifData.byteOffset + gifData.byteLength),
                frameIndex
              }
            });
            this.processNextJob();
          });
        }
        async decodeParallel(gifData, frameIndices) {
          const promises = frameIndices.map(
            (frameIndex) => this.decode(gifData, frameIndex)
          );
          return Promise.all(promises);
        }
        terminate() {
          this.workers.forEach((worker) => worker.terminate());
          this.workers = [];
          this.jobQueue = [];
        }
        getStats() {
          return {
            activeWorkers: this.activeJobs,
            completedJobs: this.completedJobs,
            avgDecodeTime: this.completedJobs > 0 ? this.totalDecodeTime / this.completedJobs : 0
          };
        }
      };
    }
  });

  // threaded-worker-pool.js
  var require_threaded_worker_pool = __commonJS({
    "threaded-worker-pool.js"(exports) {
      "use strict";
      Object.defineProperty(exports, "__esModule", { value: true });
      exports.WorkerPoolManager = void 0;
      exports.createWorkerPool = createWorkerPool;
      exports.getGlobalWorkerPool = getGlobalWorkerPool;
      exports.terminateGlobalWorkerPool = terminateGlobalWorkerPool;
      var WorkerPoolManager2 = class {
        constructor(config = {}) {
          this.workers = [];
          this.workerQueue = [];
          this.requestQueue = [];
          this.pendingRequests = /* @__PURE__ */ new Map();
          this.stats = {
            completedJobs: 0,
            totalDecodeTime: 0,
            workerUtilization: []
          };
          this.nextWorkerId = 0;
          this.nextRequestId = 0;
          this.config = {
            workerCount: config.workerCount || navigator.hardwareConcurrency || 4,
            maxQueueSize: config.maxQueueSize || 100,
            workerScript: config.workerScript || "./decoder-worker.js"
          };
          this.stats.workerUtilization = new Array(this.config.workerCount).fill(0);
        }
        async initialize() {
          console.log(`Initializing threaded worker pool with ${this.config.workerCount} workers`);
          const initPromises = Array.from({ length: this.config.workerCount }, (_, i) => this.createWorker(i));
          await Promise.all(initPromises);
          console.log(`Worker pool initialized: ${this.workers.length} workers ready`);
        }
        async createWorker(workerId) {
          return new Promise((resolve, reject) => {
            const worker = new Worker(this.config.workerScript);
            let initTimeout;
            const handleInitComplete = (event) => {
              const { type, workerId: responseWorkerId, wasmAvailable } = event.data;
              if (type === "init-complete" && responseWorkerId === workerId) {
                clearTimeout(initTimeout);
                worker.removeEventListener("message", handleInitComplete);
                worker.onmessage = (e) => this.handleWorkerMessage(e, workerId);
                worker.onerror = (e) => this.handleWorkerError(e, workerId);
                this.workers[workerId] = worker;
                this.workerQueue.push(workerId);
                console.log(`Worker ${workerId}: Ready (Wasm: ${wasmAvailable ? "Yes" : "No"})`);
                resolve();
              }
            };
            worker.addEventListener("message", handleInitComplete);
            worker.postMessage({
              type: "init",
              data: { workerId }
            });
            initTimeout = setTimeout(() => {
              worker.removeEventListener("message", handleInitComplete);
              reject(new Error(`Worker ${workerId} initialization timeout`));
            }, 1e4);
          });
        }
        handleWorkerMessage(event, workerId) {
          const { type, data } = event.data;
          if (type === "decode-complete") {
            this.handleDecodeComplete(data, workerId);
          } else if (type === "decode-error") {
            this.handleDecodeError(data, workerId);
          }
        }
        handleDecodeComplete(data, workerId) {
          const { requestId, frameIndex, pixels, delay, decodeTime, wasmUsed } = data;
          const pendingRequest = this.pendingRequests.get(requestId);
          if (pendingRequest) {
            this.stats.completedJobs++;
            this.stats.totalDecodeTime += decodeTime;
            this.stats.workerUtilization[workerId]++;
            const pixelsArray = new Uint32Array(pixels);
            pendingRequest.resolve({
              requestId,
              frameIndex,
              pixels: pixelsArray,
              delay,
              decodeTime,
              workerId,
              wasmUsed
            });
            this.pendingRequests.delete(requestId);
          }
          this.workerQueue.push(workerId);
          this.processQueue();
        }
        handleDecodeError(data, workerId) {
          const { requestId, error } = data;
          const pendingRequest = this.pendingRequests.get(requestId);
          if (pendingRequest) {
            pendingRequest.reject(new Error(`Worker ${workerId}: ${error}`));
            this.pendingRequests.delete(requestId);
          }
          this.workerQueue.push(workerId);
          this.processQueue();
        }
        handleWorkerError(error, workerId) {
          console.error(`Worker ${workerId} error:`, error);
          for (const [requestId, pending] of this.pendingRequests.entries()) {
            if (requestId.includes(`-${workerId}-`)) {
              pending.reject(new Error(`Worker ${workerId} crashed`));
              this.pendingRequests.delete(requestId);
            }
          }
        }
        // Main API: Decode frame asynchronously using worker pool
        async decodeFrame(gifData, frameIndex) {
          return new Promise((resolve, reject) => {
            const requestId = `${Date.now()}-${this.nextRequestId++}`;
            const request = {
              gifData: gifData.slice(),
              // Copy to ensure transferable
              frameIndex,
              requestId
            };
            if (this.requestQueue.length >= this.config.maxQueueSize) {
              reject(new Error("Worker pool queue is full"));
              return;
            }
            this.requestQueue.push({ request, resolve, reject });
            this.processQueue();
          });
        }
        // Parallel decode multiple frames
        async decodeFrames(gifData, frameIndices) {
          const decodePromises = frameIndices.map((frameIndex) => this.decodeFrame(gifData, frameIndex));
          return Promise.all(decodePromises);
        }
        processQueue() {
          while (this.requestQueue.length > 0 && this.workerQueue.length > 0) {
            const { request, resolve, reject } = this.requestQueue.shift();
            const workerId = this.workerQueue.shift();
            this.pendingRequests.set(request.requestId, {
              resolve,
              reject,
              startTime: performance.now()
            });
            this.workers[workerId].postMessage({
              type: "decode",
              data: request
            }, [request.gifData.buffer]);
          }
        }
        getStats() {
          const activeWorkers = this.workers.length - this.workerQueue.length;
          const avgDecodeTime = this.stats.completedJobs > 0 ? this.stats.totalDecodeTime / this.stats.completedJobs : 0;
          return {
            activeWorkers,
            queuedRequests: this.requestQueue.length,
            completedJobs: this.stats.completedJobs,
            avgDecodeTime,
            totalDecodeTime: this.stats.totalDecodeTime,
            workerUtilization: this.stats.workerUtilization.slice()
          };
        }
        // Clean shutdown
        async terminate() {
          console.log("Terminating worker pool...");
          for (const [requestId, pending] of this.pendingRequests.entries()) {
            pending.reject(new Error("Worker pool terminated"));
          }
          this.pendingRequests.clear();
          this.requestQueue.length = 0;
          const terminatePromises = this.workers.map((worker, i) => {
            return new Promise((resolve) => {
              worker.postMessage({ type: "terminate" });
              worker.terminate();
              resolve();
            });
          });
          await Promise.all(terminatePromises);
          this.workers.length = 0;
          this.workerQueue.length = 0;
          console.log("Worker pool terminated");
        }
      };
      exports.WorkerPoolManager = WorkerPoolManager2;
      var globalWorkerPool = null;
      async function createWorkerPool(config) {
        const pool = new WorkerPoolManager2(config);
        await pool.initialize();
        return pool;
      }
      async function getGlobalWorkerPool(config) {
        if (!globalWorkerPool) {
          globalWorkerPool = await createWorkerPool(config);
        }
        return globalWorkerPool;
      }
      async function terminateGlobalWorkerPool() {
        if (globalWorkerPool) {
          await globalWorkerPool.terminate();
          globalWorkerPool = null;
        }
      }
    }
  });

  // wtfgif.ts
  var wtfgif_exports = {};
  __export(wtfgif_exports, {
    GifReader: () => GifReader,
    GifWriter: () => GifWriter,
    cleanupWasm: () => cleanupWasm,
    getWasmStatus: () => getWasmStatus,
    initializeWasmGlobally: () => initializeWasmGlobally
  });
  var createWasmGifDecoder2;
  var createWasmWorkerPool2;
  var isWasmSupported2;
  var isWasmSIMDSupported2;
  var isWasmThreadsSupported2;
  var loadWasmModule = () => {
    try {
      return init_wasmDecoder(), __toCommonJS(wasmDecoder_exports);
    } catch (error) {
      return null;
    }
  };
  createWasmGifDecoder2 = async (...args) => {
    const wasmModule = loadWasmModule();
    return wasmModule ? wasmModule.createWasmGifDecoder(...args) : null;
  };
  createWasmWorkerPool2 = async (...args) => {
    const wasmModule = loadWasmModule();
    return wasmModule ? wasmModule.createWasmWorkerPool(...args) : null;
  };
  isWasmSupported2 = () => {
    const wasmModule = loadWasmModule();
    return wasmModule ? wasmModule.isWasmSupported() : false;
  };
  isWasmSIMDSupported2 = () => {
    const wasmModule = loadWasmModule();
    return wasmModule ? wasmModule.isWasmSIMDSupported() : false;
  };
  isWasmThreadsSupported2 = () => {
    const wasmModule = loadWasmModule();
    return wasmModule ? wasmModule.isWasmThreadsSupported() : false;
  };
  var globalWasmDecoder = null;
  var globalWasmWorkerPool = null;
  var wasmInitPromise = null;
  var WASM_FEATURES = {
    supported: isWasmSupported2(),
    simd: isWasmSIMDSupported2(),
    threads: isWasmThreadsSupported2()
  };
  var initializeGlobalWasm = async (wasmPath) => {
    try {
      globalWasmDecoder = await createWasmGifDecoder2(wasmPath);
      if (WASM_FEATURES.threads) {
        globalWasmWorkerPool = await createWasmWorkerPool2(wasmPath);
      }
      console.log("WebAssembly GIF decoder initialized:", {
        decoder: !!globalWasmDecoder,
        workerPool: !!globalWasmWorkerPool,
        features: WASM_FEATURES
      });
    } catch (error) {
      console.warn("Failed to initialize WebAssembly decoder:", error);
      globalWasmDecoder = null;
      globalWasmWorkerPool = null;
    }
  };
  var moduleReusableFramePixels = new Uint8Array(2048 * 2048);
  var moduleFramePixelsInUse = false;
  var decoderTablePool = [];
  var MAX_POOL_SIZE = 16;
  function createDecoderTables() {
    return {
      decTable: new Int32Array(4096 /* MAX_CODE */),
      stack: new Uint8Array(4096 /* MAX_CODE */),
      firstByte: new Int16Array(4096 /* MAX_CODE */),
      out32Cache: /* @__PURE__ */ new WeakMap(),
      hash: ""
    };
  }
  function getPooledDecoderTables(gifHash) {
    const pooledIndex = decoderTablePool.findIndex((p) => p.hash === gifHash);
    if (pooledIndex >= 0) {
      const pooled = decoderTablePool.splice(pooledIndex, 1)[0];
      return pooled;
    }
    if (decoderTablePool.length > 0) {
      const pooled = decoderTablePool.pop();
      pooled.hash = gifHash;
      pooled.out32Cache = /* @__PURE__ */ new WeakMap();
      return pooled;
    }
    const tables = createDecoderTables();
    tables.hash = gifHash;
    return tables;
  }
  function returnDecoderTablesToPool(tables) {
    if (decoderTablePool.length < MAX_POOL_SIZE) {
      decoderTablePool.push(tables);
    }
  }
  function hashGifData(data) {
    let hash = 0;
    const step = Math.max(1, Math.floor(data.length / 1024));
    for (let i = 0; i < data.length; i += step) {
      hash = (hash << 5) - hash + data[i] | 0;
    }
    return hash.toString(36);
  }
  function assertPow2(n) {
    return n >= 2 && n <= 256 && (n & n - 1) === 0;
  }
  function log2Pow2(n) {
    return 31 - Math.clz32(n);
  }
  function checkPalette(pal) {
    const n = pal.length >>> 0;
    if (!assertPow2(n))
      throw new Error("Invalid palette size (must be power of 2, 2..256).");
    return n;
  }
  function concatSubBlocks(buf, offset) {
    const mcs = buf[offset] | 0;
    let q = offset + 1 | 0;
    let total = 0;
    while (true) {
      const len = buf[q++] | 0;
      if (len === 0) break;
      total += len;
      q += len;
    }
    const out = new Uint8Array(total);
    q = offset + 1 | 0;
    let w = 0;
    while (true) {
      const len = buf[q++] | 0;
      if (len === 0) break;
      out.set(buf.subarray(q, q + len), w);
      w += len;
      q += len;
    }
    return { bytes: out, mcs };
  }
  function buildPal32(buf, paletteOffset, paletteSize, order, transparentIndex = null) {
    const pal32 = new Uint32Array(256);
    const limit = Math.min(paletteSize, 256);
    if (order === "rgba") {
      for (let i = 0; i < limit; i++) {
        const r = buf[paletteOffset + i * 3] | 0;
        const g = buf[paletteOffset + i * 3 + 1] | 0;
        const b = buf[paletteOffset + i * 3 + 2] | 0;
        const alpha = transparentIndex !== null && i === transparentIndex ? 0 : 255;
        pal32[i] = alpha << 24 | b << 16 | g << 8 | r;
      }
    } else {
      for (let i = 0; i < limit; i++) {
        const r = buf[paletteOffset + i * 3] | 0;
        const g = buf[paletteOffset + i * 3 + 1] | 0;
        const b = buf[paletteOffset + i * 3 + 2] | 0;
        const alpha = transparentIndex !== null && i === transparentIndex ? 0 : 255;
        pal32[i] = alpha << 24 | r << 16 | g << 8 | b;
      }
    }
    return pal32;
  }
  var GifWriter = class {
    constructor(buf, width, height, gopts) {
      this.buf = buf;
      this.width = width;
      this.height = height;
      const go = gopts ?? {};
      this.loopCount = go.loop === void 0 ? null : go.loop;
      this.globalPalette = go.palette === void 0 ? null : go.palette;
      if (width <= 0 || height <= 0 || width > 65535 || height > 65535)
        throw new Error("Width/Height invalid.");
      this.buf[this.p++] = 71 /* G */;
      this.buf[this.p++] = 73 /* I */;
      this.buf[this.p++] = 70 /* F */;
      this.buf[this.p++] = 56 /* _8 */;
      this.buf[this.p++] = 57 /* _9 */;
      this.buf[this.p++] = 97 /* A */;
      let gpPow2Bits = 0;
      if (this.globalPalette !== null) {
        const n = checkPalette(this.globalPalette);
        const pow = log2Pow2(n);
        gpPow2Bits = pow - 1 & 7;
        if (go.background !== void 0) {
          this.background = go.background | 0;
          if (this.background < 0 || this.background >= n)
            throw new Error("Background index out of range.");
          if (this.background === 0)
            throw new Error("Background index explicitly passed as 0.");
        }
      }
      this.buf[this.p++] = width & 255;
      this.buf[this.p++] = width >> 8 & 255;
      this.buf[this.p++] = height & 255;
      this.buf[this.p++] = height >> 8 & 255;
      const gctFlag = this.globalPalette !== null ? 128 : 0;
      this.buf[this.p++] = gctFlag | gpPow2Bits;
      this.buf[this.p++] = this.background & 255;
      this.buf[this.p++] = 0;
      if (this.globalPalette !== null) {
        for (let i = 0; i < this.globalPalette.length; i++) {
          const rgb = this.globalPalette[i] >>> 0;
          this.buf[this.p++] = rgb >> 16 & 255;
          this.buf[this.p++] = rgb >> 8 & 255;
          this.buf[this.p++] = rgb & 255;
        }
      }
      if (this.loopCount !== null) {
        const lc = this.loopCount | 0;
        if (lc < 0 || lc > 65535) throw new Error("Loop count invalid.");
        this.buf[this.p++] = 33 /* EXT */;
        this.buf[this.p++] = 255 /* APPLICATION */;
        this.buf[this.p++] = 11 /* NETSCAPE_LEN */;
        this.buf[this.p++] = 78;
        this.buf[this.p++] = 69;
        this.buf[this.p++] = 84;
        this.buf[this.p++] = 83;
        this.buf[this.p++] = 67;
        this.buf[this.p++] = 65;
        this.buf[this.p++] = 80;
        this.buf[this.p++] = 69;
        this.buf[this.p++] = 50;
        this.buf[this.p++] = 46;
        this.buf[this.p++] = 48;
        this.buf[this.p++] = 3;
        this.buf[this.p++] = 1;
        this.buf[this.p++] = lc & 255;
        this.buf[this.p++] = lc >> 8 & 255;
        this.buf[this.p++] = 0;
      }
    }
    p = 0;
    ended = false;
    loopCount;
    globalPalette;
    background = 0;
    addFrame(x, y, w, h, indexedPixels, opts) {
      if (this.ended) {
        this.p--;
        this.ended = false;
      }
      const o = opts ?? {};
      x |= 0;
      y |= 0;
      w |= 0;
      h |= 0;
      if (x < 0 || y < 0 || x > 65535 || y > 65535)
        throw new Error("x/y invalid.");
      if (w <= 0 || h <= 0 || w > 65535 || h > 65535)
        throw new Error("Width/Height invalid.");
      if (indexedPixels.length < w * h)
        throw new Error("Not enough pixels for the frame size.");
      let usingLocal = true;
      let palette = o.palette;
      if (palette == null) {
        usingLocal = false;
        palette = this.globalPalette;
      }
      if (palette == null)
        throw new Error("Must supply either a local or global palette.");
      const numColors = checkPalette(palette);
      const minCodeSize = log2Pow2(numColors);
      const lctSizeBits = minCodeSize - 1 & 7;
      const delay = (o.delay ?? 0) | 0;
      let disposal = (o.disposal ?? 0) | 0;
      if (disposal < 0 || disposal > 3) throw new Error("Disposal out of range.");
      let useTrans = false;
      let transparentIndex = 0;
      if (o.transparent !== void 0 && o.transparent !== null) {
        useTrans = true;
        transparentIndex = o.transparent | 0;
        if (transparentIndex < 0 || transparentIndex >= numColors)
          throw new Error("Transparent color index out of range.");
      }
      if (disposal !== 0 || useTrans || delay !== 0) {
        this.buf[this.p++] = 33 /* EXT */;
        this.buf[this.p++] = 249 /* GCE */;
        this.buf[this.p++] = 4;
        this.buf[this.p++] = disposal << 2 | (useTrans ? 1 : 0);
        this.buf[this.p++] = delay & 255;
        this.buf[this.p++] = delay >> 8 & 255;
        this.buf[this.p++] = transparentIndex & 255;
        this.buf[this.p++] = 0;
      }
      this.buf[this.p++] = 44 /* IMG */;
      this.buf[this.p++] = x & 255;
      this.buf[this.p++] = x >> 8 & 255;
      this.buf[this.p++] = y & 255;
      this.buf[this.p++] = y >> 8 & 255;
      this.buf[this.p++] = w & 255;
      this.buf[this.p++] = w >> 8 & 255;
      this.buf[this.p++] = h & 255;
      this.buf[this.p++] = h >> 8 & 255;
      this.buf[this.p++] = usingLocal ? 128 | lctSizeBits : 0;
      if (usingLocal) {
        for (let i = 0; i < palette.length; i++) {
          const rgb = palette[i] >>> 0;
          this.buf[this.p++] = rgb >> 16 & 255;
          this.buf[this.p++] = rgb >> 8 & 255;
          this.buf[this.p++] = rgb & 255;
        }
      }
      this.p = GifWriterOutputLZWCodeStream_fast(
        this.buf,
        this.p,
        minCodeSize < 2 ? 2 : minCodeSize,
        indexedPixels,
        numColors
      );
      return this.p;
    }
    end() {
      if (!this.ended) {
        this.buf[this.p++] = 59 /* TRAILER */;
        this.ended = true;
      }
      return this.p;
    }
    getOutputBuffer() {
      return this.buf;
    }
    setOutputBuffer(v) {
      this.buf = v;
    }
    getOutputBufferPosition() {
      return this.p;
    }
    setOutputBufferPosition(v) {
      this.p = v | 0;
    }
  };
  function GifWriterOutputLZWCodeStream_fast(buf, p0, minCodeSize, indexStream, colorCount) {
    let p = p0;
    buf[p++] = minCodeSize & 255;
    let subLenPos = p++;
    let subLen = 0;
    const CLEAR = 1 << minCodeSize;
    const EOI = CLEAR + 1;
    let nextCode = EOI + 1;
    let codeSize = minCodeSize + 1;
    let codeMask = (1 << codeSize) - 1;
    let bits = 0 >>> 0;
    let bitCount = 0;
    const CAP = 8192;
    const keys = GifWriterOutputLZWCodeStream_fast._keys ??= new Int32Array(
      CAP
    );
    const vals = GifWriterOutputLZWCodeStream_fast._vals ??= new Int16Array(
      CAP
    );
    const gen = GifWriterOutputLZWCodeStream_fast._gen ??= new Int32Array(CAP);
    let EPOCH = (GifWriterOutputLZWCodeStream_fast._epoch ?? 1) | 0;
    GifWriterOutputLZWCodeStream_fast._epoch = EPOCH + 1 | 0;
    if (GifWriterOutputLZWCodeStream_fast._epoch <= 0) {
      gen.fill(0);
      GifWriterOutputLZWCodeStream_fast._epoch = 1;
      EPOCH = 1;
    }
    function tableReset() {
      EPOCH = EPOCH + 1 | 0;
      if (EPOCH <= 0) {
        gen.fill(0);
        EPOCH = 1;
      }
    }
    function tableGet(key) {
      let i = key & CAP - 1;
      while (gen[i] === EPOCH) {
        if (keys[i] === key) return vals[i] | 0;
        i = i + 1 & CAP - 1;
      }
      return -1;
    }
    function tableSet(key, value) {
      let i = key & CAP - 1;
      while (gen[i] === EPOCH) {
        if (keys[i] === key) {
          vals[i] = value;
          return;
        }
        i = i + 1 & CAP - 1;
      }
      gen[i] = EPOCH;
      keys[i] = key | 0;
      vals[i] = value | 0;
    }
    function emit(code) {
      bits |= (code & 65535) << bitCount;
      bitCount += codeSize;
      while (bitCount >= 8) {
        buf[p++] = bits & 255;
        bits >>>= 8;
        bitCount -= 8;
        if (++subLen === 255) {
          buf[subLenPos] = 255;
          subLenPos = p++;
          subLen = 0;
        }
      }
    }
    emit(CLEAR);
    const n = indexStream.length | 0;
    const mask = colorCount - 1 | 0;
    let ib = indexStream[0] & mask;
    for (let i = 1; i < n; i++) {
      const k = indexStream[i] & mask;
      const key = ib << 8 | k;
      const found = tableGet(key);
      if (found >= 0) {
        ib = found;
        continue;
      }
      emit(ib);
      if (nextCode === 4096 /* MAX_CODE */) {
        emit(CLEAR);
        nextCode = EOI + 1;
        codeSize = minCodeSize + 1 | 0;
        codeMask = (1 << codeSize) - 1;
        tableReset();
      } else {
        if (nextCode >= codeMask + 1 && codeSize < 12) {
          codeSize++;
          codeMask = codeMask << 1 | 1;
        }
        tableSet(key, nextCode++);
      }
      ib = k;
    }
    emit(ib);
    emit(EOI);
    if (bitCount > 0) {
      buf[p++] = bits & 255;
      if (++subLen === 255) {
        buf[subLenPos] = 255;
        subLenPos = p++;
        subLen = 0;
      }
      bits = 0;
      bitCount = 0;
    }
    buf[subLenPos] = subLen & 255;
    buf[p++] = 0;
    return p;
  }
  ((GifWriterOutputLZWCodeStream_fast2) => {
  })(GifWriterOutputLZWCodeStream_fast || (GifWriterOutputLZWCodeStream_fast = {}));
  var GifReader = class _GifReader {
    constructor(buf, usePooling = true) {
      this.buf = buf;
      this.gifHash = usePooling ? hashGifData(buf) : "";
      this.pooledTables = usePooling ? getPooledDecoderTables(this.gifHash) : createDecoderTables();
      this.decTable = this.pooledTables.decTable;
      this.stack = this.pooledTables.stack;
      this.firstByte = this.pooledTables.firstByte;
      this.out32Cache = this.pooledTables.out32Cache;
      let p = 0;
      if (buf[p++] !== 71 /* G */ || buf[p++] !== 73 /* I */ || buf[p++] !== 70 /* F */ || buf[p++] !== 56 /* _8 */ || (buf[p++] + 1 & 253) !== 56 /* _8 */ || buf[p++] !== 97 /* A */) {
        throw new Error("Invalid GIF 87a/89a header.");
      }
      const width = (buf[p++] | buf[p++] << 8) >>> 0;
      const height = (buf[p++] | buf[p++] << 8) >>> 0;
      this.width_ = width;
      this.height_ = height;
      const pf0 = buf[p++];
      const gctFlag = pf0 >>> 7 & 1;
      const gctSizeBits = pf0 & 7;
      const gctColors = 1 << gctSizeBits + 1;
      const background = buf[p++];
      p++;
      if (gctFlag) {
        this.globalPaletteOffset = p;
        this.globalPaletteSize = gctColors;
        p += gctColors * 3;
      }
      let delay = 0;
      let transparent_index = null;
      let disposal = 0;
      let noEOF = true;
      while (noEOF && p < buf.length) {
        const block = buf[p++];
        switch (block) {
          case 33 /* EXT */: {
            const label = buf[p++];
            switch (label) {
              case 255 /* APPLICATION */: {
                if (buf[p] === 11 /* NETSCAPE_LEN */ && buf[p + 1] === 78 && buf[p + 2] === 69 && buf[p + 3] === 84 && buf[p + 4] === 83 && buf[p + 5] === 67 && buf[p + 6] === 65 && buf[p + 7] === 80 && buf[p + 8] === 69 && buf[p + 9] === 50 && buf[p + 10] === 46 && buf[p + 11] === 48 && buf[p + 12] === 3 && buf[p + 13] === 1 && buf[p + 16] === 0) {
                  p += 14;
                  this.loop_count = (buf[p++] | buf[p++] << 8) >>> 0;
                  p++;
                } else {
                  p += 12;
                  while (true) {
                    const size = buf[p++];
                    if (!(size >= 0)) throw new Error("Invalid block size");
                    if (size === 0) break;
                    p += size;
                  }
                }
                break;
              }
              case 249 /* GCE */: {
                if (buf[p++] !== 4 || buf[p + 4] !== 0)
                  throw new Error("Invalid graphics extension block.");
                const pf1 = buf[p++];
                delay = (buf[p++] | buf[p++] << 8) >>> 0;
                const t = buf[p++];
                transparent_index = pf1 & 1 ? t : null;
                disposal = pf1 >> 2 & 7;
                p++;
                break;
              }
              case 1 /* PLAINTEXT */:
              case 254 /* COMMENT */: {
                while (true) {
                  const size = buf[p++];
                  if (!(size >= 0)) throw new Error("Invalid block size");
                  if (size === 0) break;
                  p += size;
                }
                break;
              }
              default:
                throw new Error(
                  "Unknown graphic control label: 0x" + label.toString(16)
                );
            }
            break;
          }
          case 44 /* IMG */: {
            const x = (buf[p++] | buf[p++] << 8) >>> 0;
            const y = (buf[p++] | buf[p++] << 8) >>> 0;
            const w = (buf[p++] | buf[p++] << 8) >>> 0;
            const h = (buf[p++] | buf[p++] << 8) >>> 0;
            const pf2 = buf[p++];
            const lctFlag = pf2 >>> 7 & 1;
            const interlace = (pf2 >>> 6 & 1) !== 0;
            const lctSizeBits = pf2 & 7;
            const lctColors = 1 << lctSizeBits + 1;
            let palette_offset = this.globalPaletteOffset;
            let palette_size = this.globalPaletteSize;
            let has_local_palette = false;
            if (lctFlag) {
              has_local_palette = true;
              palette_offset = p;
              palette_size = lctColors;
              p += lctColors * 3;
            }
            const data_offset = p;
            p++;
            while (true) {
              const size = buf[p++];
              if (!(size >= 0)) throw new Error("Invalid block size");
              if (size === 0) break;
              p += size;
            }
            const { bytes: codes, mcs } = concatSubBlocks(buf, data_offset);
            const pal32rgba = buildPal32(buf, palette_offset ?? 0, palette_size ?? 0, "rgba", transparent_index);
            const pal32bgra = buildPal32(buf, palette_offset ?? 0, palette_size ?? 0, "bgra", transparent_index);
            this.frames.push({
              x,
              y,
              width: w,
              height: h,
              has_local_palette,
              palette_offset: palette_offset ?? 0,
              palette_size: palette_size ?? 0,
              data_offset,
              // keep for compatibility
              data_length: p - data_offset,
              transparent_index,
              interlaced: interlace,
              delay,
              disposal,
              // NEW:
              min_code_size: mcs,
              codes,
              pal32rgba,
              pal32bgra
            });
            delay = 0;
            transparent_index = null;
            disposal = 0;
            break;
          }
          case 59 /* TRAILER */:
            noEOF = false;
            break;
          default:
            throw new Error(
              "Unknown gif block: 0x" + block.toString(16)
            );
        }
      }
    }
    p = 0;
    width_;
    height_;
    globalPaletteOffset = null;
    globalPaletteSize = null;
    frames = [];
    loop_count = null;
    // Pooled decoder tables for reuse across instances
    pooledTables;
    gifHash;
    // Aliases for easier access
    decTable;
    stack;
    firstByte;
    out32Cache;
    // Zero-copy canvas support
    zeroCopyBuffers = /* @__PURE__ */ new Map();
    // Optional Wasm color mapping helper (Tier 2 optimization)
    colorMapWasm = null;
    // ColorMapWasm type
    wasmEnabled = false;
    rowIndicesBuffer = null;
    // GPU palette expansion (Tier 4 optimization)
    gpuRenderer = null;
    gpuEnabled = false;
    // Threaded worker pool support (when Wasm threads aren't available)
    workerPool = null;
    // WorkerPoolManager
    workerPoolEnabled = false;
    // Memory hygiene: TypedArray pools to eliminate allocations in hot loops
    memoryHygiene = null;
    // MemoryHygiene
    /* Factory method for pooled GifReader instances */
    static createPooled(buf) {
      return new _GifReader(buf, true);
    }
    /* Factory method for non-pooled GifReader instances */
    static createUnpooled(buf) {
      return new _GifReader(buf, false);
    }
    get width() {
      return this.width_;
    }
    get height() {
      return this.height_;
    }
    /* ===== WebAssembly Integration Methods ===== */
    /**
     * Initialize WebAssembly decoder for this GifReader instance
     */
    async initWasm(wasmPath) {
      if (!WASM_FEATURES.supported) {
        return false;
      }
      if (!globalWasmDecoder && !wasmInitPromise) {
        wasmInitPromise = initializeGlobalWasm(wasmPath);
      }
      if (wasmInitPromise) {
        await wasmInitPromise;
      }
      return globalWasmDecoder !== null;
    }
    /**
     * Check if WebAssembly decoder is available and initialized
     */
    isWasmReady() {
      return globalWasmDecoder !== null;
    }
    /**
     * Decode frame using WebAssembly (with fallback to JavaScript)
     */
    async framePixelsWasm(frameIndex, pixels) {
      if (this.isWasmReady() && globalWasmDecoder) {
        try {
          const result = await this.decodeFrameWasm(frameIndex, pixels);
          if (result) {
            return result;
          }
        } catch (error) {
          console.warn("Wasm decode failed, falling back to JavaScript:", error);
        }
      }
      const outputSize = this.width_ * this.height_;
      if (!pixels || pixels.length < outputSize) {
        pixels = new Uint32Array(outputSize);
      }
      const uint8Buffer = new Uint8Array(pixels.buffer, pixels.byteOffset, pixels.byteLength);
      this.decodeAndBlitFrameRGBA(frameIndex, uint8Buffer);
      return pixels;
    }
    /**
     * Internal Wasm frame decoder
     */
    async decodeFrameWasm(frameIndex, pixels) {
      if (!globalWasmDecoder) {
        return null;
      }
      if (frameIndex < 0 || frameIndex >= this.frames.length) {
        throw new Error("Frame index out of bounds");
      }
      const outputSize = this.width_ * this.height_;
      if (!pixels || pixels.length < outputSize) {
        pixels = new Uint32Array(outputSize);
      }
      const gifPtr = globalWasmDecoder.wasm_malloc(this.buf.length);
      if (gifPtr === 0) {
        throw new Error("Failed to allocate Wasm memory for GIF data");
      }
      const outPtr = globalWasmDecoder.wasm_malloc(outputSize * 4);
      if (outPtr === 0) {
        globalWasmDecoder.wasm_free(gifPtr);
        throw new Error("Failed to allocate Wasm memory for output");
      }
      try {
        globalWasmDecoder.heapU8.set(this.buf, gifPtr);
        const result = globalWasmDecoder.decode_rgba(
          gifPtr,
          this.buf.length,
          frameIndex,
          outPtr >>> 2,
          // Convert to u32 offset
          outputSize
        );
        if (result > 1e3) {
          throw new Error(`Wasm decode error: ${result}`);
        }
        const wasmOutput = globalWasmDecoder.heapU32.subarray(
          outPtr >>> 2,
          (outPtr >>> 2) + outputSize
        );
        pixels.set(wasmOutput);
        return pixels;
      } finally {
        globalWasmDecoder.wasm_free(gifPtr);
        globalWasmDecoder.wasm_free(outPtr);
      }
    }
    /**
     * Decode multiple frames in parallel using Wasm worker pool
     */
    async framePixelsParallel(frameIndices) {
      if (!WASM_FEATURES.threads || !globalWasmWorkerPool) {
        const results = [];
        for (const frameIndex of frameIndices) {
          const pixels = await this.framePixelsWasm(frameIndex);
          const delay = this.frameInfo(frameIndex).delay || 100;
          results.push({ pixels, delay });
        }
        return results;
      }
      return globalWasmWorkerPool.decodeParallel(this.buf, frameIndices);
    }
    /**
     * Get WebAssembly performance statistics
     */
    getWasmStats() {
      return {
        ...WASM_FEATURES,
        heapUsage: globalWasmDecoder?.get_heap_usage()
      };
    }
    /* ===== GPU Palette Expansion Methods (Tier 4) ===== */
    /**
     * Initialize GPU palette expansion for ultra-fast rendering
     */
    async initGPU(canvas) {
      try {
        if (!this.gpuRenderer) {
          const gpuModule = this.loadGPUModule();
          if (!gpuModule) {
            return false;
          }
          this.gpuRenderer = new gpuModule.UnifiedGPUGifRenderer();
        }
        const success = await this.gpuRenderer.initialize(canvas);
        this.gpuEnabled = success;
        return success;
      } catch (error) {
        console.warn("GPU palette expansion failed to initialize:", error);
        this.gpuEnabled = false;
        return false;
      }
    }
    /**
     * Decode frame using GPU acceleration (fastest possible path)
     */
    async framePixelsGPU(frameIndex, targetCanvas) {
      if (!this.gpuEnabled || !this.gpuRenderer) {
        if (!await this.initGPU()) {
          return null;
        }
      }
      if (frameIndex < 0 || frameIndex >= this.frames.length) {
        throw new Error("Frame index out of bounds");
      }
      const frame = this.frameInfo(frameIndex);
      const indexData = await this.decodeFrameIndices(frameIndex);
      if (!indexData) {
        return null;
      }
      if (targetCanvas) {
        const success = await this.gpuRenderer.renderToCanvas(
          indexData,
          frame.pal32rgba || new Uint32Array(256),
          frame.width,
          frame.height,
          targetCanvas
        );
        return success ? targetCanvas : null;
      } else {
        return await this.gpuRenderer.renderFrame(
          indexData,
          frame.pal32rgba || new Uint32Array(256),
          frame.width,
          frame.height
        );
      }
    }
    /**
     * Zero-copy canvas presentation using WebAssembly persistent buffer
     * Allocates buffer once, reuses for all frames of same size
     */
    frameImageDataZeroCopy(frameIndex, ctx2d) {
      if (!this.isWasmReady() || !globalWasmDecoder) {
        throw new Error("WebAssembly not available for zero-copy presentation");
      }
      const frame = this.frameInfo(frameIndex);
      const w = frame.width;
      const h = frame.height;
      const bufferKey = `${w}x${h}`;
      let buffer = this.zeroCopyBuffers.get(bufferKey);
      if (!buffer) {
        const outPtr = globalWasmDecoder.wasm_malloc(w * h * 4);
        const rgbaU8 = new Uint8ClampedArray(globalWasmDecoder.memory.buffer, outPtr, w * h * 4);
        const imageData = new ImageData(rgbaU8, w, h);
        buffer = { wasmPtr: outPtr, rgbaU8, imageData };
        this.zeroCopyBuffers.set(bufferKey, buffer);
      }
      globalWasmDecoder.decode_rgba(
        this.buf.byteOffset || 0,
        this.buf.length,
        frameIndex,
        buffer.wasmPtr,
        w * h
      );
      ctx2d.putImageData(buffer.imageData, 0, 0);
    }
    /**
     * GPU zero-copy with OffscreenCanvas and ImageBitmap transfer
     */
    async frameImageBitmapGPU(frameIndex) {
      if (!this.gpuEnabled || !this.gpuRenderer) {
        if (!await this.initGPU()) {
          return null;
        }
      }
      const frame = this.frameInfo(frameIndex);
      const offscreen = new OffscreenCanvas(frame.width, frame.height);
      const indexData = await this.decodeFrameIndices(frameIndex);
      if (!indexData) return null;
      const success = await this.gpuRenderer.renderToCanvas(
        indexData,
        frame.pal32rgba || new Uint32Array(256),
        frame.width,
        frame.height,
        offscreen
        // OffscreenCanvas compatible with HTMLCanvasElement interface
      );
      if (!success) return null;
      return createImageBitmap(offscreen);
    }
    /**
     * Worker-compatible GPU decode with transferToImageBitmap
     * Use this pattern in a worker for maximum performance
     */
    async frameTransferBitmapGPU(frameIndex) {
      if (!this.gpuEnabled || !this.gpuRenderer) {
        if (!await this.initGPU()) {
          return null;
        }
      }
      const frame = this.frameInfo(frameIndex);
      const offscreen = new OffscreenCanvas(frame.width, frame.height);
      const indexData = await this.decodeFrameIndices(frameIndex);
      if (!indexData) return null;
      const success = await this.gpuRenderer.renderToCanvas(
        indexData,
        frame.pal32rgba || new Uint32Array(256),
        frame.width,
        frame.height,
        offscreen
      );
      if (!success) return null;
      return offscreen.transferToImageBitmap();
    }
    /**
     * Cleanup zero-copy buffers when done
     */
    cleanupZeroCopyBuffers() {
      if (globalWasmDecoder) {
        for (const buffer of this.zeroCopyBuffers.values()) {
          globalWasmDecoder.wasm_free(buffer.wasmPtr);
        }
      }
      this.zeroCopyBuffers.clear();
    }
    /**
     * Initialize threaded worker pool for parallel frame decode
     * Alternative to WebAssembly threads when not available
     */
    async initWorkerPool(config) {
      if (WASM_FEATURES.threads && globalWasmWorkerPool) {
        console.log("WebAssembly threads available, skipping worker pool");
        return false;
      }
      try {
        const { getGlobalWorkerPool } = await Promise.resolve().then(() => __toESM(require_threaded_worker_pool()));
        this.workerPool = await getGlobalWorkerPool({
          workerCount: config?.workerCount || navigator.hardwareConcurrency || 4,
          maxQueueSize: config?.maxQueueSize || 100,
          workerScript: "./decoder-worker.js"
        });
        this.workerPoolEnabled = true;
        console.log("Threaded worker pool initialized");
        return true;
      } catch (error) {
        console.warn("Failed to initialize worker pool:", error);
        this.workerPoolEnabled = false;
        return false;
      }
    }
    /**
     * Parallel frame decode using threaded worker pool
     * Each worker has its own Wasm instance and LZW tables
     */
    async framePixelsThreadedPool(frameIndices) {
      if (!this.workerPoolEnabled || !this.workerPool) {
        const initialized = await this.initWorkerPool();
        if (!initialized) {
          const results = [];
          for (const frameIndex of frameIndices) {
            const pixels = await this.framePixelsWasm(frameIndex);
            const delay = this.frameInfo(frameIndex).delay || 100;
            results.push({ pixels, delay });
          }
          return results;
        }
      }
      try {
        const decodeResults = await this.workerPool.decodeFrames(this.buf, frameIndices);
        return decodeResults.map((result) => ({
          pixels: result.pixels,
          delay: result.delay
        }));
      } catch (error) {
        console.warn("Worker pool decode failed, falling back to sequential:", error);
        const results = [];
        for (const frameIndex of frameIndices) {
          const pixels = await this.framePixelsWasm(frameIndex);
          const delay = this.frameInfo(frameIndex).delay || 100;
          results.push({ pixels, delay });
        }
        return results;
      }
    }
    /**
     * Single frame decode using worker pool with load balancing
     */
    async framePixelsWorkerPool(frameIndex) {
      if (!this.workerPoolEnabled || !this.workerPool) {
        await this.initWorkerPool();
      }
      if (this.workerPoolEnabled && this.workerPool) {
        try {
          const result = await this.workerPool.decodeFrame(this.buf, frameIndex);
          return result.pixels;
        } catch (error) {
          console.warn("Worker pool single frame decode failed:", error);
        }
      }
      return this.framePixelsWasm(frameIndex);
    }
    /**
     * Get worker pool statistics and performance metrics
     */
    getWorkerPoolStats() {
      if (!this.workerPoolEnabled || !this.workerPool) {
        return null;
      }
      return this.workerPool.getStats();
    }
    /**
     * Check if worker pool is available and ready
     */
    isWorkerPoolReady() {
      return this.workerPoolEnabled && this.workerPool !== null;
    }
    /**
     * Cleanup worker pool resources
     */
    async cleanupWorkerPool() {
      if (this.workerPool) {
        await this.workerPool.terminate();
        this.workerPool = null;
        this.workerPoolEnabled = false;
      }
    }
    /**
     * Decode frame to index data only (for GPU palette expansion)
     */
    async decodeFrameIndices(frameIndex) {
      try {
        const frame = this.frameInfo(frameIndex);
        const frameSize = frame.width * frame.height;
        const indexData = new Uint8Array(frameSize);
        this.lzwDecodeToIndices(frame, indexData);
        return indexData;
      } catch (error) {
        console.error("Failed to decode frame indices:", error);
        return null;
      }
    }
    /**
     * Simplified LZW decoder that outputs palette indices instead of RGBA
     */
    lzwDecodeToIndices(frame, outputIndices) {
      const bytes = frame.codes;
      const minCodeSize = frame.min_code_size | 0;
      let q = 0;
      const CLEAR = 1 << minCodeSize;
      const EOI = CLEAR + 1;
      let nextCode = EOI + 1;
      let codeSize = minCodeSize + 1 | 0;
      let codeMask = (1 << codeSize) - 1;
      for (let i = 0; i < CLEAR; i++) {
        this.firstByte[i] = i;
      }
      let bits = 0;
      let bitCount = 0;
      let pixelIndex = 0;
      const table = this.decTable;
      const stack = this.stack;
      let sp = 0;
      let prevCode = null;
      while (true) {
        while (bitCount < codeSize && q < bytes.length) {
          bits |= (bytes[q++] | 0) << bitCount;
          bitCount += 8;
        }
        if (bitCount < codeSize) break;
        let code = bits & codeMask;
        bits >>>= codeSize;
        bitCount -= codeSize;
        if (code === CLEAR) {
          nextCode = EOI + 1;
          codeSize = minCodeSize + 1 | 0;
          codeMask = (1 << codeSize) - 1;
          prevCode = null;
          for (let i = 0; i < CLEAR; i++) {
            this.firstByte[i] = i;
          }
          continue;
        } else if (code === EOI) {
          break;
        }
        let outFirst;
        let cur = code;
        if (cur < CLEAR) {
          outFirst = cur;
          if (pixelIndex < outputIndices.length) {
            outputIndices[pixelIndex++] = outFirst & 255;
          }
        } else {
          sp = 0;
          if (cur >= nextCode) {
            if (prevCode === null) break;
            outFirst = this.firstByte[prevCode] | 0;
            stack[sp++] = outFirst;
            cur = prevCode;
          } else {
            outFirst = this.firstByte[cur] | 0;
          }
          while (cur >= CLEAR) {
            const entry = table[cur] | 0;
            stack[sp++] = entry & 255;
            cur = entry >>> 8;
          }
          const base = cur & 255;
          if (pixelIndex < outputIndices.length) {
            outputIndices[pixelIndex++] = base;
          }
          while (sp && pixelIndex < outputIndices.length) {
            outputIndices[pixelIndex++] = stack[--sp] & 255;
          }
        }
        if (prevCode !== null && nextCode < 4096 /* MAX_CODE */) {
          table[nextCode] = (prevCode & 4095) << 8 | outFirst & 255;
          this.firstByte[nextCode] = this.firstByte[prevCode];
          nextCode++;
          if (nextCode >= codeMask + 1 && codeSize < 12) {
            codeSize++;
            codeMask = codeMask << 1 | 1;
          }
        }
        prevCode = code;
      }
    }
    /**
     * Check if GPU acceleration is available and enabled
     */
    isGPUEnabled() {
      return this.gpuEnabled && this.gpuRenderer !== null;
    }
    /**
     * Get GPU backend information
     */
    getGPUBackend() {
      return this.gpuRenderer?.getBackend() || "none";
    }
    /**
     * Benchmark GPU performance
     */
    async benchmarkGPU(width = 512, height = 512) {
      if (!this.gpuEnabled || !this.gpuRenderer) {
        return null;
      }
      return await this.gpuRenderer.benchmark(width, height);
    }
    /**
     * Disable GPU acceleration
     */
    disableGPU() {
      if (this.gpuRenderer) {
        this.gpuRenderer.dispose();
        this.gpuRenderer = null;
      }
      this.gpuEnabled = false;
    }
    /**
     * Lazy-load GPU module to avoid startup cost
     */
    loadGPUModule() {
      try {
        return null;
      } catch (error) {
        return null;
      }
    }
    numFrames() {
      return this.frames.length;
    }
    loopCount() {
      return this.loop_count;
    }
    frameInfo(i) {
      if (i < 0 || i >= this.frames.length)
        throw new Error("Frame index out of range.");
      return this.frames[i];
    }
    /* Public API mirrors omggif: BGRA and RGBA outputs (Uint8Array). */
    decodeAndBlitFrameBGRA(frameNum, pixels) {
      this.decodeAndBlitFrame32(frameNum, pixels, "bgra");
    }
    decodeAndBlitFrameRGBA(frameNum, pixels) {
      this.decodeAndBlitFrame32(frameNum, pixels, "rgba");
    }
    /* Transferable-friendly API: decode into an ArrayBuffer that can be transferred between workers */
    decodeFrameToTransferableRGBA(frameNum) {
      const pixelCount = this.width_ * this.height_;
      const buffer = new ArrayBuffer(pixelCount * 4);
      const pixels = new Uint8Array(buffer);
      this.decodeAndBlitFrame32(frameNum, pixels, "rgba");
      return buffer;
    }
    decodeFrameToTransferableBGRA(frameNum) {
      const pixelCount = this.width_ * this.height_;
      const buffer = new ArrayBuffer(pixelCount * 4);
      const pixels = new Uint8Array(buffer);
      this.decodeAndBlitFrame32(frameNum, pixels, "bgra");
      return buffer;
    }
    /* Decode into a pre-allocated transferable buffer (for worker scenarios) */
    decodeFrameIntoBuffer(frameNum, buffer, format = "rgba") {
      const expectedSize = this.width_ * this.height_ * 4;
      if (buffer.byteLength < expectedSize) {
        throw new Error(`Buffer too small: need ${expectedSize} bytes, got ${buffer.byteLength}`);
      }
      const pixels = new Uint8Array(buffer, 0, expectedSize);
      this.decodeAndBlitFrame32(frameNum, pixels, format);
    }
    /* Return decoder tables to pool for reuse (call when done with this GifReader) */
    dispose() {
      if (this.pooledTables && this.gifHash) {
        returnDecoderTablesToPool(this.pooledTables);
      }
    }
    /* Alias for dispose() to match expected pooling API */
    returnToPool() {
      this.dispose();
    }
    /* Get statistics about decoder table pool usage */
    static getPoolStats() {
      return {
        available: decoderTablePool.length,
        totalCreated: decoderTablePool.length + 1,
        // Approximate
        hits: 0,
        // Would need to track in getPooledDecoderTables
        misses: 0
        // Would need to track in getPooledDecoderTables
      };
    }
    /* Enable Wasm color mapping for faster palette lookups (Tier 2 optimization) */
    enableWasmColorMapping(colorMapWasm) {
      this.colorMapWasm = colorMapWasm;
      this.wasmEnabled = true;
      const maxRowWidth = Math.min(this.width_, colorMapWasm.maxRowWidth || 4096);
      this.rowIndicesBuffer = new Uint8Array(maxRowWidth);
    }
    /* Disable Wasm color mapping (fallback to JS) */
    disableWasmColorMapping() {
      this.wasmEnabled = false;
      this.colorMapWasm = null;
      this.rowIndicesBuffer = null;
    }
    /* Check if Wasm color mapping is enabled */
    isWasmEnabled() {
      return this.wasmEnabled && this.colorMapWasm !== null;
    }
    /* Fused LZW decode → Uint32 blit with precomputed pal32, transparency, interlace. */
    decodeAndBlitFrame32(frameNum, pixels, order) {
      const frame = this.frameInfo(frameNum);
      const numPixels = frame.width * frame.height;
      const pal32 = order === "rgba" ? frame.pal32rgba : frame.pal32bgra;
      let trans = frame.transparent_index;
      if (trans === null) trans = 256;
      let out32 = this.out32Cache.get(pixels);
      if (!out32) {
        out32 = new Uint32Array(pixels.buffer, pixels.byteOffset, pixels.byteLength >>> 2);
        this.out32Cache.set(pixels, out32);
      }
      this.lzwDecodeToPixels(
        this.buf,
        frame.data_offset,
        out32,
        this.width_,
        frame,
        pal32,
        trans
      );
    }
    /* Optimized LZW decoder that streams symbols directly to destination pixels. */
    lzwDecodeToPixels(codeStream, dataOffset, out32, canvasWidth, frame, pal32, transparentIndex) {
      const bytes = frame.codes;
      const minCodeSize = frame.min_code_size | 0;
      let q = 0;
      const CLEAR = 1 << minCodeSize;
      const EOI = CLEAR + 1;
      let nextCode = EOI + 1;
      let codeSize = minCodeSize + 1 | 0;
      let codeMask = (1 << codeSize) - 1;
      for (let i = 0; i < CLEAR; i++) this.firstByte[i] = i;
      let bits = 0;
      let bitCount = 0;
      const fw = frame.width | 0;
      const fh = frame.height | 0;
      const fx = frame.x | 0;
      const fy = frame.y | 0;
      const table = this.decTable;
      const stack = this.stack;
      let sp = 0;
      let prevCode = null;
      const hasTrans = transparentIndex !== 256;
      if (!frame.interlaced) {
        let xleft = fw;
        const rowStride32 = canvasWidth - fw >>> 0;
        let dst32 = fy * canvasWidth + fx >>> 0;
        if (!hasTrans) {
          if (this.wasmEnabled && this.colorMapWasm && fw <= (this.colorMapWasm.maxRowWidth || 4096)) {
            this.lzwDecodeToPixelsWasm(bytes, minCodeSize, out32, canvasWidth, fw, fh, fx, fy, pal32);
          } else {
            while (true) {
              while (bitCount < codeSize && q < bytes.length) {
                bits |= (bytes[q++] | 0) << bitCount;
                bitCount += 8;
              }
              if (bitCount < codeSize) break;
              let code = bits & codeMask;
              bits >>>= codeSize;
              bitCount -= codeSize;
              if (code === CLEAR) {
                nextCode = EOI + 1;
                codeSize = minCodeSize + 1 | 0;
                codeMask = (1 << codeSize) - 1;
                prevCode = null;
                for (let i = 0; i < CLEAR; i++) this.firstByte[i] = i;
                continue;
              } else if (code === EOI) {
                break;
              }
              let outFirst;
              let cur = code;
              if (cur < CLEAR) {
                outFirst = cur;
                const b = outFirst & 255;
                out32[dst32] = pal32[b] >>> 0;
                dst32++;
                if (--xleft === 0) {
                  dst32 += rowStride32;
                  xleft = fw;
                }
              } else {
                sp = 0;
                if (cur >= nextCode) {
                  if (prevCode === null) break;
                  outFirst = this.firstByte[prevCode] | 0;
                  stack[sp++] = outFirst;
                  cur = prevCode;
                } else {
                  outFirst = this.firstByte[cur] | 0;
                }
                while (cur >= CLEAR) {
                  const entry = table[cur] | 0;
                  stack[sp++] = entry & 255;
                  cur = entry >>> 8;
                }
                const base = cur & 255;
                out32[dst32] = pal32[base] >>> 0;
                dst32++;
                if (--xleft === 0) {
                  dst32 += rowStride32;
                  xleft = fw;
                }
                while (sp) {
                  const b = stack[--sp] & 255;
                  out32[dst32] = pal32[b] >>> 0;
                  dst32++;
                  if (--xleft === 0) {
                    dst32 += rowStride32;
                    xleft = fw;
                  }
                }
              }
              if (prevCode !== null && nextCode < 4096 /* MAX_CODE */) {
                table[nextCode] = (prevCode & 4095) << 8 | outFirst & 255;
                this.firstByte[nextCode] = this.firstByte[prevCode];
                nextCode++;
                if (nextCode >= codeMask + 1 && codeSize < 12) {
                  codeSize++;
                  codeMask = codeMask << 1 | 1;
                }
              }
              prevCode = code;
            }
          }
        } else {
          while (true) {
            while (bitCount < codeSize && q < bytes.length) {
              bits |= (bytes[q++] | 0) << bitCount;
              bitCount += 8;
            }
            if (bitCount < codeSize) break;
            let code = bits & codeMask;
            bits >>>= codeSize;
            bitCount -= codeSize;
            if (code === CLEAR) {
              nextCode = EOI + 1;
              codeSize = minCodeSize + 1 | 0;
              codeMask = (1 << codeSize) - 1;
              prevCode = null;
              for (let i = 0; i < CLEAR; i++) this.firstByte[i] = i;
              continue;
            } else if (code === EOI) {
              break;
            }
            let outFirst;
            let cur = code;
            if (cur < CLEAR) {
              outFirst = cur;
              const b = outFirst & 255;
              out32[dst32] = pal32[b] >>> 0;
              dst32++;
              if (--xleft === 0) {
                dst32 += rowStride32;
                xleft = fw;
              }
            } else {
              sp = 0;
              if (cur >= nextCode) {
                if (prevCode === null) break;
                outFirst = this.firstByte[prevCode] | 0;
                stack[sp++] = outFirst;
                cur = prevCode;
              } else {
                outFirst = this.firstByte[cur] | 0;
              }
              while (cur >= CLEAR) {
                const entry = table[cur] | 0;
                stack[sp++] = entry & 255;
                cur = entry >>> 8;
              }
              const base = cur & 255;
              out32[dst32] = pal32[base] >>> 0;
              dst32++;
              if (--xleft === 0) {
                dst32 += rowStride32;
                xleft = fw;
              }
              while (sp) {
                const b = stack[--sp] & 255;
                out32[dst32] = pal32[b] >>> 0;
                dst32++;
                if (--xleft === 0) {
                  dst32 += rowStride32;
                  xleft = fw;
                }
              }
            }
            if (prevCode !== null && nextCode < 4096 /* MAX_CODE */) {
              table[nextCode] = (prevCode & 4095) << 8 | outFirst & 255;
              this.firstByte[nextCode] = this.firstByte[prevCode];
              nextCode++;
              if (nextCode >= codeMask + 1 && codeSize < 12) {
                codeSize++;
                codeMask = codeMask << 1 | 1;
              }
            }
            prevCode = code;
          }
        }
      } else {
        const frameSize = fw * fh;
        let framePixels;
        if (!moduleFramePixelsInUse && frameSize <= moduleReusableFramePixels.length) {
          moduleFramePixelsInUse = true;
          framePixels = moduleReusableFramePixels.subarray(0, frameSize);
        } else {
          framePixels = new Uint8Array(frameSize);
        }
        let pixelIndex = 0;
        while (true) {
          while (bitCount < codeSize && q < bytes.length) {
            bits |= (bytes[q++] | 0) << bitCount;
            bitCount += 8;
          }
          if (bitCount < codeSize) break;
          let code = bits & codeMask;
          bits >>>= codeSize;
          bitCount -= codeSize;
          if (code === CLEAR) {
            nextCode = EOI + 1;
            codeSize = minCodeSize + 1 | 0;
            codeMask = (1 << codeSize) - 1;
            prevCode = null;
            for (let i = 0; i < CLEAR; i++) this.firstByte[i] = i;
            continue;
          } else if (code === EOI) {
            break;
          }
          let outFirst;
          let cur = code;
          if (cur < CLEAR) {
            outFirst = cur;
            if (pixelIndex < framePixels.length) {
              framePixels[pixelIndex++] = outFirst & 255;
            }
          } else {
            sp = 0;
            if (cur >= nextCode) {
              if (prevCode === null) break;
              outFirst = this.firstByte[prevCode] | 0;
              stack[sp++] = outFirst;
              cur = prevCode;
            } else {
              outFirst = this.firstByte[cur] | 0;
            }
            while (cur >= CLEAR) {
              const entry = table[cur] | 0;
              stack[sp++] = entry & 255;
              cur = entry >>> 8;
            }
            const base = cur & 255;
            if (pixelIndex < framePixels.length) {
              framePixels[pixelIndex++] = base & 255;
            }
            while (sp && pixelIndex < framePixels.length) {
              framePixels[pixelIndex++] = stack[--sp] & 255;
            }
          }
          if (prevCode !== null && nextCode < 4096 /* MAX_CODE */) {
            table[nextCode] = (prevCode & 4095) << 8 | outFirst & 255;
            this.firstByte[nextCode] = this.firstByte[prevCode];
            nextCode++;
            if (nextCode >= codeMask + 1 && codeSize < 12) {
              codeSize++;
              codeMask = codeMask << 1 | 1;
            }
          }
          prevCode = code;
        }
        pixelIndex = 0;
        for (let pass = 0, yStart = 0, yStride = 8; pass < 4; pass++) {
          if (pass === 1) {
            yStart = 4;
            yStride = 8;
          } else if (pass === 2) {
            yStart = 2;
            yStride = 4;
          } else if (pass === 3) {
            yStart = 1;
            yStride = 2;
          }
          for (let yInPass = 0; ; yInPass++) {
            const row = fy + yStart + yInPass * yStride;
            if (row >= fy + fh) break;
            let dst32 = row * canvasWidth + fx >>> 0;
            for (let x = 0; x < fw && pixelIndex < framePixels.length; x++) {
              const b = framePixels[pixelIndex++] & 255;
              out32[dst32] = pal32[b] >>> 0;
              dst32++;
            }
          }
        }
        if (framePixels === moduleReusableFramePixels.subarray(0, frameSize)) {
          moduleFramePixelsInUse = false;
        }
      }
    }
    /* Wasm-accelerated row-wise decode for non-interlaced, non-transparent frames */
    lzwDecodeToPixelsWasm(bytes, minCodeSize, out32, canvasWidth, fw, fh, fx, fy, pal32) {
      if (!this.colorMapWasm || !this.rowIndicesBuffer) return;
      this.colorMapWasm.heapU32.set(pal32.subarray(0, 256), this.colorMapWasm.palPtr >>> 2);
      let q = 0;
      const CLEAR = 1 << minCodeSize;
      const EOI = CLEAR + 1;
      let nextCode = EOI + 1;
      let codeSize = minCodeSize + 1 | 0;
      let codeMask = (1 << codeSize) - 1;
      for (let i = 0; i < CLEAR; i++) this.firstByte[i] = i;
      let bits = 0;
      let bitCount = 0;
      const table = this.decTable;
      const stack = this.stack;
      let sp = 0;
      let prevCode = null;
      let xleft = fw;
      const rowStride32 = canvasWidth - fw >>> 0;
      let dst32 = fy * canvasWidth + fx >>> 0;
      let rowCount = 0;
      const idxRow = this.rowIndicesBuffer.subarray(0, fw);
      while (true) {
        while (bitCount < codeSize && q < bytes.length) {
          bits |= (bytes[q++] | 0) << bitCount;
          bitCount += 8;
        }
        if (bitCount < codeSize) break;
        let code = bits & codeMask;
        bits >>>= codeSize;
        bitCount -= codeSize;
        if (code === CLEAR) {
          nextCode = EOI + 1;
          codeSize = minCodeSize + 1 | 0;
          codeMask = (1 << codeSize) - 1;
          prevCode = null;
          for (let i = 0; i < CLEAR; i++) this.firstByte[i] = i;
          continue;
        } else if (code === EOI) {
          break;
        }
        let outFirst;
        let cur = code;
        if (cur < CLEAR) {
          outFirst = cur;
          const b = outFirst & 255;
          idxRow[rowCount++] = b;
          dst32++;
          if (--xleft === 0) {
            this.flushRowToWasm(idxRow, rowCount, out32, dst32 - fw, fw);
            dst32 += rowStride32;
            xleft = fw;
            rowCount = 0;
          }
        } else {
          sp = 0;
          if (cur >= nextCode) {
            if (prevCode === null) break;
            outFirst = this.firstByte[prevCode] | 0;
            stack[sp++] = outFirst;
            cur = prevCode;
          } else {
            outFirst = this.firstByte[cur] | 0;
          }
          while (cur >= CLEAR) {
            const entry = table[cur] | 0;
            stack[sp++] = entry & 255;
            cur = entry >>> 8;
          }
          const base = cur & 255;
          idxRow[rowCount++] = base;
          dst32++;
          if (--xleft === 0) {
            this.flushRowToWasm(idxRow, rowCount, out32, dst32 - fw, fw);
            dst32 += rowStride32;
            xleft = fw;
            rowCount = 0;
          }
          while (sp) {
            const b = stack[--sp] & 255;
            idxRow[rowCount++] = b;
            dst32++;
            if (--xleft === 0) {
              this.flushRowToWasm(idxRow, rowCount, out32, dst32 - fw, fw);
              dst32 += rowStride32;
              xleft = fw;
              rowCount = 0;
            }
          }
        }
        if (prevCode !== null && nextCode < 4096 /* MAX_CODE */) {
          table[nextCode] = (prevCode & 4095) << 8 | outFirst & 255;
          this.firstByte[nextCode] = this.firstByte[prevCode];
          nextCode++;
          if (nextCode >= codeMask + 1 && codeSize < 12) {
            codeSize++;
            codeMask = codeMask << 1 | 1;
          }
        }
        prevCode = code;
      }
      if (rowCount > 0) {
        this.flushRowToWasm(idxRow, rowCount, out32, dst32 - rowCount, rowCount);
      }
    }
    /* Helper to flush a row of indices through Wasm color mapping */
    flushRowToWasm(idxRow, count, out32, startDst32, maxCount) {
      if (!this.colorMapWasm) return;
      this.colorMapWasm.heapU8.set(idxRow.subarray(0, count), this.colorMapWasm.idxPtr);
      this.colorMapWasm.map32(
        this.colorMapWasm.idxPtr,
        this.colorMapWasm.outPtr,
        this.colorMapWasm.palPtr,
        count
      );
      const wasmOut32 = this.colorMapWasm.heapU32.subarray(
        this.colorMapWasm.outPtr >>> 2,
        (this.colorMapWasm.outPtr >>> 2) + count
      );
      out32.set(wasmOut32, startDst32);
    }
  };
  var initializeWasmGlobally = initializeGlobalWasm;
  var getWasmStatus = () => ({
    ...WASM_FEATURES,
    initialized: globalWasmDecoder !== null,
    workerPoolAvailable: globalWasmWorkerPool !== null
  });
  var cleanupWasm = () => {
    if (globalWasmWorkerPool) {
      globalWasmWorkerPool.terminate();
      globalWasmWorkerPool = null;
    }
    globalWasmDecoder = null;
    wasmInitPromise = null;
  };
  (function() {
    const browserExports = {
      GifWriter,
      GifReader,
      initializeWasmGlobally,
      getWasmStatus,
      cleanupWasm
    };
    if (typeof window !== "undefined") {
      window.wtfgif = browserExports;
    } else if (typeof globalThis !== "undefined") {
      globalThis.wtfgif = browserExports;
    }
  })();
  return __toCommonJS(wtfgif_exports);
})();
