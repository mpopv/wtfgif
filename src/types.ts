export interface UnifiedGPUGifRenderer {
  initialize: (canvas?: HTMLCanvasElement) => Promise<boolean>;
  renderFrame: (
    indexData: Uint8Array,
    palette: Uint32Array,
    width: number,
    height: number
  ) => Promise<HTMLCanvasElement | null>;
  renderToCanvas: (
    indexData: Uint8Array,
    palette: Uint32Array,
    width: number,
    height: number,
    targetCanvas: HTMLCanvasElement
  ) => Promise<boolean>;
  updatePalette: (palette: Uint32Array) => void;
  getBackend: () => string;
  isGPUAccelerated: () => boolean;
  benchmark: (width?: number, height?: number) => Promise<any>;
  dispose: () => void;
}

export interface WasmGifDecoder {
  memory: WebAssembly.Memory;
  decode_rgba: (
    gifPtr: number,
    gifLen: number,
    frameIndex: number,
    outPtr: number,
    outLen: number
  ) => number;
  decode_rgba_threaded: (
    gifPtr: number,
    gifLen: number,
    frameIndex: number,
    outPtr: number,
    outLen: number,
    numThreads: number
  ) => number;
  init_heap: () => void;
  reset_heap: () => void;
  get_heap_usage: () => number;
  test_simd: () => number;
  wasm_malloc: (size: number) => number;
  wasm_free: (ptr: number) => void;
  heapU8: Uint8Array;
  heapU32: Uint32Array;
}

export interface WasmWorkerPool {
  decode: (
    gifData: Uint8Array,
    frameIndex: number
  ) => Promise<{ pixels: Uint32Array; delay: number }>;
  decodeParallel: (
    gifData: Uint8Array,
    frameIndices: number[]
  ) => Promise<{ pixels: Uint32Array; delay: number }[]>;
  terminate: () => void;
  getStats: () => {
    activeWorkers: number;
    completedJobs: number;
    avgDecodeTime: number;
  };
}

export interface PooledDecoderTables {
  decTable: Int32Array;
  stack: Uint8Array;
  firstByte: Int16Array;
  out32Cache: WeakMap<Uint8Array, Uint32Array>;
  hash: string;
}

export type PaletteRGB = number[]; // array of 24-bit 0xRRGGBB

export type FrameInfo = {
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
