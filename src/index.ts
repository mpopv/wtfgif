import { GifWriter } from "./encoder/writer";
import { GifReader } from "./decoder/reader";
import { GifEncoderStream } from "./stream/encoder";
import { GifDecoderStream } from "./stream/decoder";
import {
  initializeGlobalWasm,
  getWasmStatus,
  cleanupWasm,
} from "./wasm/runtime";

const initializeWasmGlobally = initializeGlobalWasm;

export {
  GifWriter,
  GifReader,
  GifEncoderStream,
  GifDecoderStream,
  initializeWasmGlobally,
  getWasmStatus,
  cleanupWasm,
};

// Browser global export under wtfgif namespace
(function () {
  const browserExports = {
    GifWriter,
    GifReader,
    GifEncoderStream,
    GifDecoderStream,
    initializeWasmGlobally,
    getWasmStatus,
    cleanupWasm,
  };

    if (typeof window !== "undefined") {
      (window as any).wtfgif = browserExports;
    } else if (typeof globalThis !== "undefined") {
      (globalThis as any).wtfgif = browserExports;
    }
})();
