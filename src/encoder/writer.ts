import { PaletteRGB } from "../types";
import { GIF } from "../constants/gif";
import { checkPalette, log2Pow2 } from "../utils/palette";

function writeImageData(
  buf: Uint8Array,
  p0: number,
  minCodeSize: number,
  indexStream: Uint8Array | number[],
  colorCount: number
): number {
  // Inline-imported from index.ts GifWriterOutputLZWCodeStream_fast for now
  // To keep PR small, we reuse a local implementation here.
  let p = p0;
  buf[p++] = minCodeSize & 0xff;
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
  const keys = new Int32Array(CAP);
  const vals = new Int16Array(CAP);
  const gen = new Int32Array(CAP);
  let EPOCH = 1;

  function tableReset() {
    gen.fill(0);
    EPOCH = 1;
  }
  function tableGet(key: number): number {
    let i = key & (CAP - 1);
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
      if (++subLen === 255) {
        buf[subLenPos] = 255;
        subLenPos = p++;
        subLen = 0;
      }
    }
  }

  tableReset();
  emit(CLEAR);
  const n = (indexStream as any).length | 0;
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
    emit(ib);
    if (nextCode === GIF.MAX_CODE) {
      emit(CLEAR);
      nextCode = EOI + 1;
      codeSize = (minCodeSize + 1) | 0;
      codeMask = (1 << codeSize) - 1;
      tableReset();
    } else {
      if (minCodeSize === 1) {
        tableSet(key, nextCode++);
        if (nextCode > codeMask && codeSize < 12) {
          codeSize++;
          codeMask = (1 << codeSize) - 1;
        }
      } else {
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
  if (bitCount > 0) {
    buf[p++] = bits & 0xff;
    if (++subLen === 255) {
      buf[subLenPos] = 255;
      subLenPos = p++;
      subLen = 0;
    }
  }
  buf[subLenPos] = subLen & 0xff;
  if (subLen > 0) buf[p++] = 0;
  return p;
}

export class GifWriter {
  private p = 0;
  private ended = false;
  private loopCount: number | null;
  private globalPalette: PaletteRGB | null;
  private background = 0;
  private globalColorCount = 0;

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
      const pow = log2Pow2(n);
      gpPow2Bits = (pow - 1) & 7;
      if (go.background !== undefined) {
        this.background = go.background | 0;
        if (this.background < 0 || this.background >= n)
          throw new Error("Background index out of range.");
      }
    }
    this.buf[this.p++] = this.width & 0xff;
    this.buf[this.p++] = (this.width >> 8) & 0xff;
    this.buf[this.p++] = this.height & 0xff;
    this.buf[this.p++] = (this.height >> 8) & 0xff;
    const gctFlag = this.globalPalette !== null ? 0x80 : 0x00;
    this.buf[this.p++] = gctFlag | gpPow2Bits;
    this.buf[this.p++] = this.background & 0xff;
    this.buf[this.p++] = 0;
    if (this.globalPalette !== null) {
      for (let i = 0; i < this.globalColorCount; i++) {
        const rgb = (this.globalPalette[i] ?? 0) >>> 0;
        this.buf[this.p++] = (rgb >> 16) & 0xff;
        this.buf[this.p++] = (rgb >> 8) & 0xff;
        this.buf[this.p++] = rgb & 0xff;
      }
    }
    if (this.loopCount !== null) {
      const lc = this.loopCount | 0;
      if (lc < 0 || lc > 65535) throw new Error("Loop count invalid.");
      this.buf[this.p++] = GIF.EXT;
      this.buf[this.p++] = GIF.APPLICATION;
      this.buf[this.p++] = GIF.NETSCAPE_LEN;
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
    const minCodeSize = log2Pow2(numColors);
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
    this.buf[this.p++] = GIF.EXT;
    this.buf[this.p++] = GIF.GCE;
    this.buf[this.p++] = 4;
    this.buf[this.p++] = (disposal << 2) | (useTrans ? 1 : 0);
    this.buf[this.p++] = delay & 0xff;
    this.buf[this.p++] = (delay >> 8) & 0xff;
    this.buf[this.p++] = transparentIndex & 0xff;
    this.buf[this.p++] = 0;
    this.buf[this.p++] = GIF.IMG;
    this.buf[this.p++] = x & 0xff;
    this.buf[this.p++] = (x >> 8) & 0xff;
    this.buf[this.p++] = y & 0xff;
    this.buf[this.p++] = (y >> 8) & 0xff;
    this.buf[this.p++] = w & 0xff;
    this.buf[this.p++] = (w >> 8) & 0xff;
    this.buf[this.p++] = h & 0xff;
    this.buf[this.p++] = (h >> 8) & 0xff;
    this.buf[this.p++] = usingLocal ? 0x80 | lctSizeBits : 0x00;
    if (usingLocal) {
      for (let i = 0; i < numColors; i++) {
        const rgb = (palette[i] ?? 0) >>> 0;
        this.buf[this.p++] = (rgb >> 16) & 0xff;
        this.buf[this.p++] = (rgb >> 8) & 0xff;
        this.buf[this.p++] = rgb & 0xff;
      }
    }
    const p0 = this.p;
    this.p = writeImageData(
      this.buf,
      this.p,
      minCodeSize,
      indexedPixels,
      numColors
    );
    if (!usingLocal && this.globalPalette !== null) {
      // nothing extra
    }
    return this.p - p0;
  }

  end(): number {
    if (!this.ended) {
      this.buf[this.p++] = 0x3b;
      this.ended = true;
    }
    return this.p;
  }
}
