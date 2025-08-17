import { PaletteRGB } from "../types";
import { GIF } from "../constants/gif";
import { log2Pow2, checkPalette } from "../utils/palette";
import { writeNetscapeLoopCount } from "../utils/netscape";

export class GifWriter {
  private p = 0;
  private ended = false;

  private loopCount: number | null;
  private globalPalette: PaletteRGB | null;
  private background = 0;
  private globalColorCount = 0;

  constructor(
    private buf: Uint8Array,
    width: number,
    height: number,
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

    this.buf[this.p++] = GIF.G;
    this.buf[this.p++] = GIF.I;
    this.buf[this.p++] = GIF.F;
    this.buf[this.p++] = GIF._8;
    this.buf[this.p++] = GIF._9;
    this.buf[this.p++] = GIF.A;

    let gpPow2Bits = 0;
    if (this.globalPalette !== null) {
      const n = checkPalette(this.globalPalette);
      this.globalColorCount = n;
      const pow = log2Pow2(n); // 1..8
      gpPow2Bits = (pow - 1) & 7; // 0..7 per spec

      if (go.background !== undefined) {
        this.background = go.background | 0;
        if (this.background < 0 || this.background >= n)
          throw new Error("Background index out of range.");
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
      for (let i = 0; i < this.globalColorCount; i++) {
        const rgb = (this.globalPalette[i] ?? 0) >>> 0;
        this.buf[this.p++] = (rgb >> 16) & 0xff;
        this.buf[this.p++] = (rgb >> 8) & 0xff;
        this.buf[this.p++] = rgb & 0xff;
      }
    }

    // Netscape loop count
    if (this.loopCount !== null) {
      const lc = this.loopCount | 0;
      if (lc < 0 || lc > 65535) throw new Error("Loop count invalid.");
      this.p = writeNetscapeLoopCount(this.buf, this.p, lc);
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
    if (indexedPixels.length < w * h)
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
      for (let i = 0; i < numColors; i++) {
        const rgb = (palette[i] ?? 0) >>> 0;
        this.buf[this.p++] = (rgb >> 16) & 0xff;
        this.buf[this.p++] = (rgb >> 8) & 0xff;
        this.buf[this.p++] = rgb & 0xff;
      }
    }

    this.p = GifWriterOutputLZWCodeStream_fast(
      this.buf,
      this.p,
      minCodeSize,
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
    const keys = (
      (GifWriterOutputLZWCodeStream_fast._keys ??= new Int32Array(CAP)) as Int32Array
    );
    const vals = (
      (GifWriterOutputLZWCodeStream_fast._vals ??= new Int16Array(CAP)) as Int16Array
    );
    const gen = (
      (GifWriterOutputLZWCodeStream_fast._gen ??= new Int32Array(CAP)) as Int32Array
    );
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

  const n = indexStream.length | 0;
  let ib = (indexStream[0] as number) | 0;
  if (ib >>> 0 >= colorCount) throw new Error("Pixel index out of range.");

  for (let i = 1; i < n; i++) {
    const k = (indexStream[i] as number) | 0;
    if (k >>> 0 >= colorCount) throw new Error("Pixel index out of range.");
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
      if (minCodeSize === 1) {
        // For 1-bit palettes, codes 0 and 1 use 2-bit code size. We should
        // only increase code size after adding a new dictionary entry.
        tableSet(key, nextCode++);
        if (nextCode > codeMask && codeSize < 12) {
          codeSize++;
          codeMask = (1 << codeSize) - 1;
        }
      } else {
        // For larger palettes, match omggif's timing by growing the code size
        // before inserting the entry that would overflow the current mask.
        if (nextCode >= codeMask + 1 && codeSize < 12) {
          codeSize++;
          codeMask = (1 << codeSize) - 1;
        }
        tableSet(key, nextCode++);
      }
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
  // Only write a terminating zero-length block if the last sub-block
  // actually contained data. Otherwise the zero-length at subLenPos is
  // itself the terminator (avoids emitting an extra empty block).
  if (subLen > 0) {
    buf[p++] = 0; // terminator
  }

  return p;
}
namespace GifWriterOutputLZWCodeStream_fast {
  export let _keys: Int32Array | undefined;
  export let _vals: Int16Array | undefined;
  export let _gen: Int32Array | undefined;
  export let _epoch: number | undefined;
}
