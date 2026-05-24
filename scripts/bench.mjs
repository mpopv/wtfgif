import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { performance } from "node:perf_hooks";
import { GifReader as WtfGifReader, GifWriter as WtfGifWriter } from "../dist/index.mjs";

const require = createRequire(import.meta.url);
const {
  GifReader: OmgGifReader,
  GifWriter: OmgGifWriter,
} = require("omggif");

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const gifsDir = join(root, "test", "gifs");

const iterations = Number(process.env.BENCH_ITERATIONS ?? 10);
const warmupIterations = Number(process.env.BENCH_WARMUP_ITERATIONS ?? 3);
const frameLimit = Number(process.env.BENCH_FRAME_LIMIT ?? 5);

function median(values) {
  const sorted = values.toSorted((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)] ?? 0;
}

function percentile(values, p) {
  const sorted = values.toSorted((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.ceil(sorted.length * p) - 1);
  return sorted[index] ?? 0;
}

function measure(fn) {
  for (let i = 0; i < warmupIterations; i++) {
    fn();
  }

  const samples = [];
  for (let i = 0; i < iterations; i++) {
    const start = performance.now();
    fn();
    samples.push(performance.now() - start);
  }

  return {
    median: median(samples),
    p95: percentile(samples, 0.95),
  };
}

function createDecodeTask(Reader, data, maxFrames) {
  const reader = new Reader(data);
  const frames = Math.min(reader.numFrames(), maxFrames);
  const pixels = new Uint8Array(reader.width * reader.height * 4);
  return {
    run() {
      for (let frame = 0; frame < frames; frame++) {
        pixels.fill(0);
        reader.decodeAndBlitFrameRGBA(frame, pixels);
      }
      return pixels[pixels.length - 1] ?? 0;
    },
    cleanup() {
      reader.returnToPool?.();
    },
  };
}

function verifyFrameParity(data, file) {
  const omg = new OmgGifReader(data);
  const wtf = new WtfGifReader(data);
  const frames = Math.min(omg.numFrames(), wtf.numFrames(), frameLimit);
  const length = omg.width * omg.height * 4;
  for (let frame = 0; frame < frames; frame++) {
    const omgPixels = new Uint8Array(length);
    const wtfPixels = new Uint8Array(length);
    omg.decodeAndBlitFrameRGBA(frame, omgPixels);
    wtf.decodeAndBlitFrameRGBA(frame, wtfPixels);
    for (let i = 0; i < length; i++) {
      if (omgPixels[i] !== wtfPixels[i]) {
        throw new Error(`${file} frame ${frame} differs at byte ${i}`);
      }
    }
  }
  wtf.returnToPool();
}

function makeSyntheticFrames() {
  const width = 128;
  const height = 128;
  const palette = [0x000000, 0xffffff, 0xff0000, 0x00ff00];
  const frames = [];
  for (let frame = 0; frame < 12; frame++) {
    const pixels = new Uint8Array(width * height);
    for (let y = 0; y < height; y++) {
      for (let x = 0; x < width; x++) {
        pixels[y * width + x] = (x + y + frame) & 3;
      }
    }
    frames.push(pixels);
  }
  return { width, height, palette, frames };
}

function encodeSynthetic(Writer, fixture) {
  const out = new Uint8Array(
    fixture.width * fixture.height * fixture.frames.length * 2 + 1024
  );
  const writer = new Writer(out, fixture.width, fixture.height, {
    palette: fixture.palette,
    loop: 0,
  });
  for (const frame of fixture.frames) {
    writer.addFrame(0, 0, fixture.width, fixture.height, frame, {
      delay: 2,
    });
  }
  return writer.end();
}

function formatMs(value) {
  return value.toFixed(3).padStart(8);
}

function formatRatio(value) {
  return `${value.toFixed(2)}x`.padStart(7);
}

function printRows(title, rows) {
  console.log(`\n${title}`);
  console.log("file".padEnd(38), "omggif".padStart(8), "wtfgif".padStart(8), "speedup".padStart(7), "p95".padStart(8));
  for (const row of rows) {
    console.log(
      row.file.padEnd(38),
      formatMs(row.omg),
      formatMs(row.wtf),
      formatRatio(row.speedup),
      formatMs(row.wtfP95)
    );
  }
  const geomean = Math.exp(
    rows.reduce((sum, row) => sum + Math.log(Math.max(row.speedup, Number.EPSILON)), 0) /
      rows.length
  );
  console.log("geomean".padEnd(38), "".padStart(8), "".padStart(8), formatRatio(geomean));
}

const files = readdirSync(gifsDir)
  .filter((file) => file.endsWith(".gif"))
  .sort();

console.log(
  `wtfgif benchmark: iterations=${iterations}, warmup=${warmupIterations}, frameLimit=${frameLimit}`
);

const parseRows = [];
const decodeRows = [];

for (const file of files) {
  const data = readFileSync(join(gifsDir, file));
  verifyFrameParity(data, file);

  const omgParse = measure(() => new OmgGifReader(data));
  const wtfParse = measure(() => {
    const reader = new WtfGifReader(data);
    reader.returnToPool();
  });
  parseRows.push({
    file,
    omg: omgParse.median,
    wtf: wtfParse.median,
    wtfP95: wtfParse.p95,
    speedup: omgParse.median / Math.max(wtfParse.median, Number.EPSILON),
  });

  const omgDecodeTask = createDecodeTask(OmgGifReader, data, frameLimit);
  const wtfDecodeTask = createDecodeTask(WtfGifReader, data, frameLimit);
  const omgDecode = measure(omgDecodeTask.run);
  const wtfDecode = measure(wtfDecodeTask.run);
  omgDecodeTask.cleanup();
  wtfDecodeTask.cleanup();
  decodeRows.push({
    file,
    omg: omgDecode.median,
    wtf: wtfDecode.median,
    wtfP95: wtfDecode.p95,
    speedup: omgDecode.median / Math.max(wtfDecode.median, Number.EPSILON),
  });
}

const synthetic = makeSyntheticFrames();
const omgEncodedLength = encodeSynthetic(OmgGifWriter, synthetic);
const wtfEncodedLength = encodeSynthetic(WtfGifWriter, synthetic);
if (omgEncodedLength !== wtfEncodedLength) {
  throw new Error(
    `synthetic encode length mismatch: omggif=${omgEncodedLength}, wtfgif=${wtfEncodedLength}`
  );
}

const omgEncode = measure(() => encodeSynthetic(OmgGifWriter, synthetic));
const wtfEncode = measure(() => encodeSynthetic(WtfGifWriter, synthetic));
const encodeRows = [
  {
    file: "synthetic-128x128x12",
    omg: omgEncode.median,
    wtf: wtfEncode.median,
    wtfP95: wtfEncode.p95,
    speedup: omgEncode.median / Math.max(wtfEncode.median, Number.EPSILON),
  },
];

printRows("Parse metadata", parseRows);
printRows("Decode RGBA", decodeRows);
printRows("Encode indexed frames", encodeRows);
