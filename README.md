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

## 100× faster on the first encode

Both encoders start with the same eight real 128×128 RGBA images. They must
choose a palette, map every pixel, compress every frame, and return a valid GIF.
Wasm initialization happens first, then the clock measures exactly one encode.
There are no encode warmups, saved palettes, retained pixels, or cached results.

| 500 fresh Node processes | image-q + omggif | wtfgif | Speedup |
| --- | ---: | ---: | ---: |
| First real encode | 123.448 ms | **1.231 ms** | **100.26×** |

For repeated encoding after that first call, the same job takes 94.302 ms with
image-q + omggif and 0.315 ms with wtfgif: **298.94× faster**.

## Browser comparison

![Browser GIF encoder benchmark](docs/encoder-race.svg)

These are median first encodes from 15 fresh Chrome processes per encoder, with
package loading and wtfgif's one-time Wasm initialization outside the clock.

| Encoder | First encode | Slower than wtfgif |
| --- | ---: | ---: |
| **wtfgif** | **1.545 ms** | — |
| [gif.js 0.2.0](https://github.com/jnordberg/gif.js) | 95.865 ms | **62.05×** |
| image-q + [omggif 1.0.10](https://github.com/deanm/omggif) | 98.055 ms | **63.47×** |
| [gif.js.optimized 1.0.1](https://github.com/terikon/gif.js.optimized) | 107.510 ms | **69.59×** |
| [gifenc 1.0.3](https://github.com/mattdesl/gifenc) | 122.835 ms | **79.50×** |
| [modern-gif 2.1.0](https://github.com/qq15725/modern-gif) | 131.525 ms | **85.13×** |

wtfgif's output is 149,689 bytes at 34.12 dB PSNR with 100% alpha agreement.
It favors encode latency and visual quality over producing the smallest file.

```bash
npm run bench
npm run bench:race
```

Exact conditions, raw browser samples, output sizes, and quality results are in
[BENCHMARKS.md](BENCHMARKS.md).

## Good to know

- GIF delays use hundredths of a second, so `delay: 10` means 100 ms.
- GIF supports at most 256 colors and only fully transparent or fully opaque
  pixels. Converting from full-color RGBA always involves some color reduction.
- `wtfgif` favors very fast encoding and high visual quality over the smallest
  possible file size.
- `GifReader` and `GifWriter` are compatible with the equivalent `omggif` APIs
  if you need lower-level palette and frame control.

[Benchmarks](BENCHMARKS.md) · [MIT license](LICENSE)
