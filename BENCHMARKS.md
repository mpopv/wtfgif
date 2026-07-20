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
| omggif | 15.187 ms |
| wtfgif | 0.149 ms |
| **Speedup** | **102.18x** |

The benchmark decodes both outputs with omggif and requires exact RGBA equality
before timing. The omggif and balanced wtfgif outputs are 163,797 bytes. Fast
wtfgif output is 224,001 bytes, or 1.37x larger.

Use `BENCH_COMPRESSION=balanced` to measure normal LZW compression, or
`BENCH_COLOR_COUNTS=4,16,256` to run non-default palette sizes.

## Decode

```bash
BENCH_GIF_FILTER=GIGACHAD BENCH_ITERATIONS=100 npm run bench:decode
BENCH_GIF_FILTER=tenor BENCH_ITERATIONS=50 npm run bench:decode
```

| Fixture | Shape | omggif | wtfgif | Speedup |
| --- | ---: | ---: | ---: | ---: |
| GIGACHAD | 198 × 128×128 | 28.977 ms | 16.802 ms | **1.72x** |
| tenor | 16 × 498×498 | 33.967 ms | 7.642 ms | **4.44x** |

The decoder reuses one caller-owned RGBA buffer and invokes
`decodeAndBlitFrameRGBA()` for every frame, exactly like normal omggif usage.
Before timing, the complete final RGBA buffer must match omggif byte-for-byte.

## Reproducing results

Times are medians from the reported sample count on the development Mac with
Node.js 22. Microbenchmarks vary with hardware, power state, Node version, and
background load. Speedup ratios are more useful than raw milliseconds, but
neither should be treated as a universal result.
