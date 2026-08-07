# Benchmarks

The headline benchmark measures the job an image-stitching app actually does:
ordinary decoded images in, GIF bytes out.

```bash
npm run bench
```

## Browser encoder race

```bash
npm run bench:race
```

This comparison uses the same eight real 128×128 MakeEmoji RGBA frames and an
alpha threshold of 179. Package code, the fixture fetch, and wtfgif's one-time
Wasm initialization happen before the clock. The timed boundary includes
palette creation, pixel mapping, GIF compression, and final byte assembly.

Each sample is the first and only encode in a fresh, cross-origin-isolated
Chrome process and fresh browser profile. There are no encode warmups, retained
palettes, retained outputs, or scratch-buffer reuse between samples. The six
encoders run in a rotating order to reduce thermal and ordering bias. Results
below are medians from 15 processes per encoder on an Apple M3 Pro in Google
Chrome 151.0.7922.77.

| Implementation | Version | Median | wtfgif advantage | Bytes | PSNR | Alpha match |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| **wtfgif** | current main | **1.545 ms** | — | 149,689 | 34.12 dB | 100% |
| gif.js | 0.2.0 | 95.865 ms | **62.05×** | 80,869 | 33.08 dB | 99.78% |
| image-q + omggif | 2.1.2 + 1.0.10 | 98.055 ms | **63.47×** | 39,350 | 31.84 dB | 100% |
| gif.js.optimized | 1.0.1 | 107.510 ms | **69.59×** | 80,304 | 32.20 dB | 99.76% |
| gifenc | 1.0.3 | 122.835 ms | **79.50×** | 39,101 | 34.54 dB | 100% |
| modern-gif | 2.1.0 | 131.525 ms | **85.13×** | 43,114 | 32.65 dB | 100% |

Every output must parse as an eight-frame 128×128 animation with exact 100 ms
delays before its sample is accepted. The validator composites all frames,
measures RGB PSNR on source-opaque pixels, and measures binary alpha agreement
over every pixel. Raw timings and interquartile values are committed in
[`benchmarks/encoder-race.json`](benchmarks/encoder-race.json); the README chart
is generated from that receipt by `scripts/render-encoder-race-chart.mjs`.

The adapters use each package's highest-color normal path:

- wtfgif, modern-gif, gifenc, and image-q + omggif build one adaptive palette
  across all eight frames. gifenc uses its higher-quality RGB565 mode.
- gif.js and gif.js.optimized use their highest-quality `quality: 1` setting,
  two workers, and their normal per-frame NeuQuant palettes. Their public API
  spawns workers from `render()`, so worker creation is part of their timed job.
- GIF has one-bit transparency. wtfgif, gifenc, modern-gif, and the omggif
  pipeline apply the same threshold exactly. gif.js and gif.js.optimized only
  expose color-key transparency; the adapter supplies a reserved key, and the
  measured 99.78%/99.76% alpha agreement records the resulting collisions.
- gifenc and modern-gif ignore typed-array byte offsets internally. The adapter
  therefore copies each contiguous fixture frame into its own correctly sized
  view inside the timed boundary. This preserves correct pixels instead of
  giving either package a broken-but-fast result.

The raw gif.js outputs contain their package's normal zero padding after the
GIF trailer. The benchmark reports those bytes as emitted and validates them
with omggif, which accepts that widely tolerated padding.

## Default: arbitrary RGBA images

The committed workload is eight real images from MakeEmoji. Each image was
aspect-fitted into a transparent 128×128 canvas, then stored as one contiguous
RGBA fixture at `test/rgba/makeemoji-128x128x8.rgba`.

The clock starts with that RGBA buffer. Both implementations must create a
palette, map every source pixel, and compress every frame. File loading, image
decoding, resizing, WebAssembly initialization, warmup, validation, and quality
measurement are outside timed samples.

Results below are medians from 200 samples after 30 timed-loop warmups on an
Apple M3 Pro with Node.js 22.23.2, measured on the current main checkpoint.

| Implementation | Median | Speedup | Bytes | PSNR |
| --- | ---: | ---: | ---: | ---: |
| image-q rgbquant + omggif balanced LZW | 94.302 ms | 1.00× | 39,350 | 31.84 dB |
| wtfgif quality/global + literal LZW | 0.315 ms | **298.94×** | 149,689 | 34.12 dB |

This is one practical adaptive global-palette pipeline. The implementations do
not choose identical pixels, so the table reports source-relative PSNR and
output bytes alongside speed. The wtfgif palette is higher quality on this
fixture while remaining 298.94× faster. Literal LZW is lossless for the
indexed pixels, so the speedup does not come from lowering GIF pixel quality.

The quality path uses a weighted 4-bit-per-channel histogram for ordinary
noisy/photo-like large workloads and the full 5-bit-per-channel histogram for
smooth ramps. A small local-variation sample chooses the precision before the
mandatory full scan. This is an internal memory/throughput optimization, not a
user-selectable quality mode: both branches use adaptive palettes, and smooth
gradients use the finer histogram, with a compact 4-bit parent-cell lookup
when the dense color set does not fit in the smaller table.

For ordinary-sized inputs (up to one million pixels), the alpha check is
fused into that histogram pass. Multi-megapixel inputs use a separate tight
alpha preflight because it is faster than branching on transparency during
scattered histogram updates.

The same source with every pixel treated as opaque (`BENCH_ALPHA_THRESHOLD=0`)
took 102.895 ms for image-q + omggif and 0.377 ms for wtfgif: **273.26×**.
wtfgif measured 33.91 dB versus 32.44 dB for the baseline. This is the normal
full-color, no-transparent-pixels case.

In a separate 10-sample run after three timed-loop warmups, the larger stress
workload (ten synthetic 512×512 RGBA frames) measured
6,358.285 ms for image-q + omggif and 4.249 ms for wtfgif: **1496.60×**,
with 2,973,381 output bytes and 26.19 dB PSNR versus 24.26 dB for the
baseline. This is still arbitrary RGBA input: the palette is unknown, every
pixel is scanned, and every indexed pixel is emitted into a valid GIF.
The 1496.60× ratio is a workload-size effect, not a cache shortcut: both sides
read all 2,621,440 source pixels, while wtfgif keeps its histogram, parent-cell
lookup, and literal writer linear in the input size.

For the no-warmup, first-real-encode contract:

```bash
BENCH_ITERATIONS=500 BENCH_INITIALIZED_FIRST=1 BENCH_PHASES=1 npm run bench:rgba:cold:encode
```

That run starts a fresh Node process for every sample, initializes Wasm, and
then times exactly one real encode with no warmup. Across 500 processes the
median was 123.448 ms for image-q + omggif versus 1.231 ms for wtfgif
(**100.26×**). There is no synthetic encode, retained source pixel, palette, or
output result in initialization. The parent-observed wall clock, which also
includes launching and shutting down Node, was 166.848 ms versus 28.209 ms
(**5.91×**). Median wtfgif phases were 0.180 ms to load the fixture, 0.392 ms
to import the package, 0.944 ms to initialize Wasm, and 1.232 ms for the first
encode.

Run the optional larger synthetic stress workload with:

```bash
npm run bench:rgba:stress
```

For a stress-only receipt, add `BENCH_COLD_RGBA_FIXTURE=stress`. Across five
fresh processes, the first encode after initialization took 6,375.615 ms with
image-q + omggif and 8.276 ms with wtfgif: **770.35×**. The complete process
wall clock was 6,447.298 ms versus 58.018 ms: **111.13×**. The wtfgif output
remains 2,973,381 bytes at 26.19 dB PSNR. The large image-q allocation makes this
workload noisy, so use several samples and report the median.

## Specialized: already-indexed frames

```bash
BENCH_ITERATIONS=100 BENCH_WARMUP_ITERATIONS=10 BENCH_TARGET_SAMPLE_MS=5 \
BENCH_COLOR_COUNTS=2,4,8,16,32,64,128,256 npm run bench:encode
```

This is the direct `GifWriter` contract: 12 full 128×128 frames, a normal
256-color global palette, typed output, and wtfgif `compression: "fast"`.

| Implementation | Median | Bytes |
| --- | ---: | ---: |
| omggif | 15.827 ms | 163,797 |
| wtfgif | 0.083 ms | 224,001 |
| **Speedup** | **190.56×** | **1.37× baseline** |

Both outputs are decoded before timing and must produce exactly the same RGBA
pixels. This is a real 100× result, but it applies only after palette creation
and pixel indexing have already happened.

The same typed-output sweep across 2, 4, 8, 16, 32, 64, 128, and 256-color
palettes measured a minimum of **157.57×** (2 colors) and a geometric mean of
**262.84×** in the latest 100-sample receipt. Palettes through 64 colors use
the fixed-width 8-bit literal writer; all output still decodes to the same
indexed pixels, though the packed stream can be smaller.

## Decode

```bash
BENCH_ITERATIONS=50 BENCH_WARMUP_ITERATIONS=8 BENCH_TARGET_SAMPLE_MS=5 npm run bench:decode
```

Reader construction, GIF parsing, and decoding every composited RGBA frame are
inside each sample. One caller-owned RGBA buffer is reused, matching ordinary
omggif usage. Every final byte must match omggif before timing.

| Fixture | Shape | omggif | wtfgif | Speedup |
| --- | ---: | ---: | ---: | ---: |
| GIGACHAD | 198 × 128×128 | 29.504 ms | 14.748 ms | **2.00×** |
| tenor | 16 × 498×498 | 34.488 ms | 6.607 ms | **5.22×** |

The latest all-fixture sweep ranged from **1.03×** on the tiny Clap fixture to
**5.22×** on tenor, with a **2.68× geometric mean**. Every decoded
byte was still checked for composited RGBA parity. For 8+ frame animations,
medium rectangles use the initialized reusable Wasm blitter after the reader
has a clear animation-shaped access pattern. Single-frame and sub-512-pixel
reads stay on JavaScript.

For large canvases with partial transparent rectangles, the drop-in reader can
decode only the frame rectangle into Wasm scratch storage and overlay it onto
the caller's canvas. This avoids copying the full canvas through Wasm on every
frame while preserving RGBA/BGRA pixels exactly; the path is covered by the
large-partial-frame parity test in `test/index.test.ts`.

Prepared Wasm playback keeps full-frame and delta streams in Wasm-owned
scratch views until `dispose()`, avoiding a second whole-animation copy through
the wasm-bindgen return layer. This changes storage ownership, not pixels; the
same parity tests cover RGBA and BGRA output.

The one-off `decodeGifFramesRgba` API has an additional direct-output path for
animations whose frames are all full-canvas and opaque. On the same tenor GIF,
20 fresh-process samples measured 40.699 ms for omggif versus 1.430 ms for
wtfgif (**28.45×**).
That optimization does not change the drop-in reader's frame-by-frame contract.

The old process-startup diagnostic remains available as `npm run bench:cold`.
It measures a different contract and is intentionally not the headline race.
