import {
  FrameInfo,
  GifBinary,
  GifDecodeBackend,
  GifDecodeBackendStatus,
  GifPixelBuffer,
  PreparedFrameBackendPreference,
  PooledDecoderTables,
  PreparedFrameCacheMode,
  PreparedFrameDedupeMode,
  PreparedFrameFormat,
  PreparedGifFrame,
  PreparedGifFrames,
  PreparedGifPlayer,
  PrepareFramesOptions,
  WasmCoreInstance,
} from "../types";
import { GIF } from "../constants/gif";
import { buildPal32 } from "../utils/palette";
import { concatSubBlocks } from "../utils/subblocks";
import { readNetscapeLoopCount } from "../utils/netscape";
import {
  createDecoderTables,
} from "./pool";
import { getWasmCoreModule } from "../wasm/runtime";

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

type FastGifParse = {
  width: number;
  height: number;
  globalPaletteOffset: number | null;
  globalPaletteSize: number | null;
  frames: FrameInfo[];
  loopCount: number | null;
};

/**
 * Parse the ordinary, valid GIF shape without per-byte closures or bounds
 * checks.  The compatibility parser below remains the fallback for malformed
 * and unusual inputs; returning null here keeps its error behavior intact.
 */
function tryParseFastGif(data: Uint8Array): FastGifParse | null {
  if (data.length < 13) return null;
  if (
    data[0] !== GIF.G ||
    data[1] !== GIF.I ||
    data[2] !== GIF.F ||
    data[3] !== GIF._8 ||
    (((data[4]! + 1) & 0xfd) !== GIF._8) ||
    data[5] !== GIF.A
  ) {
    return null;
  }

  let p = 6;
  const width = (data[p++]! | (data[p++]! << 8)) >>> 0;
  const height = (data[p++]! | (data[p++]! << 8)) >>> 0;
  const packed = data[p++]! | 0;
  const gctFlag = (packed >>> 7) & 1;
  const gctColors = 1 << ((packed & 0x7) + 1);
  p += 2; // background color index and pixel aspect ratio

  let globalPaletteOffset: number | null = null;
  let globalPaletteSize: number | null = null;
  if (gctFlag) {
    const paletteBytes = gctColors * 3;
    if (p + paletteBytes > data.length) return null;
    globalPaletteOffset = p;
    globalPaletteSize = gctColors;
    p += paletteBytes;
  }

  const frames: FrameInfo[] = [];
  let delay = 0;
  let transparentIndex: number | null = null;
  let disposal = 0;
  let loopCount: number | null = null;

  while (p < data.length) {
    const block = data[p++]! | 0;
    if (block === GIF.TRAILER) {
      return {
        width,
        height,
        globalPaletteOffset,
        globalPaletteSize,
        frames,
        loopCount,
      };
    }

    if (block === GIF.EXT) {
      if (p >= data.length) return null;
      const label = data[p++]! | 0;
      if (label === GIF.APPLICATION) {
        // NETSCAPE2.0 with its canonical 0x03/0x01 loop sub-block.
        if (
          p + 16 < data.length &&
          data[p] === GIF.NETSCAPE_LEN &&
          data[p + 1] === 0x4e &&
          data[p + 2] === 0x45 &&
          data[p + 3] === 0x54 &&
          data[p + 4] === 0x53 &&
          data[p + 5] === 0x43 &&
          data[p + 6] === 0x41 &&
          data[p + 7] === 0x50 &&
          data[p + 8] === 0x45 &&
          data[p + 9] === 0x32 &&
          data[p + 10] === 0x2e &&
          data[p + 11] === 0x30 &&
          data[p + 12] === 0x03 &&
          data[p + 13] === 0x01 &&
          data[p + 16] === 0x00
        ) {
          loopCount = (data[p + 14]! | (data[p + 15]! << 8)) >>> 0;
          p += 17;
          continue;
        }

        // Unknown application extensions: skip the fixed identifier and
        // then each data sub-block.  Fall back if the stream is truncated.
        if (p + 12 > data.length) return null;
        p += 12;
        let terminated = false;
        while (p < data.length) {
          const size = data[p++]! | 0;
          if (size === 0) {
            terminated = true;
            break;
          }
          if (p + size > data.length) return null;
          p += size;
        }
        if (!terminated) return null;
        continue;
      }

      if (label === GIF.GCE) {
        if (p + 5 > data.length || data[p++] !== 0x04) return null;
        const packedGce = data[p++]! | 0;
        delay = (data[p++]! | (data[p++]! << 8)) >>> 0;
        const candidate = data[p++]! | 0;
        if (data[p++] !== 0x00) return null;
        transparentIndex = packedGce & 1 ? candidate : null;
        disposal = (packedGce >>> 2) & 0x7;
        continue;
      }

      if (label === GIF.PLAINTEXT || label === GIF.COMMENT) {
        let terminated = false;
        while (p < data.length) {
          const size = data[p++]! | 0;
          if (size === 0) {
            terminated = true;
            break;
          }
          if (p + size > data.length) return null;
          p += size;
        }
        if (!terminated) return null;
        continue;
      }

      return null;
    }

    if (block !== GIF.IMG) return null;
    if (p + 9 > data.length) return null;
    const x = (data[p++]! | (data[p++]! << 8)) >>> 0;
    const y = (data[p++]! | (data[p++]! << 8)) >>> 0;
    const frameWidth = (data[p++]! | (data[p++]! << 8)) >>> 0;
    const frameHeight = (data[p++]! | (data[p++]! << 8)) >>> 0;
    const packedFrame = data[p++]! | 0;
    const localPaletteFlag = (packedFrame >>> 7) & 1;
    const interlaced = ((packedFrame >>> 6) & 1) !== 0;
    const localColors = 1 << ((packedFrame & 0x7) + 1);
    let paletteOffset = globalPaletteOffset;
    let paletteSize = globalPaletteSize;
    let hasLocalPalette = false;
    if (localPaletteFlag) {
      const paletteBytes = localColors * 3;
      if (p + paletteBytes > data.length) return null;
      hasLocalPalette = true;
      paletteOffset = p;
      paletteSize = localColors;
      p += paletteBytes;
    }

    const dataOffset = p;
    if (p >= data.length) return null;
    const minCodeSize = data[p++]! | 0;
    let terminated = false;
    while (p < data.length) {
      const size = data[p++]! | 0;
      if (size === 0) {
        terminated = true;
        break;
      }
      if (p + size > data.length) return null;
      p += size;
    }
    if (!terminated) return null;

    frames.push({
      x,
      y,
      width: frameWidth,
      height: frameHeight,
      has_local_palette: hasLocalPalette,
      palette_offset: paletteOffset,
      palette_size: paletteSize,
      data_offset: dataOffset,
      data_length: p - dataOffset,
      transparent_index: transparentIndex,
      interlaced,
      delay,
      disposal,
      min_code_size: minCodeSize,
    });

    delay = 0;
    transparentIndex = null;
    disposal = 0;
  }

  return null;
}

/* ====== Reader (Decoder) ====== */
// moved to types.ts

export class GifReader {
  private buf!: Uint8Array;
  private width_!: number;
  private height_!: number;

  private globalPaletteOffset!: number | null;
  private globalPaletteSize!: number | null;
  private globalPal32rgba: Uint32Array | undefined;
  private globalPal32bgra: Uint32Array | undefined;
  private globalPal32TransparentIndex: number | null | undefined;

  private frames!: FrameInfo[];
  private loop_count!: number | null;

  // Decoder scratch tables owned by this one reader/job.
  private pooledTables!: PooledDecoderTables | null;

  // Aliases for easier access
  private decTable!: Int32Array;
  private stack!: Uint8Array;
  private firstByte!: Int16Array;
  private activePreparedFrames!: Set<PreparedGifFrames>;
  private wasmCore!: WasmCoreInstance | null;
  private wasmMemory!: WebAssembly.Memory | null;
  // A legacy caller that walks every frame in order pays one Wasm boundary
  // crossing per frame. Once that access pattern is proven, cache the full
  // composited stream and serve the remaining frames as cheap typed-array
  // copies. The bounded cache is opt-in by observed access order, so random
  // single-frame reads keep their existing memory and latency behavior.
  private sequentialCompositedFrames!: Uint32Array | null;
  private sequentialCompositedOrder!: "rgba" | "bgra" | null;
  private sequentialInitialCanvasZero!: boolean | null;
  private sequentialInitialCanvasBuffer!: ArrayBufferLike | null;
  private sequentialInitialCanvasByteOffset!: number;
  private sequentialInitialCanvasByteLength!: number;
  private sequentialInitialCanvasOrder!: "rgba" | "bgra" | null;
  private lastDecodeTargetBuffer!: ArrayBufferLike | null;
  private lastDecodeTargetByteOffset!: number;
  private lastDecodeTargetByteLength!: number;
  private lastDecodeTarget32!: Uint32Array | null;
  private lastDecodedFrame!: number;
  // Small legacy frames benefit from a compact one-table decoder that reads
  // GIF subblocks in place.  These are lazy so the normal prepared/Wasm paths
  // pay nothing for the tiny-frame specialization.
  private directCodeTable!: Int32Array;
  private directIndices!: Uint8Array;
  private directStack!: Uint8Array;
  private static decodeBackend: GifDecodeBackend | null = null;

  static createPooled(buf: GifBinary): GifReader {
    return new GifReader(buf, false);
  }

  static createUnpooled(buf: GifBinary): GifReader {
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

  constructor(buf: GifBinary, _usePooling: boolean = false) {
    const data = buf instanceof Uint8Array ? buf : Uint8Array.from(buf);
    this.buf = data;
    this.globalPaletteOffset = null;
    this.globalPaletteSize = null;
    this.frames = [];
    this.loop_count = null;
    // Decoder tables, Wasm state, playback caches, and the prepared-frame
    // registry are all lazy.  The legacy GifReader path is often used for a
    // single tiny GIF; avoiding a dozen sentinel writes and three empty typed
    // arrays keeps construction close to omggif's deliberately minimal
    // object setup.  Every consumer initializes its state before use.
    this.lastDecodedFrame = -1;

    const fastParse = tryParseFastGif(data);
    if (fastParse) {
      this.width_ = fastParse.width;
      this.height_ = fastParse.height;
      this.globalPaletteOffset = fastParse.globalPaletteOffset;
      this.globalPaletteSize = fastParse.globalPaletteSize;
      this.frames = fastParse.frames;
      this.loop_count = fastParse.loopCount;
      return;
    }

    let p = 0;
    const readByte = (): number => {
      const value = data[p++];
      if (value === undefined) {
        throw new Error("Unexpected end of GIF data.");
      }
      return value;
    };
    const readUint16 = (): number => (readByte() | (readByte() << 8)) >>> 0;

    // Header: GIF87a / GIF89a
    if (
      data[p++] !== GIF.G ||
      data[p++] !== GIF.I ||
      data[p++] !== GIF.F ||
      data[p++] !== GIF._8 ||
      ((data[p++]! + 1) & 0xfd) !== GIF._8 ||
      data[p++] !== GIF.A
    ) {
      throw new Error("Invalid GIF 87a/89a header.");
    }

    const width = (data[p++]! | (data[p++]! << 8)) >>> 0;
    const height = (data[p++]! | (data[p++]! << 8)) >>> 0;
    this.width_ = width;
    this.height_ = height;

    const pf0 = data[p++]!; // packed fields
    const gctFlag = (pf0 >>> 7) & 1;
    const gctSizeBits = pf0 & 0x7;
    const gctColors = 1 << (gctSizeBits + 1);
    readByte(); // background color index
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
      const block = readByte();
      switch (block) {
        case GIF.EXT: {
          const label = readByte();
          switch (label) {
            case GIF.APPLICATION: {
              const netscape = readNetscapeLoopCount(data, p);
              if (netscape) {
                this.loop_count = netscape.loopCount;
                p = netscape.nextPos;
              } else {
                // skip unknown app extension
                p += 12;
                while (true) {
                  const size = readByte();
                  if (size === 0) break;
                  p += size;
                }
              }
              break;
            }
            case GIF.GCE: {
              if (readByte() !== 0x4 || data[p + 4] !== 0)
                throw new Error("Invalid graphics extension block.");
              const pf1 = readByte();
              delay = readUint16();
              const t = readByte();
              transparent_index = pf1 & 1 ? t : null;
              disposal = (pf1 >> 2) & 0x7;
              p++; // terminator
              break;
            }
            case GIF.PLAINTEXT:
            case GIF.COMMENT: {
              while (true) {
                const size = readByte();
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
          const x = readUint16();
          const y = readUint16();
          const w = readUint16();
          const h = readUint16();
          const pf2 = readByte();
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
            const size = readByte();
            if (size === 0) break;
            p += size;
          }

          this.frames.push({
            x,
            y,
            width: w,
            height: h,
            has_local_palette,
            palette_offset,
            palette_size,
            data_offset, // keep for compatibility
            data_length: p - data_offset,
            transparent_index,
            interlaced: interlace,
            delay,
            disposal,
            min_code_size: data[data_offset]! | 0,
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
        if (q + 1 < bytes.length) {
          bits |= ((bytes[q]! | 0) | ((bytes[q + 1]! | 0) << 8)) << bitCount;
          bitCount += 16;
          q += 2;
        } else {
          bits |= (bytes[q++]! | 0) << bitCount;
          bitCount += 8;
        }
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
          outFirst = this.firstByte[prevCode]! | 0;
          stack[sp++] = outFirst;
          cur = prevCode;
        } else {
          outFirst = this.firstByte[cur]! | 0;
        }

        while (cur >= CLEAR) {
          const entry = table[cur]! | 0;
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
          outputIndices[pixelIndex++] = stack[--sp]! & 0xff;
        }
      }

      // Add new table entry
      if (prevCode !== null && nextCode < GIF.MAX_CODE) {
        table[nextCode] = ((prevCode & 0xfff) << 8) | (outFirst & 0xff);
        this.firstByte[nextCode] = this.firstByte[prevCode]!;
        nextCode++;

        if (nextCode >= codeMask + 1 && codeSize < 12) {
          codeSize++;
          codeMask = (1 << codeSize) - 1;
        }
      }

      prevCode = code;
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
    return this.frames[i]!;
  }

  private ensureDecoderTables(): void {
    if (this.pooledTables) return;

    this.pooledTables = createDecoderTables();
    this.decTable = this.pooledTables.decTable;
    this.stack = this.pooledTables.stack;
    this.firstByte = this.pooledTables.firstByte;
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
    if (frame.palette_offset === null || frame.palette_size === null) {
      throw new Error("GIF frame has no color palette.");
    }

    const usesGlobalPalette =
      frame.palette_offset === this.globalPaletteOffset &&
      frame.palette_size === this.globalPaletteSize;
    if (usesGlobalPalette) {
      const transparentIndex = frame.transparent_index;
      if (
        this.globalPal32TransparentIndex === transparentIndex &&
        (order === "rgba" ? this.globalPal32rgba : this.globalPal32bgra)
      ) {
        return (order === "rgba"
          ? this.globalPal32rgba
          : this.globalPal32bgra)!;
      }
      const palette = buildPal32(
        this.buf,
        frame.palette_offset,
        frame.palette_size,
        order,
        transparentIndex
      );
      this.globalPal32TransparentIndex = transparentIndex;
      if (order === "rgba") {
        this.globalPal32rgba = palette;
      } else {
        this.globalPal32bgra = palette;
      }
      return palette;
    }

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
          indexData[dst++] = tempIndices[pixelIndex++]!;
        }
      }
    }

    frame.indices = indexData;
    return indexData;
  }

  /* Public API mirrors omggif, including plain arrays and clamped arrays. */
  decodeAndBlitFrameBGRA(frameNum: number, pixels: GifPixelBuffer): void {
    this.decodeAndBlitCompatibleFrame(frameNum, pixels, "bgra");
  }
  decodeAndBlitFrameRGBA(frameNum: number, pixels: GifPixelBuffer): void {
    this.decodeAndBlitCompatibleFrame(frameNum, pixels, "rgba");
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
    this.clearFrameCaches();
    try {
      const normalized = this.normalizePrepareFramesOptions(options);
      const frameIndices = this.normalizeFrameIndices(normalized.frameIndices);

      if (normalized.backend !== "javascript") {
        const backendResult = this.prepareFramesWithBackend(normalized);
        if (backendResult) {
          return this.createBackendPreparedFramesResult(backendResult);
        }
        if (normalized.backend === "native") {
          throw new Error("Native GIF decode backend is not available.");
        }
      }

      return normalized.composited
        ? this.prepareCompositedFrames(normalized, frameIndices)
        : this.prepareUncompositedFrames(normalized, frameIndices);
    } finally {
      this.clearFrameCaches();
    }
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

  private createBackendPreparedFramesResult(
    prepared: PreparedGifFrames
  ): PreparedGifFrames {
    const disposeBackendResult = prepared.dispose;
    let disposed = false;
    const result: PreparedGifFrames = {
      ...prepared,
      dispose: () => {
        if (disposed) {
          return;
        }
        disposed = true;
        this.activePreparedFrames.delete(result);
        disposeBackendResult();
      },
    };
    (this.activePreparedFrames ??= new Set()).add(result);
    return result;
  }

  private prepareCompositedFrames(
    options: NormalizedPrepareFramesOptions,
    frameIndices: readonly number[]
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
      options.maxBytes ?? null
    );
  }

  private prepareUncompositedFrames(
    options: NormalizedPrepareFramesOptions,
    frameIndices: readonly number[]
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
      options.maxBytes ?? null
    );
  }

  private createPreparedFramesResult(
    format: PreparedFrameFormat,
    composited: boolean,
    frames: PreparedGifFrame[],
    byteLength: number,
    maxBytes: number | null
  ): PreparedGifFrames {
    const byIndex = new Array<PreparedGifFrame | undefined>(this.frames.length);
    for (const frame of frames) {
      byIndex[frame.index] = frame;
    }

    const result: PreparedGifFrames = {
      width: this.width_,
      height: this.height_,
      format,
      composited,
      frames,
      byteLength,
      maxBytes,
      getFrame: (index: number) => byIndex[index],
      getFramePixels: (index: number) => byIndex[index]?.pixels,
      getFrameBytes: (index: number) => {
        const pixels = byIndex[index]?.pixels;
        return pixels
          ? new Uint8Array(pixels.buffer, pixels.byteOffset, pixels.byteLength)
          : undefined;
      },
      copyFrame: (index: number, target: Uint8Array | Uint32Array) => {
        const frame = byIndex[index];
        if (!frame) {
          throw new Error("Frame index out of range.");
        }
        this.copyPreparedFrame(frame, target);
      },
      createPlayer: (target?: Uint8Array | Uint32Array) =>
        this.createPreparedPlayer(byIndex, target),
      dispose: () => {
        this.activePreparedFrames.delete(result);
        byIndex.length = 0;
        frames.length = 0;
      },
    };
    (this.activePreparedFrames ??= new Set()).add(result);
    return result;
  }

  private createPreparedPlayer(
    byIndex: readonly (PreparedGifFrame | undefined)[],
    target?: Uint8Array | Uint32Array
  ): PreparedGifPlayer {
    const target32 = target
      ? this.getPreparedTarget32(target)
      : new Uint32Array(this.width_ * this.height_);
    let currentIndex = -1;
    const drawFrame = (index: number): Uint32Array => {
      const frame = byIndex[index];
      if (!frame) {
        throw new Error("Frame index out of range.");
      }

      const previousFrame = byIndex[currentIndex];
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
        this.copyPreparedFrame32(frame, target32);
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
        return byIndex[nextIndex] ? drawFrame(nextIndex) : target32;
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
    this.copyPreparedFrame32(frame, target32);
  }

  private copyPreparedFrame32(
    frame: PreparedGifFrame,
    target32: Uint32Array
  ): void {
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

    return new Uint32Array(
      target.buffer,
      target.byteOffset,
      requiredPixels
    );
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
        `Prepared frame output exceeds maxBytes (${nextByteLength} > ${maxBytes}).`
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
    const activePreparedFrames = this.activePreparedFrames;
    if (activePreparedFrames) {
      for (const prepared of Array.from(activePreparedFrames)) {
        prepared.dispose();
      }
      activePreparedFrames.clear();
    }
    this.clearFrameCaches();
    this.releaseWasmCore();

    this.pooledTables = null;
    this.decTable = new Int32Array(0);
    this.stack = new Uint8Array(0);
    this.firstByte = new Int16Array(0);
    this.lastDecodeTargetBuffer = null;
    this.lastDecodeTargetByteOffset = 0;
    this.lastDecodeTargetByteLength = 0;
    this.lastDecodeTarget32 = null;
  }

  private releaseWasmCore(): void {
    const core = this.wasmCore;
    this.wasmCore = null;
    this.wasmMemory = null;
    core?.free();
  }

  private clearFrameCaches(): void {
    for (const frame of this.frames) {
      delete frame.codes;
      delete frame.indices;
      delete frame.pal32rgba;
      delete frame.pal32bgra;
      delete frame.rgbaColors;
      delete frame.bgraColors;
      delete frame.opaqueSpans;
      delete frame.opaquePositions;
      delete frame.decodeCount;
    }
    this.globalPal32rgba = undefined;
    this.globalPal32bgra = undefined;
    this.globalPal32TransparentIndex = undefined;
    this.sequentialCompositedFrames = null;
    this.sequentialCompositedOrder = null;
    this.sequentialInitialCanvasZero = null;
    this.sequentialInitialCanvasBuffer = null;
    this.sequentialInitialCanvasByteOffset = 0;
    this.sequentialInitialCanvasByteLength = 0;
    this.sequentialInitialCanvasOrder = null;
    this.lastDecodedFrame = -1;
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
    return {
      available: 0,
      totalCreated: 0,
      hits: 0,
      misses: 0,
    };
  }

  private tryDecodeSequentialCompositedFrame(
    frameNum: number,
    out32: Uint32Array,
    order: "rgba" | "bgra",
  ): boolean {
    const canvasPixels = this.width_ * this.height_;
    const cached = this.sequentialCompositedFrames;
    if (cached && this.sequentialCompositedOrder === order) {
      const start = frameNum * canvasPixels;
      out32.set(cached.subarray(start, start + canvasPixels), 0);
      this.lastDecodedFrame = frameNum;
      return true;
    }
    const totalPixels = canvasPixels * this.frames.length;
    const cacheCandidate =
      this.frames.length >= 8 &&
      canvasPixels >= 512 &&
      Number.isSafeInteger(totalPixels) &&
      totalPixels <= 1_000_000;
    if (
      frameNum === 0 &&
      this.lastDecodedFrame === -1 &&
      cacheCandidate
    ) {
      let zero = true;
      for (let index = 0; index < canvasPixels; index++) {
        if (out32[index] !== 0) {
          zero = false;
          break;
        }
      }
      this.sequentialInitialCanvasZero = zero;
      if (zero) {
        this.sequentialInitialCanvasBuffer = out32.buffer;
        this.sequentialInitialCanvasByteOffset = out32.byteOffset;
        this.sequentialInitialCanvasByteLength = out32.byteLength;
        this.sequentialInitialCanvasOrder = order;
      }
    }

    const sameInitialCanvas =
      this.sequentialInitialCanvasZero === true &&
      this.sequentialInitialCanvasBuffer === out32.buffer &&
      this.sequentialInitialCanvasByteOffset === out32.byteOffset &&
      this.sequentialInitialCanvasByteLength === out32.byteLength &&
      this.sequentialInitialCanvasOrder === order;
    // Do not turn an isolated frame read into a whole-animation decode. The
    // cache is deliberately armed only by the first sequential pair into the
    // same caller-owned canvas, and is bounded to keep a legacy reader from
    // unexpectedly retaining a large animation in memory (1,000,000 output
    // pixels at most).
    if (
      frameNum !== 1 ||
      this.lastDecodedFrame !== 0 ||
      !cacheCandidate ||
      !sameInitialCanvas
    ) {
      return false;
    }

    const wasmModule = getWasmCoreModule();
    if (!wasmModule) {
      return false;
    }
    this.wasmCore ??= new wasmModule.WtfGifCore(this.buf);
    this.wasmMemory ??= wasmModule.wasm_memory?.() ?? null;
    const requestedFrames = new Uint8Array(this.frames.length).fill(1);
    const prepareScratch =
      order === "rgba"
        ? this.wasmCore.prepare_composited_rgba_scratch
        : this.wasmCore.prepare_composited_bgra_scratch;
    const scratchPointer = this.wasmCore.composited_scratch_ptr;
    if (prepareScratch && scratchPointer && this.wasmMemory) {
      const preparedLength = prepareScratch.call(
        this.wasmCore,
        requestedFrames,
      );
      const preparedPointer = scratchPointer.call(this.wasmCore);
      if (
        preparedLength === totalPixels &&
        preparedPointer > 0 &&
        (preparedPointer & 3) === 0
      ) {
        const preparedView = new Uint32Array(
          this.wasmMemory.buffer,
          preparedPointer,
          totalPixels,
        );
        const prepared = new Uint32Array(totalPixels);
        prepared.set(preparedView);
        this.releaseWasmCore();
        this.sequentialCompositedFrames = prepared;
        this.sequentialCompositedOrder = order;
        const start = frameNum * canvasPixels;
        out32.set(prepared.subarray(start, start + canvasPixels), 0);
        this.lastDecodedFrame = frameNum;
        return true;
      }
    }
    const prepared =
      order === "rgba"
        ? this.wasmCore.prepare_composited_rgba(requestedFrames)
        : this.wasmCore.prepare_composited_bgra(requestedFrames);
    if (prepared.length !== totalPixels) {
      return false;
    }
    const cachedFrames = new Uint32Array(prepared);
    this.releaseWasmCore();
    this.sequentialCompositedFrames = cachedFrames;
    this.sequentialCompositedOrder = order;
    const start = frameNum * canvasPixels;
    out32.set(cachedFrames.subarray(start, start + canvasPixels), 0);
    this.lastDecodedFrame = frameNum;
    return true;
  }

  /* Fused LZW decode → Uint32 blit with precomputed pal32, transparency, interlace. */
  private getDecodeTarget32(pixels: Uint8Array): Uint32Array {
    if (
      this.lastDecodeTarget32 &&
      this.lastDecodeTargetBuffer === pixels.buffer &&
      this.lastDecodeTargetByteOffset === pixels.byteOffset &&
      this.lastDecodeTargetByteLength === pixels.byteLength
    ) {
      return this.lastDecodeTarget32;
    }

    const target = new Uint32Array(
      pixels.buffer,
      pixels.byteOffset,
      pixels.byteLength >>> 2,
    );
    this.lastDecodeTargetBuffer = pixels.buffer;
    this.lastDecodeTargetByteOffset = pixels.byteOffset;
    this.lastDecodeTargetByteLength = pixels.byteLength;
    this.lastDecodeTarget32 = target;
    return target;
  }

  private tryDecodeWasmFrame32(
    frameNum: number,
    pixels: Uint8Array,
    order: "rgba" | "bgra",
  ): boolean {
    const wasmModule = getWasmCoreModule();
    if (!wasmModule) return false;

    this.wasmCore ??= new wasmModule.WtfGifCore(this.buf);
    const decodeAndBlit =
      order === "rgba"
        ? this.wasmCore.decode_and_blit_frame_rgba
        : this.wasmCore.decode_and_blit_frame_bgra;
    if (!decodeAndBlit) return false;

    try {
      const canvasPixels = this.width_ * this.height_;
      decodeAndBlit.call(
        this.wasmCore,
        frameNum,
        pixels.byteLength === canvasPixels * 4
          ? pixels
          : pixels.subarray(0, canvasPixels * 4),
      );
      return true;
    } catch {
      // Custom/older Wasm modules may reject a frame shape.  Preserve the
      // compatible JavaScript path as a fallback.
      return false;
    }
  }

  private decodeAndBlitFrame32(
    frameNum: number,
    pixels: Uint8Array,
    order: "rgba" | "bgra"
  ) {
    if (frameNum < 0 || frameNum >= this.frames.length)
      throw new Error("Frame index out of range.");

    const out32 = this.getDecodeTarget32(pixels);

    if (
      this.frames.length >= 8 &&
      this.tryDecodeSequentialCompositedFrame(frameNum, out32, order)
    ) {
      return;
    }

    const frame = this.frames[frameNum]!;
    const framePixels = frame.width * frame.height;
    const canvasPixels = this.width_ * this.height_;
    if (framePixels <= 4096) {
      // Once a reader is clearly being used for an animation, the Wasm
      // decoder's reusable scratch path beats the JS tiny-frame loop for
      // medium rectangles.  Keep one-off/tiny reads on JS to avoid paying a
      // Wasm core construction just for a single small frame.
      if (
        (frameNum < 2 || this.wasmCore != null) &&
        this.frames.length >= 8 &&
        framePixels >= 512 &&
        canvasPixels >= 512 &&
        framePixels * 2 >= canvasPixels &&
        this.tryDecodeWasmFrame32(frameNum, pixels, order)
      ) {
        this.lastDecodedFrame = frameNum;
        return;
      }
      this.lzwDecodeAndBlitSmallFrame(frame, out32, order);
      this.lastDecodedFrame = frameNum;
      return;
    }
    // The Rust decoder is already faster for medium partial frames; the old
    // full-frame-only cutoff left measurable work on the JS path.
    if (
      canvasPixels >= 8192 &&
      framePixels >= 4096 &&
      (framePixels * 2 >= canvasPixels || canvasPixels >= 40_000)
    ) {
      const wasmModule = getWasmCoreModule();
      if (wasmModule) {
        this.wasmCore ??= new wasmModule.WtfGifCore(this.buf);
        this.wasmMemory ??= wasmModule.wasm_memory?.() ?? null;
        const scratchDecode =
          order === "rgba"
            ? this.wasmCore.decode_frame_rgba_scratch
            : this.wasmCore.decode_frame_bgra_scratch;
        const scratchPtr = this.wasmCore.decode_scratch_ptr;
        if (
          !frame.interlaced &&
          frame.transparent_index === null &&
          frame.x === 0 &&
          frame.y === 0 &&
          frame.width === this.width_ &&
          frame.height === this.height_ &&
          scratchDecode &&
          scratchPtr &&
          this.wasmMemory
        ) {
          try {
            const outputLength = scratchDecode.call(this.wasmCore, frameNum);
            const outputPointer = scratchPtr.call(this.wasmCore);
            if (
              outputLength === canvasPixels * 4 &&
              outputPointer > 0 &&
              (outputPointer & 3) === 0
            ) {
              out32.set(
                new Uint32Array(
                  this.wasmMemory.buffer,
                  outputPointer,
                  canvasPixels,
                ),
              );
              this.lastDecodedFrame = frameNum;
              return;
            }
          } catch {
            // Older/custom Wasm modules may expose the optional symbols but
            // reject this frame shape. Fall through to the compatible path.
          }
        }
        const rectDecode =
          order === "rgba"
            ? this.wasmCore.decode_frame_rect_rgba_scratch
            : this.wasmCore.decode_frame_rect_bgra_scratch;
        const rectScratchPtr = this.wasmCore.composited_scratch_ptr;
        if (
          canvasPixels >= 40_000 &&
          framePixels < canvasPixels &&
          frame.x >= 0 &&
          frame.y >= 0 &&
          frame.x + frame.width <= this.width_ &&
          frame.y + frame.height <= this.height_ &&
          rectDecode &&
          rectScratchPtr &&
          this.wasmMemory
        ) {
          try {
            const outputLength = rectDecode.call(this.wasmCore, frameNum);
            const outputPointer = rectScratchPtr.call(this.wasmCore);
            if (
              outputLength === framePixels &&
              outputPointer > 0 &&
              (outputPointer & 3) === 0
            ) {
              const source = new Uint32Array(
                this.wasmMemory.buffer,
                outputPointer,
                framePixels,
              );
              const frameWidth = frame.width | 0;
              const frameHeight = frame.height | 0;
              const frameX = frame.x | 0;
              const frameY = frame.y | 0;
              if (frame.transparent_index === null) {
                for (let row = 0; row < frameHeight; row++) {
                  out32.set(
                    source.subarray(row * frameWidth, (row + 1) * frameWidth),
                    (frameY + row) * this.width_ + frameX,
                  );
                }
              } else {
                for (let row = 0; row < frameHeight; row++) {
                  const sourceOffset = row * frameWidth;
                  let destinationOffset =
                    (frameY + row) * this.width_ + frameX;
                  for (let column = 0; column < frameWidth; column++) {
                    const color = source[sourceOffset + column]!;
                    if (color !== 0) {
                      out32[destinationOffset] = color;
                    }
                    destinationOffset++;
                  }
                }
              }
              this.lastDecodedFrame = frameNum;
              return;
            }
          } catch {
            // Older/custom Wasm modules may expose the optional symbols but
            // reject this frame shape. Fall through to the compatible path.
          }
        }
        const decodeAndBlit =
          order === "rgba"
            ? this.wasmCore.decode_and_blit_frame_rgba
            : this.wasmCore.decode_and_blit_frame_bgra;
        if (decodeAndBlit) {
          decodeAndBlit.call(
            this.wasmCore,
            frameNum,
            pixels.byteLength === canvasPixels * 4
              ? pixels
              : pixels.subarray(0, canvasPixels * 4)
          );
          this.lastDecodedFrame = frameNum;
          return;
        }
      }
    }
    this.ensureDecoderTables();
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
    this.lastDecodedFrame = frameNum;
  }

  /**
   * Decode a small frame without materializing concatenated LZW subblocks or
   * a 256-entry palette.  omggif's tiny-frame timings are dominated by those
   * two setup allocations; a reusable dictionary plus compact scratch keeps
   * the hot path cheap while retaining the exact legacy blit behavior.
   */
  private lzwDecodeAndBlitSmallFrame(
    frame: FrameInfo,
    out32: Uint32Array,
    order: "rgba" | "bgra",
  ): void {
    const paletteOffset = frame.palette_offset;
    if (paletteOffset === null || paletteOffset === undefined) {
      throw new Error("GIF frame has no color palette.");
    }

    if (!frame.interlaced) {
      if (this.tryDecodeLiteralFrame(frame, out32, order)) {
        return;
      }
      this.lzwDecodeSmallNonInterlacedDirect(
        frame,
        out32,
        order,
      );
      return;
    }

    const framePixels = (frame.width * frame.height) | 0;
    const minCodeSize = frame.min_code_size | 0;
    const clearCode = 1 << minCodeSize;
    const tableCapacity = Math.min(
      GIF.MAX_CODE,
      framePixels + clearCode + 2,
    );
    let indices = this.directIndices;
    if (!indices || indices.length < framePixels) {
      indices = new Uint8Array(framePixels);
      this.directIndices = indices;
    }

    let table = this.directCodeTable;
    if (!table || table.length < tableCapacity) {
      table = new Int32Array(tableCapacity);
      this.directCodeTable = table;
    }

    const data = this.buf;
    let p = (frame.data_offset | 0) + 1;
    let blockRemaining = data[p++]! | 0;
    const eoiCode = clearCode + 1;
    let nextCode = eoiCode + 1;
    let codeSize = (minCodeSize + 1) | 0;
    let codeMask = (1 << codeSize) - 1;
    let bitBuffer = 0;
    let bitCount = 0;
    let outputLength = 0;
    let previousCode = -1;
    let firstCode = true;
    let streamEnded = false;

    while (true) {
      // Keep a full 16-bit window so sub-block boundary checks happen once
      // per pair of input bytes instead of once per code.
      while (bitCount < 16) {
        if (blockRemaining === 0) {
          if (streamEnded) break;
          blockRemaining = data[p++]! | 0;
          if (blockRemaining === 0) {
            streamEnded = true;
            break;
          }
        }
        if (blockRemaining >= 2) {
          bitBuffer |=
            ((data[p]! | 0) | ((data[p + 1]! | 0) << 8)) << bitCount;
          bitCount += 16;
          p += 2;
          blockRemaining -= 2;
        } else {
          bitBuffer |= (data[p++]! | 0) << bitCount;
          bitCount += 8;
          blockRemaining--;
        }
      }
      if (bitCount < codeSize) break;

      const code = bitBuffer & codeMask;
      bitBuffer >>>= codeSize;
      bitCount -= codeSize;

      if (firstCode) {
        firstCode = false;
        if (code !== clearCode) {
          table.fill(0);
        }
      }

      if (code === clearCode) {
        nextCode = eoiCode + 1;
        codeSize = (minCodeSize + 1) | 0;
        codeMask = (1 << codeSize) - 1;
        previousCode = -1;
        continue;
      }
      if (code === eoiCode) break;

      const chaseCode = code < nextCode ? code : previousCode;
      if (chaseCode < 0) break;

      // Find the first byte and the sequence length, then write the sequence
      // backwards into the index scratch.  This is the same compact linked
      // dictionary layout as omggif, but it avoids a second temporary table.
      let chase = chaseCode;
      let chaseLength = 0;
      while (chase > clearCode) {
        chase = table[chase]! >>> 8;
        chaseLength++;
      }
      const first = chase & 0xff;
      const sequenceLength = chaseLength + (chaseCode !== code ? 1 : 0) + 1;
      const outputEnd = outputLength + sequenceLength;
      if (outputEnd > framePixels) break;

      // Keep the backwards-write cursor before the optional KwKwK suffix;
      // that suffix occupies outputEnd - 1 but must not be overwritten by the
      // dictionary walk below.
      let output = outputLength + chaseLength + 1;
      indices[outputLength] = first;
      if (chaseCode !== code) {
        indices[outputEnd - 1] = first;
      }

      chase = chaseCode;
      while (chaseLength--) {
        const entry = table[chase]! | 0;
        indices[--output] = entry & 0xff;
        chase = entry >>> 8;
      }
      outputLength = outputEnd;

      if (previousCode >= 0 && nextCode < GIF.MAX_CODE) {
        table[nextCode++] = (previousCode << 8) | first;
        if (nextCode >= codeMask + 1 && codeSize < 12) {
          codeSize++;
          codeMask = (codeMask << 1) | 1;
        }
      }
      previousCode = code;
    }

    const width = this.width_ | 0;
    const frameWidth = frame.width | 0;
    const frameHeight = frame.height | 0;
    const transparent = frame.transparent_index ?? 256;
    const rgba = order === "rgba";
    const palette = paletteOffset | 0;

    // Interlaced frames keep the decoder's stream order.  Mirror omggif's
    // byte cursor exactly here (including its historical pass offsets) so
    // drop-in callers get identical output for this uncommon frame shape.
    const outputBytes = new Uint8Array(
      out32.buffer,
      out32.byteOffset,
      out32.byteLength,
    );
    let xleft = frameWidth;
    const opbeg = (((frame.y | 0) * width + (frame.x | 0)) * 4) | 0;
    const opend = ((((frame.y | 0) + frameHeight) * width + (frame.x | 0)) * 4) | 0;
    let destination = opbeg;
    let scanStride = ((width - frameWidth) * 4) | 0;
    scanStride += width * 4 * 7;
    let interlaceSkip = 8;
    for (let i = 0; i < framePixels; i++) {
      if (xleft === 0) {
        destination += scanStride;
        xleft = frameWidth;
        if (destination >= opend) {
          scanStride =
            (width - frameWidth) * 4 + width * 4 * (interlaceSkip - 1);
          // This intentionally follows omggif's byte-offset formula, which
          // does not multiply the pass jump by four.
          destination =
            opbeg + (frameWidth + (width - frameWidth)) * (interlaceSkip << 1);
          interlaceSkip >>= 1;
        }
      }
      const index = indices[i]! | 0;
      if (index !== transparent) {
        const color = (palette + index * 3) | 0;
        const r = data[color]! | 0;
        const g = data[color + 1]! | 0;
        const b = data[color + 2]! | 0;
        if (rgba) {
          outputBytes[destination++] = r;
          outputBytes[destination++] = g;
          outputBytes[destination++] = b;
          outputBytes[destination++] = 255;
        } else {
          outputBytes[destination++] = b;
          outputBytes[destination++] = g;
          outputBytes[destination++] = r;
          outputBytes[destination++] = 255;
        }
      } else {
        destination += 4;
      }
      xleft--;
    }
  }

  /**
   * Decode fixed-width literal streams without constructing a LZW dictionary.
   * The exact raw/sub-block length gate keeps this path opt-in for our literal
   * writer; any ordinary compressed GIF falls through to the complete decoder.
   */
  private tryDecodeLiteralFrame(
    frame: FrameInfo,
    out32: Uint32Array,
    order: "rgba" | "bgra",
  ): boolean {
    const framePixels = (frame.width * frame.height) | 0;
    const minCodeSize = frame.min_code_size | 0;
    if (minCodeSize < 2 || minCodeSize > 8) return false;
    const codeBits = minCodeSize + 1;
    const clearCode = 1 << minCodeSize;
    const literalsPerClear = clearCode - 2;
    if (frame.palette_size === null || frame.palette_size > clearCode) {
      return false;
    }
    const clearCount = Math.ceil(framePixels / literalsPerClear);
    const rawLength = Math.ceil(
      ((framePixels + clearCount + 1) * codeBits) / 8,
    );
    const blockCount = Math.ceil(rawLength / 255);
    if (frame.data_length !== 2 + rawLength + blockCount) return false;

    const data = this.buf;
    const width = this.width_ | 0;
    const frameWidth = frame.width | 0;
    const transparent = frame.transparent_index ?? 256;
    const pal32 = this.getFramePalette(frame, order);
    const fullFrame =
      frame.x === 0 &&
      frame.y === 0 &&
      frameWidth === width &&
      frame.height === this.height_;
    let destination = ((frame.y | 0) * width + (frame.x | 0)) | 0;
    let xleft = fullFrame ? framePixels : frameWidth;
    const rowStride = fullFrame ? 0 : (width - frameWidth) | 0;
    let p = (frame.data_offset | 0) + 1;
    let blockRemaining = data[p++]! | 0;
    let bitBuffer = 0;
    let bitCount = 0;
    let outputLength = 0;

    const codeMask = clearCode * 2 - 1;
    const readCode = (): number => {
      while (bitCount < codeBits) {
        if (blockRemaining === 0) {
          blockRemaining = data[p++]! | 0;
          if (blockRemaining === 0) return -1;
        }
        if (blockRemaining >= 2) {
          bitBuffer |=
            ((data[p]! | 0) | ((data[p + 1]! | 0) << 8)) << bitCount;
          bitCount += 16;
          p += 2;
          blockRemaining -= 2;
        } else {
          bitBuffer |= (data[p++]! | 0) << bitCount;
          bitCount += 8;
          blockRemaining--;
        }
      }
      const code = bitBuffer & codeMask;
      bitBuffer >>>= codeBits;
      bitCount -= codeBits;
      return code;
    };

    for (let group = 0; group < clearCount; group++) {
      if (readCode() !== clearCode) return false;
      const groupLength = Math.min(literalsPerClear, framePixels - outputLength);
      for (let index = 0; index < groupLength; index++) {
        const code = readCode();
        if (code < 0 || code >= clearCode) return false;
        if (code >= pal32.length) return false;
        if (code !== transparent) {
          out32[destination] = pal32[code]!;
        }
        destination++;
        if (--xleft === 0) {
          destination += rowStride;
          xleft = frameWidth;
        }
        outputLength++;
      }
    }
    if (readCode() !== clearCode + 1 || outputLength !== framePixels) {
      return false;
    }

    return true;
  }

  private lzwDecodeSmallNonInterlacedDirect(
    frame: FrameInfo,
    out32: Uint32Array,
    order: "rgba" | "bgra",
  ): void {
    const framePixels = (frame.width * frame.height) | 0;
    const minCodeSize = frame.min_code_size | 0;
    const clearCode = 1 << minCodeSize;
    const tableCapacity = Math.min(
      GIF.MAX_CODE,
      framePixels + clearCode + 2,
    );
    let table = this.directCodeTable;
    if (!table || table.length < tableCapacity) {
      table = new Int32Array(tableCapacity);
      this.directCodeTable = table;
    }
    let stack = this.directStack;
    if (!stack || stack.length < framePixels) {
      stack = new Uint8Array(framePixels);
      this.directStack = stack;
    }

    const data = this.buf;
    let p = (frame.data_offset | 0) + 1;
    let blockRemaining = data[p++]! | 0;
    const eoiCode = clearCode + 1;
    let nextCode = eoiCode + 1;
    let codeSize = (minCodeSize + 1) | 0;
    let codeMask = (1 << codeSize) - 1;
    let bitBuffer = 0;
    let bitCount = 0;
    let pixelIndex = 0;
    let previousCode = -1;
    let firstCode = true;
    let streamEnded = false;

    const width = this.width_ | 0;
    const frameWidth = frame.width | 0;
    const fullFrame =
      frame.x === 0 &&
      frame.y === 0 &&
      frameWidth === width &&
      frame.height === this.height_;
    let destination = ((frame.y | 0) * width + (frame.x | 0)) | 0;
    let xleft = fullFrame ? framePixels : frameWidth;
    const rowStride = fullFrame ? 0 : (width - frameWidth) | 0;
    const transparent = frame.transparent_index ?? 256;
    const rgba = order === "rgba";
    const paletteOffset = frame.palette_offset;
    if (paletteOffset === null || paletteOffset === undefined) {
      throw new Error("GIF frame has no color palette.");
    }
    const emit = (index: number): void => {
      if (pixelIndex >= framePixels) return;
      if (index !== transparent) {
        const color = (paletteOffset + (index & 0xff) * 3) | 0;
        const r = data[color]! | 0;
        const g = data[color + 1]! | 0;
        const b = data[color + 2]! | 0;
        out32[destination] = rgba
          ? ((r | (g << 8) | (b << 16) | 0xff000000) >>> 0)
          : ((b | (g << 8) | (r << 16) | 0xff000000) >>> 0);
      }
      pixelIndex++;
      destination++;
      if (--xleft === 0) {
        destination += rowStride;
        xleft = frameWidth;
      }
    };

    while (true) {
      // Keep a full 16-bit window so sub-block boundary checks happen once
      // per pair of input bytes instead of once per code.
      while (bitCount < 16) {
        if (blockRemaining === 0) {
          if (streamEnded) break;
          blockRemaining = data[p++]! | 0;
          if (blockRemaining === 0) {
            streamEnded = true;
            break;
          }
        }
        if (blockRemaining >= 2) {
          bitBuffer |=
            ((data[p]! | 0) | ((data[p + 1]! | 0) << 8)) << bitCount;
          bitCount += 16;
          p += 2;
          blockRemaining -= 2;
        } else {
          bitBuffer |= (data[p++]! | 0) << bitCount;
          bitCount += 8;
          blockRemaining--;
        }
      }
      if (bitCount < codeSize) break;

      const code = bitBuffer & codeMask;
      bitBuffer >>>= codeSize;
      bitCount -= codeSize;

      if (firstCode) {
        firstCode = false;
        if (code !== clearCode) {
          // A few real-world GIFs omit the initial clear code.  Do not let a
          // reused reader dictionary leak entries from the previous frame.
          table.fill(0);
        }
      }
      if (code === clearCode) {
        nextCode = eoiCode + 1;
        codeSize = (minCodeSize + 1) | 0;
        codeMask = (1 << codeSize) - 1;
        previousCode = -1;
        continue;
      }
      if (code === eoiCode) break;

      const chaseCode = code < nextCode ? code : previousCode;
      if (chaseCode < 0) break;

      let chase = chaseCode;
      let stackLength = 0;
      while (chase > clearCode) {
        const entry = table[chase]! | 0;
        stack[stackLength++] = entry & 0xff;
        chase = entry >>> 8;
      }
      const first = chase & 0xff;
      emit(first);
      while (stackLength) {
        emit(stack[--stackLength]!);
      }
      if (chaseCode !== code) {
        emit(first);
      }

      if (previousCode >= 0 && nextCode < GIF.MAX_CODE) {
        table[nextCode++] = (previousCode << 8) | first;
        if (nextCode >= codeMask + 1 && codeSize < 12) {
          codeSize++;
          codeMask = (codeMask << 1) | 1;
        }
      }
      previousCode = code;
    }
  }

  private decodeAndBlitCompatibleFrame(
    frameNum: number,
    pixels: GifPixelBuffer,
    order: "rgba" | "bgra",
  ): void {
    let typedPixels: Uint8Array | null = null;
    if (pixels instanceof Uint8Array) {
      typedPixels = pixels;
    } else if (pixels instanceof Uint8ClampedArray) {
      typedPixels = new Uint8Array(
        pixels.buffer,
        pixels.byteOffset,
        pixels.byteLength,
      );
    }

    if (
      typedPixels &&
      (typedPixels.byteOffset & 3) === 0 &&
      (typedPixels.byteLength & 3) === 0
    ) {
      this.decodeAndBlitFrame32(frameNum, typedPixels, order);
      return;
    }

    const compatible = Uint8Array.from(pixels);
    this.decodeAndBlitFrame32(frameNum, compatible, order);
    for (let i = 0; i < compatible.length; i++) {
      pixels[i] = compatible[i]!;
    }
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
        colors[i] = pal32[indices[i]!]! >>> 0;
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
              colors[out++] = pal32[index]! >>> 0;
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
              colors[out++] = pal32[index]! >>> 0;
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
    _codeStream: Uint8Array,
    _dataOffset: number,
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
        // Fast path: no transparency checks while writing pixels.
          while (true) {
            // Fill bit buffer to have at least codeSize bits
            while (bitCount < codeSize && q < bytes.length) {
              if (q + 1 < bytes.length) {
                bits |=
                  ((bytes[q]! | 0) | ((bytes[q + 1]! | 0) << 8)) << bitCount;
                bitCount += 16;
                q += 2;
              } else {
                bits |= (bytes[q++]! | 0) << bitCount;
                bitCount += 8;
              }
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
              out32[dst32] = pal32[b]! >>> 0;
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
                outFirst = this.firstByte[prevCode]! | 0; // O(1) instead of chasing
                stack[sp++] = outFirst;
                cur = prevCode;
              } else {
                outFirst = this.firstByte[cur]! | 0; // O(1) instead of chasing
              }
              // unwind sequence
              while (cur >= CLEAR) {
                const entry = table[cur]! | 0;
                stack[sp++] = entry & 0xff;
                cur = entry >>> 8;
              }
              // Write first base - always write (no transparency check)
              const base = cur & 0xff;
              out32[dst32] = pal32[base]! >>> 0;
              dst32++;
              if (--xleft === 0) {
                dst32 += rowStride32;
                xleft = fw;
              }
              // Write stack backwards - always write (no transparency check)
              while (sp) {
                const b = stack[--sp]! & 0xff;
                out32[dst32] = pal32[b]! >>> 0;
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
              this.firstByte[nextCode] = this.firstByte[prevCode]!; // O(1) instead of chasing
              nextCode++;
              if (nextCode >= codeMask + 1 && codeSize < 12) {
                codeSize++;
                codeMask = (codeMask << 1) | 1;
              }
            }

            prevCode = code;
          }
      } else {
        // HAS TRANSPARENCY: Check each pixel
        while (true) {
          // Fill bit buffer to have at least codeSize bits
          while (bitCount < codeSize && q < bytes.length) {
            if (q + 1 < bytes.length) {
              bits |= ((bytes[q]! | 0) | ((bytes[q + 1]! | 0) << 8)) << bitCount;
              bitCount += 16;
              q += 2;
            } else {
              bits |= (bytes[q++]! | 0) << bitCount;
              bitCount += 8;
            }
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
              out32[dst32] = pal32[b]! >>> 0;
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
              outFirst = this.firstByte[prevCode]! | 0; // O(1) instead of chasing
              stack[sp++] = outFirst;
              cur = prevCode;
            } else {
              outFirst = this.firstByte[cur]! | 0; // O(1) instead of chasing
            }
            // unwind sequence
            while (cur >= CLEAR) {
              const entry = table[cur]! | 0;
              stack[sp++] = entry & 0xff;
              cur = entry >>> 8;
            }
            // Write first base - transparent pixels leave the caller's buffer as-is.
            const base = cur & 0xff;
            if (base !== transparentIndex) {
              out32[dst32] = pal32[base]! >>> 0;
            }
            dst32++;
            if (--xleft === 0) {
              dst32 += rowStride32;
              xleft = fw;
            }
            // Write stack backwards - transparent pixels leave the caller's buffer as-is.
            while (sp) {
              const b = stack[--sp]! & 0xff;
              if (b !== transparentIndex) {
                out32[dst32] = pal32[b]! >>> 0;
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
            this.firstByte[nextCode] = this.firstByte[prevCode]!; // O(1) instead of chasing
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
      // First decode all pixels into a job-local temporary buffer.
      const frameSize = fw * fh;
      const framePixels = new Uint8Array(frameSize);

      let pixelIndex = 0;

      // Decode all LZW symbols into linear pixel array
      while (true) {
        // Fill bit buffer
        while (bitCount < codeSize && q < bytes.length) {
          if (q + 1 < bytes.length) {
            bits |= ((bytes[q]! | 0) | ((bytes[q + 1]! | 0) << 8)) << bitCount;
            bitCount += 16;
            q += 2;
          } else {
            bits |= (bytes[q++]! | 0) << bitCount;
            bitCount += 8;
          }
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
            outFirst = this.firstByte[prevCode]! | 0; // O(1) instead of chasing
            stack[sp++] = outFirst;
            cur = prevCode;
          } else {
            outFirst = this.firstByte[cur]! | 0; // O(1) instead of chasing
          }
          while (cur >= CLEAR) {
            const entry = table[cur]! | 0;
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
            framePixels[pixelIndex++] = stack[--sp]! & 0xff;
          }
        }

        if (prevCode !== null && nextCode < GIF.MAX_CODE) {
          table[nextCode] = ((prevCode & 0xfff) << 8) | (outFirst & 0xff);
          this.firstByte[nextCode] = this.firstByte[prevCode]!; // O(1) instead of chasing
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
            const b = framePixels[pixelIndex++]! & 0xff;
            if (b !== transparentIndex) {
              out32[dst32] = pal32[b]! >>> 0;
            }
            dst32++;
          }
        }
      }
    }

    // Done
  }

}
