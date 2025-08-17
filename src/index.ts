import { GifWriter } from "./encoder/writer";
import { GifReader } from "./decoder/reader";
import { DecodeError, RuntimeError } from "./errors";
import {
  initializeGlobalWasm,
  getWasmStatus,
  cleanupWasm,
} from "./wasm/runtime";

const initializeWasmGlobally = initializeGlobalWasm;

export {
  GifWriter,
  GifReader,
  initializeWasmGlobally,
  getWasmStatus,
  cleanupWasm,
  DecodeError,
  RuntimeError,
};

// Browser global export under wtfgif namespace
(function () {
  const browserExports = {
    GifWriter,
    GifReader,
    initializeWasmGlobally,
    getWasmStatus,
    cleanupWasm,
    DecodeError,
    RuntimeError,
  };

    if (typeof window !== "undefined") {
      (window as Window & { wtfgif: typeof browserExports }).wtfgif = browserExports;
    } else if (typeof globalThis !== "undefined") {
      (globalThis as typeof globalThis & { wtfgif: typeof browserExports }).wtfgif =
        browserExports;
    }
  })();

declare global {
  interface Window {
    wtfgif: typeof browserExports;
  }
  var wtfgif: typeof browserExports;
}
