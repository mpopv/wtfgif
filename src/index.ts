import { GifWriter } from "./encoder/writer";
import { GifReader } from "./decoder/reader";
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
};

// Browser global export under wtfgif namespace
(function () {
  const browserExports = {
    GifWriter,
    GifReader,
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
