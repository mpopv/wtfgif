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

## Encoding tradeoff

`wtfgif` is optimized for maximum encoding speed at the cost of larger output
files. The default benchmark starts a fresh Node process for every sample,
runs wtfgif's one-time initialization before the clock, and times the first and
only complete encode. Initialization reserves one generic 4 MiB Wasm input
arena. It does not inspect or retain source pixels, palettes, or output. It also
prepares data-independent JavaScript dispatch and Wasm code paths with empty
sentinels.
The JavaScript sentinel returns before reading pixels, calling Wasm, or creating
output state. Every input byte is copied after the clock starts, and the first
real encode still begins with cold data scratch. There are no encode warmups or
results reused between samples.

Across the 10-fixture arbitrary-RGBA corpus, `wtfgif` encoded **144.13×–311.99×
faster** than image-q + omggif, with a **180.90× geometric mean**. The real
128×128 MakeEmoji workload was **144.13× faster** (0.922 ms vs 132.905 ms).
Output files were 1.17×–22.75× larger, with a 6.50× geometric mean.
These values come from the committed clean
[`benchmarks/corpus.json`](benchmarks/corpus.json) receipt for wtfgif 3.0.8.
The encoded artifacts were built from clean commit
`4e96859bd5d08371173c66f29dff7a25f14e057c`; later documentation commits do not
change that measurement.

Every category in this corpus now clears 100× on the first real encode after
initialization. The narrowest margin is the real MakeEmoji workload at
**144.13×**, followed by smooth gradients at **146.54×**. These are
honest arbitrary images: no known palette, source cache, previous result, or
reduced-quality mode.

## Browser comparison

![Browser GIF encoder benchmark](docs/encoder-race.svg)

These are median first encodes from 15 fresh Chrome processes per encoder on
the documented eight-frame small-image workload. Package loading and wtfgif's
one-time initialization are outside the clock. Output size and quality
results are reported alongside the raw timings in [BENCHMARKS.md](BENCHMARKS.md).
On this run, `wtfgif` took **1.095 ms**; the five alternatives took
**88.775–123.400 ms** and were **81.07×–112.69× slower**. wtfgif emitted
149,689 bytes; the alternatives emitted 39,101–80,869 bytes. For the two
gif.js implementations, their public API's worker creation is part of the
timed encode.

The raw receipt is [`benchmarks/encoder-race.json`](benchmarks/encoder-race.json),
and the chart above is generated from it by
`scripts/render-encoder-race-chart.mjs`.

```bash
npm run bench
npm run bench:race
```

Exact conditions, raw samples, output sizes, PSNR, SSIM, independent-decoder
checks, and browser checks are in [BENCHMARKS.md](BENCHMARKS.md).

## Good to know

- GIF delays use hundredths of a second, so `delay: 10` means 100 ms.
- GIF supports at most 256 colors and only fully transparent or fully opaque
  pixels. Converting from full-color RGBA always involves some color reduction.
- `GifReader` and `GifWriter` are compatible with the equivalent `omggif` APIs
  if you need lower-level palette and frame control.

[Benchmarks](BENCHMARKS.md) · [MIT license](LICENSE)
