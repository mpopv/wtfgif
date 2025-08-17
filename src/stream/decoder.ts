import { Writable, WritableOptions } from "stream";
import { GifReader } from "../decoder/reader";

/**
 * Writable stream that collects incoming GIF data and exposes a GifReader
 * instance once the stream finishes. This allows decoding GIFs provided as
 * Node.js streams without first buffering the entire file in user land.
 *
 * While the current implementation buffers data internally until the stream
 * ends, it exposes a streaming-friendly interface so that future
 * implementations can process data incrementally.
 */
export class GifDecoderStream extends Writable {
  private chunks: Buffer[] = [];
  private _reader: GifReader | null = null;

  constructor(opts?: WritableOptions) {
    super(opts);
  }

  /**
   * Access the underlying GifReader once decoding has completed. Will be null
   * until the stream has ended.
   */
  get reader(): GifReader | null {
    return this._reader;
  }

  override _write(
    chunk: Buffer,
    _encoding: BufferEncoding,
    callback: (error?: Error | null) => void
  ): void {
    this.chunks.push(Buffer.from(chunk));
    callback();
  }

  override _final(callback: (error?: Error | null) => void): void {
    const buf = Buffer.concat(this.chunks);
    const u8 = new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
    this._reader = new GifReader(u8);
    this.emit("decoded", this._reader);
    callback();
  }
}
