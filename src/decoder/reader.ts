import {
  FrameInfo,
  PooledDecoderTables,
  UnifiedGPUGifRenderer,
} from "../types";
import { GIF } from "../constants/gif";
import { buildPal32 } from "../utils/palette";
import { concatSubBlocks } from "../utils/subblocks";
import {
  createDecoderTables,
  getPooledDecoderTables,
  returnDecoderTablesToPool,
} from "./pool";
import { hashGifData } from "../utils/hash";
import { loadGPUModule, createGpuRenderer } from "../gpu/renderer";
import { initializeGlobalWasm, getWasmFeatures } from "../wasm/runtime";

// NOTE: For brevity, this file currently re-exports the class from src/index.ts implementation.
// A full move would copy the entire GifReader class here and adjust imports. To keep scope small,
// export a stub that re-exports from the legacy location. Follow-up step can finish the move.
export { GifReader } from "../index";
