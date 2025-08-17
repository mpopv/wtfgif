import {
  UnifiedGPUGifRenderer,
  ColorMapWasm,
  WasmGifDecoder,
  WasmWorkerPool,
  PooledDecoderTables,
  PaletteRGB,
  FrameInfo
} from "./types";

export {
  UnifiedGPUGifRenderer,
  ColorMapWasm,
  WasmGifDecoder,
  WasmWorkerPool,
  PooledDecoderTables,
  PaletteRGB,
  FrameInfo
};

export declare class GifWriter {
  constructor(
    buf: Uint8Array,
    width: number,
    height: number,
    gopts?: {
      loop?: number | null;
      palette?: PaletteRGB | null;
      background?: number;
    }
  );
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
  ): number;
}

export declare class GifReader {
  constructor(buf: Uint8Array, usePooling?: boolean);
  static createPooled(buf: Uint8Array): GifReader;
  static createUnpooled(buf: Uint8Array): GifReader;
  numFrames(): number;
  loopCount(): number | null;
  frameInfo(i: number): FrameInfo;
  decodeAndBlitFrameBGRA(frameNum: number, pixels: Uint8Array): void;
  decodeAndBlitFrameRGBA(frameNum: number, pixels: Uint8Array): void;
  decodeFrameToTransferableRGBA(frameNum: number): ArrayBuffer;
  decodeFrameToTransferableBGRA(frameNum: number): ArrayBuffer;
  decodeFrameIntoBuffer(frameNum: number, buffer: ArrayBuffer, format?: "rgba" | "bgra"): void;
  dispose(): void;
  returnToPool(): void;
  enableWasmColorMapping(colorMapWasm: ColorMapWasm): void;
  disableWasmColorMapping(): void;
  isWasmEnabled(): boolean;
  static getPoolStats(): {
    available: number;
    totalCreated: number;
    hits: number;
    misses: number;
  };
}

export declare function initializeWasmGlobally(): Promise<boolean>;
export declare function getWasmStatus(): {
  supported: boolean;
  simd: boolean;
  threads: boolean;
  heapUsage?: number;
};
export declare function cleanupWasm(): void;

declare global {
  interface Window {
    wtfgif: {
      GifWriter: typeof GifWriter;
      GifReader: typeof GifReader;
      initializeWasmGlobally: typeof initializeWasmGlobally;
      getWasmStatus: typeof getWasmStatus;
      cleanupWasm: typeof cleanupWasm;
    };
  }
  var wtfgif: Window["wtfgif"];
}

export {};
