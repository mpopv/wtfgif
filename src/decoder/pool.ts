import { PooledDecoderTables } from "../types";
import { GIF } from "../constants/gif";

export function createDecoderTables(): PooledDecoderTables {
  return {
    decTable: new Int32Array(GIF.MAX_CODE),
    stack: new Uint8Array(GIF.MAX_CODE),
    firstByte: new Int16Array(GIF.MAX_CODE)
  };
}
