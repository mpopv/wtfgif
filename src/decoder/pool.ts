import { PooledDecoderTables } from "../types";
import { GIF } from "../constants/gif";

const decoderTablePool: PooledDecoderTables[] = [];
const MAX_POOL_SIZE = 16;

export function createDecoderTables(): PooledDecoderTables {
  return {
    decTable: new Int32Array(GIF.MAX_CODE),
    stack: new Uint8Array(GIF.MAX_CODE),
    firstByte: new Int16Array(GIF.MAX_CODE),
    out32Cache: new WeakMap<Uint8Array, Uint32Array>(),
    hash: "",
  };
}

export function getPooledDecoderTables(gifHash: string): PooledDecoderTables {
  if (decoderTablePool.length > 0) {
    const pooled = decoderTablePool.pop()!;
    pooled.decTable.fill(0);
    pooled.stack.fill(0);
    pooled.firstByte.fill(0);
    pooled.out32Cache = new WeakMap();
    pooled.hash = gifHash;
    return pooled;
  }
  const tables = createDecoderTables();
  tables.hash = gifHash;
  return tables;
}

export function returnDecoderTablesToPool(tables: PooledDecoderTables): void {
  if (decoderTablePool.length < MAX_POOL_SIZE) {
    decoderTablePool.push(tables);
  }
}

export function getPoolStats() {
  return {
    available: decoderTablePool.length,
  };
}
