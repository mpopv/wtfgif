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

The benchmark starts with arbitrary RGBA images. Neither encoder is given a
palette: both must choose colors, map every pixel, and write a valid GIF.
The baseline is `image-q` plus `omggif`.

| Boundary | Baseline | wtfgif | Faster |
| --- | ---: | ---: | ---: |
| First normal encode after Wasm initialization, zero warmups | 130.470 ms | 2.333 ms | **55.92×** |
| Normal encode in a fresh worker | 145.221 ms | 3.780 ms | **38.42×** |
| Complete fresh normal process | 175.615 ms | 33.077 ms | **5.31×** |
| Initialized normal throughput | 100.554 ms | 0.430 ms | **233.64×** |
| First 10 × 512×512 stress encode, zero warmups | 6,516.296 ms | 11.184 ms | **582.66×** |
| Complete fresh stress process | 7,285.050 ms | 71.463 ms | **101.94×** |

The normal wtfgif output is 149,601 bytes at 34.12 dB PSNR. The stress output
is 2,973,381 bytes at 26.19 dB. Literal LZW preserves the selected pixels
exactly; wtfgif trades a larger file for speed, not lower pixel quality.

```bash
npm run bench
BENCH_INITIALIZED_FIRST=1 BENCH_WTFFIG_ENTRY=encode BENCH_ITERATIONS=60 node scripts/bench-cold-rgba.mjs
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

[Try the browser demo](https://mpopv.github.io/wtfgif/) ·
[Benchmarks](BENCHMARKS.md) · [MIT license](LICENSE)
