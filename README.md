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

If you also decode GIFs, import `initializeWasmGlobally` and
`encodeRgbaGifFrames` from `wtfgif` instead. Do not mix the two entrypoints'
initializers: each entry owns its own Wasm module.

GIF itself is limited to 256 colors per palette and binary transparency, so no
GIF encoder can preserve every full-color source pixel exactly.

The quality-first RGBA pipeline is selected explicitly with
`compression: "fast", quantization: "quality"`: adaptive palette quantization
followed by literal LZW. Literal LZW is lossless for the indexed GIF pixels, so
it never trades away visual quality for speed; it only produces larger files.
`quality` is the default quantization mode. Select `quantization: "exact"`
when every source color must already fit in one GIF palette.
The Wasm build selects a SIMD artifact when the runtime supports it and falls
back to the portable scalar artifact otherwise. Large noisy/photo-like inputs
use a fast 4-bit histogram/table pass; smooth ramps use the finer 5-bit
histogram, with a compact 4-bit parent-cell lookup only for dense color sets.
Ordinary-sized inputs fuse alpha detection into the histogram scan; very large
inputs use a faster separate alpha preflight.

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

In the full `wtfgif` entry, `await initializeWasmGlobally()` also enables the
exact-parity Rust/Wasm backend used by `preparePlayback()` and `prepareFrames()`.
The omggif-compatible `decodeAndBlitFrameRGBA/BGRA()` methods do not require
the prepared-frame API.

Encoder backend selection is explicit: use `wasm`, `native-addon`, or
`javascript`. The package root is side-effect free; import `wtfgif/global`
only when a script-tag-style `window.wtfgif` namespace is required.

## How fast?

The snapshot below uses eight real MakeEmoji images, normalized to
128×128 RGBA frames, with 200 timed samples after 30 warmups. Palette creation,
pixel mapping, and GIF compression are all timed. WebAssembly initialization and
image loading are not.

| Real-image quality encode | Baseline | wtfgif | Speedup | wtfgif output |
| --- | ---: | ---: | ---: | ---: |
| Adaptive global palette | 98.424 ms | 0.453 ms | **217.39×** | 149,601 bytes / 34.12 dB |

The baseline is `image-q` plus omggif with balanced LZW. Both implementations
create an adaptive global palette and map every RGBA pixel. WebAssembly is
initialized before timed samples. The race measures this single quality-first
RGBA pipeline; there is no fixed-palette or skipped-pixel shortcut in the
race.

On the larger 10-frame 512×512 stress workload, the initialized SIMD path is
**1,196.59× faster** (6,511.142 ms for image-q + omggif versus 5.441 ms for
wtfgif), with 2,973,381 output bytes and 26.12 dB PSNR versus the baseline's
24.26 dB:

```bash
BENCH_RGBA_FIXTURE=stress BENCH_ITERATIONS=3 BENCH_WARMUP_ITERATIONS=1 node scripts/bench-rgba.mjs
```

That 1,196.59× result is not a cache trick: both encoders read all 2,621,440
source pixels and produce a valid GIF. The ratio grows on this larger fixture
because wtfgif's histogram, lookup, and literal writer stay linear.

The same run with every source pixel treated as opaque
(`BENCH_ALPHA_THRESHOLD=0`) measured 107.288 ms for image-q + omggif versus
0.510 ms for wtfgif: **210.52×**, at 33.91 dB PSNR.

For a true no-cache measurement, run `BENCH_ITERATIONS=10 node scripts/bench-cold-rgba.mjs`; it starts a new
Node process for every sample and includes imports, Wasm initialization, and
the complete encode. The current ten-process median is 146.208 ms for the
full entry versus 9.225 ms for wtfgif (**15.85×**). A ten-process encode-only
run measured 150.257 ms versus 7.814 ms (**19.23×**). Cold process startup is
a separate boundary; initialize Wasm during page or worker startup for the hot
numbers above:

```bash
npm run bench:rgba:cold:encode
```

This includes process startup and Wasm initialization. Initialize Wasm during
page or worker startup when measuring the hot path above.

If your frames are already palette-indexed—the direct `GifWriter` contract—
wtfgif is **241.77× faster** in the current 80-sample run: 16.204 ms for
omggif versus 0.067 ms for wtfgif. That result is byte-decoded and checked
for exact RGBA equality before timing.

Decode remains workload-dependent rather than 100×: the latest 50-sample
fixture sweep ranges from **1.08×** on a tiny frame to **4.45×** on a larger
input (**2.35× geometric mean**), with exact composited RGBA parity. Once a
reader is clearly processing an animation, medium rectangles use the reusable
initialized Wasm blitter. Single-frame and sub-512-pixel reads stay on
JavaScript.
Large GIFs with partial transparent rectangles also use a
rectangle-only Wasm path so the caller canvas is not copied through Wasm for
every frame.
Prepared Wasm playback keeps its full-frame and delta streams in Wasm-owned
scratch views until `dispose()`, avoiding a second whole-animation copy through
the binding layer without changing pixel output.
The one-off `decodeGifFramesRgba` API reaches **20.20×** on an all-full-canvas,
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

[MIT](LICENSE)
