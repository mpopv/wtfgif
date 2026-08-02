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

Results below are medians from 100 samples after twenty warmups on an Apple M3
Pro with Node.js 22.17.1, measured on the current optimized tree.

| Implementation | Median | Speedup | Bytes | PSNR |
| --- | ---: | ---: | ---: | ---: |
| image-q rgbquant + omggif balanced LZW | 100.907 ms | 1.00× | 39,350 | 31.84 dB |
| wtfgif quality/global + literal LZW (default) | 0.491 ms | **205.41×** | 149,601 | 34.12 dB |

This is one practical adaptive global-palette pipeline. The implementations do
not choose identical pixels, so the table reports source-relative PSNR and
output bytes alongside speed. The wtfgif palette is higher quality on this
fixture while remaining 205.41× faster. Literal LZW is lossless for the
indexed pixels, so the speedup does not come from lowering GIF pixel quality.

The quality path uses a weighted 4-bit-per-channel histogram for ordinary
noisy/photo-like large workloads and the full 5-bit-per-channel histogram for
smooth ramps. A small local-variation sample chooses the precision before the
mandatory full scan. This is an internal memory/throughput optimization, not a
user-selectable quality mode: both branches use adaptive palettes, and smooth
gradients use the finer histogram, with a compact 4-bit parent-cell lookup
when the dense color set does not fit in the smaller table.

The same source with every pixel treated as opaque (`BENCH_ALPHA_THRESHOLD=0`)
took 109.066 ms for image-q + omggif and 0.637 ms for wtfgif: **171.20×**,
with 33.91 dB PSNR. This is the normal full-color, no-transparent-pixels case.

The larger stress workload (ten synthetic 512×512 RGBA frames) measured
6,692.028 ms for image-q + omggif and 5.314 ms for wtfgif: **1,259.25×**,
with 2,973,381 output bytes and 26.12 dB PSNR versus 24.26 dB for the
baseline. This is still arbitrary RGBA input: the palette is unknown, every
pixel is scanned, and every indexed pixel is emitted into a valid GIF.
The 1,300× ratio is a workload-size effect, not a cache shortcut: both sides
read all 2,621,440 source pixels, while wtfgif keeps its histogram, parent-cell
lookup, and literal writer linear in the input size.

For the no-cache contract:

```bash
BENCH_ITERATIONS=10 node scripts/bench-cold-rgba.mjs
```

That run starts a fresh Node process for every sample and includes fixture
loading, dynamic imports, Wasm initialization, palette creation, pixel mapping,
and GIF compression. The current ten-process median is 148.428 ms for the
full entry versus 7.274 ms for wtfgif (20.41×). This includes process startup
and Wasm initialization and is not the initialized hot-path contract; initialize
Wasm during page or worker startup for the hot measurements. A ten-process
encode-only run measured 149.346 ms versus 6.280 ms (**23.78×**).

Run the optional larger synthetic stress workload with:

```bash
npm run bench:rgba:stress
```

For a stress-only receipt, use `BENCH_RGBA_FIXTURE=stress`. Three initialized
samples after one warmup took 6,692.028 ms with image-q + omggif and 5.314 ms
with wtfgif: **1,259.25×**, with 26.12 dB PSNR and 2,973,381 output bytes.
The large image-q allocation makes this workload noisy, so use several samples
and report the median.

## Specialized: already-indexed frames

```bash
BENCH_ITERATIONS=300 BENCH_WARMUP_ITERATIONS=30 npm run bench:encode
```

This is the direct `GifWriter` contract: 12 full 128×128 frames, a normal
256-color global palette, typed output, and wtfgif `compression: "fast"`.

| Implementation | Median | Bytes |
| --- | ---: | ---: |
| omggif | 16.672 ms | 163,797 |
| wtfgif | 0.075 ms | 224,001 |
| **Speedup** | **223.16×** | **1.37× baseline** |

Both outputs are decoded before timing and must produce exactly the same RGBA
pixels. This is a real 100× result, but it applies only after palette creation
and pixel indexing have already happened.

## Decode

```bash
BENCH_GIF_FILTER=GIGACHAD BENCH_ITERATIONS=100 npm run bench:decode
BENCH_GIF_FILTER=tenor BENCH_ITERATIONS=50 npm run bench:decode
```

Reader construction, GIF parsing, and decoding every composited RGBA frame are
inside each sample. One caller-owned RGBA buffer is reused, matching ordinary
omggif usage. Every final byte must match omggif before timing.

| Fixture | Shape | omggif | wtfgif | Speedup |
| --- | ---: | ---: | ---: | ---: |
| GIGACHAD | 198 × 128×128 | 30.753 ms | 15.113 ms | **2.03×** |
| tenor | 16 × 498×498 | 36.351 ms | 8.199 ms | **4.43×** |

The targeted sweep ranged from **0.86×** on the tiny Clap fixture to
**4.43×** on tenor. Every decoded byte was
still checked for composited RGBA parity.

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

The 2.0 release removes initialization-time warmup primers. Benchmark setup
still initializes WebAssembly before timed samples, and no hidden synthetic
encode, decode, or remux work is performed during initialization.
