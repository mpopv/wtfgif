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

For the fastest path, use a preallocated `Uint8Array`, a normal 256-color
palette, and `compression: "fast"`:

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
| Encode 12 × 128×128 frames | 15.187 ms | 0.149 ms | **102.18x** |
| Decode 198 × 128×128 frames | 28.977 ms | 16.802 ms | **1.72x** |
| Decode 16 × 498×498 frames | 33.967 ms | 7.642 ms | **4.44x** |

Every result is checked against omggif for exact RGBA pixel equality before it
is timed. Decode and encode are measured separately.

```bash
npm run bench:encode
BENCH_GIF_FILTER=GIGACHAD npm run bench:decode
BENCH_GIF_FILTER=tenor npm run bench:decode
```

See [BENCHMARKS.md](BENCHMARKS.md) for the benchmark contract and caveats.

[MIT](LICENSE)
