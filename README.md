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
});
```

If you also decode GIFs, import `initializeWasmGlobally` and
`encodeRgbaGifFrames` from `wtfgif` instead. Do not mix the two entrypoints'
initializers: each entry owns its own Wasm module.

GIF itself is limited to 256 colors per palette and binary transparency, so no
GIF encoder can preserve every full-color source pixel exactly.

The RGBA path uses one quality-first pipeline: adaptive palette quantization
followed by literal LZW. Literal LZW is lossless for the indexed GIF pixels, so
it never trades away visual quality for speed; it only produces larger files.
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

The default benchmark uses eight real MakeEmoji images, normalized to
128×128 RGBA frames, with 100 timed samples after 20 warmups. Palette creation,
pixel mapping, and GIF compression are all timed. WebAssembly initialization and
image loading are not.

| Real-image quality encode | Baseline | wtfgif | Speedup | wtfgif output |
| --- | ---: | ---: | ---: | ---: |
| Adaptive global palette | 100.742 ms | 0.548 ms | **183.98×** | 149,601 bytes / 34.12 dB |

The baseline is `image-q` plus omggif with balanced LZW. Both implementations
create an adaptive global palette and map every RGBA pixel. WebAssembly is
initialized before timed samples. The race measures this single quality-first
RGBA pipeline; there is no fixed-palette or skipped-pixel shortcut in the
race.

On the larger 10-frame 512×512 stress workload, the initialized SIMD path is
**1,316.47× faster** (6,759.166 ms for image-q + omggif versus 5.134 ms for
wtfgif), with 2,973,381 output bytes and 26.12 dB PSNR versus the baseline's
24.26 dB:

```bash
BENCH_RGBA_FIXTURE=stress BENCH_ITERATIONS=3 BENCH_WARMUP_ITERATIONS=1 node scripts/bench-rgba.mjs
```

That 1,316× result is not a cache trick: both encoders read all 2,621,440
source pixels and produce a valid GIF. The ratio grows on this larger fixture
because wtfgif's histogram, lookup, and literal writer stay linear.

The same run with every source pixel treated as opaque
(`BENCH_ALPHA_THRESHOLD=0`) measured 109.946 ms for image-q + omggif versus
0.674 ms for wtfgif: **163.18×**, at 33.91 dB PSNR.

For a true no-cache measurement, run `BENCH_ITERATIONS=31 node scripts/bench-cold-rgba.mjs`; it starts a new
Node process for every sample and includes imports, Wasm initialization, and
the complete encode. The current 31-sample median is 150.719 ms for the
full entry versus 7.918 ms for wtfgif (19.04×). A ten-process encode-only
run measured 148.766 ms versus 6.415 ms (**23.19×**). Cold process startup is
a separate boundary; initialize Wasm during page or worker startup for the hot
numbers above:

```bash
npm run bench:rgba:cold:encode
```

This includes process startup and Wasm initialization. Initialize Wasm during
page or worker startup when measuring the hot path above.

If your frames are already palette-indexed—the direct `GifWriter` contract—
wtfgif is **224.64× faster** in the current 300-sample run: 16.743 ms for
omggif versus 0.075 ms for wtfgif.
That result is byte-decoded and checked for exact RGBA equality before timing.

Decode remains workload-dependent rather than 100×: the current fixture sweep
ranges from roughly parity on tiny frames to about **4×** on the larger
inputs, with exact composited RGBA parity.

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
