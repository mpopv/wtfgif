# wtfgif

An `omggif`-compatible GIF reader and writer, plus pixel-perfect fast paths
that avoid rebuilding pixels when the requested edit does not require it.

**[Run the browser benchmark →](https://mpopv.github.io/wtfgif/)**

```bash
npm install wtfgif
```

## Replace omggif

For the core `GifReader` and `GifWriter` API, change the import:

```diff
- import { GifReader, GifWriter } from "omggif";
+ import { GifReader, GifWriter } from "wtfgif";
```

CommonJS and TypeScript are supported:

```js
const { GifReader, GifWriter } = require("wtfgif");
```

This is the compatibility path. It accepts the same indexed pixels, palettes,
frame rectangles, delays, transparency, and disposal options used by omggif.
It does not promise a 100x speedup for arbitrary new pixels.

## The 100x fast paths

wtfgif is fastest when pixels are already valid GIF data and the edit can be
expressed as a container change:

| Operation | Fixtures | Slowest speedup | Correctness |
| --- | ---: | ---: | --- |
| Compiled timing edit | 12/12 | **951.5x** | Exact pixels, delays, and original LZW payloads |
| Prepared Wasm remux | 12/12 | **244.09x** | Exact decoded animation |
| Strict frame reverse | 2/12 eligible | **5,329.5x** | Exact mapped pixels and delays |
| Strict boomerang | 2/12 eligible | **6,866.7x** | Exact mapped pixels and delays |

These are not claims that every wtfgif call is 100x faster. They are results
for the named operations across the accepted benchmark fixtures. Timing,
reverse, and boomerang are compared with MakeEmoji's `image-q` + `omggif`
export; remux is compared with `omggif` decode + fresh encode. See
[BENCHMARKS.md](BENCHMARKS.md) for commands, fixtures, sample counts, cold
results, and correctness gates.

## Edit timing without touching pixels

Compile once when a GIF is loaded:

```ts
import { compileGif } from "wtfgif";

const gif = compileGif(inputGif);
```

Export new frame delays later. GIF delays are measured in centiseconds:

```ts
const outputGif = gif
	.withDelays(Uint16Array.of(4, 8, 12, 8))
	.toUint8Array();
```

`compileGif()` retains the original GIF structure and compressed image data.
`withDelays()` changes only timing metadata, inserting a graphics control
extension when one is missing. It never decodes, quantizes, or recompresses a
frame.

For a one-shot timing edit:

```ts
import { retimeGifPixelPerfect } from "wtfgif";

const outputGif = retimeGifPixelPerfect(inputGif, delaysInCentiseconds);
```

These timing APIs are synchronous JavaScript. They need no Wasm initialization
and run in Node, browsers, Web Workers, Cloudflare Workers, and Vercel Edge.

## Reverse or boomerang proven-independent frames

```ts
const gif = compileGif(inputGif);

if (gif.canReorderFrames) {
	const reversed = gif.reverseFrames();
	const boomerang = gif.boomerangFrames();
	// A, B, C becomes A, B, C, C, B, A
}
```

This path reorders complete compressed frame records. It deliberately rejects
GIFs whose appearance can depend on the preceding frame, including unsafe
partial-frame composition or metadata anchored between frames. A rejection is
a request to use a real decode-and-render pipeline, not a malformed output.

## Remux with Wasm

Initialize Wasm once per JavaScript realm during page or worker startup:

```ts
import {
	initializeWasmGlobally,
	remuxGifPixelPerfect,
} from "wtfgif";

await initializeWasmGlobally();

const outputGif = remuxGifPixelPerfect(inputGif);
```

`remuxGifPixelPerfect()` validates and rewrites the GIF container while
preserving palettes and compressed frame payloads. It guarantees the displayed
animation: dimensions, frames, timing, loop behavior, disposal, transparency,
and decoded pixels. Non-rendering extension metadata such as comments is not
part of its contract.

Cloudflare Workers and Vercel Edge require static Wasm imports:

```ts
import {
	initializeWasmModule,
	remuxGifPixelPerfect,
} from "wtfgif";
import init, * as rust from "wtfgif/wasm-web";
import wasm from "wtfgif/wasm-web/wasm";

await initializeWasmModule({ ...rust, default: init }, wasm);
```

Initialization is not global across a website: the window and each Worker,
Node process, or edge isolate are separate realms and must initialize their
own module.

## What is not 100x

- `GifReader` and `GifWriter` are drop-in compatibility APIs, not universal
  100x APIs.
- Encoding arbitrary new pixels still requires palette selection and LZW
  compression. Wasm initialization cannot make that work disappear.
- `reencodeGifPixelPerfect()` performs fresh LZW encoding and is substantially
  slower than remuxing.
- Cold startup is excluded from the prepared-Wasm and compiled-edit results.
  The measured Wasm initialization and preparation cost was 8.64 ms median;
  one tiny-GIF timing run also produced a 59.3x cold one-shot outlier.
- Reverse and boomerang are fast only when wtfgif can prove that raw frame
  reordering preserves the rendered animation.

The speedup comes from recognizing when an edit does not need a pixel pipeline.
Instead of decode → quantize → recompress, wtfgif validates the structure and
copies already-correct compressed bytes.

## Verify

```bash
npm run check

BENCH_ITERATIONS=5 BENCH_WARMUP_ITERATIONS=2 \
npm run bench:makeemoji-retime
```

[MIT](LICENSE)
