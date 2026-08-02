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
128×128 RGBA frames, with 200 timed samples after 30 warmups. Palette creation,
pixel mapping, and GIF compression are all timed. WebAssembly initialization and
image loading are not.

| Real-image quality encode | Baseline | wtfgif | Speedup | wtfgif output |
| --- | ---: | ---: | ---: | ---: |
| Adaptive global palette | 99.069 ms | 0.752 ms | **131.72×** | 149,601 bytes / 34.12 dB |

The baseline is `image-q` plus omggif with balanced LZW. Both implementations
create an adaptive global palette and map every RGBA pixel. WebAssembly is
initialized before timed samples. The race measures this single quality-first
RGBA pipeline; there is no hidden lower-quality shortcut in the race.

The same run with every source pixel treated as opaque
(`BENCH_ALPHA_THRESHOLD=0`) measured 120.423 ms for image-q + omggif versus
0.969 ms for wtfgif: **124.31×**, at 33.91 dB PSNR.

For a true no-cache measurement, run `BENCH_ITERATIONS=31 node scripts/bench-cold-rgba.mjs`; it starts a new
Node process for every sample and includes imports, Wasm initialization, and
the complete encode. The current 31-sample median is 153.763 ms for the full
entry versus 8.137 ms for wtfgif (18.90×). The encode-only entry measured
156.294 ms versus 7.390 ms (**21.15×**):

```bash
npm run bench:rgba:cold:encode
```

This includes process startup and Wasm initialization. Initialize Wasm during
page or worker startup when measuring the hot path above.

If your frames are already palette-indexed—the direct `GifWriter` contract—
wtfgif is **188.01× faster**: 17.565 ms for omggif versus 0.093 ms for wtfgif.
That result is byte-decoded and checked for exact RGBA equality before timing.

Decode remains workload-dependent: **1.90×** on the included 198-frame
128×128 GIF and **4.07×** on the included 16-frame 498×498 GIF, with exact
composited RGBA parity.

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
