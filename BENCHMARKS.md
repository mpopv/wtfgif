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

Results below are medians from 15 samples after four warmups on an Apple M3 Pro
with Node.js 22.17.1.

| Profile | Implementation | Median | Speedup | Bytes | PSNR |
| --- | --- | ---: | ---: | ---: | ---: |
| Quality | image-q rgbquant + omggif balanced LZW | 93.062 ms | 1.00× | 39,350 | 31.84 dB |
| Quality | wtfgif quality/global + balanced LZW | 2.985 ms | **31.17×** | 45,730 | 32.71 dB |
| Turbo | RGB332 mapping + omggif balanced LZW | 5.299 ms | 1.00× | 30,098 | 19.55 dB |
| Turbo | wtfgif fast/global + literal LZW | 0.282 ms | **18.82×** | 149,601 | 19.55 dB |

Quality compares two practical adaptive global-palette pipelines. They do not
choose identical pixels, so the table reports source-relative PSNR and output
bytes alongside speed.

Turbo gives both encoders the identical RGB332 palette and indices. Their
decoded animations must match byte-for-byte. wtfgif's extra speed comes with a
4.97× larger file because literal LZW deliberately minimizes encoder work.

Run the optional larger synthetic stress workload with:

```bash
npm run bench:rgba:stress
```

## Specialized: already-indexed frames

```bash
BENCH_ITERATIONS=300 npm run bench:encode
```

This is the direct `GifWriter` contract: 12 full 128×128 frames, a normal
256-color global palette, typed output, and wtfgif `compression: "fast"`.

| Implementation | Median | Bytes |
| --- | ---: | ---: |
| omggif | 16.827 ms | 163,797 |
| wtfgif | 0.152 ms | 224,001 |
| **Speedup** | **110.37×** | **1.37× baseline** |

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
| GIGACHAD | 198 × 128×128 | 32.014 ms | 18.050 ms | **1.77×** |
| tenor | 16 × 498×498 | 37.028 ms | 8.434 ms | **4.39×** |

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
