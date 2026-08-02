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

Results below are medians from 200 samples after thirty warmups on an Apple M3
Pro with Node.js 22.17.1, measured on the current optimized tree.

| Implementation | Median | Speedup | Bytes | PSNR |
| --- | ---: | ---: | ---: | ---: |
| image-q rgbquant + omggif balanced LZW | 99.069 ms | 1.00× | 39,350 | 31.84 dB |
| wtfgif quality/global + literal LZW (default) | 0.752 ms | **131.72×** | 149,601 | 34.12 dB |

This is one practical adaptive global-palette pipeline. The implementations do
not choose identical pixels, so the table reports source-relative PSNR and
output bytes alongside speed. The wtfgif palette is higher quality on this
fixture while remaining 131.72× faster. Literal LZW is lossless for the
indexed pixels, so the speedup does not come from lowering GIF pixel quality.

The quality path uses a weighted 4-bit-per-channel histogram for workloads up
to one million pixels and the full 5-bit-per-channel histogram above that
threshold. This is an internal memory/throughput optimization, not a user
selectable quality mode; both branches use adaptive palettes and the same
literal GIF writer.

The same source with every pixel treated as opaque (`BENCH_ALPHA_THRESHOLD=0`)
took 120.423 ms for image-q + omggif and 0.969 ms for wtfgif: **124.31×**,
with 33.91 dB PSNR. This is the normal full-color, no-transparent-pixels case.

For the no-cache contract:

```bash
BENCH_ITERATIONS=31 node scripts/bench-cold-rgba.mjs
```

That run starts a fresh Node process for every sample and includes fixture
loading, dynamic imports, Wasm initialization, palette creation, pixel mapping,
and GIF compression. The current 31-sample median is 151.523 ms for the full
entry versus 8.137 ms for wtfgif (18.90×). The encode-only entry
(`npm run bench:rgba:cold:encode`) measured 156.294 ms versus 7.390 ms
(21.15×). This includes process startup and Wasm initialization and is not the
initialized hot-path contract.

Run the optional larger synthetic stress workload with:

```bash
npm run bench:rgba:stress
```

For a stress-only receipt, use `BENCH_RGBA_FIXTURE=stress`. Five initialized
samples after two warmups took 7,435.153 ms with image-q + omggif and 11.739 ms
with wtfgif: **633.37×**, with 26.46 dB PSNR and 2,973,381 output bytes. The
two-sample zero-warmup run is intentionally not used here because the large
image-q allocation makes it noisy. A three-process strict-cold encode-only
stress run measured **163.29×** (7,270.549 ms vs 44.526 ms).

## Specialized: already-indexed frames

```bash
BENCH_ITERATIONS=300 npm run bench:encode
```

This is the direct `GifWriter` contract: 12 full 128×128 frames, a normal
256-color global palette, typed output, and wtfgif `compression: "fast"`.

| Implementation | Median | Bytes |
| --- | ---: | ---: |
| omggif | 17.565 ms | 163,797 |
| wtfgif | 0.093 ms | 224,001 |
| **Speedup** | **188.01×** | **1.37× baseline** |

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
| GIGACHAD | 198 × 128×128 | 32.583 ms | 17.165 ms | **1.90×** |
| tenor | 16 × 498×498 | 38.804 ms | 9.539 ms | **4.07×** |

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
