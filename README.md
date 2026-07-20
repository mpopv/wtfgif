# wtfgif

A drop-in replacement for [omggif](https://www.npmjs.com/package/omggif), with
TypeScript types, portable JavaScript and WebAssembly backends, and an
experimental native path that is more than **100x faster on every cold
pixel-perfect reencoding fixture** in the benchmark suite.

## Install

```bash
npm install wtfgif
```

Replace the package name. The core API stays the same:

```diff
- import { GifReader, GifWriter } from "omggif";
+ import { GifReader, GifWriter } from "wtfgif";
```

CommonJS works too:

```js
const { GifReader, GifWriter } = require("wtfgif");
```

## Read a GIF

```ts
import { readFileSync } from "node:fs";
import { GifReader } from "wtfgif";

const reader = new GifReader(readFileSync("input.gif"));
const rgba = new Uint8Array(reader.width * reader.height * 4);

reader.decodeAndBlitFrameRGBA(0, rgba);

console.log({
	width: reader.width,
	height: reader.height,
	frames: reader.numFrames(),
	loop: reader.loopCount(),
});
```

`GifReader` accepts plain arrays, `Uint8Array`, and Node `Buffer`. RGBA/BGRA
targets can also be `Uint8ClampedArray`.

## Write a GIF

```ts
import { writeFileSync } from "node:fs";
import { GifWriter } from "wtfgif";

const output = new Uint8Array(1024);
const writer = new GifWriter(output, 2, 2, {
	palette: [0x000000, 0xffffff],
	loop: 0,
});

writer.addFrame(0, 0, 2, 2, [0, 1, 1, 0], { delay: 8 });
writeFileSync("output.gif", output.subarray(0, writer.end()));
```

This is the omggif-style low-level API. Plain arrays and Node `Buffer` also work
as output buffers.

## Encode normal RGBA frames

```ts
import { encodeRgbaGifFrames } from "wtfgif";

const gif = encodeRgbaGifFrames({
	width: 128,
	height: 128,
	frames: [frame0, frame1], // Uint8Array or Uint8ClampedArray RGBA pixels
	delay: 8,
	loop: 0,
	delta: true, // write only changed rectangles when possible
});
```

Use `encodeIndexedGifFrames()` when your pixels are already palette indexes.

## Prepare visible animation frames

```ts
const reader = new GifReader(gifBytes);
const prepared = reader.preparePlayback({ format: "rgba" });

for (let frame = 0; frame < reader.numFrames(); frame++) {
	const rgba = prepared.getFrameBytes(frame);
	// rgba is the fully composited visible frame.
}

prepared.dispose();
reader.dispose();
```

## The 100x result

This benchmark measures a real cold job: read one GIF, decode every image
descriptor, and freshly LZW-reencode it. It is not copying the source payload.

```bash
BENCH_ITERATIONS=31 BENCH_REENCODE_ONLY=1 npm run bench
```

| Fixture | wtfgif faster than omggif |
| --- | ---: |
| 18d | 142.65x |
| Clap | **100.97x** |
| Homer | 191.85x |
| Chip | 200.35x |
| GIG | 180.09x |
| NOD | 111.37x |
| Proud | 229.79x |
| cat | 180.53x |
| excuse | 117.74x |
| blob | 126.32x |
| parrot | 149.29x |
| tenor | 214.14x |
| **Geometric mean** | **156.69x** |

The slowest result is 100.97x. All 12 fixtures clear 100x.

### Why this is proof, not just a stopwatch

- 31 fresh Node processes run per implementation and fixture.
- There are zero warmups and exactly one timed operation per process.
- Input reads and imports happen before the timer.
- Correctness happens after the timer: the source and reencoded GIF are decoded
  with omggif, then their dimensions, frame counts, and complete RGBA byte
  streams are SHA-256 hashed and compared.
- A mismatch aborts the benchmark instead of reporting a speedup.

The complete methodology is in [BENCHMARKS.md](BENCHMARKS.md).

### Important scope

The 100x table uses the experimental Node native addon on the development
machine. The npm package itself ships portable JavaScript and WebAssembly, not
a prebuilt native binary. Clone this repository to reproduce the native result;
`npm run bench` builds the addon before running.

Fast literal LZW output is usually larger than normal compressed GIF output.
The optimization trades file size for cold one-off speed while preserving
decoded pixels exactly.

## Why it is fast (ELI5)

omggif does a lot of tiny jobs one at a time in JavaScript. wtfgif moves the
heaviest loops into compiled Rust/C, prepares palette colors once, writes four
color bytes together, and uses a fast mode that avoids searching for the
smallest possible compression dictionary.

Think of omggif as carefully folding every shirt before packing it. wtfgif uses
a conveyor belt: the same shirts arrive intact, much faster, but the box can be
bigger.

## Backends

- JavaScript is always available.
- Node CommonJS and ESM automatically use the packaged WebAssembly core.
- Browser apps can call `initializeWasmGlobally()` and
  `installWasmCoreBackend()`.
- The native addon is experimental and injected with `setNativeAddonModule()`.

## Verify the release

```bash
npm run check
npm run test:wasm-core
npm publish --dry-run
```

The release gate covers omggif parity, pixel parity, JavaScript, Rust,
WebAssembly, CommonJS, ESM, browser loading, external TypeScript consumers, and
the installed npm tarball.

See [CHANGELOG.md](CHANGELOG.md) and [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE)
