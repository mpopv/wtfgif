# Benchmarks

wtfgif benchmarks encode and decode separately against the same public
`GifWriter` and `GifReader` APIs from `omggif`.

WebAssembly initialization and warmup happen before timed samples, matching an
app that initializes wtfgif during page load. Reader construction and GIF
parsing remain inside every timed decode sample.

## Encode

```bash
BENCH_ITERATIONS=300 npm run bench:encode
```

The default encode benchmark is a normal 256-color animation: 12 full 128×128
indexed frames, a global palette, typed output, and `compression: "fast"`.

| Implementation | Median |
| --- | ---: |
| omggif | 17.839 ms |
| wtfgif | 0.166 ms |
| **Speedup** | **107.49x** |

The benchmark decodes both outputs with omggif and requires exact RGBA equality
before timing. The omggif and balanced wtfgif outputs are 163,797 bytes. Fast
wtfgif output is 224,001 bytes, or 1.37x larger.

Use `BENCH_COMPRESSION=balanced` to measure normal LZW compression, or
`BENCH_COLOR_COUNTS=4,16,256` to run non-default palette sizes.

## Arbitrary RGBA encode

```bash
BENCH_ITERATIONS=50 npm run bench:rgba
```

This benchmark starts with deterministic, full-color RGBA frames containing
more than 256 colors. Its omggif baseline includes the same mandatory RGB332
pixel mapping performed by wtfgif's `fast` quantizer. Wasm initialization,
fixture construction, output validation, and quality measurement remain
outside timed samples.

| Shape | Operation | Median | Speedup | Bytes | PSNR |
| --- | --- | ---: | ---: | ---: | ---: |
| 12 × 128×128 | omggif + RGB332 mapping | 15.417 ms | 1.00x | 128,505 | 21.90 dB |
| 12 × 128×128 | wtfgif fast/global | 0.501 ms | **30.80x** | 224,001 | 21.90 dB |
| 12 × 128×128 | wtfgif quality/global | 9.408 ms | — | 224,001 | 26.06 dB |
| 12 × 128×128 | wtfgif quality/local | 33.339 ms | — | 232,449 | 28.58 dB |
| 10 × 512×512 | omggif + RGB332 mapping | 159.008 ms | 1.00x | 1,231,192 | 21.88 dB |
| 10 × 512×512 | wtfgif fast/global | 6.554 ms | **24.26x** | 2,973,381 | 21.88 dB |
| 10 × 512×512 | wtfgif quality/global | 21.241 ms | — | 2,973,381 | 26.02 dB |
| 10 × 512×512 | wtfgif quality/local | 45.184 ms | — | 2,980,293 | 28.41 dB |

`fast` and `quality` describe palette generation, independently of
`compression`. Fast LZW produces larger output than balanced LZW. Local
palettes improve this fixture's color accuracy but repeat quantization and
palette tables for every frame. Quality rows deliberately omit a speedup
against the RGB332 baseline because their pixels are not equivalent.

## Decode

```bash
BENCH_GIF_FILTER=GIGACHAD BENCH_ITERATIONS=100 npm run bench:decode
BENCH_GIF_FILTER=tenor BENCH_ITERATIONS=50 npm run bench:decode
```

| Fixture | Shape | omggif | wtfgif | Speedup |
| --- | ---: | ---: | ---: | ---: |
| GIGACHAD | 198 × 128×128 | 31.204 ms | 18.632 ms | **1.67x** |
| tenor | 16 × 498×498 | 37.743 ms | 8.687 ms | **4.34x** |

The decoder reuses one caller-owned RGBA buffer and invokes
`decodeAndBlitFrameRGBA()` for every frame, exactly like normal omggif usage.
Before timing, the complete final RGBA buffer must match omggif byte-for-byte.

## Reproducing results

Times are medians from the reported sample count on the development Mac with
Node.js 22. Microbenchmarks vary with hardware, power state, Node version, and
background load. Speedup ratios are more useful than raw milliseconds, but
neither should be treated as a universal result.
