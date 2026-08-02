# Benchmarks

The headline benchmark measures the job an image-stitching app actually does:
ordinary decoded images in, GIF bytes out.

```bash
npm run bench
```

## Default: arbitrary RGBA images

The committed workload is eight real images from MakeEmoji. Each image was
aspect-fitted into a transparent 128×128 canvas, then stored as one contiguous
RGBA fixture at `test/rgba/makeemoji-128x128x8.rgba`.

The clock starts with that RGBA buffer. Both implementations must create a
palette, map every source pixel, and compress every frame. File loading, image
decoding, resizing, WebAssembly initialization, warmup, validation, and quality
measurement are outside timed samples.

Results below are medians from 120 samples after twenty warmups on an Apple M3
Pro with Node.js 22.17.1, measured on the current optimized tree.

| Implementation | Median | Speedup | Bytes | PSNR |
| --- | ---: | ---: | ---: | ---: |
| image-q rgbquant + omggif balanced LZW | 101.026 ms | 1.00× | 39,350 | 31.84 dB |
| wtfgif quality/global + literal LZW (default) | 0.451 ms | **224.23×** | 149,601 | 34.12 dB |

This is one practical adaptive global-palette pipeline. The implementations do
not choose identical pixels, so the table reports source-relative PSNR and
output bytes alongside speed. The wtfgif palette is higher quality on this
fixture while remaining 224.23× faster. Literal LZW is lossless for the
indexed pixels, so the speedup does not come from lowering GIF pixel quality.

The quality path uses a weighted 4-bit-per-channel histogram for ordinary
noisy/photo-like large workloads and the full 5-bit-per-channel histogram for
smooth ramps. A small local-variation sample chooses the precision before the
mandatory full scan. This is an internal memory/throughput optimization, not a
user-selectable quality mode: both branches use adaptive palettes, and smooth
gradients use the finer histogram, with a compact 4-bit parent-cell lookup
when the dense color set does not fit in the smaller table.

The same source with every pixel treated as opaque (`BENCH_ALPHA_THRESHOLD=0`)
took 110.562 ms for image-q + omggif and 0.500 ms for wtfgif: **221.01×**,
with 33.91 dB PSNR. This is the normal full-color, no-transparent-pixels case.

The larger stress workload (ten synthetic 512×512 RGBA frames) measured
6,675.854 ms for image-q + omggif and 5.292 ms for wtfgif: **1,261.58×**,
with 2,973,381 output bytes and 26.12 dB PSNR versus 24.26 dB for the
baseline. This is still arbitrary RGBA input: the palette is unknown, every
pixel is scanned, and every indexed pixel is emitted into a valid GIF.
The 1,261× ratio is a workload-size effect, not a cache shortcut: both sides
read all 2,621,440 source pixels, while wtfgif keeps its histogram, parent-cell
lookup, and literal writer linear in the input size.

For the no-cache contract:

```bash
BENCH_ITERATIONS=10 node scripts/bench-cold-rgba.mjs
```

That run starts a fresh Node process for every sample and includes fixture
loading, dynamic imports, Wasm initialization, palette creation, pixel mapping,
and GIF compression. The current ten-process median is 145.838 ms for the
full entry versus 8.717 ms for wtfgif (**16.73×**). This includes process
startup and Wasm initialization, including the bounded synthetic warmup, and
is not the initialized hot-path contract; initialize Wasm during page or worker
startup for the hot measurements. A ten-process encode-only run measured
155.849 ms versus 8.194 ms (**19.02×**).

Run the optional larger synthetic stress workload with:

```bash
npm run bench:rgba:stress
```

For a stress-only receipt, use `BENCH_RGBA_FIXTURE=stress`. Three initialized
samples after one warmup took 6,675.854 ms with image-q + omggif and 5.292 ms
with wtfgif: **1,261.58×**, with 26.12 dB PSNR and 2,973,381 output bytes.
The large image-q allocation makes this workload noisy, so use several samples
and report the median.

## Specialized: already-indexed frames

```bash
BENCH_ITERATIONS=200 BENCH_WARMUP_ITERATIONS=40 npm run bench:encode
```

This is the direct `GifWriter` contract: 12 full 128×128 frames, a normal
256-color global palette, typed output, and wtfgif `compression: "fast"`.

| Implementation | Median | Bytes |
| --- | ---: | ---: |
| omggif | 16.541 ms | 163,797 |
| wtfgif | 0.074 ms | 224,001 |
| **Speedup** | **222.58×** | **1.37× baseline** |

Both outputs are decoded before timing and must produce exactly the same RGBA
pixels. This is a real 100× result, but it applies only after palette creation
and pixel indexing have already happened.

The same typed-output sweep across 2, 4, 8, 16, 32, 64, 128, and 256-color
palettes measured a minimum of **110.57×** (32 colors) and a geometric mean of
**167×** in the latest 180-sample receipt. Low-color literal streams use a
dedicated fixed-width writer; all output still decodes to the same indexed
pixels.

## Decode

```bash
BENCH_ITERATIONS=50 BENCH_WARMUP_ITERATIONS=5 BENCH_TARGET_SAMPLE_MS=1 npm run bench:decode
```

Reader construction, GIF parsing, and decoding every composited RGBA frame are
inside each sample. One caller-owned RGBA buffer is reused, matching ordinary
omggif usage. Every final byte must match omggif before timing.

| Fixture | Shape | omggif | wtfgif | Speedup |
| --- | ---: | ---: | ---: | ---: |
| GIGACHAD | 198 × 128×128 | 31.341 ms | 16.569 ms | **1.89×** |
| tenor | 16 × 498×498 | 36.785 ms | 8.423 ms | **4.37×** |

The latest 50-sample all-fixture sweep ranged from **0.96×** on the tiny Clap
fixture to **4.37×** on tenor, with a **2.19× geometric mean**. Every decoded
byte was still checked for composited RGBA parity.

For large canvases with partial transparent rectangles, the drop-in reader can
decode only the frame rectangle into Wasm scratch storage and overlay it onto
the caller's canvas. This avoids copying the full canvas through Wasm on every
frame while preserving RGBA/BGRA pixels exactly; the path is covered by the
large-partial-frame parity test in `test/index.test.ts`.

The one-off `decodeGifFramesRgba` API has an additional direct-output path for
animations whose frames are all full-canvas and opaque. On the same tenor GIF,
50 samples measured 35.182 ms for omggif versus 7.069 ms for wtfgif (**4.98×**).
That optimization does not change the drop-in reader's frame-by-frame contract.

## Browser racer

The [live racer](https://mpopv.github.io/wtfgif/) uses the same eight-image
workload by default and also accepts multiple PNG, JPEG, or WebP uploads. It
keeps Encode, Decode, and the separate structural remux experiment in distinct
modes so a remux shortcut can never be mistaken for arbitrary-image encoding.

Browser results vary with device, browser, thermal state, and background load.
Speedup ratios are generally more useful than raw milliseconds, but neither is
universal.

The old process-startup diagnostic remains available as `npm run bench:cold`.
It measures a different contract and is intentionally not the headline race.

## Release notes

Initialization performs a bounded synthetic encode/decode warmup to pay Wasm
JIT and allocator setup before the first real operation. The timed hot-path
benchmarks still begin after initialization; strict-cold commands include this
setup by design and are reported separately.
