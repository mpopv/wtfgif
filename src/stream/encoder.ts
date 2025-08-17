import { Readable, ReadableOptions } from "stream";
import { GifWriter } from "../encoder/writer";
import { PaletteRGB } from "../types";

/**
 * Readable stream wrapper around GifWriter. Encoded data is emitted when
 * `finish()` is called, allowing consumers to pipe the result without keeping
 * it in memory manually.
 */
export class GifEncoderStream extends Readable {
  private buf: Uint8Array;
  private writer: GifWriter;

  constructor(
    width: number,
    height: number,
    opts?: {
      loop?: number | null;
      palette?: PaletteRGB | null;
      background?: number;
      bufferSize?: number;
    },
    streamOpts?: ReadableOptions
  ) {
    super(streamOpts);
    const bufferSize = opts?.bufferSize ?? 1024 * 1024;
    this.buf = new Uint8Array(bufferSize);
    const { bufferSize: _ignored, ...writerOpts } = opts ?? {};
    this.writer = new GifWriter(this.buf, width, height, writerOpts);
  }

  addFrame(
    x: number,
    y: number,
    w: number,
    h: number,
    indexedPixels: Uint8Array | number[],
    opts?: {
      palette?: PaletteRGB | null;
      delay?: number;
      disposal?: number;
      transparent?: number | null;
    }
  ): number {
    return this.writer.addFrame(x, y, w, h, indexedPixels, opts);
  }

  finish(): void {
    const end = this.writer.end();
    const out = this.buf.subarray(0, end);
    this.push(Buffer.from(out));
    this.push(null);
  }

  // No-op _read as data is pushed manually via finish()
  override _read(_size: number): void {}
}
