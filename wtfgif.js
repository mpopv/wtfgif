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
Object.defineProperty(exports, "__esModule", { value: true });
exports.GifReader = exports.GifWriter = void 0;
/* ===== Module-level reusable arrays ===== */
// Reuse these across all instances to avoid allocations (since we never nest calls)
const moduleReusableFramePixels = new Uint8Array(2048 * 2048); // Reasonable max for most GIFs
let moduleFramePixelsInUse = false;
/* ===== Helper / small utilities ===== */
function assertPow2(n) {
    return n >= 2 && n <= 256 && (n & (n - 1)) === 0;
}
function log2Pow2(n) {
    /* n is power-of-two (2..256) */ return 31 - Math.clz32(n);
}
function checkPalette(pal) {
    const n = pal.length >>> 0;
    if (!assertPow2(n))
        throw new Error("Invalid palette size (must be power of 2, 2..256).");
    return n;
}
function concatSubBlocks(buf, offset) {
    const mcs = buf[offset] | 0; // min code size
    let q = (offset + 1) | 0; // first sub-block size byte
    let total = 0;
    // first pass: measure
    while (true) {
        const len = buf[q++] | 0;
        if (len === 0)
            break;
        total += len;
        q += len;
    }
    // second pass: copy
    const out = new Uint8Array(total);
    q = (offset + 1) | 0;
    let w = 0;
    while (true) {
        const len = buf[q++] | 0;
        if (len === 0)
            break;
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
function buildPal32(buf, paletteOffset, paletteSize, order, transparentIndex = null) {
    const pal32 = new Uint32Array(256); // up to 256 entries
    const limit = Math.min(paletteSize, 256);
    if (order === "rgba") {
        for (let i = 0; i < limit; i++) {
            const r = buf[paletteOffset + i * 3] | 0;
            const g = buf[paletteOffset + i * 3 + 1] | 0;
            const b = buf[paletteOffset + i * 3 + 2] | 0;
            // Set alpha to 0 for transparent index to avoid unnecessary writes
            const alpha = (transparentIndex !== null && i === transparentIndex) ? 0 : 255;
            pal32[i] = (alpha << 24) | (b << 16) | (g << 8) | r;
        }
    }
    else {
        // bgra
        for (let i = 0; i < limit; i++) {
            const r = buf[paletteOffset + i * 3] | 0;
            const g = buf[paletteOffset + i * 3 + 1] | 0;
            const b = buf[paletteOffset + i * 3 + 2] | 0;
            // Set alpha to 0 for transparent index to avoid unnecessary writes
            const alpha = (transparentIndex !== null && i === transparentIndex) ? 0 : 255;
            pal32[i] = (alpha << 24) | (r << 16) | (g << 8) | b;
        }
    }
    // Others remain 0; caller should ensure indices are valid.
    return pal32;
}
/* ====== Writer (Encoder) ====== */
class GifWriter {
    constructor(buf, width, height, gopts) {
        this.buf = buf;
        this.width = width;
        this.height = height;
        this.p = 0;
        this.ended = false;
        this.background = 0;
        const go = gopts ?? {};
        this.loopCount = go.loop === undefined ? null : go.loop;
        this.globalPalette = go.palette === undefined ? null : go.palette;
        if (width <= 0 || height <= 0 || width > 65535 || height > 65535)
            throw new Error("Width/Height invalid.");
        /* Header: GIF89a */
        this.buf[this.p++] = 71 /* GIF.G */;
        this.buf[this.p++] = 73 /* GIF.I */;
        this.buf[this.p++] = 70 /* GIF.F */;
        this.buf[this.p++] = 56 /* GIF._8 */;
        this.buf[this.p++] = 57 /* GIF._9 */;
        this.buf[this.p++] = 97 /* GIF.A */;
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
            if (lc < 0 || lc > 65535)
                throw new Error("Loop count invalid.");
            this.buf[this.p++] = 33 /* GIF.EXT */;
            this.buf[this.p++] = 255 /* GIF.APPLICATION */;
            this.buf[this.p++] = 11 /* GIF.NETSCAPE_LEN */;
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
    addFrame(x, y, w, h, indexedPixels, opts) {
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
        let palette = o.palette;
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
        if (disposal < 0 || disposal > 3)
            throw new Error("Disposal out of range.");
        let useTrans = false;
        let transparentIndex = 0;
        if (o.transparent !== undefined && o.transparent !== null) {
            useTrans = true;
            transparentIndex = o.transparent | 0;
            if (transparentIndex < 0 || transparentIndex >= numColors)
                throw new Error("Transparent color index out of range.");
        }
        // Graphics Control Extension (only when needed)
        if (disposal !== 0 || useTrans || delay !== 0) {
            this.buf[this.p++] = 33 /* GIF.EXT */;
            this.buf[this.p++] = 249 /* GIF.GCE */;
            this.buf[this.p++] = 4; // block size
            this.buf[this.p++] = (disposal << 2) | (useTrans ? 1 : 0);
            this.buf[this.p++] = delay & 0xff;
            this.buf[this.p++] = (delay >> 8) & 0xff;
            this.buf[this.p++] = transparentIndex & 0xff;
            this.buf[this.p++] = 0; // terminator
        }
        // Image Descriptor
        this.buf[this.p++] = 44 /* GIF.IMG */;
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
        this.p = GifWriterOutputLZWCodeStream_fast(this.buf, this.p, minCodeSize < 2 ? 2 : minCodeSize, indexedPixels, numColors);
        return this.p;
    }
    end() {
        if (!this.ended) {
            this.buf[this.p++] = 59 /* GIF.TRAILER */;
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
}
exports.GifWriter = GifWriter;
/* ===== Encoder internals: typed-array hash + tight bit packer ===== */
/** Emit LZW code stream for indexed pixels. */
function GifWriterOutputLZWCodeStream_fast(buf, p0, minCodeSize, indexStream, colorCount) {
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
    const keys = (GifWriterOutputLZWCodeStream_fast._keys ?? (GifWriterOutputLZWCodeStream_fast._keys = new Int32Array(CAP)));
    const vals = (GifWriterOutputLZWCodeStream_fast._vals ?? (GifWriterOutputLZWCodeStream_fast._vals = new Int16Array(CAP)));
    const gen = (GifWriterOutputLZWCodeStream_fast._gen ?? (GifWriterOutputLZWCodeStream_fast._gen = new Int32Array(CAP)));
    let EPOCH = (GifWriterOutputLZWCodeStream_fast._epoch ?? 1) | 0;
    GifWriterOutputLZWCodeStream_fast._epoch = (EPOCH + 1) | 0;
    if (GifWriterOutputLZWCodeStream_fast._epoch <= 0) {
        gen.fill(0);
        GifWriterOutputLZWCodeStream_fast._epoch = 1;
        EPOCH = 1;
    }
    function tableReset() {
        // Instead of clearing arrays, bump generation.
        EPOCH = (EPOCH + 1) | 0;
        if (EPOCH <= 0) {
            gen.fill(0);
            EPOCH = 1;
        }
    }
    function tableGet(key) {
        // returns -1 if miss
        let i = key & (CAP - 1);
        // linear probing
        while (gen[i] === EPOCH) {
            if (keys[i] === key)
                return vals[i] | 0;
            i = (i + 1) & (CAP - 1);
        }
        return -1;
    }
    function tableSet(key, value) {
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
    function emit(code) {
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
    // Emit initial clear
    emit(CLEAR);
    const n = indexStream.length | 0;
    const mask = (colorCount - 1) | 0; // (palette size is power-of-two)
    let ib = indexStream[0] & mask;
    for (let i = 1; i < n; i++) {
        const k = indexStream[i] & mask;
        const key = (ib << 8) | k;
        const found = tableGet(key);
        if (found >= 0) {
            ib = found;
            continue;
        }
        // emit buffer
        emit(ib);
        if (nextCode === 4096 /* GIF.MAX_CODE */) {
            // Clear
            emit(CLEAR);
            nextCode = EOI + 1;
            codeSize = (minCodeSize + 1) | 0;
            codeMask = (1 << codeSize) - 1;
            tableReset();
        }
        else {
            if (nextCode >= codeMask + 1 && codeSize < 12) {
                codeSize++;
                codeMask = (codeMask << 1) | 1;
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
(function (GifWriterOutputLZWCodeStream_fast) {
})(GifWriterOutputLZWCodeStream_fast || (GifWriterOutputLZWCodeStream_fast = {}));
class GifReader {
    constructor(buf) {
        this.buf = buf;
        this.p = 0;
        this.globalPaletteOffset = null;
        this.globalPaletteSize = null;
        this.frames = [];
        this.loop_count = null;
        // Reusable decoder tables
        this.decTable = new Int32Array(4096 /* GIF.MAX_CODE */); // prefix<<8 | suffix
        this.stack = new Uint8Array(4096 /* GIF.MAX_CODE */); // for sequence unwind
        this.firstByte = new Int16Array(4096 /* GIF.MAX_CODE */); // -1 means unknown, tracks first symbol for O(1) lookup
        // Cache for Uint32 view of the destination pixel buffer
        this.out32Cache = new WeakMap();
        let p = 0;
        // Header: GIF87a / GIF89a
        if (buf[p++] !== 71 /* GIF.G */ ||
            buf[p++] !== 73 /* GIF.I */ ||
            buf[p++] !== 70 /* GIF.F */ ||
            buf[p++] !== 56 /* GIF._8 */ ||
            ((buf[p++] + 1) & 0xfd) !== 56 /* GIF._8 */ ||
            buf[p++] !== 97 /* GIF.A */) {
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
        let transparent_index = null;
        let disposal = 0;
        // Parse blocks
        let noEOF = true;
        while (noEOF && p < buf.length) {
            const block = buf[p++];
            switch (block) {
                case 33 /* GIF.EXT */: {
                    const label = buf[p++];
                    switch (label) {
                        case 255 /* GIF.APPLICATION */: {
                            // Possibly NETSCAPE2.0
                            if (buf[p] === 11 /* GIF.NETSCAPE_LEN */ &&
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
                                buf[p + 16] === 0x00) {
                                p += 14;
                                this.loop_count = (buf[p++] | (buf[p++] << 8)) >>> 0;
                                p++; // terminator
                            }
                            else {
                                // skip unknown app extension
                                p += 12;
                                while (true) {
                                    const size = buf[p++];
                                    if (!(size >= 0))
                                        throw new Error("Invalid block size");
                                    if (size === 0)
                                        break;
                                    p += size;
                                }
                            }
                            break;
                        }
                        case 249 /* GIF.GCE */: {
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
                        case 1 /* GIF.PLAINTEXT */:
                        case 254 /* GIF.COMMENT */: {
                            while (true) {
                                const size = buf[p++];
                                if (!(size >= 0))
                                    throw new Error("Invalid block size");
                                if (size === 0)
                                    break;
                                p += size;
                            }
                            break;
                        }
                        default:
                            throw new Error("Unknown graphic control label: 0x" +
                                label.toString(16));
                    }
                    break;
                }
                case 44 /* GIF.IMG */: {
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
                        if (!(size >= 0))
                            throw new Error("Invalid block size");
                        if (size === 0)
                            break;
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
                        pal32bgra
                    });
                    // Reset GCE state for next frame
                    delay = 0;
                    transparent_index = null;
                    disposal = 0;
                    break;
                }
                case 59 /* GIF.TRAILER */:
                    noEOF = false;
                    break;
                default:
                    throw new Error("Unknown gif block: 0x" + block.toString(16));
            }
        }
    }
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
        // NEW: use prebuilt palettes directly - zero lookup, zero reallocation
        const pal32 = (order === "rgba" ? frame.pal32rgba : frame.pal32bgra);
        let trans = frame.transparent_index;
        if (trans === null)
            trans = 256; // sentinel; indexes are 0..255
        // Reuse a cached Uint32 view for this pixels buffer
        let out32 = this.out32Cache.get(pixels);
        if (!out32) {
            out32 = new Uint32Array(pixels.buffer, pixels.byteOffset, pixels.byteLength >>> 2);
            this.out32Cache.set(pixels, out32);
        }
        // Streaming decode directly to out32
        this.lzwDecodeToPixels(this.buf, frame.data_offset, out32, this.width_, frame, pal32, trans);
    }
    /* Optimized LZW decoder that streams symbols directly to destination pixels. */
    lzwDecodeToPixels(codeStream, dataOffset, out32, canvasWidth, frame, pal32, transparentIndex) {
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
        for (let i = 0; i < CLEAR; i++)
            this.firstByte[i] = i;
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
        let prevCode = null;
        // NEW: Split "no transparency" vs "has transparency" decode loops
        const hasTrans = (transparentIndex !== 256); // 256 is sentinel
        if (!frame.interlaced) {
            // Fast path for non-interlaced frames
            let xleft = fw;
            const rowStride32 = (canvasWidth - fw) >>> 0;
            let dst32 = ((fy * canvasWidth) + fx) >>> 0;
            if (!hasTrans) {
                // FAST PATH: No transparency - always write pixels
                while (true) {
                    // Fill bit buffer to have at least codeSize bits
                    while (bitCount < codeSize && q < bytes.length) {
                        bits |= (bytes[q++] | 0) << bitCount;
                        bitCount += 8;
                    }
                    if (bitCount < codeSize)
                        break;
                    let code = bits & codeMask;
                    bits >>>= codeSize;
                    bitCount -= codeSize;
                    if (code === CLEAR) {
                        nextCode = EOI + 1;
                        codeSize = (minCodeSize + 1) | 0;
                        codeMask = (1 << codeSize) - 1;
                        prevCode = null;
                        // Initialize firstByte table for base codes
                        for (let i = 0; i < CLEAR; i++)
                            this.firstByte[i] = i;
                        continue;
                    }
                    else if (code === EOI) {
                        break;
                    }
                    // Decode sequence for 'code'
                    let outFirst;
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
                    }
                    else {
                        // Chase with stack
                        sp = 0;
                        if (cur >= nextCode) {
                            // KwKwK case
                            if (prevCode === null)
                                break;
                            outFirst = this.firstByte[prevCode] | 0; // O(1) instead of chasing
                            stack[sp++] = outFirst;
                            cur = prevCode;
                        }
                        else {
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
                    if (prevCode !== null && nextCode < 4096 /* GIF.MAX_CODE */) {
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
            else {
                // HAS TRANSPARENCY: Check each pixel
                while (true) {
                    // Fill bit buffer to have at least codeSize bits
                    while (bitCount < codeSize && q < bytes.length) {
                        bits |= (bytes[q++] | 0) << bitCount;
                        bitCount += 8;
                    }
                    if (bitCount < codeSize)
                        break;
                    let code = bits & codeMask;
                    bits >>>= codeSize;
                    bitCount -= codeSize;
                    if (code === CLEAR) {
                        nextCode = EOI + 1;
                        codeSize = (minCodeSize + 1) | 0;
                        codeMask = (1 << codeSize) - 1;
                        prevCode = null;
                        // Initialize firstByte table for base codes
                        for (let i = 0; i < CLEAR; i++)
                            this.firstByte[i] = i;
                        continue;
                    }
                    else if (code === EOI) {
                        break;
                    }
                    // Decode sequence for 'code'
                    let outFirst;
                    let cur = code;
                    if (cur < CLEAR) {
                        // Single byte - check transparency
                        outFirst = cur;
                        const b = outFirst & 0xff;
                        if (b !== transparentIndex)
                            out32[dst32] = pal32[b] >>> 0;
                        dst32++;
                        if (--xleft === 0) {
                            dst32 += rowStride32;
                            xleft = fw;
                        }
                    }
                    else {
                        // Chase with stack
                        sp = 0;
                        if (cur >= nextCode) {
                            // KwKwK case
                            if (prevCode === null)
                                break;
                            outFirst = this.firstByte[prevCode] | 0; // O(1) instead of chasing
                            stack[sp++] = outFirst;
                            cur = prevCode;
                        }
                        else {
                            outFirst = this.firstByte[cur] | 0; // O(1) instead of chasing
                        }
                        // unwind sequence
                        while (cur >= CLEAR) {
                            const entry = table[cur] | 0;
                            stack[sp++] = entry & 0xff;
                            cur = entry >>> 8;
                        }
                        // Write first base - check transparency
                        const base = cur & 0xff;
                        if (base !== transparentIndex)
                            out32[dst32] = pal32[base] >>> 0;
                        dst32++;
                        if (--xleft === 0) {
                            dst32 += rowStride32;
                            xleft = fw;
                        }
                        // Write stack backwards - check transparency
                        while (sp) {
                            const b = stack[--sp] & 0xff;
                            if (b !== transparentIndex)
                                out32[dst32] = pal32[b] >>> 0;
                            dst32++;
                            if (--xleft === 0) {
                                dst32 += rowStride32;
                                xleft = fw;
                            }
                        }
                    }
                    // Add new table entry
                    if (prevCode !== null && nextCode < 4096 /* GIF.MAX_CODE */) {
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
        }
        else {
            // INTERLACED PATH: Use pass-loops with inline pixel positioning
            // First decode all pixels into a temporary buffer (reuse module-level array)
            const frameSize = fw * fh;
            let framePixels;
            if (!moduleFramePixelsInUse && frameSize <= moduleReusableFramePixels.length) {
                moduleFramePixelsInUse = true;
                framePixels = moduleReusableFramePixels.subarray(0, frameSize);
            }
            else {
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
                if (bitCount < codeSize)
                    break;
                let code = bits & codeMask;
                bits >>>= codeSize;
                bitCount -= codeSize;
                if (code === CLEAR) {
                    nextCode = EOI + 1;
                    codeSize = (minCodeSize + 1) | 0;
                    codeMask = (1 << codeSize) - 1;
                    prevCode = null;
                    // Initialize firstByte table for base codes
                    for (let i = 0; i < CLEAR; i++)
                        this.firstByte[i] = i;
                    continue;
                }
                else if (code === EOI) {
                    break;
                }
                let outFirst;
                let cur = code;
                if (cur < CLEAR) {
                    // Single byte
                    outFirst = cur;
                    if (pixelIndex < framePixels.length) {
                        framePixels[pixelIndex++] = outFirst & 0xff;
                    }
                }
                else {
                    // Chase with stack
                    sp = 0;
                    if (cur >= nextCode) {
                        if (prevCode === null)
                            break;
                        outFirst = this.firstByte[prevCode] | 0; // O(1) instead of chasing
                        stack[sp++] = outFirst;
                        cur = prevCode;
                    }
                    else {
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
                if (prevCode !== null && nextCode < 4096 /* GIF.MAX_CODE */) {
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
                }
                else if (pass === 2) {
                    yStart = 2;
                    yStride = 4;
                }
                else if (pass === 3) {
                    yStart = 1;
                    yStride = 2;
                }
                for (let yInPass = 0;; yInPass++) {
                    const row = fy + yStart + yInPass * yStride;
                    if (row >= fy + fh)
                        break;
                    let dst32 = (row * canvasWidth + fx) >>> 0;
                    // Emit exactly fw pixels on this row
                    for (let x = 0; x < fw && pixelIndex < framePixels.length; x++) {
                        const b = framePixels[pixelIndex++] & 0xff;
                        if (!hasTrans) {
                            // Fast path: always write
                            out32[dst32] = pal32[b] >>> 0;
                        }
                        else {
                            // Check transparency - but palette already has alpha=0 for transparent index
                            if (b !== transparentIndex)
                                out32[dst32] = pal32[b] >>> 0;
                        }
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
}
exports.GifReader = GifReader;
// Browser global export under wtfgif namespace
(function () {
    if (typeof window !== 'undefined') {
        window.wtfgif = { GifWriter, GifReader };
    }
    else if (typeof globalThis !== 'undefined') {
        globalThis.wtfgif = { GifWriter, GifReader };
    }
})();
