// (c) Matt Popovich, 2025.
//
// https://github.com/mpopv/wtfgif
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to
// deal in the Software without restriction, including without limitation the
// rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
// sell copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
// IN THE SOFTWARE.
//
// wtfgif is a TypeScript implementation of a GIF 89a encoder and decoder,
// including animation and compression. It is a drop-in, optimized replacement
// for omggif with fused decode→blit, 32-bit palettes, interlace pass scheduling,
// and a typed-array hash encoder. It runs in browsers and Node.

"use strict";

/* ===== WebAssembly Integration ===== */
// Import Wasm decoder types and factory functions
let createWasmGifDecoder: any;
let createWasmWorkerPool: any;
let isWasmSupported: any;
let isWasmSIMDSupported: any;
let isWasmThreadsSupported: any;
// TypeScript type definitions for WebAssembly integration
interface UnifiedGPUGifRenderer {
  initialize: (canvas?: HTMLCanvasElement) => Promise<boolean>;
  renderFrame: (indexData: Uint8Array, palette: Uint32Array, width: number, height: number) => Promise<HTMLCanvasElement | null>;
  renderToCanvas: (indexData: Uint8Array, palette: Uint32Array, width: number, height: number, targetCanvas: HTMLCanvasElement) => Promise<boolean>;
  updatePalette: (palette: Uint32Array) => void;
  getBackend: () => string;
  isGPUAccelerated: () => boolean;
  benchmark: (width?: number, height?: number) => Promise<any>;
  dispose: () => void;
}

// TypeScript type definitions for WebAssembly integration
interface WasmGifDecoder {
  memory: WebAssembly.Memory;
  decode_rgba: (gifPtr: number, gifLen: number, frameIndex: number, outPtr: number, outLen: number) => number;
  decode_rgba_threaded: (gifPtr: number, gifLen: number, frameIndex: number, outPtr: number, outLen: number, numThreads: number) => number;
  init_heap: () => void;
  reset_heap: () => void;
  get_heap_usage: () => number;
  test_simd: () => number;
  wasm_malloc: (size: number) => number;
  wasm_free: (ptr: number) => void;
  heapU8: Uint8Array;
  heapU32: Uint32Array;
}

interface WasmWorkerPool {
  decode: (gifData: Uint8Array, frameIndex: number) => Promise<{ pixels: Uint32Array; delay: number }>;
  decodeParallel: (gifData: Uint8Array, frameIndices: number[]) => Promise<{ pixels: Uint32Array; delay: number }[]>;
  terminate: () => void;
  getStats: () => { activeWorkers: number; completedJobs: number; avgDecodeTime: number };
}

// Lazy-load WebAssembly module functions
const loadWasmModule = () => {
  try {
    return require('../wasm-full/wasmDecoder');
  } catch (error) {
    return null;
  }
};

// Initialize WebAssembly functions with lazy loading
createWasmGifDecoder = async (...args: any[]) => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.createWasmGifDecoder(...args) : null;
};

createWasmWorkerPool = async (...args: any[]) => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.createWasmWorkerPool(...args) : null;
};

isWasmSupported = () => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.isWasmSupported() : false;
};

isWasmSIMDSupported = () => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.isWasmSIMDSupported() : false;
};

isWasmThreadsSupported = () => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.isWasmThreadsSupported() : false;
};

// Global Wasm instances (lazy-loaded)
let globalWasmDecoder: WasmGifDecoder | null = null;
let globalWasmWorkerPool: WasmWorkerPool | null = null;
let wasmInitPromise: Promise<void> | null = null;

// Wasm feature flags
const WASM_FEATURES = {
  supported: isWasmSupported(),
  simd: isWasmSIMDSupported(),
  threads: isWasmThreadsSupported(),
};

/**
 * Initialize global WebAssembly decoder and worker pool
 */
const initializeGlobalWasm = async (wasmPath?: string): Promise<void> => {
  try {
    // Initialize main decoder
    globalWasmDecoder = await createWasmGifDecoder(wasmPath);
    
    // Initialize worker pool if threading is supported
    if (WASM_FEATURES.threads) {
      globalWasmWorkerPool = await createWasmWorkerPool(wasmPath);
    }
    
    console.log('WebAssembly GIF decoder initialized:', {
      decoder: !!globalWasmDecoder,
      workerPool: !!globalWasmWorkerPool,
      features: WASM_FEATURES,
    });
    
  } catch (error) {
    console.warn('Failed to initialize WebAssembly decoder:', error);
    globalWasmDecoder = null;
    globalWasmWorkerPool = null;
  }
};

/* ===== GIF constants ===== */
const enum GIF {
  // Header
  G = 0x47,
  I = 0x49,
  F = 0x46,
  _8 = 0x38,
  _9 = 0x39,
  A = 0x61,

  // Blocks
  EXT = 0x21,
  IMG = 0x2c,
  TRAILER = 0x3b,

  // Extension labels
  GCE = 0xf9,
  APPLICATION = 0xff,
  PLAINTEXT = 0x01,
  COMMENT = 0xfe,

  // NETSCAPE2.0
  NETSCAPE_LEN = 0x0b,

  // Limits
  MAX_CODE = 4096,
}

type PaletteRGB = number[]; // array of 24-bit 0xRRGGBB

/* ===== Module-level reusable arrays ===== */
// Reuse these across all instances to avoid allocations (since we never nest calls)
const moduleReusableFramePixels = new Uint8Array(2048 * 2048); // Reasonable max for most GIFs
let moduleFramePixelsInUse = false;

/* ===== GifReader Object Pooling ===== */
interface PooledDecoderTables {
  decTable: Int32Array;
  stack: Uint8Array; 
  firstByte: Int16Array;
  out32Cache: WeakMap<Uint8Array, Uint32Array>;
  hash: string; // Hash of GIF data for reuse validation
}

const decoderTablePool: PooledDecoderTables[] = [];
const MAX_POOL_SIZE = 16; // Reasonable limit for memory usage

function createDecoderTables(): PooledDecoderTables {
  return {
    decTable: new Int32Array(GIF.MAX_CODE),
    stack: new Uint8Array(GIF.MAX_CODE),
    firstByte: new Int16Array(GIF.MAX_CODE),
    out32Cache: new WeakMap<Uint8Array, Uint32Array>(),
    hash: ""
  };
}

function getPooledDecoderTables(gifHash: string): PooledDecoderTables {
  // Try to find an existing pooled instance for this GIF
  const pooledIndex = decoderTablePool.findIndex(p => p.hash === gifHash);
  if (pooledIndex >= 0) {
    const pooled = decoderTablePool.splice(pooledIndex, 1)[0];
    return pooled;
  }
  
  // Try to reuse any available pooled instance
  if (decoderTablePool.length > 0) {
    const pooled = decoderTablePool.pop()!;
    pooled.hash = gifHash;
    pooled.out32Cache = new WeakMap(); // Fresh cache for new GIF
    return pooled;
  }
  
  // Create new instance
  const tables = createDecoderTables();
  tables.hash = gifHash;
  return tables;
}

function returnDecoderTablesToPool(tables: PooledDecoderTables): void {
  if (decoderTablePool.length < MAX_POOL_SIZE) {
    decoderTablePool.push(tables);
  }
  // If pool is full, let it get GC'd
}

// Simple hash function for GIF data
function hashGifData(data: Uint8Array): string {
  let hash = 0;
  const step = Math.max(1, Math.floor(data.length / 1024)); // Sample every ~1KB
  for (let i = 0; i < data.length; i += step) {
    hash = ((hash << 5) - hash + data[i]) | 0;
  }
  return hash.toString(36);
}

/* ===== Helper / small utilities ===== */
function assertPow2(n: number): boolean {
  return n >= 2 && n <= 256 && (n & (n - 1)) === 0;
}
function log2Pow2(n: number): number {
  /* n is power-of-two (2..256) */ return 31 - Math.clz32(n);
}

function checkPalette(pal: PaletteRGB): number {
  const n = pal.length >>> 0;
  if (!assertPow2(n))
    throw new Error("Invalid palette size (must be power of 2, 2..256).");
  return n;
}

function concatSubBlocks(buf: Uint8Array, offset: number): { bytes: Uint8Array, mcs: number } {
  const mcs = buf[offset] | 0;         // min code size
  let q = (offset + 1) | 0;            // first sub-block size byte
  let total = 0;
  // first pass: measure
  while (true) {
    const len = buf[q++] | 0;
    if (len === 0) break;
    total += len;
    q += len;
  }
  // second pass: copy
  const out = new Uint8Array(total);
  q = (offset + 1) | 0;
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

/* Precompute 32-bit palette values for fast Uint32 writes.
   For RGBA bytes in memory under little-endian, the Uint32 value must be (A<<24)|(B<<16)|(G<<8)|R.
   For BGRA bytes in memory, it's (A<<24)|(R<<16)|(G<<8)|B.
*/
function buildPal32(
  buf: Uint8Array,
  paletteOffset: number,
  paletteSize: number,
  order: "rgba" | "bgra",
  transparentIndex: number | null = null
): Uint32Array {
  const pal32 = new Uint32Array(256); // up to 256 entries
  const limit = Math.min(paletteSize, 256);
  if (order === "rgba") {
    for (let i = 0; i < limit; i++) {
      const r = buf[paletteOffset + i * 3] | 0;
      const g = buf[paletteOffset + i * 3 + 1] | 0;
      const b = buf[paletteOffset + i * 3 + 2] | 0;
      // Pre-bake transparency into palette: 0-alpha for transparent index
      // This eliminates branches in inner decode loops
      const alpha = (transparentIndex !== null && i === transparentIndex) ? 0 : 255;
      pal32[i] = (alpha << 24) | (b << 16) | (g << 8) | r;
    }
  } else {
    // bgra
    for (let i = 0; i < limit; i++) {
      const r = buf[paletteOffset + i * 3] | 0;
      const g = buf[paletteOffset + i * 3 + 1] | 0;
      const b = buf[paletteOffset + i * 3 + 2] | 0;
      // Pre-bake transparency into palette: 0-alpha for transparent index
      // This eliminates branches in inner decode loops
      const alpha = (transparentIndex !== null && i === transparentIndex) ? 0 : 255;
      pal32[i] = (alpha << 24) | (r << 16) | (g << 8) | b;
    }
  }
  // Others remain 0; caller should ensure indices are valid.
  return pal32;
}

/* ====== Writer (Encoder) ====== */
export class GifWriter {
  private p = 0;
  private ended = false;

  private loopCount: number | null;
  private globalPalette: PaletteRGB | null;
  private background = 0;

  constructor(
    private buf: Uint8Array,
    private width: number,
    private height: number,
    gopts?: {
      loop?: number | null;
      palette?: PaletteRGB | null;
      background?: number;
    }
  ) {
    const go = gopts ?? {};
    this.loopCount = go.loop === undefined ? null : go.loop;
    this.globalPalette = go.palette === undefined ? null : go.palette;

    if (width <= 0 || height <= 0 || width > 65535 || height > 65535)
      throw new Error("Width/Height invalid.");

    /* Header: GIF89a */
    this.buf[this.p++] = GIF.G;
    this.buf[this.p++] = GIF.I;
    this.buf[this.p++] = GIF.F;
    this.buf[this.p++] = GIF._8;
    this.buf[this.p++] = GIF._9;
    this.buf[this.p++] = GIF.A;

    // Global Color Table handling
    let gpPow2Bits = 0; // packed-field size bits
    if (this.globalPalette !== null) {
      const n = checkPalette(this.globalPalette);
      const pow = log2Pow2(n); // 1..8
      gpPow2Bits = (pow - 1) & 7; // 0..7 per spec

      if (go.background !== undefined) {
        this.background = go.background | 0;
        if (this.background < 0 || this.background >= n)
          throw new Error("Background index out of range.");
        if (this.background === 0)
          throw new Error("Background index explicitly passed as 0.");
      }
    }

    /* Logical Screen Descriptor */
    this.buf[this.p++] = width & 0xff;
    this.buf[this.p++] = (width >> 8) & 0xff;
    this.buf[this.p++] = height & 0xff;
    this.buf[this.p++] = (height >> 8) & 0xff;

    const gctFlag = this.globalPalette !== null ? 0x80 : 0x00;
    this.buf[this.p++] = gctFlag | gpPow2Bits;
    this.buf[this.p++] = this.background & 0xff; // background color index
    this.buf[this.p++] = 0; // pixel aspect ratio

    // Global Color Table
    if (this.globalPalette !== null) {
      for (let i = 0; i < this.globalPalette.length; i++) {
        const rgb = this.globalPalette[i] >>> 0;
        this.buf[this.p++] = (rgb >> 16) & 0xff;
        this.buf[this.p++] = (rgb >> 8) & 0xff;
        this.buf[this.p++] = rgb & 0xff;
      }
    }

    // Netscape loop count
    if (this.loopCount !== null) {
      const lc = this.loopCount | 0;
      if (lc < 0 || lc > 65535) throw new Error("Loop count invalid.");
      this.buf[this.p++] = GIF.EXT;
      this.buf[this.p++] = GIF.APPLICATION;
      this.buf[this.p++] = GIF.NETSCAPE_LEN;
      // "NETSCAPE2.0"
      this.buf[this.p++] = 0x4e;
      this.buf[this.p++] = 0x45;
      this.buf[this.p++] = 0x54;
      this.buf[this.p++] = 0x53;
      this.buf[this.p++] = 0x43;
      this.buf[this.p++] = 0x41;
      this.buf[this.p++] = 0x50;
      this.buf[this.p++] = 0x45;
      this.buf[this.p++] = 0x32;
      this.buf[this.p++] = 0x2e;
      this.buf[this.p++] = 0x30;
      // Sub-block: 3 bytes
      this.buf[this.p++] = 0x03;
      this.buf[this.p++] = 0x01;
      this.buf[this.p++] = lc & 0xff;
      this.buf[this.p++] = (lc >> 8) & 0xff;
      this.buf[this.p++] = 0x00;
    }
  }

  addFrame(
    x: number,
    y: number,
    w: number,
    h: number,
    indexedPixels: Uint8Array | number[],
    opts?: {
      palette?: PaletteRGB | null;
      delay?: number;
      disposal?: number;
      transparent?: number | null;
    }
  ): number {
    if (this.ended) {
      this.p--;
      this.ended = false;
    } // un-end if user adds more frames

    const o = opts ?? {};
    x |= 0;
    y |= 0;
    w |= 0;
    h |= 0;

    if (x < 0 || y < 0 || x > 65535 || y > 65535)
      throw new Error("x/y invalid.");
    if (w <= 0 || h <= 0 || w > 65535 || h > 65535)
      throw new Error("Width/Height invalid.");
    if ((indexedPixels as any).length < w * h)
      throw new Error("Not enough pixels for the frame size.");

    let usingLocal = true;
    let palette: PaletteRGB | null | undefined = o.palette;
    if (palette == null) {
      usingLocal = false;
      palette = this.globalPalette;
    }
    if (palette == null)
      throw new Error("Must supply either a local or global palette.");

    const numColors = checkPalette(palette);
    const minCodeSize = log2Pow2(numColors); // 1..8
    const lctSizeBits = (minCodeSize - 1) & 7;

    const delay = (o.delay ?? 0) | 0;
    let disposal = (o.disposal ?? 0) | 0;
    if (disposal < 0 || disposal > 3) throw new Error("Disposal out of range.");

    let useTrans = false;
    let transparentIndex = 0;
    if (o.transparent !== undefined && o.transparent !== null) {
      useTrans = true;
      transparentIndex = (o.transparent as number) | 0;
      if (transparentIndex < 0 || transparentIndex >= numColors)
        throw new Error("Transparent color index out of range.");
    }

    // Graphics Control Extension (only when needed)
    if (disposal !== 0 || useTrans || delay !== 0) {
      this.buf[this.p++] = GIF.EXT;
      this.buf[this.p++] = GIF.GCE;
      this.buf[this.p++] = 4; // block size
      this.buf[this.p++] = (disposal << 2) | (useTrans ? 1 : 0);
      this.buf[this.p++] = delay & 0xff;
      this.buf[this.p++] = (delay >> 8) & 0xff;
      this.buf[this.p++] = transparentIndex & 0xff;
      this.buf[this.p++] = 0; // terminator
    }

    // Image Descriptor
    this.buf[this.p++] = GIF.IMG;
    this.buf[this.p++] = x & 0xff;
    this.buf[this.p++] = (x >> 8) & 0xff;
    this.buf[this.p++] = y & 0xff;
    this.buf[this.p++] = (y >> 8) & 0xff;
    this.buf[this.p++] = w & 0xff;
    this.buf[this.p++] = (w >> 8) & 0xff;
    this.buf[this.p++] = h & 0xff;
    this.buf[this.p++] = (h >> 8) & 0xff;
    // local color table flag + size (no sort, non-interlaced)
    this.buf[this.p++] = usingLocal ? 0x80 | lctSizeBits : 0x00;

    if (usingLocal) {
      for (let i = 0; i < palette.length; i++) {
        const rgb = palette[i] >>> 0;
        this.buf[this.p++] = (rgb >> 16) & 0xff;
        this.buf[this.p++] = (rgb >> 8) & 0xff;
        this.buf[this.p++] = rgb & 0xff;
      }
    }

    this.p = GifWriterOutputLZWCodeStream_fast(
      this.buf,
      this.p,
      minCodeSize < 2 ? 2 : minCodeSize,
      indexedPixels as Uint8Array | number[],
      numColors
    );

    return this.p;
  }

  end(): number {
    if (!this.ended) {
      this.buf[this.p++] = GIF.TRAILER;
      this.ended = true;
    }
    return this.p;
  }
  getOutputBuffer(): Uint8Array {
    return this.buf;
  }
  setOutputBuffer(v: Uint8Array) {
    this.buf = v;
  }
  getOutputBufferPosition(): number {
    return this.p;
  }
  setOutputBufferPosition(v: number) {
    this.p = v | 0;
  }
}

/* ===== Encoder internals: typed-array hash + tight bit packer ===== */

/** Emit LZW code stream for indexed pixels. */
function GifWriterOutputLZWCodeStream_fast(
  buf: Uint8Array,
  p0: number,
  minCodeSize: number,
  indexStream: Uint8Array | number[],
  colorCount: number
): number {
  let p = p0;

  // Write LZW min code size and set up first sub-block
  buf[p++] = minCodeSize & 0xff;
  let subLenPos = p++; // reserve length
  let subLen = 0;

  const CLEAR = 1 << minCodeSize;
  const EOI = CLEAR + 1;
  let nextCode = EOI + 1;
  let codeSize = minCodeSize + 1;
  let codeMask = (1 << codeSize) - 1;

  // Bit buffer
  let bits = 0 >>> 0;
  let bitCount = 0;

  // ---- typed-array open addressing hash ----
  // Key space is 20 bits: (prefix<<8)|k, values up to 12 bits.
  // Capacity: power-of-two >= 8192 for low probe counts, modest memory.
  const CAP = 8192;
  const keys = (GifWriterOutputLZWCodeStream_fast._keys ??= new Int32Array(
    CAP
  ));
  const vals = (GifWriterOutputLZWCodeStream_fast._vals ??= new Int16Array(
    CAP
  ));
  const gen = (GifWriterOutputLZWCodeStream_fast._gen ??= new Int32Array(CAP));
  let epoch = ((GifWriterOutputLZWCodeStream_fast._epoch ?? 0) + 1) | 0;
  GifWriterOutputLZWCodeStream_fast._epoch = epoch;
  if (epoch <= 0) {
    gen.fill(0);
    epoch = 1;
    GifWriterOutputLZWCodeStream_fast._epoch = epoch;
  }
  let EPOCH = epoch;

  function tableReset() {
    // Instead of clearing arrays, bump generation.
    EPOCH = (EPOCH + 1) | 0;
    if (EPOCH <= 0) {
      gen.fill(0);
      EPOCH = 1;
    }
  }

  function tableGet(key: number): number {
    // returns -1 if miss
    let i = key & (CAP - 1);
    // linear probing
    while (gen[i] === EPOCH) {
      if (keys[i] === key) return vals[i] | 0;
      i = (i + 1) & (CAP - 1);
    }
    return -1;
  }
  function tableSet(key: number, value: number) {
    let i = key & (CAP - 1);
    while (gen[i] === EPOCH) {
      if (keys[i] === key) {
        vals[i] = value;
        return;
      }
      i = (i + 1) & (CAP - 1);
    }
    gen[i] = EPOCH;
    keys[i] = key | 0;
    vals[i] = value | 0;
  }

  function emit(code: number) {
    bits |= (code & 0xffff) << bitCount;
    bitCount += codeSize;
    while (bitCount >= 8) {
      buf[p++] = bits & 0xff;
      bits >>>= 8;
      bitCount -= 8;
      // sub-block manage
      if (++subLen === 255) {
        buf[subLenPos] = 255;
        subLenPos = p++;
        subLen = 0;
      }
    }
  }

  // Ensure fresh dictionary per image
  tableReset();

  // Emit initial clear
  emit(CLEAR);

  const n = (indexStream as any).length | 0;
  const mask = (colorCount - 1) | 0; // (palette size is power-of-two)
  let ib = (indexStream[0] as number) & mask;

  for (let i = 1; i < n; i++) {
    const k = (indexStream[i] as number) & mask;
    const key = (ib << 8) | k;
    const found = tableGet(key);
    if (found >= 0 && found < nextCode) {
      ib = found;
      continue;
    }

    // emit buffer
    emit(ib);

    if (nextCode === GIF.MAX_CODE) {
      // Clear
      emit(CLEAR);
      nextCode = EOI + 1;
      codeSize = (minCodeSize + 1) | 0;
      codeMask = (1 << codeSize) - 1;
      tableReset();
    } else {
      if (nextCode >= codeMask + 1 && codeSize < 12) {
        codeSize++;
        codeMask = (1 << codeSize) - 1;
      }
      tableSet(key, nextCode++);
    }

    ib = k;
  }

  emit(ib);
  emit(EOI);

  // flush remaining bits
  if (bitCount > 0) {
    buf[p++] = bits & 0xff;
    if (++subLen === 255) {
      buf[subLenPos] = 255;
      subLenPos = p++;
      subLen = 0;
    }
    bits = 0;
    bitCount = 0;
  }

  // finalize sub-blocks
  buf[subLenPos] = subLen & 0xff;
  buf[p++] = 0; // terminator

  return p;
}
namespace GifWriterOutputLZWCodeStream_fast {
  export let _keys: Int32Array | undefined;
  export let _vals: Int16Array | undefined;
  export let _gen: Int32Array | undefined;
  export let _epoch: number | undefined;
}

/* ====== Reader (Decoder) ====== */
type FrameInfo = {
  x: number;
  y: number;
  width: number;
  height: number;
  has_local_palette: boolean;
  palette_offset: number;
  palette_size: number;
  data_offset: number;
  data_length: number;
  transparent_index: number | null;
  interlaced: boolean;
  delay: number;
  disposal: number;
  min_code_size: number;
  codes: Uint8Array;
  pal32rgba?: Uint32Array;
  pal32bgra?: Uint32Array;
};

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
  private zeroCopyBuffers: Map<string, {
    wasmPtr: number;
    rgbaU8: Uint8ClampedArray;
    imageData: ImageData;
  }> = new Map();
  
  // Optional Wasm color mapping helper (Tier 2 optimization)
  private colorMapWasm: any = null; // ColorMapWasm type
  private wasmEnabled = false;
  private rowIndicesBuffer: Uint8Array | null = null;
  
  // GPU palette expansion (Tier 4 optimization)
  private gpuRenderer: UnifiedGPUGifRenderer | null = null;
  private gpuEnabled = false;
  
  // Threaded worker pool support (when Wasm threads aren't available)
  private workerPool: any = null; // WorkerPoolManager
  private workerPoolEnabled = false;
  
  // Memory hygiene: TypedArray pools to eliminate allocations in hot loops
  private memoryHygiene: any = null; // MemoryHygiene

  /* Factory method for pooled GifReader instances */
  static createPooled(buf: Uint8Array): GifReader {
    return new GifReader(buf, true);
  }

  /* Factory method for non-pooled GifReader instances */
  static createUnpooled(buf: Uint8Array): GifReader {
    return new GifReader(buf, false);
  }

  constructor(private buf: Uint8Array, usePooling: boolean = true) {
    // Get or create pooled decoder tables
    this.gifHash = usePooling ? hashGifData(buf) : "";
    this.pooledTables = usePooling ? getPooledDecoderTables(this.gifHash) : createDecoderTables();
    
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
          const pal32rgba = buildPal32(buf, (palette_offset ?? 0), (palette_size ?? 0), "rgba", transparent_index);
          const pal32bgra = buildPal32(buf, (palette_offset ?? 0), (palette_size ?? 0), "bgra", transparent_index);

          this.frames.push({
            x,
            y,
            width: w,
            height: h,
            has_local_palette,
            palette_offset: palette_offset ?? 0,
            palette_size: palette_size ?? 0,
            data_offset,                 // keep for compatibility
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

  /* ===== WebAssembly Integration Methods ===== */
  
  /**
   * Initialize WebAssembly decoder for this GifReader instance
   */
  async initWasm(wasmPath?: string): Promise<boolean> {
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
  isWasmReady(): boolean {
    return globalWasmDecoder !== null;
  }
  
  /**
   * Decode frame using WebAssembly (with fallback to JavaScript)
   */
  async framePixelsWasm(frameIndex: number, pixels?: Uint32Array): Promise<Uint32Array> {
    // Try Wasm first if available
    if (this.isWasmReady() && globalWasmDecoder) {
      try {
        const result = await this.decodeFrameWasm(frameIndex, pixels);
        if (result) {
          return result;
        }
      } catch (error) {
        console.warn('Wasm decode failed, falling back to JavaScript:', error);
      }
    }
    
    // Fallback to JavaScript decoder
    const outputSize = this.width_ * this.height_;
    if (!pixels || pixels.length < outputSize) {
      pixels = new Uint32Array(outputSize);
    }
    
    // Use the existing JavaScript decoder
    const uint8Buffer = new Uint8Array(pixels.buffer, pixels.byteOffset, pixels.byteLength);
    this.decodeAndBlitFrameRGBA(frameIndex, uint8Buffer);
    
    return pixels;
  }
  
  /**
   * Internal Wasm frame decoder
   */
  private async decodeFrameWasm(frameIndex: number, pixels?: Uint32Array): Promise<Uint32Array | null> {
    if (!globalWasmDecoder) {
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
    const gifPtr = globalWasmDecoder.wasm_malloc(this.buf.length);
    if (gifPtr === 0) {
      throw new Error('Failed to allocate Wasm memory for GIF data');
    }
    
    const outPtr = globalWasmDecoder.wasm_malloc(outputSize * 4);
    if (outPtr === 0) {
      globalWasmDecoder.wasm_free(gifPtr);
      throw new Error('Failed to allocate Wasm memory for output');
    }
    
    try {
      // Copy GIF data to Wasm heap
      globalWasmDecoder.heapU8.set(this.buf, gifPtr);
      
      // Call Wasm decoder
      const result = globalWasmDecoder.decode_rgba(
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
  async framePixelsParallel(frameIndices: number[]): Promise<{ pixels: Uint32Array; delay: number }[]> {
    if (!WASM_FEATURES.threads || !globalWasmWorkerPool) {
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
    return globalWasmWorkerPool.decodeParallel(this.buf, frameIndices);
  }
  
  /**
   * Get WebAssembly performance statistics
   */
  getWasmStats(): { supported: boolean; simd: boolean; threads: boolean; heapUsage?: number } {
    return {
      ...WASM_FEATURES,
      heapUsage: globalWasmDecoder?.get_heap_usage(),
    };
  }

  /* ===== GPU Palette Expansion Methods (Tier 4) ===== */
  
  /**
   * Initialize GPU palette expansion for ultra-fast rendering
   */
  async initGPU(canvas?: HTMLCanvasElement): Promise<boolean> {
    try {
      // Lazy-load GPU renderer
      if (!this.gpuRenderer) {
        const gpuModule = this.loadGPUModule();
        if (!gpuModule) {
          return false;
        }
        
        this.gpuRenderer = new gpuModule.UnifiedGPUGifRenderer();
      }
      
      const success = await this.gpuRenderer!.initialize(canvas);
      this.gpuEnabled = success;
      return success;
      
    } catch (error) {
      console.warn('GPU palette expansion failed to initialize:', error);
      this.gpuEnabled = false;
      return false;
    }
  }
  
  /**
   * Decode frame using GPU acceleration (fastest possible path)
   */
  async framePixelsGPU(frameIndex: number, targetCanvas?: HTMLCanvasElement): Promise<HTMLCanvasElement | null> {
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
  
  /**
   * Zero-copy canvas presentation using WebAssembly persistent buffer
   * Allocates buffer once, reuses for all frames of same size
   */
  frameImageDataZeroCopy(frameIndex: number, ctx2d: CanvasRenderingContext2D): void {
    if (!this.isWasmReady() || !globalWasmDecoder) {
      throw new Error("WebAssembly not available for zero-copy presentation");
    }

    const frame = this.frameInfo(frameIndex);
    const w = frame.width;
    const h = frame.height;
    const bufferKey = `${w}x${h}`;
    
    let buffer = this.zeroCopyBuffers.get(bufferKey);
    
    if (!buffer) {
      // Allocate persistent WebAssembly buffer
      const outPtr = globalWasmDecoder.wasm_malloc(w * h * 4);
      const rgbaU8 = new Uint8ClampedArray(globalWasmDecoder.memory.buffer, outPtr, w * h * 4);
      const imageData = new ImageData(rgbaU8, w, h); // shares the same buffer
      
      buffer = { wasmPtr: outPtr, rgbaU8, imageData };
      this.zeroCopyBuffers.set(bufferKey, buffer);
    }
    
    // Decode frame directly into persistent buffer
    globalWasmDecoder.decode_rgba(
      this.buf.byteOffset || 0, 
      this.buf.length, 
      frameIndex, 
      buffer.wasmPtr, 
      w * h
    );
    
    // Zero-copy presentation - no .set(), no GC
    ctx2d.putImageData(buffer.imageData, 0, 0);
  }

  /**
   * GPU zero-copy with OffscreenCanvas and ImageBitmap transfer
   */
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

  /**
   * Worker-compatible GPU decode with transferToImageBitmap
   * Use this pattern in a worker for maximum performance
   */
  async frameTransferBitmapGPU(frameIndex: number): Promise<ImageBitmap | null> {
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

  /**
   * Cleanup zero-copy buffers when done
   */
  cleanupZeroCopyBuffers(): void {
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
  async initWorkerPool(): Promise<boolean> {
    // Threaded worker pool support removed in cleanup build
    this.workerPoolEnabled = false;
    return false;
  }

  /**
   * Parallel frame decode using threaded worker pool
   * Each worker has its own Wasm instance and LZW tables
   */
  async framePixelsThreadedPool(frameIndices: number[]): Promise<{ pixels: Uint32Array; delay: number }[]> {
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
      const decodeResults = await this.workerPool.decodeFrames(this.buf, frameIndices);
      
      // Convert worker results to expected format
      return decodeResults.map((result: any) => ({
        pixels: result.pixels,
        delay: result.delay
      }));
      
    } catch (error) {
      console.warn('Worker pool decode failed, falling back to sequential:', error);
      
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
        return result.pixels;
      } catch (error) {
        console.warn('Worker pool single frame decode failed:', error);
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
  private async decodeFrameIndices(frameIndex: number): Promise<Uint8Array | null> {
    try {
      const frame = this.frameInfo(frameIndex);
      const frameSize = frame.width * frame.height;
      const indexData = new Uint8Array(frameSize);
      
      // Use simplified LZW decoder that outputs indices directly
      this.lzwDecodeToIndices(frame, indexData);
      
      return indexData;
      
    } catch (error) {
      console.error('Failed to decode frame indices:', error);
      return null;
    }
  }
  
  /**
   * Simplified LZW decoder that outputs palette indices instead of RGBA
   */
  private lzwDecodeToIndices(frame: FrameInfo, outputIndices: Uint8Array): void {
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
          outputIndices[pixelIndex++] = outFirst & 0xFF;
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
          stack[sp++] = entry & 0xFF;
          cur = entry >>> 8;
        }
        
        // Output base symbol
        const base = cur & 0xFF;
        if (pixelIndex < outputIndices.length) {
          outputIndices[pixelIndex++] = base;
        }
        
        // Output stack in reverse
        while (sp && pixelIndex < outputIndices.length) {
          outputIndices[pixelIndex++] = stack[--sp] & 0xFF;
        }
      }
      
      // Add new table entry
      if (prevCode !== null && nextCode < GIF.MAX_CODE) {
        table[nextCode] = ((prevCode & 0xFFF) << 8) | (outFirst & 0xFF);
        this.firstByte[nextCode] = this.firstByte[prevCode];
        nextCode++;
        
        if (nextCode >= codeMask + 1 && codeSize < 12) {
          codeSize++;
          codeMask = (codeMask << 1) | 1;
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
    return this.gpuRenderer?.getBackend() || 'none';
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
      // In a real implementation, this would be a dynamic import
      // For now, return null to indicate GPU module not available
      return null;
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
  decodeFrameIntoBuffer(frameNum: number, buffer: ArrayBuffer, format: "rgba" | "bgra" = "rgba"): void {
    const expectedSize = this.width_ * this.height_ * 4;
    if (buffer.byteLength < expectedSize) {
      throw new Error(`Buffer too small: need ${expectedSize} bytes, got ${buffer.byteLength}`);
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
  static getPoolStats(): { available: number; totalCreated: number; hits: number; misses: number } {
    // Simple stats tracking - in real implementation you'd track hits/misses
    return {
      available: decoderTablePool.length,
      totalCreated: decoderTablePool.length + 1, // Approximate
      hits: 0, // Would need to track in getPooledDecoderTables
      misses: 0 // Would need to track in getPooledDecoderTables
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
    const pal32 = (order === "rgba" ? frame.pal32rgba! : frame.pal32bgra!);
    
    let trans = frame.transparent_index;
    if (trans === null) trans = 256; // sentinel; indexes are 0..255

    // Reuse a cached Uint32 view for this pixels buffer
    let out32 = this.out32Cache.get(pixels);
    if (!out32) {
      out32 = new Uint32Array(pixels.buffer, pixels.byteOffset, pixels.byteLength >>> 2);
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
    let q = 0;                 // cursor into contiguous bytes

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
    const hasTrans = (transparentIndex !== 256); // 256 is sentinel

    if (!frame.interlaced) {
      // Fast path for non-interlaced frames
      let xleft = fw;
      const rowStride32 = (canvasWidth - fw) >>> 0;
      let dst32 = ((fy * canvasWidth) + fx) >>> 0;

      if (!hasTrans) {
        // FAST PATH: No transparency - use Wasm if available and suitable
        if (this.wasmEnabled && this.colorMapWasm && fw <= (this.colorMapWasm.maxRowWidth || 4096)) {
          this.lzwDecodeToPixelsWasm(bytes, minCodeSize, out32, canvasWidth, fw, fh, fx, fy, pal32);
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
            if (--xleft === 0) { dst32 += rowStride32; xleft = fw; }
          } else {
            // Chase with stack
            sp = 0;
            if (cur >= nextCode) {
              // KwKwK case
              if (prevCode === null) break;
              outFirst = this.firstByte[prevCode] | 0;  // O(1) instead of chasing
              stack[sp++] = outFirst;
              cur = prevCode;
            } else {
              outFirst = this.firstByte[cur] | 0;  // O(1) instead of chasing
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
            if (--xleft === 0) { dst32 += rowStride32; xleft = fw; }
            // Write stack backwards - always write (no transparency check)
            while (sp) {
              const b = stack[--sp] & 0xff;
              out32[dst32] = pal32[b] >>> 0;
              dst32++;
              if (--xleft === 0) { dst32 += rowStride32; xleft = fw; }
            }
          }

          // Add new table entry
          if (prevCode !== null && nextCode < GIF.MAX_CODE) {
            table[nextCode] = ((prevCode & 0xfff) << 8) | (outFirst & 0xff);
            this.firstByte[nextCode] = this.firstByte[prevCode];  // O(1) instead of chasing
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
            if (--xleft === 0) { dst32 += rowStride32; xleft = fw; }
          } else {
            // Chase with stack
            sp = 0;
            if (cur >= nextCode) {
              // KwKwK case
              if (prevCode === null) break;
              outFirst = this.firstByte[prevCode] | 0;  // O(1) instead of chasing
              stack[sp++] = outFirst;
              cur = prevCode;
            } else {
              outFirst = this.firstByte[cur] | 0;  // O(1) instead of chasing
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
            if (--xleft === 0) { dst32 += rowStride32; xleft = fw; }
            // Write stack backwards - transparency pre-baked in palette
            while (sp) {
              const b = stack[--sp] & 0xff;
              out32[dst32] = pal32[b] >>> 0;
              dst32++;
              if (--xleft === 0) { dst32 += rowStride32; xleft = fw; }
            }
          }

          // Add new table entry
          if (prevCode !== null && nextCode < GIF.MAX_CODE) {
            table[nextCode] = ((prevCode & 0xfff) << 8) | (outFirst & 0xff);
            this.firstByte[nextCode] = this.firstByte[prevCode];  // O(1) instead of chasing
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
      
      if (!moduleFramePixelsInUse && frameSize <= moduleReusableFramePixels.length) {
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
            outFirst = this.firstByte[prevCode] | 0;  // O(1) instead of chasing
            stack[sp++] = outFirst;
            cur = prevCode;
          } else {
            outFirst = this.firstByte[cur] | 0;  // O(1) instead of chasing
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
          this.firstByte[nextCode] = this.firstByte[prevCode];  // O(1) instead of chasing
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
        if (pass === 1) { yStart = 4; yStride = 8; }
        else if (pass === 2) { yStart = 2; yStride = 4; }
        else if (pass === 3) { yStart = 1; yStride = 2; }

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
    this.colorMapWasm.heapU32.set(pal32.subarray(0, 256), this.colorMapWasm.palPtr >>> 2);
    
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
    let dst32 = ((fy * canvasWidth) + fx) >>> 0;
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
    this.colorMapWasm.heapU8.set(idxRow.subarray(0, count), this.colorMapWasm.idxPtr);
    
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

/* ===== Module-level WebAssembly Exports ===== */

/**
 * Initialize WebAssembly globally (can be called before creating any GifReader instances)
 */
export const initializeWasmGlobally = initializeGlobalWasm;

/**
 * Get global WebAssembly feature support and status
 */
export const getWasmStatus = (): {
  supported: boolean;
  simd: boolean;
  threads: boolean;
  initialized: boolean;
  workerPoolAvailable: boolean;
} => ({
  ...WASM_FEATURES,
  initialized: globalWasmDecoder !== null,
  workerPoolAvailable: globalWasmWorkerPool !== null,
});

/**
 * Cleanup global WebAssembly resources
 */
export const cleanupWasm = (): void => {
  if (globalWasmWorkerPool) {
    globalWasmWorkerPool.terminate();
    globalWasmWorkerPool = null;
  }
  
  globalWasmDecoder = null;
  wasmInitPromise = null;
};

// Browser global export under wtfgif namespace
(function() {
  const browserExports = { 
    GifWriter, 
    GifReader, 
    initializeWasmGlobally, 
    getWasmStatus, 
    cleanupWasm 
  };
  
  if (typeof window !== 'undefined') {
    (window as any).wtfgif = browserExports;
  } else if (typeof globalThis !== 'undefined') {
    (globalThis as any).wtfgif = browserExports;
  }
})();
