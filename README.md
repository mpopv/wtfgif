
![wtfgif logo](https://github.com/mpopv/wtfgif/blob/main/src/gifs/wtf.gif?raw=true)

# wtfgif

![animated fire emoji](https://github.com/mpopv/wtfgif/blob/main/src/gifs/fire.gif?raw=true) A drop-in optimized replacement for [omggif](https://www.npmjs.com/package/omggif) for Node and the browser ![animated fire emoji](https://github.com/mpopv/wtfgif/blob/main/src/gifs/fire.gif?raw=true)

## Installation

```bash
npm install wtfgif
```

## Usage

```ts
import { GifReader, GifWriter } from 'wtfgif';
import { readFileSync } from 'node:fs';

// Decode a GIF
const data = readFileSync('some.gif');
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

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

[MIT](LICENSE)
