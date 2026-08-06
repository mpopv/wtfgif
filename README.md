# wtfgif

Fast GIF encode and decode for Node.js, browsers, Workers, and edge runtimes.

```bash
npm install wtfgif
```

## Encode

Initialize WebAssembly once when your app starts. Then give wtfgif ordinary
RGBA images—no palette or preprocessing required.

```ts
import { encodeRgbaGifFrames, initializeWasmGlobally } from "wtfgif/encode";

await initializeWasmGlobally();

const gif = encodeRgbaGifFrames({
	width: 128,
	height: 128,
	frames: rgbaFrames, // one flat RGBA buffer containing every frame
	frameCount: 8,
	delay: 10, // hundredths of a second; may also be one value per frame
	loop: 0,
});
```

`wtfgif/encode` has one path: adaptive global palette creation in Wasm followed
by lossless literal LZW. It does not require a known palette, skip pixels, reuse
an earlier result, or lower its palette quality to win the benchmark.

GIF supports at most 256 colors in a palette and only binary transparency, so
no GIF encoder can preserve every full-color RGBA source pixel exactly. Literal
LZW is lossless after palette mapping; the speed tradeoff is a larger file, not
lower visual quality.

## Decode

`GifReader` is an omggif-compatible decoder:

```ts
import { GifReader } from "wtfgif";

const reader = new GifReader(gifBytes);
const rgba = new Uint8ClampedArray(reader.width * reader.height * 4);

for (let frame = 0; frame < reader.numFrames(); frame += 1) {
	reader.decodeAndBlitFrameRGBA(frame, rgba);
}
```

## Speed

The normal encode benchmark uses eight real MakeEmoji images at 128×128. Both
sides create a palette, map every RGBA pixel, and write a valid GIF.

| Boundary | image-q + omggif | wtfgif | Speedup |
| --- | ---: | ---: | ---: |
| Initialized encode | 94.632 ms | 0.376 ms | **251.68×** |
| First real encode after initialization | 121.707 ms | 2.122 ms | **57.36×** |
| Fresh worker operation | 135.744 ms | 3.463 ms | **39.20×** |
| Complete process wall clock | 164.648 ms | 30.355 ms | **5.42×** |
| Fresh worker, 10 × 512×512 stress encode | 6,171.189 ms | 28.650 ms | **215.40×** |
| Complete stress-process wall clock | 6,211.252 ms | 58.928 ms | **105.40×** |

The normal wtfgif output is 149,601 bytes at 34.12 dB PSNR. The baseline is
39,350 bytes at 31.84 dB. In other words, the initialized path is over 200×
faster and produces the higher-quality pixels, but the file is larger. The
fresh-worker clock includes fixture loading, package import, Wasm initialization,
and encoding after Node starts. The process-wall clock also includes launching
Node itself; that fixed startup dominates the small real-image job.

```bash
npm run bench
BENCH_WTFFIG_ENTRY=encode BENCH_ITERATIONS=50 node scripts/bench-cold-rgba.mjs
```

See [BENCHMARKS.md](BENCHMARKS.md) for exact conditions and more results.

[Browser demo](https://mpopv.github.io/wtfgif/) · [MIT](LICENSE)
