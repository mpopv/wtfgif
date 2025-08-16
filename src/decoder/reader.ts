import {
  FrameInfo,
  PooledDecoderTables,
  UnifiedGPUGifRenderer,
} from "../types";
import { GIF } from "../constants/gif";
import { buildPal32 } from "../utils/palette";
import { concatSubBlocks } from "../utils/subblocks";
import {
  createDecoderTables,
  getPooledDecoderTables,
  returnDecoderTablesToPool,
} from "./pool";
import { hashGifData } from "../utils/hash";
import { loadGPUModule as loadGpuModule, createGpuRenderer } from "../gpu/renderer";
import {
  initializeGlobalWasm,
  getWasmFeatures,
  getWasmDecoder,
  getWasmWorkerPool,
  getWasmInitPromise,
  setWasmInitPromise,
  isWasmReady as isGlobalWasmReady,
} from "../wasm/runtime";

const moduleReusableFramePixels = new Uint8Array(2048 * 2048);
let moduleFramePixelsInUse = false;

const WASM_FEATURES = getWasmFeatures();

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

  // Pooled decoder tables for reuse across instances
  private pooledTables: PooledDecoderTables;
  private gifHash: string;

  // Aliases for easier access
  private decTable: Int32Array;
  private stack: Uint8Array;
  private firstByte: Int16Array;
  private out32Cache: WeakMap<Uint8Array, Uint32Array>;

  // Zero-copy canvas support
  private zeroCopyBuffers: Map<
    string,
    {
      wasmPtr: number;
      rgbaU8: Uint8ClampedArray;
      imageData: ImageData;
    }
  > = new Map();

  private colorMapWasm: any = null; // ColorMapWasm type
  private wasmEnabled = false;
  private rowIndicesBuffer: Uint8Array | null = null;

  private gpuRenderer: UnifiedGPUGifRenderer | null = null;
  private gpuEnabled = false;

  private workerPool: any = null; // WorkerPoolManager
  private workerPoolEnabled = false;

  private memoryHygiene: any = null; // MemoryHygiene

  static createPooled(buf: Uint8Array): GifReader {
    return new GifReader(buf, true);
  }

  static createUnpooled(buf: Uint8Array): GifReader {
    return new GifReader(buf, false);
  }

  constructor(private buf: Uint8Array, usePooling: boolean = true) {
    // Get or create pooled decoder tables
    this.gifHash = usePooling ? hashGifData(buf) : "";
    this.pooledTables = usePooling
      ? getPooledDecoderTables(this.gifHash)
      : createDecoderTables();

    // Set up aliases for easier access
    this.decTable = this.pooledTables.decTable;
    this.stack = this.pooledTables.stack;
    this.firstByte = this.pooledTables.firstByte;
    this.out32Cache = this.pooledTables.out32Cache;
    let p = 0;
    // Header: GIF87a / GIF89a
    if (
      buf[p++] !== GIF.G ||
      buf[p++] !== GIF.I ||
      buf[p++] !== GIF.F ||
      buf[p++] !== GIF._8 ||
      ((buf[p++] + 1) & 0xfd) !== GIF._8 ||
      buf[p++] !== GIF.A
    ) {
      throw new Error("Invalid GIF 87a/89a header.");
    }

    const width = (buf[p++] | (buf[p++] << 8)) >>> 0;
    const height = (buf[p++] | (buf[p++] << 8)) >>> 0;
    this.width_ = width;
    this.height_ = height;

    const pf0 = buf[p++]; // packed fields
    const gctFlag = (pf0 >>> 7) & 1;
    const gctSizeBits = pf0 & 0x7;
    const gctColors = 1 << (gctSizeBits + 1);
    const background = buf[p++]; // unused here
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
              // Possibly NETSCAPE2.0
              if (
                buf[p] === GIF.NETSCAPE_LEN &&
                buf[p + 1] === 0x4e &&
                buf[p + 2] === 0x45 &&
                buf[p + 3] === 0x54 &&
                buf[p + 4] === 0x53 &&
                buf[p + 5] === 0x43 &&
                buf[p + 6] === 0x41 &&
                buf[p + 7] === 0x50 &&
                buf[p + 8] === 0x45 &&
                buf[p + 9] === 0x32 &&
                buf[p + 10] === 0x2e &&
                buf[p + 11] === 0x30 &&
                buf[p + 12] === 0x03 &&
                buf[p + 13] === 0x01 &&
                buf[p + 16] === 0x00
              ) {
                p += 14;
                this.loop_count = (buf[p++] | (buf[p++] << 8)) >>> 0;
                p++; // terminator
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

          // NEW: flatten payload & capture min code size
          const { bytes: codes, mcs } = concatSubBlocks(buf, data_offset);

          // NEW: prebuild pal32 variants once per frame with transparent index optimization
          const pal32rgba = buildPal32(
            buf,
            palette_offset ?? 0,
            palette_size ?? 0,
            "rgba",
            transparent_index
          );
          const pal32bgra = buildPal32(
            buf,
            palette_offset ?? 0,
            palette_size ?? 0,
            "bgra",
            transparent_index
          );

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
            // NEW:
            min_code_size: mcs,
            codes,
            pal32rgba,
            pal32bgra,
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
      if (!WASM_FEATURES.supported) {
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
      if (!WASM_FEATURES.threads || !workerPool) {
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
      return workerPool.decodeParallel(this.buf, frameIndices);
  }

  getWasmStats(): {
    supported: boolean;
    simd: boolean;
    threads: boolean;
    heapUsage?: number;
  } {
    return {
      ...WASM_FEATURES,
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

    let buffer = this.zeroCopyBuffers.get(bufferKey);

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
      this.zeroCopyBuffers.set(bufferKey, buffer);
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
      frame.pal32rgba || new Uint32Array(256),
      frame.width,
      frame.height,
      offscreen as any // OffscreenCanvas compatible with HTMLCanvasElement interface
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
      frame.pal32rgba || new Uint32Array(256),
      frame.width,
      frame.height,
      offscreen as any
    );

    if (!success) return null;

    // Transfer ownership to ImageBitmap (can be posted to main thread)
    return offscreen.transferToImageBitmap();
  }

    cleanupZeroCopyBuffers(): void {
      const decoder = getWasmDecoder();
      if (decoder) {
        for (const buffer of this.zeroCopyBuffers.values()) {
          decoder.wasm_free(buffer.wasmPtr);
        }
      }
      this.zeroCopyBuffers.clear();
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
      return decodeResults.map((result: any) => {
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
  getWorkerPoolStats(): any {
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
      const frame = this.frameInfo(frameIndex);
      const frameSize = frame.width * frame.height;
      const indexData = new Uint8Array(frameSize);

      if (frame.interlaced) {
        // For interlaced frames, decode to temp buffer then deinterlace
        const tempIndices = new Uint8Array(frameSize);
        this.lzwDecodeToIndices(frame, tempIndices);

        // Deinterlace using same pass logic as RGBA decoder
        // Pass 0: rows 0,8,16... Pass 1: rows 4,12,20... Pass 2: rows 2,6,10,14... Pass 3: rows 1,3,5,7,9...
        let pixelIndex = 0;
        for (let pass = 0; pass < 4; pass++) {
          let yStart = 0,
            yStride = 8;
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
            const row = yStart + yInPass * yStride;
            if (row >= frame.height) break;

            const dst = row * frame.width;
            for (
              let x = 0;
              x < frame.width && pixelIndex < tempIndices.length;
              x++
            ) {
              indexData[dst + x] = tempIndices[pixelIndex++];
            }
          }
        }
      } else {
        // Non-interlaced: decode directly
        this.lzwDecodeToIndices(frame, indexData);
      }

      return indexData;
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
    const bytes = frame.codes;
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
  async benchmarkGPU(width = 512, height = 512): Promise<any> {
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
  private loadGPUModule(): any {
    try {
      return loadGpuModule();
    } catch (error) {
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

  /* Return decoder tables to pool for reuse (call when done with this GifReader) */
  dispose(): void {
    if (this.pooledTables && this.gifHash) {
      returnDecoderTablesToPool(this.pooledTables);
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
  enableWasmColorMapping(colorMapWasm: any): void {
    this.colorMapWasm = colorMapWasm;
    this.wasmEnabled = true;

    // Pre-allocate row buffer for indices (reused across frames)
    const maxRowWidth = Math.min(this.width_, colorMapWasm.maxRowWidth || 4096);
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
    const frame = this.frameInfo(frameNum);
    const numPixels = frame.width * frame.height;

    // NEW: use prebuilt palettes directly - zero lookup, zero reallocation
    const pal32 = order === "rgba" ? frame.pal32rgba! : frame.pal32bgra!;

    let trans = frame.transparent_index;
    if (trans === null) trans = 256; // sentinel; indexes are 0..255

    // Reuse a cached Uint32 view for this pixels buffer
    let out32 = this.out32Cache.get(pixels);
    if (!out32) {
      out32 = new Uint32Array(
        pixels.buffer,
        pixels.byteOffset,
        pixels.byteLength >>> 2
      );
      this.out32Cache.set(pixels, out32);
    }

    // Streaming decode directly to out32
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
  private lzwDecodeToPixels(
    codeStream: Uint8Array,
    dataOffset: number,
    out32: Uint32Array,
    canvasWidth: number,
    frame: FrameInfo,
    pal32: Uint32Array,
    transparentIndex: number
  ) {
    // NEW: use flattened bytes
    const bytes = frame.codes;
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
            // Single byte - transparency pre-baked in palette
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
            // Write first base - transparency pre-baked in palette
            const base = cur & 0xff;
            out32[dst32] = pal32[base] >>> 0;
            dst32++;
            if (--xleft === 0) {
              dst32 += rowStride32;
              xleft = fw;
            }
            // Write stack backwards - transparency pre-baked in palette
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
      // INTERLACED PATH: Use pass-loops with inline pixel positioning
      // First decode all pixels into a temporary buffer (reuse module-level array)
      const frameSize = fw * fh;
      let framePixels: Uint8Array;

      if (
        !moduleFramePixelsInUse &&
        frameSize <= moduleReusableFramePixels.length
      ) {
        moduleFramePixelsInUse = true;
        framePixels = moduleReusableFramePixels.subarray(0, frameSize);
      } else {
        // Fallback to allocation if reusable array is in use or too small
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
            // Always write: transparency is pre-baked into palette (0-alpha)
            // No branching needed - palette[transparentIndex] already has alpha=0
            out32[dst32] = pal32[b] >>> 0;
            dst32++;
          }
        }
      }

      // Release module-level array if we were using it
      if (framePixels === moduleReusableFramePixels.subarray(0, frameSize)) {
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

    // Flush any remaining partial row
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
      count
    );

    // Copy result back to output buffer
    const wasmOut32 = this.colorMapWasm.heapU32.subarray(
      this.colorMapWasm.outPtr >>> 2,
      (this.colorMapWasm.outPtr >>> 2) + count
    );

    out32.set(wasmOut32, startDst32);
  }
}

