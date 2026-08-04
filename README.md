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
The encode entry ships a quality-only Wasm artifact and selects its SIMD
variant when the runtime supports it, with a portable scalar fallback. Large
noisy/photo-like inputs use a fast 4-bit histogram/table pass; smooth ramps use
the finer 5-bit histogram, with a compact 4-bit parent-cell lookup only for
dense color sets. Ordinary-sized inputs fuse alpha detection into the
histogram scan; very large inputs use a faster separate alpha preflight.

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
| Adaptive global palette | 94.385 ms | 0.496 ms | **190.15×** | 149,601 bytes / 34.12 dB |

The baseline is `image-q` plus omggif with balanced LZW. Both implementations
create an adaptive global palette and map every RGBA pixel. WebAssembly is
initialized before timed samples. Initialization only binds the module and
memory; the first encode reserves its exact input range, and no synthetic
encode or input, palette, or output is retained. The race measures this single
quality-first RGBA pipeline; there is no fixed-palette or skipped-pixel
shortcut in the race.

The strict first-real-encode boundary is separate: five fresh processes measured
136.925 ms for image-q + omggif versus 4.432 ms for wtfgif (**30.90×**). There
are no timed warmup encodes in this receipt. The remaining gap is portable
Wasm's first-call lazy compilation, not a retained result cache.
Reproduce it with:

```bash
BENCH_INITIALIZED_FIRST=1 BENCH_WTFFIG_ENTRY=encode BENCH_ITERATIONS=5 node scripts/bench-cold-rgba.mjs
```

On the larger 10-frame 512×512 stress workload, the initialized SIMD path is
**1,007.97× faster** (6,149.544 ms for image-q + omggif versus 6.101 ms for
wtfgif), with 2,973,381 output bytes and 26.12 dB PSNR versus the baseline's
24.26 dB:

```bash
BENCH_RGBA_FIXTURE=stress BENCH_ITERATIONS=5 BENCH_WARMUP_ITERATIONS=1 node scripts/bench-rgba.mjs
```

That 1,007.97× result is not a cache trick: both encoders read all 2,621,440
source pixels and produce a valid GIF. The ratio grows on this larger fixture
because wtfgif's histogram, lookup, and literal writer stay linear.

The same run with every source pixel treated as opaque
(`BENCH_ALPHA_THRESHOLD=0`) measured 104.423 ms for image-q + omggif versus
0.611 ms for wtfgif: **171.02×**, at 33.91 dB PSNR.

For a true no-cache measurement, run `BENCH_WTFFIG_ENTRY=encode BENCH_ITERATIONS=7 node scripts/bench-cold-rgba.mjs`; it starts a new
Node process for every sample and includes imports, Wasm initialization, and
the complete encode. The current seven-process median is 149.470 ms for the
baseline versus 6.743 ms for wtfgif's encode entry (**22.17×**). Cold process startup is
a separate boundary; initialize Wasm during page or worker startup for the hot
numbers above:

```bash
npm run bench:rgba:cold:encode
```

This includes process startup and Wasm initialization. Initialize Wasm during
page or worker startup when measuring the hot path above.

If your frames are already palette-indexed—the direct `GifWriter` contract—
wtfgif is **182.10× faster** in the current run: 16.603 ms for
omggif versus 0.091 ms for wtfgif. That result is byte-decoded and checked
for exact RGBA equality before timing.

For the fast indexed path, the fixed-width literal writer is used through
64-color palettes. The wider 8-bit stream can be larger than omggif's packed
stream, but it decodes to exactly the same pixels.

Decode remains workload-dependent rather than 100×: the latest fixture sweep
ranges from **1.10×** on a tiny frame to **4.69×** on a larger input
(**2.48× geometric mean**), with exact composited RGBA parity. Once a
reader is clearly processing an animation, medium rectangles use the reusable
initialized Wasm blitter. Single-frame and sub-512-pixel reads stay on
JavaScript.
Large GIFs with partial transparent rectangles also use a
rectangle-only Wasm path so the caller canvas is not copied through Wasm for
every frame.
Prepared Wasm playback keeps its full-frame and delta streams in Wasm-owned
scratch views until `dispose()`, avoiding a second whole-animation copy through
the binding layer without changing pixel output.
The one-off `decodeGifFramesRgba` API reaches **24.41×** on an all-full-canvas,
opaque animation by writing frames directly into the final stream.

```bash
npm run bench
BENCH_ITERATIONS=40 BENCH_WARMUP_ITERATIONS=10 npm run bench:encode
BENCH_GIF_FILTER=GIGACHAD npm run bench:decode
BENCH_RGBA_FIXTURE=stress BENCH_ITERATIONS=5 BENCH_WARMUP_ITERATIONS=2 npm run bench:rgba
```

See [BENCHMARKS.md](BENCHMARKS.md) for every condition, output-size tradeoff,
and reproduction command. The initialized race is the normal app contract;
strict-cold numbers include process and Wasm startup and are shown separately.

[MIT](LICENSE)
