import {
  FrameInfo,
  GifDecodeBackend,
  GifDecodeBackendStatus,
  PooledDecoderTables,
  PreparedFrameBackendPreference,
  PreparedFrameCacheMode,
  PreparedFrameDedupeMode,
  PreparedFrameFormat,
  PreparedGifFrame,
  PreparedGifFrames,
  PreparedGifPlayer,
  PrepareFramesOptions,
  UnifiedGPUGifRenderer,
  ColorMapWasm,
  WasmWorkerPool,
} from "../types";
import { GIF } from "../constants/gif";
import { buildPal32 } from "../utils/palette";
import { concatSubBlocks } from "../utils/subblocks";
import { readNetscapeLoopCount } from "../utils/netscape";
import {
  createDecoderTables,
  getPooledDecoderTables,
  returnDecoderTablesToPool,
} from "./pool";
import {
  loadGPUModule as loadGpuModule,
  createGpuRenderer,
  GPUModule,
} from "../gpu/renderer";
import {
  initializeGlobalWasm,
  getWasmFeatures,
  getWasmDecoder,
  getWasmWorkerPool,
  getWasmInitPromise,
  setWasmInitPromise,
  isWasmReady as isGlobalWasmReady,
} from "../wasm/runtime";

// Reusable buffer for interlaced frame pixels. Size grows dynamically based on
// requested frame dimensions to avoid allocating a large fixed array upfront.
let moduleReusableFramePixels = new Uint8Array(0);
let moduleFramePixelsInUse = false;

type NormalizedPrepareFramesOptions = PrepareFramesOptions & {
  format: PreparedFrameFormat;
  composited: boolean;
  cache: PreparedFrameCacheMode;
  backend: PreparedFrameBackendPreference;
  deltas: boolean;
  dedupe: PreparedFrameDedupeMode;
};

type ChangedRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

/* ====== Reader (Decoder) ====== */
// moved to types.ts

export class GifReader {
  private p = 0;
  private width_: number;
  private height_: number;

  private globalPaletteOffset: number | null = null;
  private globalPaletteSize: number | null = null;

  private frames: FrameInfo[] = [];
  private loop_count: number | null = null;

  private backgroundIndex = 0;

  // Pooled decoder tables for reuse across instances
  private pooledTables: PooledDecoderTables | null = null;
  private gifHash: string;
  private usePooling: boolean;

  // Aliases for easier access
  private decTable: Int32Array = new Int32Array(0);
  private stack: Uint8Array = new Uint8Array(0);
  private firstByte: Int16Array = new Int16Array(0);
  private out32Cache: WeakMap<Uint8Array, Uint32Array> | null = null;

  // Zero-copy canvas support
  private zeroCopyBuffers: Map<
    string,
    {
      wasmPtr: number;
      rgbaU8: Uint8ClampedArray;
      imageData: ImageData;
    }
  > | null = null;

  private colorMapWasm: ColorMapWasm | null = null; // ColorMapWasm type
  private wasmEnabled = false;
  private rowIndicesBuffer: Uint8Array | null = null;

  private gpuRenderer: UnifiedGPUGifRenderer | null = null;
  private gpuEnabled = false;

  private workerPool: WasmWorkerPool | null = null; // WorkerPoolManager
  private workerPoolEnabled = false;

  private preparedFramesCache = new Map<string, PreparedGifFrames>();
  private preparedOut32Cache = new WeakMap<Uint8Array, Uint32Array>();

  private static decodeBackend: GifDecodeBackend | null = null;

  static createPooled(buf: Uint8Array): GifReader {
    return new GifReader(buf, true);
  }

  static createUnpooled(buf: Uint8Array): GifReader {
    return new GifReader(buf, false);
  }

  static setDecodeBackend(backend: GifDecodeBackend | null): void {
    GifReader.decodeBackend = backend;
  }

  static getDecodeBackendStatus(): GifDecodeBackendStatus {
    const backend = GifReader.decodeBackend;
    if (!backend) {
      return { name: "javascript", available: false };
    }

    try {
      return { name: backend.name, available: backend.isAvailable() };
    } catch {
      return { name: backend.name, available: false };
    }
  }

  constructor(private buf: Uint8Array, usePooling: boolean = true) {
    this.usePooling = usePooling;
    this.gifHash = usePooling ? "pooled" : "";
    let p = 0;
    // Header: GIF87a / GIF89a
    if (
      buf[p++] !== GIF.G ||
      buf[p++] !== GIF.I ||
      buf[p++] !== GIF.F ||
      buf[p++] !== GIF._8 ||
      ((buf[p++]! + 1) & 0xfd) !== GIF._8 ||
      buf[p++] !== GIF.A
    ) {
      throw new Error("Invalid GIF 87a/89a header.");
    }

    const width = (buf[p++]! | (buf[p++]! << 8)) >>> 0;
    const height = (buf[p++]! | (buf[p++]! << 8)) >>> 0;
    this.width_ = width;
    this.height_ = height;

    const pf0 = buf[p++]!; // packed fields
    const gctFlag = (pf0 >>> 7) & 1;
    const gctSizeBits = pf0 & 0x7;
    const gctColors = 1 << (gctSizeBits + 1);
    const background = buf[p++]!;
    this.backgroundIndex = background;
    p++; // pixel aspect ratio

    if (gctFlag) {
      this.globalPaletteOffset = p;
      this.globalPaletteSize = gctColors;
      p += gctColors * 3;
    }

    let delay = 0;
    let transparent_index: number | null = null;
    let disposal = 0;

    // Parse blocks
    let noEOF = true;
    while (noEOF && p < buf.length) {
      const block = buf[p++];
      switch (block) {
        case GIF.EXT: {
          const label = buf[p++];
          switch (label) {
            case GIF.APPLICATION: {
              const netscape = readNetscapeLoopCount(buf, p);
              if (netscape) {
                this.loop_count = netscape.loopCount;
                p = netscape.nextPos;
              } else {
                // skip unknown app extension
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
            case GIF.GCE: {
              if (buf[p++] !== 0x4 || buf[p + 4] !== 0)
                throw new Error("Invalid graphics extension block.");
              const pf1 = buf[p++];
              delay = (buf[p++] | (buf[p++] << 8)) >>> 0;
              const t = buf[p++];
              transparent_index = pf1 & 1 ? t : null;
              disposal = (pf1 >> 2) & 0x7;
              p++; // terminator
              break;
            }
            case GIF.PLAINTEXT:
            case GIF.COMMENT: {
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
                "Unknown graphic control label: 0x" +
                  (label as number).toString(16)
              );
          }
          break;
        }

        case GIF.IMG: {
          const x = (buf[p++] | (buf[p++] << 8)) >>> 0;
          const y = (buf[p++] | (buf[p++] << 8)) >>> 0;
          const w = (buf[p++] | (buf[p++] << 8)) >>> 0;
          const h = (buf[p++] | (buf[p++] << 8)) >>> 0;
          const pf2 = buf[p++];
          const lctFlag = (pf2 >>> 7) & 1;
          const interlace = ((pf2 >>> 6) & 1) !== 0;
          const lctSizeBits = pf2 & 0x7;
          const lctColors = 1 << (lctSizeBits + 1);
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
          p++; // codesize
          while (true) {
            const size = buf[p++];
            if (!(size >= 0)) throw new Error("Invalid block size");
            if (size === 0) break;
            p += size;
          }

          this.frames.push({
            x,
            y,
            width: w,
            height: h,
            has_local_palette,
            palette_offset: palette_offset ?? 0,
            palette_size: palette_size ?? 0,
            data_offset, // keep for compatibility
            data_length: p - data_offset,
            transparent_index,
            interlaced: interlace,
            delay,
            disposal,
            min_code_size: buf[data_offset] | 0,
          });

          // Reset GCE state for next frame
          delay = 0;
          transparent_index = null;
          disposal = 0;
          break;
        }

        case GIF.TRAILER:
          noEOF = false;
          break;

        default:
          throw new Error(
            "Unknown gif block: 0x" + (block as number).toString(16)
          );
      }
    }
  }

  get width(): number {
    return this.width_;
  }
  get height(): number {
    return this.height_;
  }

  async initWasm(wasmPath?: string): Promise<boolean> {
    const features = getWasmFeatures();
    if (!features.supported) {
      return false;
    }

    let initPromise = getWasmInitPromise();
    if (!getWasmDecoder() && !initPromise) {
      initPromise = initializeGlobalWasm(wasmPath);
      setWasmInitPromise(initPromise);
    }

    if (initPromise) {
      await initPromise;
    }

    return getWasmDecoder() !== null;
  }

  isWasmReady(): boolean {
    return isGlobalWasmReady();
  }

  async framePixelsWasm(
    frameIndex: number,
    pixels?: Uint32Array
  ): Promise<Uint32Array> {
      // Try Wasm first if available
      const decoder = getWasmDecoder();
      if (this.isWasmReady() && decoder) {
      try {
        const result = await this.decodeFrameWasm(frameIndex, pixels);
        if (result) {
          return result;
        }
      } catch (error) {
        console.warn("Wasm decode failed, falling back to JavaScript:", error);
      }
    }

    // Fallback to JavaScript decoder
    const outputSize = this.width_ * this.height_;
    if (!pixels || pixels.length < outputSize) {
      pixels = new Uint32Array(outputSize);
    }

    // Use the existing JavaScript decoder
    const uint8Buffer = new Uint8Array(
      pixels.buffer,
      pixels.byteOffset,
      pixels.byteLength
    );
    this.decodeAndBlitFrameRGBA(frameIndex, uint8Buffer);

    return pixels;
  }

    private async decodeFrameWasm(
      frameIndex: number,
      pixels?: Uint32Array
    ): Promise<Uint32Array | null> {
      const decoder = getWasmDecoder();
      if (!decoder) {
        return null;
      }

    if (frameIndex < 0 || frameIndex >= this.frames.length) {
      throw new Error("Frame index out of bounds");
    }

    // Calculate required output size
    const outputSize = this.width_ * this.height_;

    // Allocate or reuse output buffer
    if (!pixels || pixels.length < outputSize) {
      pixels = new Uint32Array(outputSize);
    }

    // Allocate GIF data in Wasm heap
      const gifPtr = decoder.wasm_malloc(this.buf.length);
    if (gifPtr === 0) {
      throw new Error("Failed to allocate Wasm memory for GIF data");
    }

      const outPtr = decoder.wasm_malloc(outputSize * 4);
    if (outPtr === 0) {
        decoder.wasm_free(gifPtr);
      throw new Error("Failed to allocate Wasm memory for output");
    }

    try {
      // Copy GIF data to Wasm heap
        decoder.heapU8.set(this.buf, gifPtr);

      // Call Wasm decoder
        const result = decoder.decode_rgba(
        gifPtr,
        this.buf.length,
        frameIndex,
        outPtr >>> 2, // Convert to u32 offset
        outputSize
      );

      // Check for errors
      if (result > 1000) {
        throw new Error(`Wasm decode error: ${result}`);
      }

      // Copy result back to JavaScript
        const wasmOutput = decoder.heapU32.subarray(
        outPtr >>> 2,
        (outPtr >>> 2) + outputSize
      );
      // Parity check: ensure decoder produced expected number of pixels
      if (wasmOutput.length !== outputSize) {
        throw new Error(
          `Wasm output length ${wasmOutput.length} does not match expected ${outputSize}`
        );
      }
      pixels.set(wasmOutput);

      return pixels;
    } finally {
        decoder.wasm_free(gifPtr);
        decoder.wasm_free(outPtr);
    }
  }

  async framePixelsParallel(
    frameIndices: number[]
  ): Promise<{ pixels: Uint32Array; delay: number }[]> {
    const workerPool = getWasmWorkerPool();
    const features = getWasmFeatures();
    if (!features.threads || !workerPool) {
      // Fallback: decode sequentially using regular method
      const results = [];
      for (const frameIndex of frameIndices) {
        const pixels = await this.framePixelsWasm(frameIndex);
        const delay = this.frameInfo(frameIndex).delay || 100;
        results.push({ pixels, delay });
      }
      return results;
    }

    // Use worker pool for parallel decode
    return workerPool.decodeFrames(this.buf, frameIndices);
  }

  getWasmStats(): {
    supported: boolean;
    simd: boolean;
    threads: boolean;
    heapUsage?: number;
  } {
    const features = getWasmFeatures();
    return {
      ...features,
      heapUsage: getWasmDecoder()?.get_heap_usage(),
    };
  }

  async initGPU(canvas?: HTMLCanvasElement): Promise<boolean> {
    try {
      // Lazy-load GPU renderer
      if (!this.gpuRenderer) {
        const gpuModule = this.loadGPUModule();
        if (!gpuModule) {
          return false;
        }

        this.gpuRenderer = createGpuRenderer(gpuModule);
        if (!this.gpuRenderer) return false;
      }

      const success = await this.gpuRenderer!.initialize(canvas);
      this.gpuEnabled = success;
      return success;
    } catch (error) {
      console.warn("GPU palette expansion failed to initialize:", error);
      this.gpuEnabled = false;
      return false;
    }
  }

  async framePixelsGPU(
    frameIndex: number,
    targetCanvas?: HTMLCanvasElement
  ): Promise<HTMLCanvasElement | null> {
    if (!this.gpuEnabled || !this.gpuRenderer) {
      // Auto-initialize GPU if not done yet
      if (!(await this.initGPU())) {
        return null; // GPU not available, use other methods
      }
    }

    if (frameIndex < 0 || frameIndex >= this.frames.length) {
      throw new Error("Frame index out of bounds");
    }

    // Get frame info and decode indices using JavaScript LZW decoder
    const frame = this.frameInfo(frameIndex);
    const indexData = await this.decodeFrameIndices(frameIndex);

    if (!indexData) {
      return null;
    }

    // Use GPU for palette expansion
    if (!this.gpuRenderer) return null;

    if (targetCanvas) {
      const success = await this.gpuRenderer.renderToCanvas(
        indexData,
        this.getFramePalette(frame, "rgba"),
        frame.width,
        frame.height,
        targetCanvas
      );
      return success ? targetCanvas : null;
    } else {
      return await this.gpuRenderer.renderFrame(
        indexData,
        this.getFramePalette(frame, "rgba"),
        frame.width,
        frame.height
      );
    }
  }

    frameImageDataZeroCopy(
      frameIndex: number,
      ctx2d: CanvasRenderingContext2D
    ): void {
      const decoder = getWasmDecoder();
      if (!this.isWasmReady() || !decoder) {
        throw new Error("WebAssembly not available for zero-copy presentation");
      }

    const frame = this.frameInfo(frameIndex);
    const w = frame.width;
    const h = frame.height;
    const bufferKey = `${w}x${h}`;

    const zeroCopyBuffers = (this.zeroCopyBuffers ??= new Map());
    let buffer = zeroCopyBuffers.get(bufferKey);

      if (!buffer) {
        // Allocate persistent WebAssembly buffer
        const outPtr = decoder.wasm_malloc(w * h * 4);
        const rgbaU8 = new Uint8ClampedArray(
          decoder.memory.buffer,
          outPtr,
          w * h * 4
        );
      const imageData = new ImageData(rgbaU8, w, h); // shares the same buffer

      buffer = { wasmPtr: outPtr, rgbaU8, imageData };
      zeroCopyBuffers.set(bufferKey, buffer);
    }

      // Decode frame directly into persistent buffer
      decoder.decode_rgba(
        this.buf.byteOffset || 0,
        this.buf.length,
        frameIndex,
        buffer.wasmPtr,
        w * h
      );

    // Zero-copy presentation - no .set(), no GC
    ctx2d.putImageData(buffer.imageData, 0, 0);
  }

  async frameImageBitmapGPU(frameIndex: number): Promise<ImageBitmap | null> {
    if (!this.gpuEnabled || !this.gpuRenderer) {
      if (!(await this.initGPU())) {
        return null;
      }
    }

    // Render to OffscreenCanvas
    const frame = this.frameInfo(frameIndex);
    const offscreen = new OffscreenCanvas(frame.width, frame.height);
    const indexData = await this.decodeFrameIndices(frameIndex);

    if (!indexData) return null;

    if (!this.gpuRenderer) return null;
    const success = await this.gpuRenderer.renderToCanvas(
      indexData,
      this.getFramePalette(frame, "rgba"),
      frame.width,
      frame.height,
      offscreen as unknown as HTMLCanvasElement // OffscreenCanvas compatible with HTMLCanvasElement interface
    );

    if (!success) return null;

    // Create ImageBitmap for efficient transfer and drawing
    return createImageBitmap(offscreen);
  }

  async frameTransferBitmapGPU(
    frameIndex: number
  ): Promise<ImageBitmap | null> {
    if (!this.gpuEnabled || !this.gpuRenderer) {
      if (!(await this.initGPU())) {
        return null;
      }
    }

    const frame = this.frameInfo(frameIndex);
    const offscreen = new OffscreenCanvas(frame.width, frame.height);
    const indexData = await this.decodeFrameIndices(frameIndex);

    if (!indexData) return null;

    if (!this.gpuRenderer) return null;
    const success = await this.gpuRenderer.renderToCanvas(
      indexData,
      this.getFramePalette(frame, "rgba"),
      frame.width,
      frame.height,
      offscreen as unknown as HTMLCanvasElement
    );

    if (!success) return null;

    // Transfer ownership to ImageBitmap (can be posted to main thread)
    return offscreen.transferToImageBitmap();
  }

    cleanupZeroCopyBuffers(): void {
      const decoder = getWasmDecoder();
      if (decoder && this.zeroCopyBuffers) {
        for (const buffer of this.zeroCopyBuffers.values()) {
          decoder.wasm_free(buffer.wasmPtr);
        }
      }
      this.zeroCopyBuffers?.clear();
      this.zeroCopyBuffers = null;
    }

  async initWorkerPool(): Promise<boolean> {
    // Threaded worker pool support removed in cleanup build
    this.workerPoolEnabled = false;
    return false;
  }

  /**
   * Parallel frame decode using threaded worker pool
   * Each worker has its own Wasm instance and LZW tables
   */
  async framePixelsThreadedPool(
    frameIndices: number[]
  ): Promise<{ pixels: Uint32Array; delay: number }[]> {
    if (!this.workerPoolEnabled || !this.workerPool) {
      // Auto-initialize worker pool
      const initialized = await this.initWorkerPool();
      if (!initialized) {
        // Fallback to sequential decode
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
      // Decode frames in parallel using worker pool
      const decodeResults = await this.workerPool.decodeFrames(
        this.buf,
        frameIndices
      );

      // Convert worker results to expected format with parity checks
      return decodeResults.map((result: { pixels: Uint32Array; delay: number }) => {
        if (
          !result.pixels ||
          result.pixels.length !== this.width_ * this.height_
        ) {
          throw new Error("Worker pool returned invalid pixel data");
        }
        return {
          pixels: result.pixels,
          delay: result.delay,
        };
      });
    } catch (error) {
      console.warn(
        "Worker pool decode failed, falling back to sequential:",
        error
      );

      // Sequential fallback
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
  async framePixelsWorkerPool(frameIndex: number): Promise<Uint32Array> {
    if (!this.workerPoolEnabled || !this.workerPool) {
      await this.initWorkerPool();
    }

    if (this.workerPoolEnabled && this.workerPool) {
      try {
        const result = await this.workerPool.decodeFrame(this.buf, frameIndex);
        if (
          !result.pixels ||
          result.pixels.length !== this.width_ * this.height_
        ) {
          throw new Error("Worker pool returned invalid pixel data");
        }
        return result.pixels;
      } catch (error) {
        console.warn("Worker pool single frame decode failed:", error);
      }
    }

    // Fallback to regular method
    return this.framePixelsWasm(frameIndex);
  }

  /**
   * Get worker pool statistics and performance metrics
   */
  getWorkerPoolStats(): ReturnType<WasmWorkerPool["getStats"]> | null {
    if (!this.workerPoolEnabled || !this.workerPool) {
      return null;
    }

    return this.workerPool.getStats();
  }

  /**
   * Check if worker pool is available and ready
   */
  isWorkerPoolReady(): boolean {
    return this.workerPoolEnabled && this.workerPool !== null;
  }

  /**
   * Cleanup worker pool resources
   */
  async cleanupWorkerPool(): Promise<void> {
    if (this.workerPool) {
      await this.workerPool.terminate();
      this.workerPool = null;
      this.workerPoolEnabled = false;
    }
  }

  /**
   * Decode frame to index data only (for GPU palette expansion)
   */
  private async decodeFrameIndices(
    frameIndex: number
  ): Promise<Uint8Array | null> {
    try {
      return this.getFrameIndices(this.frameInfo(frameIndex));
    } catch (error) {
      console.error("Failed to decode frame indices:", error);
      return null;
    }
  }

  /**
   * Simplified LZW decoder that outputs palette indices instead of RGBA
   */
  private lzwDecodeToIndices(
    frame: FrameInfo,
    outputIndices: Uint8Array
  ): void {
    this.ensureDecoderTables();
    const bytes = this.getFrameCodes(frame);
    const minCodeSize = frame.min_code_size | 0;
    let q = 0;

    const CLEAR = 1 << minCodeSize;
    const EOI = CLEAR + 1;
    let nextCode = EOI + 1;
    let codeSize = (minCodeSize + 1) | 0;
    let codeMask = (1 << codeSize) - 1;

    // Initialize firstByte table for base codes
    for (let i = 0; i < CLEAR; i++) {
      this.firstByte[i] = i;
    }

    let bits = 0;
    let bitCount = 0;
    let pixelIndex = 0;

    const table = this.decTable;
    const stack = this.stack;
    let sp = 0;
    let prevCode: number | null = null;

    while (true) {
      // Fill bit buffer
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
        codeSize = (minCodeSize + 1) | 0;
        codeMask = (1 << codeSize) - 1;
        prevCode = null;
        for (let i = 0; i < CLEAR; i++) {
          this.firstByte[i] = i;
        }
        continue;
      } else if (code === EOI) {
        break;
      }

      let outFirst: number;
      let cur = code;

      if (cur < CLEAR) {
        // Single byte
        outFirst = cur;
        if (pixelIndex < outputIndices.length) {
          outputIndices[pixelIndex++] = outFirst & 0xff;
        }
      } else {
        // Multi-byte sequence
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
          stack[sp++] = entry & 0xff;
          cur = entry >>> 8;
        }

        // Output base symbol
        const base = cur & 0xff;
        if (pixelIndex < outputIndices.length) {
          outputIndices[pixelIndex++] = base;
        }

        // Output stack in reverse
        while (sp && pixelIndex < outputIndices.length) {
          outputIndices[pixelIndex++] = stack[--sp] & 0xff;
        }
      }

      // Add new table entry
      if (prevCode !== null && nextCode < GIF.MAX_CODE) {
        table[nextCode] = ((prevCode & 0xfff) << 8) | (outFirst & 0xff);
        this.firstByte[nextCode] = this.firstByte[prevCode];
        nextCode++;

        if (nextCode >= codeMask + 1 && codeSize < 12) {
          codeSize++;
          codeMask = (1 << codeSize) - 1;
        }
      }

      prevCode = code;
    }
  }

  /**
   * Check if GPU acceleration is available and enabled
   */
  isGPUEnabled(): boolean {
    return this.gpuEnabled && this.gpuRenderer !== null;
  }

  /**
   * Get GPU backend information
   */
  getGPUBackend(): string {
    return this.gpuRenderer?.getBackend() || "none";
  }

  /**
   * Benchmark GPU performance
   */
  async benchmarkGPU(width = 512, height = 512): Promise<number | null> {
    if (!this.gpuEnabled || !this.gpuRenderer) {
      return null;
    }

    return await this.gpuRenderer.benchmark(width, height);
  }

  /**
   * Disable GPU acceleration
   */
  disableGPU(): void {
    if (this.gpuRenderer) {
      this.gpuRenderer.dispose();
      this.gpuRenderer = null;
    }
    this.gpuEnabled = false;
  }

  /**
   * Lazy-load GPU module to avoid startup cost
   */
  private loadGPUModule(): GPUModule | null {
    try {
      return loadGpuModule();
    } catch {
      return null;
    }
  }

  numFrames(): number {
    return this.frames.length;
  }
  loopCount(): number | null {
    return this.loop_count;
  }

  frameInfo(i: number): FrameInfo {
    if (i < 0 || i >= this.frames.length)
      throw new Error("Frame index out of range.");
    return this.frames[i];
  }

  private ensureDecoderTables(): void {
    if (this.pooledTables) return;

    this.pooledTables = this.usePooling
      ? getPooledDecoderTables(this.gifHash)
      : createDecoderTables();
    this.decTable = this.pooledTables.decTable;
    this.stack = this.pooledTables.stack;
    this.firstByte = this.pooledTables.firstByte;
    this.out32Cache = this.pooledTables.out32Cache;
  }

  private getFrameCodes(frame: FrameInfo): Uint8Array {
    if (!frame.codes) {
      const { bytes, mcs } = concatSubBlocks(this.buf, frame.data_offset);
      frame.codes = bytes;
      frame.min_code_size = mcs;
    }
    return frame.codes;
  }

  private getFramePalette(
    frame: FrameInfo,
    order: "rgba" | "bgra"
  ): Uint32Array {
    if (order === "rgba") {
      frame.pal32rgba ??= buildPal32(
        this.buf,
        frame.palette_offset,
        frame.palette_size,
        "rgba",
        frame.transparent_index
      );
      return frame.pal32rgba;
    }

    frame.pal32bgra ??= buildPal32(
      this.buf,
      frame.palette_offset,
      frame.palette_size,
      "bgra",
      frame.transparent_index
    );
    return frame.pal32bgra;
  }

  private getFrameIndices(frame: FrameInfo): Uint8Array {
    if (frame.indices) {
      return frame.indices;
    }

    const frameSize = frame.width * frame.height;
    const indexData = new Uint8Array(frameSize);

    if (!frame.interlaced) {
      this.lzwDecodeToIndices(frame, indexData);
      frame.indices = indexData;
      return indexData;
    }

    const tempIndices = new Uint8Array(frameSize);
    this.lzwDecodeToIndices(frame, tempIndices);

    let pixelIndex = 0;
    for (let pass = 0; pass < 4; pass++) {
      let yStart = 0;
      let yStride = 8;
      if (pass === 1) {
        yStart = 4;
      } else if (pass === 2) {
        yStart = 2;
        yStride = 4;
      } else if (pass === 3) {
        yStart = 1;
        yStride = 2;
      }

      for (let yInPass = 0; ; yInPass++) {
        const row = yStart + yInPass * yStride;
        if (row >= frame.height) break;

        let dst = row * frame.width;
        for (let x = 0; x < frame.width && pixelIndex < frameSize; x++) {
          indexData[dst++] = tempIndices[pixelIndex++];
        }
      }
    }

    frame.indices = indexData;
    return indexData;
  }

  /* Public API mirrors omggif: BGRA and RGBA outputs (Uint8Array). */
  decodeAndBlitFrameBGRA(frameNum: number, pixels: Uint8Array) {
    this.decodeAndBlitFrame32(frameNum, pixels, "bgra");
  }
  decodeAndBlitFrameRGBA(frameNum: number, pixels: Uint8Array) {
    this.decodeAndBlitFrame32(frameNum, pixels, "rgba");
  }

  /* Transferable-friendly API: decode into an ArrayBuffer that can be transferred between workers */
  decodeFrameToTransferableRGBA(frameNum: number): ArrayBuffer {
    const pixelCount = this.width_ * this.height_;
    const buffer = new ArrayBuffer(pixelCount * 4);
    const pixels = new Uint8Array(buffer);
    this.decodeAndBlitFrame32(frameNum, pixels, "rgba");
    return buffer;
  }

  decodeFrameToTransferableBGRA(frameNum: number): ArrayBuffer {
    const pixelCount = this.width_ * this.height_;
    const buffer = new ArrayBuffer(pixelCount * 4);
    const pixels = new Uint8Array(buffer);
    this.decodeAndBlitFrame32(frameNum, pixels, "bgra");
    return buffer;
  }

  /* Decode into a pre-allocated transferable buffer (for worker scenarios) */
  decodeFrameIntoBuffer(
    frameNum: number,
    buffer: ArrayBuffer,
    format: "rgba" | "bgra" = "rgba"
  ): void {
    const expectedSize = this.width_ * this.height_ * 4;
    if (buffer.byteLength < expectedSize) {
      throw new Error(
        `Buffer too small: need ${expectedSize} bytes, got ${buffer.byteLength}`
      );
    }
    const pixels = new Uint8Array(buffer, 0, expectedSize);
    this.decodeAndBlitFrame32(frameNum, pixels, format);
  }

  preparePlayback(
    options: Omit<PrepareFramesOptions, "composited"> = {}
  ): PreparedGifFrames {
    return this.prepareFrames({
      ...options,
      composited: true,
      cache: "composited",
    });
  }

  prepareFrames(options: PrepareFramesOptions = {}): PreparedGifFrames {
    const normalized = this.normalizePrepareFramesOptions(options);
    const frameIndices = this.normalizeFrameIndices(normalized.frameIndices);
    const cacheKey = this.getPreparedFramesCacheKey(normalized, frameIndices);
    const cached = this.preparedFramesCache.get(cacheKey);
    if (cached) {
      return cached;
    }

    if (normalized.backend !== "javascript") {
      const backendResult = this.prepareFramesWithBackend(normalized);
      if (backendResult) {
        this.preparedFramesCache.set(cacheKey, backendResult);
        return backendResult;
      }

      if (normalized.backend === "native") {
        throw new Error("Native GIF decode backend is not available.");
      }
    }

    const prepared = normalized.composited
      ? this.prepareCompositedFrames(normalized, frameIndices, cacheKey)
      : this.prepareUncompositedFrames(normalized, frameIndices, cacheKey);
    this.preparedFramesCache.set(cacheKey, prepared);
    return prepared;
  }

  async prepareFramesAsync(
    options: PrepareFramesOptions = {}
  ): Promise<PreparedGifFrames> {
    return this.prepareFrames(options);
  }

  decodeAndBlitCompositedFrameRGBA(frameNum: number, pixels: Uint8Array): void {
    this.preparePlayback({ format: "rgba" }).copyFrame(frameNum, pixels);
  }

  decodeAndBlitCompositedFrameBGRA(frameNum: number, pixels: Uint8Array): void {
    this.preparePlayback({ format: "bgra" }).copyFrame(frameNum, pixels);
  }

  private normalizePrepareFramesOptions(
    options: PrepareFramesOptions
  ): NormalizedPrepareFramesOptions {
    return {
      ...options,
      format: options.format ?? "rgba",
      composited: options.composited ?? false,
      cache: options.cache ?? (options.composited ? "composited" : "auto"),
      backend: options.backend ?? "auto",
      deltas: options.deltas ?? false,
      dedupe: options.dedupe ?? "adjacent",
    };
  }

  private normalizeFrameIndices(frameIndices?: readonly number[]): number[] {
    if (!frameIndices) {
      const all = new Array<number>(this.frames.length);
      for (let i = 0; i < this.frames.length; i++) {
        all[i] = i;
      }
      return all;
    }

    const seen = new Set<number>();
    const normalized: number[] = [];
    for (const frameIndex of frameIndices) {
      if (!Number.isInteger(frameIndex)) {
        throw new Error("Frame index out of range.");
      }
      const index = frameIndex | 0;
      if (index < 0 || index >= this.frames.length) {
        throw new Error("Frame index out of range.");
      }
      if (!seen.has(index)) {
        seen.add(index);
        normalized.push(index);
      }
    }
    return normalized;
  }

  private getPreparedFramesCacheKey(
    options: NormalizedPrepareFramesOptions,
    frameIndices: readonly number[]
  ): string {
    return [
      options.format,
      options.composited ? "1" : "0",
      options.cache,
      options.backend === "javascript"
        ? "javascript"
        : (GifReader.decodeBackend?.name ?? "javascript"),
      options.deltas ? "d1" : "d0",
      options.dedupe,
      options.maxBytes ?? -1,
      frameIndices.join(","),
    ].join("|");
  }

  private prepareFramesWithBackend(
    options: NormalizedPrepareFramesOptions
  ): PreparedGifFrames | null {
    const backend = GifReader.decodeBackend;
    if (!backend || !backend.prepareFrames) {
      return null;
    }

    let available = false;
    try {
      available = backend.isAvailable();
    } catch {
      available = false;
    }

    if (!available) {
      return null;
    }

    return backend.prepareFrames(this.buf, options) ?? null;
  }

  private prepareCompositedFrames(
    options: NormalizedPrepareFramesOptions,
    frameIndices: readonly number[],
    cacheKey: string
  ): PreparedGifFrames {
    const requested = new Set(frameIndices);
    const maxFrame = frameIndices.length > 0 ? Math.max(...frameIndices) : -1;
    const canvas = new Uint32Array(this.width_ * this.height_);
    const frames: PreparedGifFrame[] = [];
    const dedupe =
      options.dedupe === "all" ? new Map<number, Uint32Array[]>() : null;
    let byteLength = 0;
    let previousPixels: Uint32Array | null = null;
    let previousPreparedPixels: Uint32Array | null = null;

    for (let frameIndex = 0; frameIndex <= maxFrame; frameIndex++) {
      const frame = this.frames[frameIndex]!;
      const restore =
        frame.disposal === 3 ? new Uint32Array(canvas) : null;

      this.blitFrameIndicesToCanvas(frame, canvas, this.width_, options.format);

      if (requested.has(frameIndex)) {
        let pixels: Uint32Array | null = null;
        let bucket: Uint32Array[] | undefined;
        let hash = 0;
        if (dedupe) {
          hash = GifReader.hashPixels(canvas);
          bucket = dedupe.get(hash);
          pixels = GifReader.findMatchingPixels(canvas, bucket);
        } else if (
          options.dedupe === "adjacent" &&
          previousPreparedPixels &&
          GifReader.pixelsEqual(previousPreparedPixels, canvas)
        ) {
          pixels = previousPreparedPixels;
        }
        const changedRect =
          options.deltas && previousPixels
            ? GifReader.findChangedRect(
                previousPixels,
                canvas,
                this.width_,
                this.height_
              )
            : null;
        const changedPixels = changedRect
          ? GifReader.copyRectPixels(canvas, this.width_, changedRect)
          : undefined;
        let frameBytes = 0;
        const deltaBytes = changedPixels?.byteLength ?? 0;

        if (!pixels) {
          this.enforcePreparedByteBudget(
            byteLength + canvas.byteLength + deltaBytes,
            options.maxBytes
          );
          pixels = new Uint32Array(canvas);
          frameBytes = pixels.byteLength;
          byteLength += frameBytes + deltaBytes;

          if (dedupe) {
            if (bucket) {
              bucket.push(pixels);
            } else {
              dedupe.set(hash, [pixels]);
            }
          }
        } else {
          this.enforcePreparedByteBudget(
            byteLength + deltaBytes,
            options.maxBytes
          );
          byteLength += deltaBytes;
        }

        frames.push({
          index: frameIndex,
          x: 0,
          y: 0,
          width: this.width_,
          height: this.height_,
          delay: frame.delay,
          disposal: frame.disposal,
          byteLength: frameBytes + deltaBytes,
          isFullCanvas: true,
          ...(changedRect
            ? {
                changedX: changedRect.x,
                changedY: changedRect.y,
                changedWidth: changedRect.width,
                changedHeight: changedRect.height,
                changedPixels,
              }
            : {}),
          pixels,
        });
        previousPixels = pixels;
        previousPreparedPixels = pixels;
      }

      this.applyFrameDisposal(frame, canvas, restore);
    }

    return this.createPreparedFramesResult(
      options.format,
      true,
      frames,
      byteLength,
      options.maxBytes ?? null,
      cacheKey
    );
  }

  private prepareUncompositedFrames(
    options: NormalizedPrepareFramesOptions,
    frameIndices: readonly number[],
    cacheKey: string
  ): PreparedGifFrames {
    const frames: PreparedGifFrame[] = [];
    let byteLength = 0;

    for (const frameIndex of frameIndices) {
      const frame = this.frames[frameIndex]!;
      let prepared: PreparedGifFrame;

      if (options.cache === "indices") {
        const indices = this.getFrameIndices(frame);
        const palette = this.getFramePalette(frame, options.format);
        prepared = {
          index: frameIndex,
          x: frame.x,
          y: frame.y,
          width: frame.width,
          height: frame.height,
          delay: frame.delay,
          disposal: frame.disposal,
          byteLength: indices.byteLength + palette.byteLength,
          isFullCanvas: false,
          indices,
          palette,
        };
      } else {
        const colors = this.getFrameColors(frame, options.format, this.width_);
        const spans = frame.opaqueSpans;
        const positions = spans ? undefined : frame.opaquePositions;
        prepared = {
          index: frameIndex,
          x: frame.x,
          y: frame.y,
          width: frame.width,
          height: frame.height,
          delay: frame.delay,
          disposal: frame.disposal,
          byteLength:
            colors.byteLength +
            (spans?.byteLength ?? positions?.byteLength ?? 0),
          isFullCanvas: false,
          colors,
        };
        if (spans) {
          prepared.spans = spans;
        } else if (positions) {
          prepared.positions = positions;
        }
      }

      byteLength += prepared.byteLength;
      this.enforcePreparedByteBudget(byteLength, options.maxBytes);
      frames.push(prepared);
    }

    return this.createPreparedFramesResult(
      options.format,
      false,
      frames,
      byteLength,
      options.maxBytes ?? null,
      cacheKey
    );
  }

  private createPreparedFramesResult(
    format: PreparedFrameFormat,
    composited: boolean,
    frames: PreparedGifFrame[],
    byteLength: number,
    maxBytes: number | null,
    cacheKey: string
  ): PreparedGifFrames {
    const byIndex = new Map<number, PreparedGifFrame>();
    for (const frame of frames) {
      byIndex.set(frame.index, frame);
    }

    return {
      width: this.width_,
      height: this.height_,
      format,
      composited,
      frames,
      byteLength,
      maxBytes,
      getFrame: (index: number) => byIndex.get(index),
      getFramePixels: (index: number) => byIndex.get(index)?.pixels,
      getFrameBytes: (index: number) => {
        const pixels = byIndex.get(index)?.pixels;
        return pixels
          ? new Uint8Array(pixels.buffer, pixels.byteOffset, pixels.byteLength)
          : undefined;
      },
      copyFrame: (index: number, target: Uint8Array | Uint32Array) => {
        const frame = byIndex.get(index);
        if (!frame) {
          throw new Error("Frame index out of range.");
        }
        this.copyPreparedFrame(frame, target);
      },
      createPlayer: (target?: Uint8Array | Uint32Array) =>
        this.createPreparedPlayer(byIndex, target),
      dispose: () => {
        this.preparedFramesCache.delete(cacheKey);
      },
    };
  }

  private createPreparedPlayer(
    byIndex: Map<number, PreparedGifFrame>,
    target?: Uint8Array | Uint32Array
  ): PreparedGifPlayer {
    const target32 = target
      ? this.getPreparedTarget32(target)
      : new Uint32Array(this.width_ * this.height_);
    let currentIndex = -1;
    const drawFrame = (index: number): Uint32Array => {
      const frame = byIndex.get(index);
      if (!frame) {
        throw new Error("Frame index out of range.");
      }

      const previousFrame = byIndex.get(currentIndex);
      if (currentIndex === index - 1 && previousFrame?.pixels === frame.pixels) {
        currentIndex = index;
        return target32;
      }

      const changedPixelCount = frame.changedPixels?.length ?? 0;
      const fullPixelCount = frame.pixels?.length ?? target32.length;
      if (
        frame.changedPixels &&
        frame.changedX !== undefined &&
        frame.changedY !== undefined &&
        frame.changedWidth !== undefined &&
        frame.changedHeight !== undefined &&
        changedPixelCount <= (fullPixelCount >>> 3) &&
        currentIndex === index - 1
      ) {
        GifReader.blitRectPixels(
          frame.changedPixels,
          target32,
          this.width_,
          frame.changedX,
          frame.changedY,
          frame.changedWidth,
          frame.changedHeight
        );
      } else {
        this.copyPreparedFrame(frame, target32);
      }

      currentIndex = index;
      return target32;
    };

    return {
      target: target32,
      get currentIndex() {
        return currentIndex;
      },
      drawFrame,
      next: () => {
        const nextIndex = currentIndex + 1;
        return byIndex.has(nextIndex) ? drawFrame(nextIndex) : target32;
      },
      reset: () => {
        currentIndex = -1;
        target32.fill(0);
      },
    };
  }

  private copyPreparedFrame(
    frame: PreparedGifFrame,
    target: Uint8Array | Uint32Array
  ): void {
    const target32 = this.getPreparedTarget32(target);
    if (frame.pixels) {
      target32.set(frame.pixels);
      return;
    }

    if (frame.colors) {
      this.blitPreparedColors(frame, target32);
      return;
    }

    if (frame.indices && frame.palette) {
      this.blitPreparedIndices(frame, target32);
      return;
    }
  }

  private blitPreparedColors(
    frame: PreparedGifFrame,
    target32: Uint32Array
  ): void {
    const colors = frame.colors!;
    if (frame.spans) {
      const spans = frame.spans;
      for (let i = 0; i < spans.length; i += 3) {
        const dst = spans[i]!;
        const length = spans[i + 1]!;
        const src = spans[i + 2]!;
        if (length >= 8) {
          target32.set(colors.subarray(src, src + length), dst);
        } else {
          for (let j = 0; j < length; j++) {
            target32[dst + j] = colors[src + j]!;
          }
        }
      }
      return;
    }

    if (frame.positions) {
      const positions = frame.positions;
      for (let i = 0; i < colors.length; i++) {
        target32[positions[i]!] = colors[i]!;
      }
      return;
    }

    let src = 0;
    let dst = (frame.y * this.width_ + frame.x) | 0;
    if (frame.x === 0 && frame.width === this.width_) {
      target32.set(colors, dst);
      return;
    }

    const rowStride = this.width_ - frame.width;
    for (let y = 0; y < frame.height; y++) {
      for (let x = 0; x < frame.width; x++) {
        target32[dst++] = colors[src++]!;
      }
      dst += rowStride;
    }
  }

  private blitPreparedIndices(
    frame: PreparedGifFrame,
    target32: Uint32Array
  ): void {
    const indices = frame.indices!;
    const palette = frame.palette!;
    const sourceFrame = this.frames[frame.index]!;
    const transparentIndex = sourceFrame.transparent_index ?? 256;
    let src = 0;
    let dst = (frame.y * this.width_ + frame.x) | 0;
    const rowStride = this.width_ - frame.width;

    for (let y = 0; y < frame.height; y++) {
      for (let x = 0; x < frame.width; x++) {
        const index = indices[src++]!;
        if (index !== transparentIndex) {
          target32[dst] = palette[index]!;
        }
        dst++;
      }
      dst += rowStride;
    }
  }

  private getPreparedTarget32(target: Uint8Array | Uint32Array): Uint32Array {
    const requiredPixels = this.width_ * this.height_;
    if (target instanceof Uint32Array) {
      if (target.length < requiredPixels) {
        throw new Error(
          `Buffer too small: need ${requiredPixels * 4} bytes, got ${
            target.byteLength
          }`
        );
      }
      return target;
    }

    if (target.byteLength < requiredPixels * 4) {
      throw new Error(
        `Buffer too small: need ${requiredPixels * 4} bytes, got ${
          target.byteLength
        }`
      );
    }

    if ((target.byteOffset & 3) !== 0) {
      throw new Error("Pixel buffer byteOffset must be aligned to 4 bytes.");
    }

    let target32 = this.preparedOut32Cache.get(target);
    if (!target32) {
      target32 = new Uint32Array(
        target.buffer,
        target.byteOffset,
        requiredPixels
      );
      this.preparedOut32Cache.set(target, target32);
    }
    return target32;
  }

  private applyFrameDisposal(
    frame: FrameInfo,
    canvas: Uint32Array,
    restore: Uint32Array | null
  ): void {
    if (frame.disposal === 2) {
      this.clearFrameRect(canvas, frame);
    } else if (frame.disposal === 3 && restore) {
      canvas.set(restore);
    }
  }

  private clearFrameRect(canvas: Uint32Array, frame: FrameInfo): void {
    const x = Math.max(0, frame.x | 0);
    const y = Math.max(0, frame.y | 0);
    const right = Math.min(this.width_, x + (frame.width | 0));
    const bottom = Math.min(this.height_, y + (frame.height | 0));
    const width = right - x;
    if (width <= 0) {
      return;
    }

    for (let row = y; row < bottom; row++) {
      const start = row * this.width_ + x;
      canvas.fill(0, start, start + width);
    }
  }

  private static findChangedRect(
    previous: Uint32Array,
    current: Uint32Array,
    width: number,
    height: number
  ): ChangedRect | null {
    let top = 0;
    let bottom = height - 1;

    while (top < height) {
      const row = top * width;
      let changed = false;
      for (let x = 0; x < width; x++) {
        if (previous[row + x] !== current[row + x]) {
          changed = true;
          break;
        }
      }
      if (changed) break;
      top++;
    }

    if (top === height) {
      return null;
    }

    while (bottom > top) {
      const row = bottom * width;
      let changed = false;
      for (let x = 0; x < width; x++) {
        if (previous[row + x] !== current[row + x]) {
          changed = true;
          break;
        }
      }
      if (changed) break;
      bottom--;
    }

    let left = width - 1;
    let right = 0;
    for (let y = top; y <= bottom; y++) {
      const row = y * width;
      for (let x = 0; x < width; x++) {
        if (previous[row + x] !== current[row + x]) {
          if (x < left) left = x;
          if (x > right) right = x;
        }
      }
    }

    return {
      x: left,
      y: top,
      width: right - left + 1,
      height: bottom - top + 1,
    };
  }

  private static copyRectPixels(
    source: Uint32Array,
    sourceWidth: number,
    rect: ChangedRect
  ): Uint32Array {
    const pixels = new Uint32Array(rect.width * rect.height);
    for (let y = 0; y < rect.height; y++) {
      const src = (rect.y + y) * sourceWidth + rect.x;
      pixels.set(source.subarray(src, src + rect.width), y * rect.width);
    }
    return pixels;
  }

  private static blitRectPixels(
    source: Uint32Array,
    target: Uint32Array,
    targetWidth: number,
    x: number,
    y: number,
    width: number,
    height: number
  ): void {
    for (let row = 0; row < height; row++) {
      const src = row * width;
      const dst = (y + row) * targetWidth + x;
      target.set(source.subarray(src, src + width), dst);
    }
  }

  private enforcePreparedByteBudget(
    nextByteLength: number,
    maxBytes: number | undefined
  ): void {
    if (maxBytes !== undefined && nextByteLength > maxBytes) {
      throw new Error(
        `Prepared frame cache exceeds maxBytes (${nextByteLength} > ${maxBytes}).`
      );
    }
  }

  private static hashPixels(pixels: Uint32Array): number {
    let hash = 2166136261;
    for (let i = 0; i < pixels.length; i++) {
      hash ^= pixels[i]!;
      hash = Math.imul(hash, 16777619);
    }
    return hash >>> 0;
  }

  private static findMatchingPixels(
    pixels: Uint32Array,
    bucket: Uint32Array[] | undefined
  ): Uint32Array | null {
    if (!bucket) {
      return null;
    }

    for (const candidate of bucket) {
      if (candidate.length !== pixels.length) {
        continue;
      }

      let match = true;
      for (let i = 0; i < pixels.length; i++) {
        if (candidate[i] !== pixels[i]) {
          match = false;
          break;
        }
      }
      if (match) {
        return candidate;
      }
    }
    return null;
  }

  private static pixelsEqual(a: Uint32Array, b: Uint32Array): boolean {
    if (a.length !== b.length) {
      return false;
    }

    for (let i = 0; i < a.length; i++) {
      if (a[i] !== b[i]) {
        return false;
      }
    }
    return true;
  }

  /* Return decoder tables to pool for reuse (call when done with this GifReader) */
  dispose(): void {
    if (this.pooledTables && this.usePooling) {
      returnDecoderTablesToPool(this.pooledTables);
      this.pooledTables = null;
    }
  }

  /* Alias for dispose() to match expected pooling API */
  returnToPool(): void {
    this.dispose();
  }

  /* Get statistics about decoder table pool usage */
  static getPoolStats(): {
    available: number;
    totalCreated: number;
    hits: number;
    misses: number;
  } {
    // Simple stats tracking - in real implementation you'd track hits/misses
    return {
      available: 0,
      totalCreated: 0,
      hits: 0, // Would need to track in getPooledDecoderTables
      misses: 0, // Would need to track in getPooledDecoderTables
    };
  }

  /* Enable Wasm color mapping for faster palette lookups (Tier 2 optimization) */
  enableWasmColorMapping(colorMapWasm: ColorMapWasm): void {
    this.colorMapWasm = colorMapWasm;
    this.wasmEnabled = true;

    // Pre-allocate row buffer for indices (reused across frames)
    const maxRowWidth = Math.min(
      this.width_,
      colorMapWasm.maxRowWidth ?? 4096
    );
    this.rowIndicesBuffer = new Uint8Array(maxRowWidth);
  }

  /* Disable Wasm color mapping (fallback to JS) */
  disableWasmColorMapping(): void {
    this.wasmEnabled = false;
    this.colorMapWasm = null;
    this.rowIndicesBuffer = null;
  }

  /* Check if Wasm color mapping is enabled */
  isWasmEnabled(): boolean {
    return this.wasmEnabled && this.colorMapWasm !== null;
  }

  /* Fused LZW decode → Uint32 blit with precomputed pal32, transparency, interlace. */
  private decodeAndBlitFrame32(
    frameNum: number,
    pixels: Uint8Array,
    order: "rgba" | "bgra"
  ) {
    if (frameNum < 0 || frameNum >= this.frames.length)
      throw new Error("Frame index out of range.");

    this.ensureDecoderTables();

    // Reuse a cached Uint32 view for this pixels buffer
    const out32Cache = this.out32Cache!;
    let out32 = out32Cache.get(pixels);
    if (!out32) {
      out32 = new Uint32Array(
        pixels.buffer,
        pixels.byteOffset,
        pixels.byteLength >>> 2
      );
      out32Cache.set(pixels, out32);
    }

    const frame = this.frames[frameNum];
    const cachedColors = order === "rgba" ? frame.rgbaColors : frame.bgraColors;
    if (cachedColors || frame.indices || (frame.decodeCount ?? 0) > 0) {
      this.blitFrameColors(frame, out32, this.width_, order);
      return;
    }

    frame.decodeCount = 1;
    const pal32 = this.getFramePalette(frame, order);
    const trans = frame.transparent_index ?? 256;
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

  private getFrameColors(
    frame: FrameInfo,
    order: "rgba" | "bgra",
    canvasWidth: number,
  ): Uint32Array {
    const existing = order === "rgba" ? frame.rgbaColors : frame.bgraColors;
    if (existing) {
      return existing;
    }

    const indices = this.getFrameIndices(frame);
    const pal32 = this.getFramePalette(frame, order);
    const fw = frame.width | 0;
    const fh = frame.height | 0;
    const trans = frame.transparent_index ?? 256;
    const total = fw * fh;
    let colors: Uint32Array;

    if (trans === 256) {
      colors = new Uint32Array(total);
      for (let i = 0; i < total; i++) {
        colors[i] = pal32[indices[i]] >>> 0;
      }
    } else {
      let spans = frame.opaqueSpans;
      let positions = frame.opaquePositions;
      let opaqueCount = 0;
      if (spans) {
        for (let i = 1; i < spans.length; i += 3) {
          opaqueCount += spans[i]!;
        }
      } else if (positions) {
        opaqueCount = positions.length;
      } else {
        let spanCount = 0;
        for (let y = 0; y < fh; y++) {
          const row = y * fw;
          let x = 0;
          while (x < fw) {
            while (x < fw && indices[row + x] === trans) {
              x++;
            }
            if (x >= fw) break;
            spanCount++;
            while (x < fw && indices[row + x] !== trans) {
              opaqueCount++;
              x++;
            }
          }
        }

        if (spanCount === 0 || opaqueCount / spanCount >= 4) {
          spans = new Uint32Array(spanCount * 3);
          frame.opaqueSpans = spans;
        } else {
          positions = new Uint32Array(opaqueCount);
          frame.opaquePositions = positions;
        }
      }

      colors = new Uint32Array(opaqueCount);
      if (spans) {
        let out = 0;
        let spanOut = 0;
        for (let y = 0; y < fh; y++) {
          const row = y * fw;
          const dstRow = ((frame.y + y) | 0) * canvasWidth + (frame.x | 0);
          let x = 0;
          while (x < fw) {
            while (x < fw && indices[row + x] === trans) {
              x++;
            }
            if (x >= fw) break;

            const dst = dstRow + x;
            const colorStart = out;
            const xStart = x;
            while (x < fw) {
              const index = indices[row + x]!;
              if (index === trans) break;
              colors[out++] = pal32[index] >>> 0;
              x++;
            }

            if (spanOut < spans.length) {
              spans[spanOut++] = dst;
              spans[spanOut++] = x - xStart;
              spans[spanOut++] = colorStart;
            }
          }
        }
      } else if (positions) {
        let out = 0;
        for (let y = 0; y < fh; y++) {
          const row = y * fw;
          let dst = ((frame.y + y) | 0) * canvasWidth + (frame.x | 0);
          for (let x = 0; x < fw; x++) {
            const index = indices[row + x]!;
            if (index !== trans) {
              positions[out] = dst;
              colors[out++] = pal32[index] >>> 0;
            }
            dst++;
          }
        }
      }
    }

    if (order === "rgba") {
      frame.rgbaColors = colors;
    } else {
      frame.bgraColors = colors;
    }

    return colors;
  }

  private blitFrameColors(
    frame: FrameInfo,
    out32: Uint32Array,
    canvasWidth: number,
    order: "rgba" | "bgra"
  ): void {
    const colors = this.getFrameColors(frame, order, canvasWidth);
    const spans = frame.opaqueSpans;

    if (spans) {
      for (let i = 0; i < spans.length; i += 3) {
        const dst = spans[i]!;
        const length = spans[i + 1]!;
        const src = spans[i + 2]!;
        if (length >= 8) {
          out32.set(colors.subarray(src, src + length), dst);
        } else {
          for (let j = 0; j < length; j++) {
            out32[dst + j] = colors[src + j]!;
          }
        }
      }
      return;
    }

    const positions = frame.opaquePositions;
    if (positions) {
      for (let i = 0; i < colors.length; i++) {
        out32[positions[i]!] = colors[i]!;
      }
      return;
    }

    const fw = frame.width | 0;
    const fh = frame.height | 0;
    let src = 0;
    let dst = ((frame.y | 0) * canvasWidth + (frame.x | 0)) | 0;

    if ((frame.x | 0) === 0 && fw === canvasWidth) {
      out32.set(colors, dst);
      return;
    }

    const rowStride = canvasWidth - fw;
    for (let y = 0; y < fh; y++) {
      for (let x = 0; x < fw; x++) {
        out32[dst++] = colors[src++];
      }
      dst += rowStride;
    }
  }

  private blitFrameIndicesToCanvas(
    frame: FrameInfo,
    out32: Uint32Array,
    canvasWidth: number,
    order: "rgba" | "bgra"
  ): void {
    const indices = this.getFrameIndices(frame);
    const pal32 = this.getFramePalette(frame, order);
    const fw = frame.width | 0;
    const fh = frame.height | 0;
    const trans = frame.transparent_index ?? 256;
    const rowStride = canvasWidth - fw;
    let src = 0;
    let dst = ((frame.y | 0) * canvasWidth + (frame.x | 0)) | 0;

    if (trans === 256) {
      for (let y = 0; y < fh; y++) {
        for (let x = 0; x < fw; x++) {
          out32[dst++] = pal32[indices[src++]!]!;
        }
        dst += rowStride;
      }
      return;
    }

    for (let y = 0; y < fh; y++) {
      for (let x = 0; x < fw; x++) {
        const index = indices[src++]!;
        if (index !== trans) {
          out32[dst] = pal32[index]!;
        }
        dst++;
      }
      dst += rowStride;
    }
  }

  /* Optimized LZW decoder that streams symbols directly to destination pixels. */
  private lzwDecodeToPixels(
    codeStream: Uint8Array,
    dataOffset: number,
    out32: Uint32Array,
    canvasWidth: number,
    frame: FrameInfo,
    pal32: Uint32Array,
    transparentIndex: number
  ) {
    this.ensureDecoderTables();
    const bytes = this.getFrameCodes(frame);
    const minCodeSize = frame.min_code_size | 0;
    let q = 0; // cursor into contiguous bytes

    const CLEAR = 1 << minCodeSize;
    const EOI = CLEAR + 1;
    let nextCode = EOI + 1;

    let codeSize = (minCodeSize + 1) | 0;
    let codeMask = (1 << codeSize) - 1;

    // Initialize firstByte table for base codes at start
    for (let i = 0; i < CLEAR; i++) this.firstByte[i] = i;

    // Bit buffer
    let bits = 0;
    let bitCount = 0;

    // Output cursor state (handles interlace & row wraps)
    const fw = frame.width | 0;
    const fh = frame.height | 0;
    const fx = frame.x | 0;
    const fy = frame.y | 0;

    // Decoder tables
    const table = this.decTable;

    // Stack for sequence unwind (single-pass)
    const stack = this.stack;
    let sp = 0;

    let prevCode: number | null = null;

    // NEW: Split "no transparency" vs "has transparency" decode loops
    const hasTrans = transparentIndex !== 256; // 256 is sentinel

    if (!frame.interlaced) {
      // Fast path for non-interlaced frames
      let xleft = fw;
      const rowStride32 = (canvasWidth - fw) >>> 0;
      let dst32 = (fy * canvasWidth + fx) >>> 0;

      if (!hasTrans) {
        // FAST PATH: No transparency - use Wasm if available and suitable
        if (
          this.wasmEnabled &&
          this.colorMapWasm &&
          fw <= (this.colorMapWasm.maxRowWidth || 4096)
        ) {
          this.lzwDecodeToPixelsWasm(
            bytes,
            minCodeSize,
            out32,
            canvasWidth,
            fw,
            fh,
            fx,
            fy,
            pal32
          );
        } else {
          // Fallback to JS implementation
          while (true) {
            // Fill bit buffer to have at least codeSize bits
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
              codeSize = (minCodeSize + 1) | 0;
              codeMask = (1 << codeSize) - 1;
              prevCode = null;
              // Initialize firstByte table for base codes
              for (let i = 0; i < CLEAR; i++) this.firstByte[i] = i;
              continue;
            } else if (code === EOI) {
              break;
            }

            // Decode sequence for 'code'
            let outFirst: number;
            let cur = code;

            if (cur < CLEAR) {
              // Single byte - always write (no transparency check)
              outFirst = cur;
              const b = outFirst & 0xff;
              out32[dst32] = pal32[b] >>> 0;
              dst32++;
              if (--xleft === 0) {
                dst32 += rowStride32;
                xleft = fw;
              }
            } else {
              // Chase with stack
              sp = 0;
              if (cur >= nextCode) {
                // KwKwK case
                if (prevCode === null) break;
                outFirst = this.firstByte[prevCode] | 0; // O(1) instead of chasing
                stack[sp++] = outFirst;
                cur = prevCode;
              } else {
                outFirst = this.firstByte[cur] | 0; // O(1) instead of chasing
              }
              // unwind sequence
              while (cur >= CLEAR) {
                const entry = table[cur] | 0;
                stack[sp++] = entry & 0xff;
                cur = entry >>> 8;
              }
              // Write first base - always write (no transparency check)
              const base = cur & 0xff;
              out32[dst32] = pal32[base] >>> 0;
              dst32++;
              if (--xleft === 0) {
                dst32 += rowStride32;
                xleft = fw;
              }
              // Write stack backwards - always write (no transparency check)
              while (sp) {
                const b = stack[--sp] & 0xff;
                out32[dst32] = pal32[b] >>> 0;
                dst32++;
                if (--xleft === 0) {
                  dst32 += rowStride32;
                  xleft = fw;
                }
              }
            }

            // Add new table entry
            if (prevCode !== null && nextCode < GIF.MAX_CODE) {
              table[nextCode] = ((prevCode & 0xfff) << 8) | (outFirst & 0xff);
              this.firstByte[nextCode] = this.firstByte[prevCode]; // O(1) instead of chasing
              nextCode++;
              if (nextCode >= codeMask + 1 && codeSize < 12) {
                codeSize++;
                codeMask = (codeMask << 1) | 1;
              }
            }

            prevCode = code;
          }
        }
      } else {
        // HAS TRANSPARENCY: Check each pixel
        while (true) {
          // Fill bit buffer to have at least codeSize bits
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
            codeSize = (minCodeSize + 1) | 0;
            codeMask = (1 << codeSize) - 1;
            prevCode = null;
            // Initialize firstByte table for base codes
            for (let i = 0; i < CLEAR; i++) this.firstByte[i] = i;
            continue;
          } else if (code === EOI) {
            break;
          }

          // Decode sequence for 'code'
          let outFirst: number;
          let cur = code;

          if (cur < CLEAR) {
            // Single byte - transparent pixels leave the caller's buffer as-is.
            outFirst = cur;
            const b = outFirst & 0xff;
            if (b !== transparentIndex) {
              out32[dst32] = pal32[b] >>> 0;
            }
            dst32++;
            if (--xleft === 0) {
              dst32 += rowStride32;
              xleft = fw;
            }
          } else {
            // Chase with stack
            sp = 0;
            if (cur >= nextCode) {
              // KwKwK case
              if (prevCode === null) break;
              outFirst = this.firstByte[prevCode] | 0; // O(1) instead of chasing
              stack[sp++] = outFirst;
              cur = prevCode;
            } else {
              outFirst = this.firstByte[cur] | 0; // O(1) instead of chasing
            }
            // unwind sequence
            while (cur >= CLEAR) {
              const entry = table[cur] | 0;
              stack[sp++] = entry & 0xff;
              cur = entry >>> 8;
            }
            // Write first base - transparent pixels leave the caller's buffer as-is.
            const base = cur & 0xff;
            if (base !== transparentIndex) {
              out32[dst32] = pal32[base] >>> 0;
            }
            dst32++;
            if (--xleft === 0) {
              dst32 += rowStride32;
              xleft = fw;
            }
            // Write stack backwards - transparent pixels leave the caller's buffer as-is.
            while (sp) {
              const b = stack[--sp] & 0xff;
              if (b !== transparentIndex) {
                out32[dst32] = pal32[b] >>> 0;
              }
              dst32++;
              if (--xleft === 0) {
                dst32 += rowStride32;
                xleft = fw;
              }
            }
          }

          // Add new table entry
          if (prevCode !== null && nextCode < GIF.MAX_CODE) {
            table[nextCode] = ((prevCode & 0xfff) << 8) | (outFirst & 0xff);
            this.firstByte[nextCode] = this.firstByte[prevCode]; // O(1) instead of chasing
            nextCode++;
            if (nextCode >= codeMask + 1 && codeSize < 12) {
              codeSize++;
              codeMask = (codeMask << 1) | 1;
            }
          }

          prevCode = code;
        }
      }
    } else {
      // INTERLACED PATH: Use pass-loops with inline pixel positioning
      // First decode all pixels into a temporary buffer (reuse module-level array)
      const frameSize = fw * fh;
      let framePixels: Uint8Array;

      if (!moduleFramePixelsInUse) {
        // Grow reusable buffer if needed
        if (frameSize > moduleReusableFramePixels.length) {
          moduleReusableFramePixels = new Uint8Array(frameSize);
        }
        moduleFramePixelsInUse = true;
        framePixels = moduleReusableFramePixels.subarray(0, frameSize);
      } else {
        // Fallback to allocation if reusable array is currently in use
        framePixels = new Uint8Array(frameSize);
      }

      let pixelIndex = 0;

      // Decode all LZW symbols into linear pixel array
      while (true) {
        // Fill bit buffer
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
          codeSize = (minCodeSize + 1) | 0;
          codeMask = (1 << codeSize) - 1;
          prevCode = null;
          // Initialize firstByte table for base codes
          for (let i = 0; i < CLEAR; i++) this.firstByte[i] = i;
          continue;
        } else if (code === EOI) {
          break;
        }

        let outFirst: number;
        let cur = code;

        if (cur < CLEAR) {
          // Single byte
          outFirst = cur;
          if (pixelIndex < framePixels.length) {
            framePixels[pixelIndex++] = outFirst & 0xff;
          }
        } else {
          // Chase with stack
          sp = 0;
          if (cur >= nextCode) {
            if (prevCode === null) break;
            outFirst = this.firstByte[prevCode] | 0; // O(1) instead of chasing
            stack[sp++] = outFirst;
            cur = prevCode;
          } else {
            outFirst = this.firstByte[cur] | 0; // O(1) instead of chasing
          }
          while (cur >= CLEAR) {
            const entry = table[cur] | 0;
            stack[sp++] = entry & 0xff;
            cur = entry >>> 8;
          }
          // Write first base
          const base = cur & 0xff;
          if (pixelIndex < framePixels.length) {
            framePixels[pixelIndex++] = base & 0xff;
          }
          // Write stack backwards
          while (sp && pixelIndex < framePixels.length) {
            framePixels[pixelIndex++] = stack[--sp] & 0xff;
          }
        }

        if (prevCode !== null && nextCode < GIF.MAX_CODE) {
          table[nextCode] = ((prevCode & 0xfff) << 8) | (outFirst & 0xff);
          this.firstByte[nextCode] = this.firstByte[prevCode]; // O(1) instead of chasing
          nextCode++;
          if (nextCode >= codeMask + 1 && codeSize < 12) {
            codeSize++;
            codeMask = (codeMask << 1) | 1;
          }
        }

        prevCode = code;
      }

      // NEW: Pass-loops with inline pixel positioning - no callback overhead
      pixelIndex = 0;
      for (let pass = 0, yStart = 0, yStride = 8; pass < 4; pass++) {
        // Set pass parameters: pass 0: start=0, stride=8; pass 1: start=4, stride=8; pass 2: start=2, stride=4; pass 3: start=1, stride=2
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

          let dst32 = (row * canvasWidth + fx) >>> 0;

          // Emit exactly fw pixels on this row
          for (let x = 0; x < fw && pixelIndex < framePixels.length; x++) {
            const b = framePixels[pixelIndex++] & 0xff;
            if (b !== transparentIndex) {
              out32[dst32] = pal32[b] >>> 0;
            }
            dst32++;
          }
        }
      }

      // Release module-level array if we were using it
      if (framePixels.buffer === moduleReusableFramePixels.buffer) {
        moduleFramePixelsInUse = false;
      }
    }

    // Done
  }

  /* Wasm-accelerated row-wise decode for non-interlaced, non-transparent frames */
  private lzwDecodeToPixelsWasm(
    bytes: Uint8Array,
    minCodeSize: number,
    out32: Uint32Array,
    canvasWidth: number,
    fw: number,
    fh: number,
    fx: number,
    fy: number,
    pal32: Uint32Array
  ) {
    if (!this.colorMapWasm || !this.rowIndicesBuffer) return;

    // Copy palette to Wasm memory once per frame
    this.colorMapWasm.heapU32.set(
      pal32.subarray(0, 256),
      this.colorMapWasm.palPtr >>> 2
    );

    let q = 0;
    const CLEAR = 1 << minCodeSize;
    const EOI = CLEAR + 1;
    let nextCode = EOI + 1;
    let codeSize = (minCodeSize + 1) | 0;
    let codeMask = (1 << codeSize) - 1;

    // Initialize firstByte table for base codes
    for (let i = 0; i < CLEAR; i++) this.firstByte[i] = i;

    let bits = 0;
    let bitCount = 0;
    const table = this.decTable;
    const stack = this.stack;
    let sp = 0;
    let prevCode: number | null = null;

    // Row processing state
    let xleft = fw;
    const rowStride32 = (canvasWidth - fw) >>> 0;
    let dst32 = (fy * canvasWidth + fx) >>> 0;
    let rowCount = 0;
    const idxRow = this.rowIndicesBuffer.subarray(0, fw);

    while (true) {
      // Fill bit buffer
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
        codeSize = (minCodeSize + 1) | 0;
        codeMask = (1 << codeSize) - 1;
        prevCode = null;
        for (let i = 0; i < CLEAR; i++) this.firstByte[i] = i;
        continue;
      } else if (code === EOI) {
        break;
      }

      // Decode sequence for 'code'
      let outFirst: number;
      let cur = code;

      if (cur < CLEAR) {
        // Single byte
        outFirst = cur;
        const b = outFirst & 0xff;

        // Stage into row buffer instead of direct write
        idxRow[rowCount++] = b;
        dst32++; // Still advance logical cursor

        if (--xleft === 0) {
          // End of row - flush to Wasm and copy result
          this.flushRowToWasm(idxRow, rowCount, out32, dst32 - fw, fw);
          dst32 += rowStride32;
          xleft = fw;
          rowCount = 0;
        }
      } else {
        // Chase with stack
        sp = 0;
        if (cur >= nextCode) {
          if (prevCode === null) break;
          outFirst = this.firstByte[prevCode] | 0;
          stack[sp++] = outFirst;
          cur = prevCode;
        } else {
          outFirst = this.firstByte[cur] | 0;
        }

        // Unwind sequence
        while (cur >= CLEAR) {
          const entry = table[cur] | 0;
          stack[sp++] = entry & 0xff;
          cur = entry >>> 8;
        }

        // Write first base
        const base = cur & 0xff;
        idxRow[rowCount++] = base;
        dst32++;
        if (--xleft === 0) {
          this.flushRowToWasm(idxRow, rowCount, out32, dst32 - fw, fw);
          dst32 += rowStride32;
          xleft = fw;
          rowCount = 0;
        }

        // Write stack backwards
        while (sp) {
          const b = stack[--sp] & 0xff;
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

      // Add new table entry
      if (prevCode !== null && nextCode < GIF.MAX_CODE) {
        table[nextCode] = ((prevCode & 0xfff) << 8) | (outFirst & 0xff);
        this.firstByte[nextCode] = this.firstByte[prevCode];
        nextCode++;
        if (nextCode >= codeMask + 1 && codeSize < 12) {
          codeSize++;
          codeMask = (codeMask << 1) | 1;
        }
      }

      prevCode = code;
    }

    // Flush remaining partial row
    if (rowCount > 0) {
      this.flushRowToWasm(idxRow, rowCount, out32, dst32 - rowCount, rowCount);
    }
  }

  /* Helper to flush a row of indices through Wasm color mapping */
  private flushRowToWasm(
    idxRow: Uint8Array,
    count: number,
    out32: Uint32Array,
    startDst32: number,
    maxCount: number
  ) {
    if (!this.colorMapWasm) return;

    // Copy indices to Wasm memory
    this.colorMapWasm.heapU8.set(
      idxRow.subarray(0, count),
      this.colorMapWasm.idxPtr
    );

    // Call Wasm to map indices to colors
    this.colorMapWasm.map32(
      this.colorMapWasm.idxPtr,
      this.colorMapWasm.outPtr,
      this.colorMapWasm.palPtr,
      count,
      1
    );

    // Copy result back to output buffer
    const wasmOut32 = this.colorMapWasm.heapU32.subarray(
      this.colorMapWasm.outPtr >>> 2,
      (this.colorMapWasm.outPtr >>> 2) + count
    );

    out32.set(wasmOut32, startDst32);
  }
}
