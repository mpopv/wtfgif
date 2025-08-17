# wtfgif

A ![animated fire emoji](https://github.com/mpopv/wtfgif/blob/main/src/gifs/fire.gif?raw=true) fast ![animated fire emoji](https://github.com/mpopv/wtfgif/blob/main/src/gifs/fire.gif?raw=true) drop-in replacement for [omggif](https://www.npmjs.com/package/omggif) for Node and browser environments

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

### React

Here's a small component that fetches a GIF and shows its dimensions:

```tsx
import { useEffect, useState } from "react";
import { GifReader } from "wtfgif";

function GifInfo({ url }: { url: string }) {
  const [size, setSize] = useState<{ width: number; height: number }>();

  useEffect(() => {
    async function load() {
      const res = await fetch(url);
      const buf = new Uint8Array(await res.arrayBuffer());
      const reader = new GifReader(buf);
      setSize({ width: reader.width, height: reader.height });
    }
    load();
  }, [url]);

  return size ? <p>{size.width}×{size.height}</p> : <p>Loading…</p>;
}
```

## Development

- `npm run build` – bundle the library with type definitions.
- `npm test` – run tests verifying behaviour against the latest `omggif`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

[MIT](LICENSE)
