# wtfgif

`wtfgif` is a JavaScript/TypeScript library for making and reading GIF files.
It works in Node.js, browsers, Workers, and edge runtimes.

- To make a GIF, give it one or more images as RGBA pixel arrays.
- To read a GIF, give it the file bytes and get RGBA pixel arrays back.

```bash
npm install wtfgif
```

## Make a GIF

This example creates a two-frame GIF and saves it in Node.js:

```ts
import { writeFile } from "node:fs/promises";
import { encodeRgbaGifFrames, initializeWasmGlobally } from "wtfgif/encode";

await initializeWasmGlobally(); // Do this once when your app starts.

const width = 128;
const height = 128;

function solidFrame(red: number, green: number, blue: number) {
	const frame = new Uint8Array(width * height * 4);
	for (let i = 0; i < frame.length; i += 4) {
		frame.set([red, green, blue, 255], i);
	}
	return frame;
}

const gif = encodeRgbaGifFrames({
	width,
	height,
	frames: [solidFrame(255, 0, 0), solidFrame(0, 0, 255)],
	delay: 50, // 50 hundredths of a second = 500 ms per frame
	loop: 0, // Repeat forever
});

await writeFile("output.gif", gif);
```

Each frame must contain `width * height * 4` bytes: red, green, blue, and
alpha for every pixel. In a browser, `canvas.getContext("2d").getImageData()`
provides pixels in this format.

## Read or process a GIF

`preparePlayback()` gives you complete frames with GIF transparency and frame
placement already handled:

```ts
import { readFile } from "node:fs/promises";
import { GifReader } from "wtfgif";

const file = await readFile("input.gif");
const reader = new GifReader(file);
const decoded = reader.preparePlayback();

console.log(reader.width, reader.height, reader.numFrames());

for (let i = 0; i < reader.numFrames(); i += 1) {
	const rgba = new Uint8Array(reader.width * reader.height * 4);
	decoded.copyFrame(i, rgba);

	// Read, edit, resize, analyze, or draw this frame here.
	console.log(`frame ${i}: ${decoded.frames[i]!.delay * 10} ms`, rgba);
}

decoded.dispose();
reader.dispose();
```

To save edited frames as a new GIF, collect the changed RGBA arrays and pass
them to `encodeRgbaGifFrames()`. Use the decoded frame delays and
`reader.loopCount()` if you want to preserve the original timing.

In a browser, get GIF bytes with
`new Uint8Array(await file.arrayBuffer())`. To turn encoded bytes into a file
or URL, use `new Blob([gif], { type: "image/gif" })`.

## Speed

![Browser GIF encoder benchmark](docs/encoder-race.svg)

This race starts with eight real 128×128 RGBA images. Every encoder must choose
colors, map pixels, compress all eight frames, and return GIF bytes. Each number
is the median first encode from 15 fresh Chrome processes, after package loading
and wtfgif's one-time Wasm initialization, with zero encode warmups.

| Encoder | Median | wtfgif advantage | Bytes | PSNR | Alpha match |
| --- | ---: | ---: | ---: | ---: | ---: |
| **wtfgif (current main)** | **1.660 ms** | — | 149,617 | 34.12 dB | 100% |
| [gif.js 0.2.0](https://github.com/jnordberg/gif.js) | 83.150 ms | **50.09×** | 80,869 | 33.08 dB | 99.78% |
| image-q + [omggif 1.0.10](https://github.com/deanm/omggif) | 89.755 ms | **54.07×** | 39,350 | 31.84 dB | 100% |
| [gif.js.optimized 1.0.1](https://github.com/terikon/gif.js.optimized) | 95.215 ms | **57.36×** | 80,304 | 32.20 dB | 99.76% |
| [modern-gif 2.1.0](https://github.com/qq15725/modern-gif) | 119.160 ms | **71.78×** | 43,114 | 32.65 dB | 100% |
| [gifenc 1.0.3](https://github.com/mattdesl/gifenc) | 120.135 ms | **72.37×** | 39,101 | 34.54 dB | 100% |

The tradeoff is file size: wtfgif emits a larger file because its literal LZW
path prioritizes encode latency. It does not lower the selected pixel quality;
its 34.12 dB result is second only to gifenc here. The two gif.js variants only
support color-key transparency, which is why their alpha match is not 100%.

The narrower initialized Node benchmark against image-q + omggif is 95.713 ms
versus 0.352 ms: **272.17× faster**. Its zero-warmup first encode is 124.994 ms
versus 1.521 ms: **82.17× faster**.

```bash
npm run bench
npm run bench:race
```

Full conditions and additional results are in [BENCHMARKS.md](BENCHMARKS.md).

## Good to know

- GIF delays use hundredths of a second, so `delay: 10` means 100 ms.
- GIF supports at most 256 colors and only fully transparent or fully opaque
  pixels. Converting from full-color RGBA always involves some color reduction.
- `wtfgif` favors very fast encoding and high visual quality over the smallest
  possible file size.
- `GifReader` and `GifWriter` are compatible with the equivalent `omggif` APIs
  if you need lower-level palette and frame control.

[Benchmarks](BENCHMARKS.md) · [MIT license](LICENSE)
