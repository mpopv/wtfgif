# Benchmarks

The default benchmark measures complete RGBA-to-GIF encoding across 10 real and
synthetic workloads:

```bash
npm run bench
```

## Representative corpus

Both implementations receive the same RGBA pixels and must discover a global
palette of up to 256 colors, map every pixel, encode the same animation shape
and delays, and return a complete GIF. No fixture supplies a known palette.
wtfgif receives one independently allocated typed array per frame, matching the
public API shape used by an app that stitches decoded images together. Those
arrays are created before timing.
The image-q + omggif baseline receives its normal contiguous RGBA input. Both
use an alpha threshold of 179. Every fixture encodes complete frames through the
public `wtfgif/encode` entry point and the equivalent image-q + omggif pipeline.

Each sample runs in a fresh Node process. Package loading and wtfgif
initialization happen before the clock; the first and only synchronous encode
of user input is timed. Initialization reserves a fixed 4 MiB input arena and
runs fixed synthetic high-color, leading-opaque mixed-alpha, clear-canvas,
normal-resolution Wu-planner, Cartesian-grid, and exact-palette inputs through
the real encoder paths. These source-independent calls prepare
JavaScript and Wasm code during app startup. They do not inspect, key, or retain
the fixture, user pixels, palettes, or encoded results. wtfgif copies every
fixture byte into its Wasm arena after the clock starts; the baseline likewise
does all palette, mapping, and encoding work inside its timed call. After
initialization and input allocation, each process fills 64 MiB of unrelated
memory and yields one zero-delay event-loop turn without touching the encoder
or fixture. There are zero fixture-derived warmups and no prior encode results
retained between processes. Encoder order alternates by fixture and process,
and each worker loads only the implementation it is measuring. Validation and
quality measurement are outside the clock. Results below are medians from 40
processes per implementation on an Apple M3 Pro with Node.js 22.23.2.

The receipt records package version 3.0.19 at source commit
`8b5a3b3139770fe1913b9cb8163991e5ec555cd4`. Its dirty flag is true because
the receipt was written while the SIMD candidate was under test. The
benchmark command rebuilds both scalar and SIMD quality Wasm before bundling,
so the timed artifact is produced from that recorded source commit.

| Fixture | Shape | wtfgif | image-q + omggif | Speedup | File-size ratio |
| --- | ---: | ---: | ---: | ---: | ---: |
| MakeEmoji production sample | 128×128×8 | 0.369 ms | 142.470 ms | **385.88×** | 3.80× |
| Photographic animation | 128×96×8 | 0.222 ms | 70.512 ms | **317.33×** | 1.84× |
| Pixel art | 64×64×12 | 0.095 ms | 26.471 ms | **277.66×** | 6.35× |
| Smooth gradients | 128×128×8 | 0.427 ms | 124.338 ms | **291.45×** | 8.00× |
| Random noise | 128×128×8 | 0.245 ms | 101.846 ms | **415.56×** | 7.28× |
| Transparency | 128×128×8 | 0.180 ms | 45.281 ms | **252.21×** | 20.85× |
| Disjoint frame palettes | 128×128×8 | 0.153 ms | 64.988 ms | **424.29×** | 7.64× |
| Nearly static animation | 128×128×12 | 0.217 ms | 80.588 ms | **371.45×** | 12.19× |
| Tiny animation | 16×16×6 | 0.071 ms | 52.602 ms | **744.80×** | 1.17× |
| One-megapixel animation | 512×512×4 | 1.873 ms | 538.617 ms | **287.61×** | 22.75× |

The observed range is 252.21×–744.80×, with a 358.49× geometric-mean speedup.
The corresponding files are 1.17×–22.75× larger, with a 6.50× geometric mean.
This is the library's intended tradeoff: encode latency takes priority over
compression ratio.

These results describe the ten committed fixtures on the recorded Apple M3
Pro and Node.js runtime. They do not claim codec-only performance, equal output
size, or unmeasured hardware and runtimes.

Every category exceeds the 100× floor on its first real encode after
initialization. Transparency has the narrowest margin at 252.21×, followed by
the one-megapixel animation at 287.61×.
No result depends on a known palette, source cache, previous result, or
reduced-quality mode.

![wtfgif speedup across the arbitrary-RGBA corpus](docs/corpus-speedup.svg)

The chart is generated from the same receipt as the table with
`npm run bench:charts`. Bar length represents speedup over image-q + omggif;
the labels report both median encode times, the output file-size ratio, and
source-relative RGB quality for both encoders. “Lossless RGB” means every
source-opaque RGB pixel survived palette mapping exactly. Shape, frame timing,
and binary alpha must be exact on every fixture regardless of that RGB label.

The baseline uses image-q `rgbquant` palette generation and nearest-color
mapping followed by omggif LZW. wtfgif uses its global quality quantizer and
literal LZW. The algorithms can select different indexed pixels, so the receipt
reports output bytes, opaque-source RGB PSNR, and per-frame SSIM after binary
alpha compositing against black. Every output is decoded and checked for shape,
frame count, delays, and exact binary alpha before it is accepted.

[`benchmarks/corpus.json`](benchmarks/corpus.json) records all raw samples,
medians, p95 values, output hashes, quality values, fixture hashes and
provenance, package-lock hash, commit, dirty state, and runtime environment.
The corpus covers real small images, photographic content, flat pixel art,
gradients, noise, transparency, disjoint frame palettes, similar adjacent
frames, tiny animations, and a one-megapixel workload. Add the optional
three-megapixel fixture with `npm run bench:corpus:stress`.

## Optimization profile (historical diagnostic receipt)

A source-built 3.0.19 SIMD Wasm profile split an earlier first public encode into its
JavaScript and Wasm phases. Each median below comes from 120 fresh Node
processes after normal source-independent initialization, a 64 MiB unrelated
memory eviction, and one event-loop yield. The fixture is first touched after
the clock starts.

| Fixture | Reserve | RGBA copy in | Wasm encoder | GIF copy out | Total |
| --- | ---: | ---: | ---: | ---: | ---: |
| MakeEmoji production sample | 0.003 ms | 0.015 ms | 0.332 ms | 0.024 ms | 0.376 ms |
| Photographic animation | 0.003 ms | 0.012 ms | 0.168 ms | 0.022 ms | 0.207 ms |

Wasm therefore accounted for about 88% of that MakeEmoji boundary. A newer
4,000-repeat sampling run on the current candidate resolved native Wasm
symbols without changing the public timing boundary: about 42% of samples were
in quality-histogram construction, 22% in nearest-palette search, 20% in
literal-LZW mapping and emission, and 16% elsewhere. These percentages guide
future work; the corpus table above is the authoritative latency receipt.

## Mixed-alpha and Wu initialization A/B

The current initialization now enters two additional real encoder call graphs
with fixed synthetic pixels: one begins opaque and contains sparse transparent
pixels, while the other occupies 2,560 coarse cells with unequal counts so it
reaches the normal-resolution Wu planner. These calls happen during explicit
app initialization. They do not inspect or retain user pixels, palettes, or
encoded results.

Against the previous committed initialization, alternating fresh-process A/B
runs measured paired-median first-encode improvements of **1.1896×** on
MakeEmoji, **1.0384×** on photographic content, **1.0750×** on pixel art,
**1.0160×** on gradients, **1.0327×** on noise, **1.0676×** on transparency,
**1.0859×** on disjoint palettes, **1.0256×** on nearly-static animation,
**1.0701×** on tiny animation, and **1.0050×** on the one-megapixel control.
The MakeEmoji, gradient, and exact/small controls used 80–120 pairs after 64
MiB cache eviction. Every candidate emitted the same complete-GIF SHA-256 as
its baseline before timing was considered.

Several exact-output screens were rejected rather than folded into the public
encoder:

- Replacing `new Uint8Array(outputView)` with either typed-array `slice()` or
  `ArrayBuffer.slice()` made GIF ownership copy-out 18% or 28% slower in 300
  paired fresh processes.
- Removing the compiler's `cold` placement hint from nearest-palette search
  left Wasm size unchanged and every complete-GIF SHA-256 hash identical. A
  500-pair confirmation measured MakeEmoji **1.0055×**, photographic
  **0.9990×**, noise **1.0048×**, nearly-static **0.9944×**, tiny **1.0119×**,
  and one-megapixel **1.0060×** versus the retained build. The repeatable
  nearly-static regression made the layout trade unacceptable.
- Wider mixed-opaque histogram unrolls, an out-of-line fallback, and
  interleaved bin updates each improved selected fixtures but regressed at
  least one disjoint-palette, nearly-static, photographic, or tiny control.

These are diagnostic A/B results, not replacements for the clean committed
40-process corpus receipt above. Candidate and baseline output hashes were
required to match before any timing was considered.

## Fused literal-mapping A/B

The 3.0.15 encoder maps two independent eight-pixel groups before writing
either literal-LZW result. This preserves the existing code stream and output
bytes while exposing more independent palette-table lookups and halving loop
control over each full block's remaining 216 literals.

Against the 3.0.14 SIMD Wasm, two separate 100-pair fresh-process runs after
64 MiB cache eviction measured **1.0511×** and **1.0592× faster inside Wasm**
on transparency. Two 60-pair one-megapixel runs measured **1.0162×** and
**1.0194× faster inside Wasm**. Two 100-pair MakeEmoji runs remained slightly
positive at **1.0041×** and **1.0088×**; photo, noise, and gradient controls
were neutral. Every comparison emitted the same complete-GIF SHA-256 hash.

The profiler worker accepts `PROFILE_REPEAT` so a sampling profiler can collect
enough stacks to identify hot native functions. That switch is diagnostic only:
the default corpus and every A/B result above still time one user-input encode
per fresh process.

## Selective post-link optimization A/B

The quality-only Wasm build now runs a second global-use analysis and
optimizing-inlining pass after the normal `-O3` pass. The build keeps the large
high-resolution quantizer and its planner kernels out of that second inlining
step; applying the same pass without those exclusions made the one-megapixel
case slower.

Against the 3.0.15 SIMD Wasm, 40 paired fresh-process comparisons improved the
complete MakeEmoji encode **1.0784×**, photographic content **1.0568×**,
nearly-static animation **1.1668×**, pixel art **1.1295×**, and transparency
**1.0455×**. The one-megapixel result was neutral at **0.9963×**, and the other
controls stayed within 1.2% of baseline. Every fixture emitted the exact same
complete-GIF SHA-256 hash before and after the compiler-only change.

## Wider SIMD palette search A/B

The current SIMD encoder searches 16 palette colors per loop through four
independent vector accumulators, then reduces those accumulators once after the
scan. Fully opaque spans inside otherwise mixed-alpha images also issue two
branch-free histogram updates per packed pixel pair instead of first testing
whether both pixels share a bin. Neither change alters the palette, indexed
pixels, LZW stream, or complete GIF bytes.

Against the preceding clean SIMD Wasm, 240 paired fresh MakeEmoji processes
after 64 MiB cache eviction measured **1.1584× faster inside Wasm** and
**1.1494× faster end to end** (0.776 ms to 0.675 ms). A separate sweep covered
all other corpus fixtures in 80 paired processes each, plus 40 one-megapixel
pairs. No measured end-to-end regression exceeded 0.9%, and every baseline and
candidate pair emitted the same complete-GIF SHA-256 hash.

## Fixed dominant-palette scan A/B

The 3.0.16 SIMD encoder treats the dominant 255/256-color palette as one fixed
256-entry table. A 255-color table duplicates its final real color into the
unused lane; the packed distance-and-index comparison guarantees that duplicate
can never win a tie. The search processes two 16-color groups per loop and
reduces its winning lanes entirely in SIMD registers. The packed two-pixel
histogram update is also expanded directly instead of constructing and
iterating a temporary pair. These changes preserve the selected palette,
indexed pixels, LZW stream, and complete GIF bytes.

Against the preceding clean SIMD Wasm, 160 paired fresh MakeEmoji processes
after 64 MiB cache eviction measured **1.1070× faster inside Wasm** and
**1.0931× faster end to end** (0.680 ms to 0.623 ms). A 240-pair direct run of
the more aggressively unrolled candidate improved MakeEmoji further but made
the disjoint-palette control **2.1% slower end to end**, so it was rejected.
The retained two-group loop held that control to **0.8%**, near process noise,
and every comparison emitted the same complete-GIF SHA-256 hash.

## Packed 16-bit palette search A/B

The 3.0.17 SIMD encoder stores each dominant-palette RGB channel in signed
16-bit lanes. RGB deltas fit exactly in that range; the encoder widens each
squared product to 32 bits before summing, so Euclidean distance and packed
distance-plus-index tie ordering are unchanged. Each vector load now supplies
eight palette candidates instead of four.

Against the clean 3.0.16 SIMD Wasm at the same fixed module path, 80 alternating
fresh, cache-evicted MakeEmoji pairs measured **1.0295× faster end to end**
(0.518 ms to 0.503 ms). Photographic input was neutral. Separate 120-pair
disjoint-palette and tiny controls measured **0.9981×** and **0.9995×**,
respectively. A four-group unroll was 2% slower on MakeEmoji and a one-group
loop measured **0.9996×** against the retained two-group loop, so both were
rejected. Every comparison emitted the same complete-GIF SHA-256 hash.

## Direct typed-frame copy A/B

The 3.0.18 JavaScript boundary copies an exact-size `Uint8Array` or
`Uint8ClampedArray` frame directly into the Wasm input arena. Earlier releases
created a temporary `subarray()` view for every independently allocated frame,
even though normal image-stitching inputs already have exactly the required
`width * height * 4` bytes. Oversized arrays still copy only the required
prefix. This changes neither validation nor the bytes presented to Wasm.

Against clean 3.0.17 at the same fixed module path, 480 alternating fresh,
cache-evicted MakeEmoji pairs measured **1.0231× faster end to end** (0.504 ms
to 0.491 ms). In 100-pair screens, photographic input improved **1.0155×**,
pixel art **1.0717×**, gradient **1.0207×**, noise **1.0262×**, transparency
**1.0469×**, disjoint palettes **1.0296×**, and tiny input **1.0256×** by paired
median. A 300-pair nearly-static repeat improved **1.0137×**. The
one-megapixel control was neutral: its separate medians improved **1.0067×**
while its paired median measured **0.9934×**. Every comparison emitted the
same complete-GIF SHA-256 hash.

Destination-view caching, validation/copy fusion, offset accumulation, and a
two-frame unroll measured neutral or regressed on repeat, so none are retained.

## Runtime preparation and cold-cache check

The fixed mixed-alpha preparation now runs four times during explicit startup.
Against the same Wasm with one call, 100 paired fresh processes after 64 MiB
cache eviction measured **1.0921×** faster MakeEmoji, **1.1754×** transparency,
**1.0205×** gradient, and **1.0106×** noise inside Wasm. Photographic and large
inputs were neutral, every complete-GIF SHA-256 hash matched, and the one-time
initialization cost increased by about 1.6 ms. The profiler exposes both mixed
and split-pair iteration counts so this V8 tier boundary remains reproducible.

One fixed pair-coherent opaque encode now prepares the existing split-pair
histogram loop before the existing mixed-alpha preparation restores its shared
hot state. Against the same Wasm bytes without that call, 300 paired fresh
photographic processes measured **1.2979× faster inside Wasm** and **1.2034×
faster end to end** (0.315 ms to 0.257 ms). MakeEmoji improved **1.0226×**;
500-pair noise and 300-pair transparency controls were neutral. Every pair
emitted the same complete-GIF SHA-256 hash. Initialization increased by about
2.5 ms and remains outside the timed boundary.

The current dominant-palette channel layout was also measured directly against
the previous clean SIMD Wasm. Across 240 paired, fresh, cache-evicted
MakeEmoji processes, it is **1.0772× faster inside Wasm** and **1.0693× faster
end to end** (0.565 ms to 0.530 ms), with the same complete-GIF SHA-256 hash.

Explicit initialization performs fixed synthetic encodes to compile and tier
the actual JavaScript and Wasm paths before the app's first user encode. The
inputs are constants in the library; they are not derived from the benchmark
fixture or any user image. Initialization does not build a source-keyed palette
or output cache. The current initialization also exercises the independently
allocated frame-array path with one fixed two-pixel animation. Across 20 fresh,
cache-evicted MakeEmoji processes, that final preparation reduces the first
separate-frame encode from 0.713 ms to 0.604 ms, a **1.180×** end-to-end gain
with identical GIF bytes.

The earlier committed preparation profiler isolates the SIMD palette-planner
work that preceded this separate-frame change. Its receipt touched one byte per
cache line in a 64 MiB unrelated-memory arena after initialization and before
timing either implementation. The current profiler fills the entire arena and
yields one event-loop turn before timing so future diagnostics match the corpus
boundary more closely. Against
the clean 3.0.11 encoder and with the same source-independent preparation on
both sides, 180 fresh MakeEmoji pairs measured **1.1349×** faster inside Wasm
and **1.1272×** faster end to end.
The baseline and candidate emitted the same SHA-256 hash. A separate 220-pair
transparent single-merge screen was neutral inside Wasm and **1.0523×** faster
end to end, also with identical bytes. Raw medians, paired ratios, and hashes
are in
[`benchmarks/runtime-preparation.json`](benchmarks/runtime-preparation.json).

## Independent correctness checks

```bash
npm run test:conformance:independent
npm run test:conformance:browsers
npm run fuzz:smoke
```

The independent conformance check encodes every corpus fixture with scalar and
SIMD Wasm, requires byte-for-byte agreement between them, and validates the
results through both omggif and Sharp/libvips. The browser check compares the
first rendered frame from Chromium, Firefox, and WebKit with the independent
decoder pixels. Property tests cover 200 deterministic arbitrary indexed and
RGBA animations. Bounded libFuzzer targets exercise malformed decoding and
encode-then-decode round trips; the local smoke check runs 500 cases per target.

CI is configured for Node 20 and 22 on Linux, Node 22 on macOS and Windows, all
three browser engines on Linux, and weekly 10,000-case fuzz runs. These checks
support file validity and rendering compatibility; they do not make the two
quantizers pixel-identical.

## Browser encoder race

```bash
npm run bench:race
```

This comparison uses the same eight real 128×128 MakeEmoji RGBA frames and an
alpha threshold of 179. Package code, the fixture fetch, and wtfgif's one-time
initialization happen before the clock. The timed boundary includes
palette creation, pixel mapping, GIF compression, and final byte assembly.

Each sample is the first and only encode in a fresh, cross-origin-isolated
Chrome process and fresh browser profile. During untimed initialization,
wtfgif reserves its generic input arena and runs the same fixed synthetic
runtime preparation described above. The harness creates wtfgif's eight
independent frame arrays, fills 64 MiB of unrelated memory, and waits one
`requestAnimationFrame` without touching encoder code or fixture pixels. It
then copies the frames into Wasm and performs all palette, mapping, compression,
and assembly work after the clock starts. There are no fixture-derived warmups,
image-derived retained state, or results reused between samples. The six
encoders run in a rotating order to reduce thermal and ordering bias. Results
below are medians from 15 processes per encoder on an Apple M3 Pro in Google
Chrome 151.0.7922.77.

This is public-API time-to-result, not a codec-kernel microbenchmark or an
equal-file-size comparison. The
gif.js and gif.js.optimized APIs create workers when `render()` begins, so that
worker creation is inside their timed jobs. The README chart uses bar length
for median encode time, marks 100× wtfgif latency with a dashed reference line,
and prints each encoder's emitted GIF size, relative slowdown, PSNR, and alpha
agreement beside the time.

| Implementation | Version | Median | wtfgif advantage | Bytes | PSNR | Alpha match |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| **wtfgif** | 3.0.19 | **0.430 ms** | — | 149,689 | 34.12 dB | 100% |
| gif.js | 0.2.0 | 83.290 ms | **193.70×** | 80,869 | 33.08 dB | 99.78% |
| image-q + omggif | 2.1.2 + 1.0.10 | 91.835 ms | **213.57×** | 39,350 | 31.84 dB | 100% |
| gif.js.optimized | 1.0.1 | 96.115 ms | **223.52×** | 80,304 | 32.20 dB | 99.76% |
| modern-gif | 2.1.0 | 118.220 ms | **274.93×** | 43,114 | 32.65 dB | 100% |
| gifenc | 1.0.3 | 123.270 ms | **286.67×** | 39,101 | 34.54 dB | 100% |

The browser receipt records wtfgif 3.0.19 at clean commit
`0968d6ff29bbc539272c3c94ba1c176ff4a94f5c`, together with the package-lock
hash and complete runtime environment.

Every output must parse as an eight-frame 128×128 animation with exact 100 ms
delays before its sample is accepted. The validator composites all frames,
measures RGB PSNR on source-opaque pixels, and measures binary alpha agreement
over every pixel. Raw timings and interquartile values are committed in
[`benchmarks/encoder-race.json`](benchmarks/encoder-race.json); the README chart
is generated from that receipt by `scripts/render-encoder-race-chart.mjs`.

Regenerate both committed SVG charts from their receipts with:

```bash
npm run bench:charts
```

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

## Earlier single-workload measurements

These retained release-3.0.6 receipts document narrower throughput, stress,
indexed-input, and decode contracts. They are historical context, not the
current 10-fixture headline result above.

The committed workload is eight real images from MakeEmoji. Each image was
aspect-fitted into a transparent 128×128 canvas, then stored as one contiguous
RGBA fixture at `test/rgba/makeemoji-128x128x8.rgba`.

The clock starts with that RGBA buffer. Both implementations must create a
palette, map every source pixel, and compress every frame. File loading, image
decoding, resizing, WebAssembly initialization, warmup, validation, and quality
measurement are outside timed samples.

Results below are medians from 200 samples after 30 timed-loop warmups on an
Apple M3 Pro with Node.js 22.23.2, measured on release 3.0.6.

| Implementation | Median | Speedup | Bytes | PSNR |
| --- | ---: | ---: | ---: | ---: |
| image-q rgbquant + omggif balanced LZW | 94.900 ms | 1.00× | 39,350 | 31.84 dB |
| wtfgif quality/global + literal LZW | 0.330 ms | **287.47×** | 149,689 | 34.12 dB |

This is one practical adaptive global-palette pipeline. The implementations do
not choose identical pixels, so the table reports source-relative PSNR and
output bytes alongside speed. The wtfgif palette is higher quality on this
fixture while remaining 287.47× faster. Literal LZW is lossless for the
indexed pixels, so the speedup does not come from lowering GIF pixel quality.

The quality path keeps exact source colors when a small palette is sufficient.
Its low-resolution first-encode path uses a weighted 4-bit-per-channel
histogram for ordinary photo-like, noisy, and smooth inputs. Occupied coarse
cells map directly when they fit the GIF palette; a one-cell overflow uses a
targeted weighted merge, while other dense inputs use the general adaptive
planner. Larger inputs can use a finer histogram. These are internal
input-dependent optimizations, not user-selectable quality levels, and none
reuse a prior source or palette.

For ordinary-sized inputs (up to one million pixels), the alpha check is
fused into that histogram pass. Multi-megapixel inputs use a separate tight
alpha preflight because it is faster than branching on transparency during
scattered histogram updates.

The same source with every pixel treated as opaque (`BENCH_ALPHA_THRESHOLD=0`)
took 109.245 ms for image-q + omggif and 0.383 ms for wtfgif: **285.36×**.
wtfgif measured 33.91 dB versus 32.44 dB for the baseline. This is the normal
full-color, no-transparent-pixels case.

In a separate 10-sample run after three timed-loop warmups, the larger stress
workload (ten synthetic 512×512 RGBA frames) measured
6,465.213 ms for image-q + omggif and 4.075 ms for wtfgif: **1586.44×**,
with 2,973,381 output bytes and 26.19 dB PSNR versus 24.26 dB for the
baseline. This is still arbitrary RGBA input: the palette is unknown, every
pixel is scanned, and every indexed pixel is emitted into a valid GIF.
The 1586.44× ratio is a workload-size effect, not a cache shortcut: both sides
read all 2,621,440 source pixels, while wtfgif keeps its histogram, parent-cell
lookup, and literal writer linear in the input size.

For the no-warmup, first-real-encode contract:

```bash
BENCH_ITERATIONS=500 BENCH_INITIALIZED_FIRST=1 BENCH_PHASES=1 npm run bench:rgba:cold:encode
```

That run starts a fresh Node process for every sample, initializes Wasm, and
then times exactly one real encode with no warmup. Across 500 processes the
median was 127.979 ms for image-q + omggif versus 1.257 ms for wtfgif
(**101.83×**). That historical run predated the current fixed synthetic code
preparation; it retained no source pixel, palette, or output result in
initialization. The parent-observed wall clock, which also
includes launching and shutting down Node, was 171.766 ms versus 29.610 ms
(**5.80×**). Median wtfgif phases were 0.212 ms to load the fixture, 0.430 ms
to import the package, 0.893 ms to initialize Wasm, and 1.257 ms for the first
encode.

Run the optional larger synthetic stress workload with:

```bash
npm run bench:rgba:stress
```

For a stress-only receipt, add `BENCH_COLD_RGBA_FIXTURE=stress`. Across five
fresh processes, the first encode after initialization took 6,409.521 ms with
image-q + omggif and 8.668 ms with wtfgif: **739.45×**. The complete process
wall clock was 6,491.454 ms versus 60.287 ms: **107.68×**. The wtfgif output
remains 2,973,381 bytes at 26.19 dB PSNR. The large image-q allocation makes this
workload noisy, so use several samples and report the median.

## Specialized: already-indexed frames

```bash
BENCH_ITERATIONS=100 BENCH_WARMUP_ITERATIONS=10 BENCH_TARGET_SAMPLE_MS=5 \
BENCH_COLOR_COUNTS=2,4,8,16,32,64,128,256 npm run bench:encode
```

This is the direct `GifWriter` contract: 12 full 128×128 frames, a normal
256-color global palette, and typed output. wtfgif always uses literal LZW.

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
