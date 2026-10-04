# wtfgif

`wtfgif` encodes and decodes GIF files in Node.js, browsers, Workers, and edge
runtimes. Its one encoder gives maximum speed without a lower-quality speed
mode, at the cost of larger files. It is more than 200× faster than the tested
encoders.

## Install

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

## Browser and edge runtimes

The fast encoder is WebAssembly. In Node.js and regular browser bundlers, call
the same `initializeWasmGlobally()` function shown above once during app or
worker startup. It selects SIMD when the runtime supports it and otherwise uses
the scalar Wasm build. Encoding is synchronous after that promise resolves.

Cloudflare Workers, Vercel Edge, and other runtimes that require a static Wasm
module can initialize that module explicitly:

```ts
import { encodeRgbaGifFrames, initializeWasmModule } from "wtfgif/encode";
import initWasm, * as wasm from "wtfgif/wasm-encode";
import wasmModule from "wtfgif/wasm-encode/wasm";

await initializeWasmModule({ ...wasm, default: initWasm }, wasmModule);
```

No native addon is required in any of these environments.

## Speed and output size

The default benchmark starts a fresh Node process for every sample, completes
wtfgif's one-time initialization before the clock, and then times the first and
only user-input encode. Palette creation, pixel mapping, LZW, and complete GIF
assembly are timed. Package loading and initialization are not. Before timing,
the harness evicts 64 MiB of unrelated memory and yields one event-loop turn
without touching the encoder or fixture.

Initialization prepares code with fixed synthetic inputs and reserves a generic
4 MiB Wasm input arena. It never inspects or retains user pixels, palettes, or
encoded results. The exact preparation A/B receipts live in
[BENCHMARKS.md](BENCHMARKS.md); they are kept out of the headline result above.

<!-- benchmark:readme-corpus:start -->
Across the 10 arbitrary-RGBA workloads, wtfgif is **245.95×–735.05× faster**
than image-q + omggif, with a **353.10× geometric mean**. The real 128×128
MakeEmoji workload is **365.94× faster** (0.388 ms vs 141.894 ms). Output files
are **1.17×–22.75× larger**, with a **6.50× geometric mean**.

![wtfgif speedup across the arbitrary-RGBA corpus](docs/corpus-speedup.svg)

Bar length is speedup over image-q + omggif. Each label also gives both median
encode times, the file-size ratio, and source-relative RGB quality. The chart
and values come from the recorded 40-process
[`benchmarks/corpus.json`](benchmarks/corpus.json) receipt. Shape, frame timing,
and binary transparency must be exact. PSNR and SSIM show the color reduction
that occurs when arbitrary RGBA pixels become a GIF palette.

The receipt identifies wtfgif 3.0.20 at source commit
`f626ac2fac04405396e6a19f242a77ea55658858`. Its dirty flag is false.
<!-- benchmark:readme-corpus:end -->

Current profiling puts about **88%** of the MakeEmoji first-encode latency
inside the Wasm encoder. Reserving its arena, copying the independent RGBA
frames in, and copying the GIF out together take about 0.04 ms. Within Wasm,
about 42% of samples are in palette-histogram construction, 22% in
nearest-palette search, and 20% in literal LZW mapping and emission. Those are the active
optimization targets; the JavaScript boundary is no longer the bottleneck.
The phase timings, sampling method, and rejected exact-output experiments are
recorded in [BENCHMARKS.md](BENCHMARKS.md).

## Browser comparison

<!-- benchmark:readme-browser:start -->
![Browser GIF encoder benchmark](docs/encoder-race.svg)

These are public-API time-to-result medians from 15 fresh Chrome processes per
encoder on the same 8-frame MakeEmoji workload. Package loading and wtfgif
initialization are outside the clock. Each process evicts 64 MiB of unrelated
memory and waits one animation frame before timing. wtfgif took **0.445 ms**;
the five alternatives took **90.700–127.965 ms** and were
**203.82×–287.56× slower**.
wtfgif emitted 149,689 bytes. The alternatives emitted 39,101–80,869 bytes.
The dashed line marks 100× wtfgif's measured latency. The chart gives output
size, PSNR, and alpha agreement with every timing. gif.js worker creation is
part of its public timed operation. [BENCHMARKS.md](BENCHMARKS.md) gives the
exact settings and raw samples.

The raw receipt is [`benchmarks/encoder-race.json`](benchmarks/encoder-race.json).
`scripts/bench/render-encoder-race-chart.mjs` generates the chart from it.
<!-- benchmark:readme-browser:end -->

```bash
npm run bench
npm run bench:race
npm run bench:charts
```

These receipts support the exact workloads, hardware, runtimes, and timing
boundaries shown here; they are not codec-only or matched-file-size claims.
Exact conditions, raw samples, output sizes, PSNR, SSIM, independent-decoder
checks, and browser checks are in [BENCHMARKS.md](BENCHMARKS.md).

## Good to know

- GIF delays use hundredths of a second, so `delay: 10` means 100 ms.
- GIF supports at most 256 colors and only fully transparent or fully opaque
  pixels. Converting from full-color RGBA always involves some color reduction.
- Encoding always uses wtfgif's latency-first literal-LZW path. Smaller output
  is not an alternate mode of this library.
- `GifReader` and `GifWriter` are compatible with the equivalent `omggif` APIs
  if you need lower-level palette and frame control.

[Benchmarks](BENCHMARKS.md) · [MIT license](LICENSE)
