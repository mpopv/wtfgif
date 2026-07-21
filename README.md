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

## Encode images

Give wtfgif one flat RGBA buffer containing every frame:

```ts
import { encodeRgbaGifFrames } from "wtfgif";

const gif = encodeRgbaGifFrames({
	width,
	height,
	frames: rgbaFrames,
	frameCount,
	delay: 10,
	loop: 0,
	quantization: "quality",
	paletteMode: "global",
	compression: "balanced",
});
```

Use `quantization: "fast"` with `compression: "fast"` for maximum speed. It
uses fewer colors accurately and produces a much larger GIF. Use `quality` and
`balanced` for the normal default.

GIF itself is limited to 256 colors per palette and binary transparency, so no
GIF encoder can preserve every full-color source pixel exactly.

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
128×128 RGBA frames. Palette creation, pixel mapping, and GIF compression are
all timed. WebAssembly initialization and image loading are not.

| Real-image encode | Baseline | wtfgif | Speedup | wtfgif output |
| --- | ---: | ---: | ---: | ---: |
| Quality | 93.062 ms | 2.985 ms | **31.17×** | 45,730 bytes / 32.71 dB |
| Turbo, exact decoded parity | 5.299 ms | 0.282 ms | **18.82×** | 149,601 bytes / 19.55 dB |

The Quality baseline is `image-q` plus omggif. Turbo gives both encoders the
same RGB332 pixels; wtfgif is faster because its literal LZW mode trades file
size for CPU time.

If your frames are already palette-indexed—the direct `GifWriter` contract—
wtfgif is **110.37× faster**: 16.827 ms for omggif versus 0.152 ms for wtfgif.
That result is byte-decoded and checked for exact RGBA equality before timing.

Decode remains workload-dependent: **1.77×** on the included 198-frame
128×128 GIF and **4.39×** on the included 16-frame 498×498 GIF, with exact
composited RGBA parity.

```bash
npm run bench
npm run bench:encode
BENCH_GIF_FILTER=GIGACHAD npm run bench:decode
```

See [BENCHMARKS.md](BENCHMARKS.md) for every condition, output-size tradeoff,
and reproduction command.

[MIT](LICENSE)
