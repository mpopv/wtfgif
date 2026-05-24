# wtfgif

A fast, TypeScript-friendly GIF decoder and encoder for Node and browsers. It is
compatible with the core [omggif](https://www.npmjs.com/package/omggif)
`GifReader` and `GifWriter` style, and also includes simpler helpers for common
app tasks like "make frames from a GIF" and "make a GIF from frames".

## Installation

```bash
npm install wtfgif
```

## Import

```ts
import {
	GifReader,
	GifWriter,
	encodeRgbaGifFrames,
	encodeIndexedGifFrames,
} from "wtfgif";
```

There is no default export. Browser builds also expose `window.wtfgif`.

## Plain English Concepts

- **RGBA frames** are normal image pixels: red, green, blue, alpha. This is what
  browser canvas `ImageData` uses. If you do not know GIF internals, use RGBA.
- **Indexed frames** are frames where each pixel is a number pointing into a
  color list. This is closer to how GIF stores pixels. Most app code should not
  start here unless it already has color-number frames.
- **Palette** means the list of colors a GIF can use. In this library it is an
  array of `0xRRGGBB` numbers, for example `[0x000000, 0xffffff]`.
- **Delta** means "mostly static frames". If only a small part of the image
  changes from frame to frame, `delta: true` lets wtfgif write only the changed
  area. Use this for stickers, blinking text, small moving sprites, cursors, and
  UI captures with a static background.
- **Wasm/native backend** means the optional Rust/WebAssembly implementation.
  The public option is currently named `backend: "native"`, but it selects Wasm.
  `backend: "auto"` uses Wasm when available and falls back to JavaScript.

## Common Tasks

### Get Basic GIF Info

```ts
import { readFileSync } from "node:fs";
import { GifReader } from "wtfgif";

const data = readFileSync("input.gif");
const reader = new GifReader(data);

console.log(reader.width, reader.height);
console.log(reader.numFrames());
console.log(reader.loopCount()); // null means the GIF has no loop extension

reader.returnToPool();
```

### Make Frames From A GIF

Use `preparePlayback()` when you want the visible frames of the animation. This
handles GIF frame positioning and disposal for you.

```ts
import { readFileSync } from "node:fs";
import { GifReader } from "wtfgif";

const data = readFileSync("input.gif");
const reader = new GifReader(data);

const prepared = reader.preparePlayback({ format: "rgba" });
const frames: Uint8Array[] = [];

for (let i = 0; i < reader.numFrames(); i++) {
	const bytes = prepared.getFrameBytes(i);
	if (!bytes) throw new Error(`Missing frame ${i}`);
	frames.push(new Uint8Array(bytes));
}

prepared.dispose();
reader.returnToPool();
```

Each frame is `reader.width * reader.height * 4` bytes.

### Make A GIF From Normal RGBA Frames

Use `encodeRgbaGifFrames()` when your frames come from canvas, screenshots,
video frames, generated images, or any other normal pixel source.

```ts
import { encodeRgbaGifFrames } from "wtfgif";

const width = 128;
const height = 128;
const frames: Uint8Array[] = [
	new Uint8Array(width * height * 4),
	new Uint8Array(width * height * 4),
];

// Fill frames with RGBA bytes before encoding.

const gif = encodeRgbaGifFrames({
	width,
	height,
	frames,
	delay: 8, // GIF delay units are hundredths of a second
	loop: 0, // 0 means loop forever; null/undefined means omit loop metadata
});
```

If you know the exact colors you want the GIF to use, pass a palette:

```ts
const gif = encodeRgbaGifFrames({
	width,
	height,
	frames,
	palette: [0x000000, 0xffffff, 0xff0000],
});
```

If you do not pass a palette, wtfgif builds one when possible. If the frames use
more than 256 colors, it falls back to a simple 256-color palette.

For mostly static animations:

```ts
const gif = encodeRgbaGifFrames({
	width,
	height,
	frames,
	delay: 8,
	loop: 0,
	delta: true,
});
```

### Make A GIF From Color-Number Frames

Use `encodeIndexedGifFrames()` only when your pixels are already palette color
numbers. This is useful in image pipelines, game tooling, and GIF-specific code.

```ts
import { encodeIndexedGifFrames } from "wtfgif";

const width = 2;
const height = 2;
const palette = [0x000000, 0xffffff];

const frame0 = new Uint8Array([0, 1, 1, 0]);
const frame1 = new Uint8Array([1, 0, 0, 1]);

const gif = encodeIndexedGifFrames({
	width,
	height,
	frames: [frame0, frame1],
	palette,
	delay: 10,
	loop: 0,
});
```

For mostly static indexed frames:

```ts
const gif = encodeIndexedGifFrames({
	width,
	height,
	frames: [frame0, frame1],
	palette,
	delta: true,
});
```

### Decode One Frame Into A Buffer

This mirrors omggif's low-level API. The target buffer must be large enough for
the full GIF canvas: `reader.width * reader.height * 4`.

```ts
const pixels = new Uint8Array(reader.width * reader.height * 4);
reader.decodeAndBlitFrameRGBA(0, pixels);
```

Use `decodeAndBlitFrameBGRA()` if you need BGRA byte order.

### Repeated Playback Or Scrubbing

Use prepared playback when the same GIF will be displayed repeatedly, scrubbed
on a timeline, or previewed frame by frame.

```ts
const prepared = reader.preparePlayback({
	format: "rgba",
	backend: "auto",
	deltas: true,
});

const player = prepared.createPlayer();
const firstFramePixels = player.drawFrame(0);
const secondFramePixels = player.next();

prepared.dispose();
```

`copyFrame(index, target)` copies a prepared frame into your own `Uint8Array` or
`Uint32Array`.

## API Reference

### `new GifReader(data, usePooling?)`

Parses GIF metadata and prepares the decoder.

```ts
const reader = new GifReader(data);
```

- `data`: `Uint8Array`
- `usePooling`: optional boolean, default `true`. Reuses decoder work buffers.

Static constructors:

- `GifReader.createPooled(data)` creates a pooled reader.
- `GifReader.createUnpooled(data)` creates a reader without pooled buffers.

Properties:

- `reader.width`: GIF canvas width.
- `reader.height`: GIF canvas height.

Methods:

| Method | Purpose |
| --- | --- |
| `numFrames()` | Returns the number of frames. |
| `loopCount()` | Returns the GIF loop count, or `null` if not present. |
| `frameInfo(index)` | Returns metadata for one frame. |
| `decodeAndBlitFrameRGBA(index, target)` | Decodes one frame into RGBA bytes. |
| `decodeAndBlitFrameBGRA(index, target)` | Decodes one frame into BGRA bytes. |
| `decodeFrameToTransferableRGBA(index)` | Returns a new transferable `ArrayBuffer` with RGBA bytes. |
| `decodeFrameToTransferableBGRA(index)` | Returns a new transferable `ArrayBuffer` with BGRA bytes. |
| `decodeFrameIntoBuffer(index, buffer, format?)` | Decodes into an existing `ArrayBuffer`. |
| `decodeAndBlitCompositedFrameRGBA(index, target)` | Copies the visible animation frame into RGBA bytes. |
| `decodeAndBlitCompositedFrameBGRA(index, target)` | Copies the visible animation frame into BGRA bytes. |
| `preparePlayback(options?)` | Prepares visible animation frames for fast playback. |
| `prepareFrames(options?)` | Lower-level prepared-frame API. |
| `prepareFramesAsync(options?)` | Promise wrapper around `prepareFrames()`. |
| `dispose()` | Returns pooled buffers and frees reader-owned temporary state. |
| `returnToPool()` | Alias for `dispose()`. |

`frameInfo(index)` returns:

```ts
type FrameInfo = {
	x: number;
	y: number;
	width: number;
	height: number;
	delay: number;
	disposal: number;
	transparent_index: number | null;
	interlaced: boolean;
	has_local_palette: boolean;
	palette_offset: number;
	palette_size: number;
	data_offset: number;
	data_length: number;
	min_code_size: number;
};
```

### `preparePlayback(options?)` and `prepareFrames(options?)`

These APIs prepare frames once so repeated playback, scrubbing, and copying are
cheap.

Options:

```ts
type PrepareFramesOptions = {
	format?: "rgba" | "bgra";
	frameIndices?: readonly number[];
	maxBytes?: number;
	backend?: "auto" | "javascript" | "native";
	deltas?: boolean;
	dedupe?: "none" | "adjacent" | "all";
	cache?: "auto" | "indices" | "rgba" | "sparse-rgba" | "composited";
	composited?: boolean;
};
```

- `format`: output byte order. Default is `"rgba"`.
- `frameIndices`: prepare only selected frames.
- `maxBytes`: throw if preparation would use more than this many bytes.
- `backend`: `"auto"` uses Wasm when available; `"javascript"` forces JS;
  `"native"` requires the optional Wasm backend.
- `deltas`: store only changed areas for mostly static animations when possible.
- `dedupe`: reuse identical frame pixel buffers.
- `cache`: controls the internal prepared-frame storage shape.
- `composited`: whether frames should represent the full visible animation
  state. `preparePlayback()` always sets this to `true`.

The returned object:

```ts
type PreparedGifFrames = {
	width: number;
	height: number;
	format: "rgba" | "bgra";
	composited: boolean;
	frames: PreparedGifFrame[];
	byteLength: number;
	maxBytes: number | null;
	getFrame(index: number): PreparedGifFrame | undefined;
	getFramePixels(index: number): Uint32Array | undefined;
	getFrameBytes(index: number): Uint8Array | undefined;
	copyFrame(index: number, target: Uint8Array | Uint32Array): void;
	createPlayer(target?: Uint8Array | Uint32Array): PreparedGifPlayer;
	dispose(): void;
};
```

`PreparedGifPlayer`:

```ts
type PreparedGifPlayer = {
	target: Uint32Array;
	currentIndex: number;
	drawFrame(index: number): Uint32Array;
	next(): Uint32Array;
	reset(): void;
};
```

### `new GifWriter(buffer, width, height, options?)`

Low-level GIF writer compatible with the omggif style. Use this when you want to
stream frames into a caller-owned output buffer.

```ts
const out = new Uint8Array(1024 * 1024);
const writer = new GifWriter(out, width, height, {
	palette: [0x000000, 0xffffff],
	loop: 0,
});

writer.addFrame(0, 0, width, height, indexedPixels, { delay: 8 });
const length = writer.end();
const gif = out.slice(0, length);
```

Constructor options:

```ts
type GifWriterOptions = {
	palette?: number[] | null;
	loop?: number | null;
	background?: number;
};
```

Methods:

| Method | Purpose |
| --- | --- |
| `addFrame(x, y, width, height, indexedPixels, options?)` | Adds an indexed frame or sub-frame. |
| `addFrameDelta(indexedPixels, options?)` | Adds a full-canvas indexed frame, but writes only the changed area when possible. |
| `end()` | Writes the GIF trailer and returns the byte length. |
| `getOutputBuffer()` | Returns the writer's output buffer. |
| `setOutputBuffer(buffer)` | Replaces the writer's output buffer. |
| `getOutputBufferPosition()` | Returns the current write position. |
| `setOutputBufferPosition(position)` | Sets the current write position. |

Frame options:

```ts
type GifFrameOptions = {
	palette?: number[] | null;
	delay?: number;
	disposal?: number;
	transparent?: number | null;
};
```

- `palette`: local frame palette. If omitted, the writer uses the global
  palette from the constructor.
- `delay`: frame delay in hundredths of a second.
- `disposal`: GIF disposal mode, `0` through `3`.
- `transparent`: palette index to treat as transparent.

### `encodeRgbaGifFrames(options)`

High-level helper for the common app case: normal image frames in, GIF bytes out.

```ts
const gif = encodeRgbaGifFrames({
	width,
	height,
	frames,
	frameCount,
	palette,
	delay,
	loop,
	backend,
	delta,
});
```

Options:

```ts
type EncodeRgbaGifFramesOptions = {
	width: number;
	height: number;
	frames: Uint8Array | Uint8ClampedArray | (Uint8Array | Uint8ClampedArray)[];
	frameCount?: number;
	palette?: number[];
	delay?: number;
	loop?: number | null;
	backend?: "auto" | "javascript" | "native";
	delta?: boolean;
};
```

- `frames`: either one flat byte stream containing all frames, or an array of
  per-frame RGBA byte arrays.
- `frameCount`: required only when `frames` is a flat stream and you want an
  explicit count check.
- `palette`: optional `0xRRGGBB` color list. If omitted, wtfgif chooses one.
- `delay`: same delay for every frame, in hundredths of a second.
- `loop`: `0` loops forever. `null` or `undefined` omits loop metadata.
- `backend`: `"auto"` tries Wasm first, then JS. `"javascript"` forces JS.
  `"native"` requires Wasm and throws if it is unavailable.
- `delta`: use the mostly-static-frame path.

Returns a `Uint8Array` containing the complete GIF.

### `encodeIndexedGifFrames(options)`

High-level helper for frames that are already color numbers.

```ts
const gif = encodeIndexedGifFrames({
	width,
	height,
	frames,
	palette,
	delay,
	loop,
	backend,
	delta,
});
```

Options:

```ts
type EncodeIndexedGifFramesOptions = {
	width: number;
	height: number;
	frames: Uint8Array | (Uint8Array | number[])[];
	frameCount?: number;
	palette: number[];
	delay?: number;
	loop?: number | null;
	backend?: "auto" | "javascript" | "native";
	delta?: boolean;
};
```

Every pixel value must be a valid index into `palette`. Returns a `Uint8Array`
containing the complete GIF.

### Wasm Backend Helpers

The normal APIs work without calling these. Use them only when you are wiring the
optional Rust/Wasm core yourself or want to force/check the accelerated backend.

```ts
import {
	createWasmCoreDecodeBackend,
	installWasmCoreBackend,
	setWasmCoreModule,
	getWasmStatus,
	cleanupWasm,
} from "wtfgif";
```

| Function | Purpose |
| --- | --- |
| `setWasmCoreModule(moduleOrNull)` | Manually provides or clears the Rust/Wasm core module. |
| `createWasmCoreDecodeBackend()` | Creates a decode backend object for `GifReader.setDecodeBackend()`. |
| `installWasmCoreBackend()` | Installs the Wasm backend globally on `GifReader` and returns status. |
| `GifReader.setDecodeBackend(backendOrNull)` | Sets or clears the global prepared-frame decode backend. |
| `GifReader.getDecodeBackendStatus()` | Returns `{ name, available }` for the current backend. |
| `getWasmStatus()` | Returns support and initialization flags for the legacy Wasm loader. |
| `initializeWasmGlobally(path?)` | Initializes the legacy Wasm decoder loader. |
| `cleanupWasm()` | Terminates Wasm worker resources and clears global Wasm state. |

The option value `"native"` is kept for API compatibility, but in practice it
means "use the optional Rust/Wasm core".

### Advanced Reader Helpers

These methods are available for specialized browser, worker, or experimental
rendering flows:

- `reader.initWasm(path?)`
- `reader.isWasmReady()`
- `reader.framePixelsWasm(index, target?)`
- `reader.framePixelsParallel(indices)`
- `reader.getWasmStats()`
- `reader.initGPU(canvas?)`
- `reader.framePixelsGPU(index, canvas?)`
- `reader.frameImageBitmapGPU(index)`
- `reader.frameTransferBitmapGPU(index)`
- `reader.isGPUEnabled()`
- `reader.getGPUBackend()`
- `reader.benchmarkGPU(width?, height?)`
- `reader.disableGPU()`
- `reader.initWorkerPool()`
- `reader.framePixelsThreadedPool(indices)`
- `reader.framePixelsWorkerPool(index)`
- `reader.getWorkerPoolStats()`
- `reader.isWorkerPoolReady()`
- `reader.cleanupWorkerPool()`
- `reader.enableWasmColorMapping(colorMapWasm)`
- `reader.disableWasmColorMapping()`
- `reader.isWasmEnabled()`

Most users should prefer `decodeAndBlitFrameRGBA()`, `preparePlayback()`, or
`encodeRgbaGifFrames()`.

## Type Exports

wtfgif exports these public TypeScript types:

- `EncodeIndexedGifFramesBackend`
- `EncodeIndexedGifFramesOptions`
- `EncodeRgbaGifFramesOptions`
- `IndexedGifFrame`
- `IndexedGifFrames`
- `RgbaGifFrame`
- `RgbaGifFrames`
- `GifDecodeBackend`
- `GifDecodeBackendStatus`
- `PreparedFrameBackendPreference`
- `PreparedFrameCacheMode`
- `PreparedFrameDedupeMode`
- `PreparedFrameFormat`
- `PreparedGifFrame`
- `PreparedGifFrames`
- `PreparedGifPlayer`
- `PrepareFramesOptions`
- `WasmCoreInstance`
- `WasmCoreModule`

## Development

- `npm run build` builds the library and type definitions.
- `npm test` runs tests against omggif behavior.
- `npm run bench` compares common decode and encode paths against omggif.
- `npm run check` runs lint, typecheck, tests, build, and package dry-run.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

[MIT](LICENSE)
