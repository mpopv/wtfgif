![wtfgif logo](https://github.com/mpopv/wtfgif/blob/main/wtf.gif?raw=true)

# wtfgif

A drop-in optimized replacement for [omggif](https://www.npmjs.com/package/omggif): fused decode→blit, 32-bit palettes, interlace pass scheduling and typed-array hashing. Runs in Node and the browser.

## Installation

```bash
npm install wtfgif
```

## Usage

```ts
import { GifReader, GifWriter } from "wtfgif";
import { readFileSync } from "node:fs";

// Decode a GIF
const data = readFileSync("some.gif");
const reader = new GifReader(data);
console.log(reader.width, reader.height);

// Encode a GIF
const buf = Buffer.alloc(1024 * 1024);
const writer = new GifWriter(buf, 2, 2, { palette: [0x000000, 0xffffff] });
writer.finish();
```

## Development

- `npm run build` – bundle the library with type definitions.
- `npm test` – run tests verifying behaviour against the latest `omggif`.

## License

MIT
