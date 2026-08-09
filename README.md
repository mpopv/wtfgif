# wtfgif

`wtfgif` is a JavaScript/TypeScript library for making and reading GIF files in
Node.js, browsers, Workers, and edge runtimes. It is deliberately optimized for
minimum encoding latency, not minimum file size.

On the current 10-workload receipt, the first encode after initialization is
**183.00×–877.22× faster** than image-q + omggif. The resulting files are
**1.17×–22.75× larger**. The benchmark includes arbitrary RGBA photographs,
pixel art, gradients, transparency, noise, tiny animations, and a one-megapixel
animation—not known palettes or cached results.

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

## Speed and output size

The default benchmark starts a fresh Node process for every sample, completes
wtfgif's one-time initialization before the clock, and then times the first and
only user-input encode. Palette creation, pixel mapping, LZW, and complete GIF
assembly are timed. Package loading and initialization are not.

Initialization costs about 15 ms once on the benchmark machine. It prepares
code with fixed synthetic inputs and reserves a generic 4 MiB Wasm input arena;
it never inspects or retains user pixels, palettes, or encoded results. The
current dominant-palette preparation adds about 1.6 ms of that one-time work
and retains a 1.0338× first-encode gain after 64 MiB cache eviction, with
identical GIF bytes. See
[`benchmarks/runtime-preparation.json`](benchmarks/runtime-preparation.json).

Across the 10 arbitrary-RGBA workloads, wtfgif is **183.00×–877.22× faster**
than image-q + omggif, with a **270.99× geometric mean**. The real 128×128
MakeEmoji workload is **199.38× faster** (0.676 ms vs 134.761 ms). Output files
are **1.17×–22.75× larger**, with a **6.50× geometric mean**.

![wtfgif speedup across the arbitrary-RGBA corpus](docs/corpus-speedup.svg)

Bar length is speedup over image-q + omggif; every label also reports the
file-size ratio. Both the chart and values above are generated from the clean
40-process [`benchmarks/corpus.json`](benchmarks/corpus.json) receipt.

The measured artifact is wtfgif 3.0.11 at clean source commit
`53ddd2cdd9e5145ea1ae3fc91a18edbee6bca880`. The following commit updates
receipts, documentation, and charts only.

## Browser comparison

![Browser GIF encoder benchmark](docs/encoder-race.svg)

These are median first encodes from 15 fresh Chrome processes per encoder on
the same eight-frame MakeEmoji workload. Package loading and wtfgif
initialization are outside the clock. wtfgif took **0.730 ms**; the five
alternatives took **98.815–137.695 ms** and were **135.36×–188.62× slower**.
wtfgif emitted 149,689 bytes; the alternatives emitted 39,101–80,869 bytes.
gif.js worker creation is part of its public timed operation. Exact settings,
quality results, and raw samples are in [BENCHMARKS.md](BENCHMARKS.md).

The raw receipt is [`benchmarks/encoder-race.json`](benchmarks/encoder-race.json),
and the chart above is generated from it by
`scripts/render-encoder-race-chart.mjs`.

```bash
npm run bench
npm run bench:race
npm run bench:charts
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
