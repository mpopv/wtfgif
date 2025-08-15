"use strict";
var wtfgif = (() => {
  var __defProp = Object.defineProperty;
  var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
  var __getOwnPropNames = Object.getOwnPropertyNames;
  var __hasOwnProp = Object.prototype.hasOwnProperty;
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
  var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

  // wtfgif.ts
  var wtfgif_exports = {};
  __export(wtfgif_exports, {
    GifReader: () => GifReader,
    GifWriter: () => GifWriter
  });
  var moduleReusableFramePixels = new Uint8Array(2048 * 2048);
  var moduleFramePixelsInUse = false;
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
  var GifReader = class {
    constructor(buf) {
      this.buf = buf;
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
    // Reusable decoder tables
    decTable = new Int32Array(4096 /* MAX_CODE */);
    // prefix<<8 | suffix
    stack = new Uint8Array(4096 /* MAX_CODE */);
    // for sequence unwind
    firstByte = new Int16Array(4096 /* MAX_CODE */);
    // -1 means unknown, tracks first symbol for O(1) lookup
    // Cache for Uint32 view of the destination pixel buffer
    out32Cache = /* @__PURE__ */ new WeakMap();
    get width() {
      return this.width_;
    }
    get height() {
      return this.height_;
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
              if (b !== transparentIndex) out32[dst32] = pal32[b] >>> 0;
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
              if (base !== transparentIndex) out32[dst32] = pal32[base] >>> 0;
              dst32++;
              if (--xleft === 0) {
                dst32 += rowStride32;
                xleft = fw;
              }
              while (sp) {
                const b = stack[--sp] & 255;
                if (b !== transparentIndex) out32[dst32] = pal32[b] >>> 0;
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
              if (!hasTrans) {
                out32[dst32] = pal32[b] >>> 0;
              } else {
                if (b !== transparentIndex) out32[dst32] = pal32[b] >>> 0;
              }
              dst32++;
            }
          }
        }
        if (framePixels === moduleReusableFramePixels.subarray(0, frameSize)) {
          moduleFramePixelsInUse = false;
        }
      }
    }
  };
  (function() {
    if (typeof window !== "undefined") {
      window.wtfgif = { GifWriter, GifReader };
    } else if (typeof globalThis !== "undefined") {
      globalThis.wtfgif = { GifWriter, GifReader };
    }
  })();
  return __toCommonJS(wtfgif_exports);
})();
