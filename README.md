# wtfgif

Fast GIF encode and decode for Node.js, browsers, Workers, and edge runtimes.

[Race it against omggif in your browser →](https://mpopv.github.io/wtfgif/)

```bash
npm install wtfgif
```

## Start once

Initialize WebAssembly during page or worker startup. Node.js can initialize
automatically, but using the same explicit call everywhere keeps startup out of
your hot path.

```ts
import { initializeWasmGlobally } from "wtfgif";

await initializeWasmGlobally();
```

Initialization only loads and compiles WebAssembly. There is no required
primer call or synthetic encode; after this one-time startup, calls use the
same initialized module.

## Encode images

Encode-only apps can import `wtfgif/encode` so the reader and remux exports are
not parsed during startup:

Give wtfgif one flat RGBA buffer containing every frame:

```ts
import { encodeRgbaGifFrames } from "wtfgif/encode";

const gif = encodeRgbaGifFrames({
	width,
	height,
	frames: rgbaFrames,
	frameCount,
	delay: 10,
	loop: 0,
});
```

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
128×128 RGBA frames, with 100 timed samples after 30 warmups. Palette creation,
pixel mapping, and GIF compression are all timed. WebAssembly initialization and
image loading are not.

| Real-image quality encode | Baseline | wtfgif | Speedup | wtfgif output |
| --- | ---: | ---: | ---: | ---: |
| Adaptive global palette | 100.240 ms | 0.818 ms | **122.57×** | 149,601 bytes / 34.12 dB |

The baseline is `image-q` plus omggif with balanced LZW. Both implementations
create an adaptive global palette and map every RGBA pixel. WebAssembly is
initialized before timed samples. The race measures this single quality-first
RGBA pipeline; there is no hidden lower-quality shortcut in the race.

The same run with every source pixel treated as opaque
(`BENCH_ALPHA_THRESHOLD=0`) measured 118.046 ms for image-q + omggif versus
0.982 ms for wtfgif: **120.23×**, at 33.91 dB PSNR.

For a true no-cache measurement, run `BENCH_ITERATIONS=31 node scripts/bench-cold-rgba.mjs`; it starts a new
Node process for every sample and includes imports, Wasm initialization, and
the complete encode. The current 31-sample median is 177.529 ms for the
baseline versus 8.679 ms for wtfgif (20.46×). The encode-only entry measured
7.890 ms (21.55×):

```bash
npm run bench:rgba:cold:encode
```

This includes process startup and Wasm initialization. Initialize Wasm during
page or worker startup when measuring the hot path above.

If your frames are already palette-indexed—the direct `GifWriter` contract—
wtfgif is **180.48× faster**: 17.798 ms for omggif versus 0.099 ms for wtfgif.
That result is byte-decoded and checked for exact RGBA equality before timing.

Decode remains workload-dependent: **1.90×** on the included 198-frame
128×128 GIF and **4.46×** on the included 16-frame 498×498 GIF, with exact
composited RGBA parity.

```bash
npm run bench
npm run bench:encode
BENCH_GIF_FILTER=GIGACHAD npm run bench:decode
BENCH_RGBA_FIXTURE=stress BENCH_ITERATIONS=5 BENCH_WARMUP_ITERATIONS=2 npm run bench:rgba
```

See [BENCHMARKS.md](BENCHMARKS.md) for every condition, output-size tradeoff,
and reproduction command.

## Upgrade note for 2.0

The old internal WebAssembly warmup-primer hooks were removed. If you used
`WasmWebModule` directly, remove calls to
`prepare_reencode_hot_path`, `reencode_hot_path_primer`, and
`remux_hot_path_primer`; `initializeWasmGlobally()` or `initializeWasmModule()`
is now sufficient. The public GIF reader, writer, RGBA encoder, and compression
options remain available.

[MIT](LICENSE)
