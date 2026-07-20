# wtfgif

A faster, TypeScript-ready replacement for
[`omggif`](https://github.com/deanm/omggif).

```bash
npm install wtfgif
```

## Initialize once

Load WebAssembly during page startup, before encoding or decoding:

```ts
import { initializeWasmGlobally } from "wtfgif";

await initializeWasmGlobally();
```

Node.js loads the bundled WebAssembly automatically. Browsers, Web Workers,
Cloudflare Workers, and Vercel Edge should call the initializer once.

## Decode

Use the same API as `omggif`:

```ts
import { GifReader } from "wtfgif";

const reader = new GifReader(gifBytes);
const rgba = new Uint8ClampedArray(reader.width * reader.height * 4);

for (let frame = 0; frame < reader.numFrames(); frame += 1) {
	reader.decodeAndBlitFrameRGBA(frame, rgba);
}
```

`frameInfo()`, `loopCount()`, `decodeAndBlitFrameRGBA()`, and
`decodeAndBlitFrameBGRA()` are drop-in compatible.

## Encode

### Arbitrary images

Pass decoded RGBA frames directly. `quality` builds an adaptive palette;
`fast` uses a fixed palette and trades color accuracy for speed.

```ts
import { encodeRgbaGifFrames } from "wtfgif";

const gifBytes = encodeRgbaGifFrames({
	width,
	height,
	frames: rgbaFrames, // one flat Uint8Array, RGBA, all frames
	frameCount,
	delay: 10,
	loop: 0,
	compression: "fast",
	quantization: "quality", // "exact" | "fast" | "quality"
	paletteMode: "global", // use "local" for a palette per frame
});
```

`exact` rejects input that GIF cannot represent without color loss. `fast`
uses RGB332. `quality` uses adaptive median-cut quantization. A global palette
is fastest and smallest; local palettes improve unrelated photographic frames
but cost more CPU and 768 bytes per frame. Local palettes cannot be combined
with `delta` or a supplied global palette.

GIF cannot exactly represent arbitrary full-color images: each frame is
limited to 256 colors and binary transparency.

### Already-indexed frames

For the fastest possible encoder, use a preallocated `Uint8Array`, a normal
256-color palette, and `compression: "fast"`:

```ts
import { GifWriter } from "wtfgif";

const output = new Uint8Array(width * height * frameCount * 2 + 4096);
const writer = new GifWriter(output, width, height, {
	palette, // 256 RGB integers, just like omggif
	loop: 0,
	compression: "fast",
});

for (const indexedPixels of frames) {
	writer.addFrame(0, 0, width, height, indexedPixels, {
		delay: 10,
		disposal: 2,
	});
}

const gifBytes = output.subarray(0, writer.end());
```

Everything except `compression` is ordinary `omggif` usage. Omit
`compression: "fast"` for balanced LZW compression and omggif-sized output.
Fast compression is pixel-perfect and standards-valid, but the benchmark GIF
is 1.37x larger.

## Speed

After initialization, on the included normal 256-color benchmark:

| Operation | omggif | wtfgif | Speedup |
| --- | ---: | ---: | ---: |
| Encode 12 × 128×128 frames | 17.839 ms | 0.166 ms | **107.49x** |
| Decode 198 × 128×128 frames | 31.204 ms | 18.632 ms | **1.67x** |
| Decode 16 × 498×498 frames | 37.743 ms | 8.687 ms | **4.34x** |

Every result is checked against omggif for exact RGBA pixel equality before it
is timed. Decode and encode are measured separately.

For arbitrary RGBA input, wtfgif also measures palette creation, pixel mapping,
and encoding together:

| RGBA workload | Mode | wtfgif | vs omggif + same required mapping |
| --- | --- | ---: | ---: |
| 12 × 128×128 | fast/global | 0.501 ms | **30.80x** |
| 12 × 128×128 | quality/global | 9.408 ms | — |
| 10 × 512×512 | fast/global | 6.554 ms | **24.26x** |
| 10 × 512×512 | quality/global | 21.241 ms | — |

Quality modes are not assigned a speedup against the RGB332 omggif pipeline
because they produce materially higher-quality pixels.

```bash
npm run bench:encode
npm run bench:rgba
BENCH_GIF_FILTER=GIGACHAD npm run bench:decode
BENCH_GIF_FILTER=tenor npm run bench:decode
```

See [BENCHMARKS.md](BENCHMARKS.md) for the benchmark contract and caveats.

[MIT](LICENSE)
