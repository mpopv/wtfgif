# wtfgif

A drop-in `omggif` replacement with a Rust fast path that runs in Node,
browsers, Cloudflare Workers, and Vercel Edge.

**[Race wtfgif against omggif in your browser →](https://mpopv.github.io/wtfgif/)**

```bash
npm install wtfgif
```

## Use it like omggif

Change the package name:

```diff
- import { GifReader, GifWriter } from "omggif";
+ import { GifReader, GifWriter } from "wtfgif";
```

CommonJS works too:

```js
const { GifReader, GifWriter } = require("wtfgif");
```

## Pixel-perfect GIF rewrite

Initialize Rust/Wasm during page load, then remux GIFs without changing a
single decoded pixel:

```ts
import {
	initializeWasmGlobally,
	remuxGifPixelPerfect,
} from "wtfgif";

await initializeWasmGlobally();

const outputGif = remuxGifPixelPerfect(inputGif);
```

`remuxGifPixelPerfect()` preserves each frame's original LZW payload and
palette. It does not decode and recompress pixels. That is why it is extremely
fast, keeps pixels exact, and usually produces a smaller file than wtfgif's
speed-first fresh reencoder.

Use `reencodeGifPixelPerfect()` when you specifically require fresh LZW codes.
That path is pixel-perfect too, but it does more work.

### Cloudflare Workers and Vercel Edge

Edge runtimes need a static Wasm import:

```ts
import {
	initializeWasmModule,
	remuxGifPixelPerfect,
} from "wtfgif";
import init, * as rust from "wtfgif/wasm-web";
import wasm from "wtfgif/wasm-web/wasm";

await initializeWasmModule({ ...rust, default: init }, wasm);

export default {
	fetch: async (request: Request) => {
		const input = new Uint8Array(await request.arrayBuffer());
		return new Response(remuxGifPixelPerfect(input), {
			headers: { "content-type": "image/gif" },
		});
	},
};
```

## 100x proof

This benchmark initializes and prepares Wasm before the clock, just as the
browser example does. Each timed sample is the first real GIF processed in a
fresh process. The primer is an unrelated generated GIF, so no fixture input or
output is cached.

```bash
BENCH_ITERATIONS=31 \
BENCH_REENCODE_ONLY=1 \
WTFGIF_BENCH_BACKEND=wasm \
WTFGIF_PREPARE_WASM_AT_PAGE_LOAD=1 \
WTFGIF_REENCODE_MODE=remux \
npm run bench
```

| Fixture | Faster than omggif |
| --- | ---: |
| 18d | 738.64x |
| Clap | 309.84x |
| Homer | 983.53x |
| Chipmunk | 768.77x |
| GIGACHAD | 607.23x |
| NODDERS | 512.23x |
| Proud | 681.13x |
| catJAM | 519.37x |
| excuseme | 480.06x |
| party_blob | 955.88x |
| partyparrot | 244.09x |
| tenor | 1,238.09x |
| **Geometric mean** | **609.73x** |

The slowest result is **244.09x**. All 12 fixtures clear 100x.

On the benchmark machine, the excluded one-time initialization and generic
preparation cost had an 8.64 ms median and 9.86 ms p95 across 31 fresh
processes.

For every timed sample, the source and output are decoded after the timer and
their complete RGBA frame streams are compared. Any mismatch aborts the run.

This is a same-result benchmark, not a same-algorithm benchmark: omggif decodes
and freshly recompresses each frame; wtfgif preserves already-valid compressed
frame data. See [BENCHMARKS.md](BENCHMARKS.md) for the fresh-recompression and
true-cold numbers.

## Important behavior

- Rust/Wasm runs in Node, browsers, Cloudflare Workers, and Vercel Edge.
- Wasm preparation happens inside `initializeWasmGlobally()` or
  `initializeWasmModule()`.
- `remuxGifPixelPerfect()` guarantees the displayed animation's dimensions,
  frames, timing, loop, disposal, transparency, and pixels.
- Non-rendering extension metadata such as comments is not part of the remux
  contract and may be preserved or removed.
- `reencodeGifPixelPerfect()` writes fresh literal LZW data and can produce
  larger files in exchange for speed.

## Verify

```bash
npm run check
npm run test:wasm-core
```

[MIT](LICENSE)
