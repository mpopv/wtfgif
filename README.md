# wtfgif

Fast GIF encode and decode for Node.js, browsers, Workers, and edge runtimes.

[Race it against omggif in your browser →](https://mpopv.github.io/wtfgif/)

```bash
npm install wtfgif
```

## Encode images

For an encode-only app, import both functions from `wtfgif/encode`. This entry
contains the smaller encode-only WebAssembly module; initialize it once during
page or worker startup, then pass one flat RGBA buffer containing every frame:

```ts
import {
	encodeRgbaGifFrames,
	initializeWasmGlobally,
} from "wtfgif/encode";

await initializeWasmGlobally();

const gif = encodeRgbaGifFrames({
	width,
	height,
	frames: rgbaFrames,
	frameCount,
	delay: 10,
	loop: 0,
	compression: "fast",
	quantization: "quality",
});
```

Initialization includes a tiny internal Wasm warmup (no user data) so the
first real encode uses the same hot path as later calls.

If you also decode GIFs, import `initializeWasmGlobally` and
`encodeRgbaGifFrames` from `wtfgif` instead. Do not mix the two entrypoints'
initializers: each entry owns its own Wasm module.

GIF itself is limited to 256 colors per palette and binary transparency, so no
GIF encoder can preserve every full-color source pixel exactly.

The quality-first RGBA pipeline is selected explicitly with
`compression: "fast", quantization: "quality"`: adaptive palette quantization
followed by literal LZW. Literal LZW is lossless for the indexed GIF pixels, so
it never trades away visual quality for speed; it only produces larger files.
For backwards compatibility, omitting `quantization` keeps the older exact
fast-mode behavior; use the quality options above for arbitrary full-color
input.
The Wasm build selects a SIMD artifact when the runtime supports it and falls
back to the portable scalar artifact otherwise. Large noisy/photo-like inputs
use a fast 4-bit histogram/table pass; smooth ramps use the finer 5-bit
histogram, with a compact 4-bit parent-cell lookup only for dense color sets.

## Decode GIFs

`GifReader` uses the same core API as omggif:

```ts
import { GifReader } from "wtfgif";

const reader = new GifReader(gifBytes);
const rgba = new Uint8ClampedArray(reader.width * reader.height * 4);

for (let frame = 0; frame < reader.numFrames(); frame += 1) {
	reader.decodeAndBlitFrameRGBA(frame, rgba);
}
```

## How fast?

The snapshot below uses eight real MakeEmoji images, normalized to
128×128 RGBA frames, with 120 timed samples after 20 warmups. Palette creation,
pixel mapping, and GIF compression are all timed. WebAssembly initialization and
image loading are not.

| Real-image quality encode | Baseline | wtfgif | Speedup | wtfgif output |
| --- | ---: | ---: | ---: | ---: |
| Adaptive global palette | 101.026 ms | 0.451 ms | **224.23×** | 149,601 bytes / 34.12 dB |

The baseline is `image-q` plus omggif with balanced LZW. Both implementations
create an adaptive global palette and map every RGBA pixel. WebAssembly is
initialized before timed samples. The race measures this single quality-first
RGBA pipeline; there is no fixed-palette or skipped-pixel shortcut in the
race.

On the larger 10-frame 512×512 stress workload, the initialized SIMD path is
**1,261.58× faster** (6,675.854 ms for image-q + omggif versus 5.292 ms for
wtfgif), with 2,973,381 output bytes and 26.12 dB PSNR versus the baseline's
24.26 dB:

```bash
BENCH_RGBA_FIXTURE=stress BENCH_ITERATIONS=3 BENCH_WARMUP_ITERATIONS=1 node scripts/bench-rgba.mjs
```

That 1,261× result is not a cache trick: both encoders read all 2,621,440
source pixels and produce a valid GIF. The ratio grows on this larger fixture
because wtfgif's histogram, lookup, and literal writer stay linear.

The same run with every source pixel treated as opaque
(`BENCH_ALPHA_THRESHOLD=0`) measured 110.562 ms for image-q + omggif versus
0.500 ms for wtfgif: **221.01×**, at 33.91 dB PSNR.

For a true no-cache measurement, run `BENCH_ITERATIONS=10 node scripts/bench-cold-rgba.mjs`; it starts a new
Node process for every sample and includes imports, Wasm initialization, and
the complete encode. The current ten-process median is 145.838 ms for the
full entry versus 8.717 ms for wtfgif (**16.73×**). A ten-process encode-only
run measured 155.849 ms versus 8.194 ms (**19.02×**). Cold process startup is
a separate boundary; initialize Wasm during page or worker startup for the hot
numbers above:

```bash
npm run bench:rgba:cold:encode
```

This includes process startup and Wasm initialization. Initialize Wasm during
page or worker startup when measuring the hot path above.

If your frames are already palette-indexed—the direct `GifWriter` contract—
wtfgif is **222.58× faster** in the current 200-sample run: 16.541 ms for
omggif versus 0.074 ms for wtfgif.
That result is byte-decoded and checked for exact RGBA equality before timing.
Across the 2–256-color typed-output sweep, the minimum measured speedup is
**110.57×** (32 colors), with a **167× geometric mean** in the latest
180-sample receipt.

Decode remains workload-dependent rather than 100×: the latest 50-sample
fixture sweep ranges from **0.96×** on a tiny frame to **4.37×** on a larger
input (**2.19× geometric mean**), with exact composited RGBA parity. Large GIFs
with partial transparent rectangles also use a rectangle-only Wasm path so the
caller canvas is not copied through Wasm for every frame.
The one-off `decodeGifFramesRgba` API reaches **4.98×** on an all-full-canvas,
opaque animation by writing frames directly into the final stream.

```bash
npm run bench
npm run bench:encode
BENCH_GIF_FILTER=GIGACHAD npm run bench:decode
BENCH_RGBA_FIXTURE=stress BENCH_ITERATIONS=5 BENCH_WARMUP_ITERATIONS=2 npm run bench:rgba
```

See [BENCHMARKS.md](BENCHMARKS.md) for every condition, output-size tradeoff,
and reproduction command. The initialized race is the normal app contract;
strict-cold numbers include process and Wasm startup and are shown separately.

## Upgrade note for 2.0

The old internal WebAssembly warmup-primer hooks were removed. If you used
`WasmWebModule` directly, remove calls to
`prepare_reencode_hot_path`, `reencode_hot_path_primer`, and
`remux_hot_path_primer`; `initializeWasmGlobally()` or `initializeWasmModule()`
is now sufficient. The public GIF reader, writer, RGBA encoder, and compression
options remain available.

[MIT](LICENSE)
