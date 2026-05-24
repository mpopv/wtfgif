import { WasmCoreModule, WasmGifDecoder, WasmWorkerPool } from "../types";

interface WasmModule {
  createWasmGifDecoder: (wasmPath?: string) => Promise<WasmGifDecoder | null>;
  createWasmWorkerPool: (wasmPath?: string) => Promise<WasmWorkerPool | null>;
  isWasmSupported: () => boolean;
  isWasmSIMDSupported: () => boolean;
  isWasmThreadsSupported: () => boolean;
}

let cachedWasmModule: WasmModule | null | undefined;
let cachedWasmCoreModule: WasmCoreModule | null | undefined;

const loadWasmModule = (): WasmModule | null => {
  if (cachedWasmModule === undefined) {
    try {
      cachedWasmModule = require("../wasm-full/wasmDecoder") as WasmModule;
    } catch {
      cachedWasmModule = null;
    }
  }
  return cachedWasmModule;
};

const tryRequire = (id: string): unknown => {
  if (typeof require !== "function") {
    return null;
  }

  try {
    return require(id);
  } catch {
    return null;
  }
};

const loadWasmCoreModule = (): WasmCoreModule | null => {
  if (cachedWasmCoreModule !== undefined) {
    return cachedWasmCoreModule;
  }

  const loaded =
    tryRequire("../../crates/wtfgif-core/pkg/wtfgif_core.js") ??
    tryRequire("../crates/wtfgif-core/pkg/wtfgif_core.js");
  cachedWasmCoreModule =
    loaded && typeof (loaded as WasmCoreModule).WtfGifCore === "function"
      ? (loaded as WasmCoreModule)
      : null;
  return cachedWasmCoreModule;
};

export function setWasmCoreModule(module: WasmCoreModule | null): void {
  cachedWasmCoreModule = module;
}

export function getWasmCoreModule(): WasmCoreModule | null {
  return loadWasmCoreModule();
}

const createWasmGifDecoder = async (
  wasmPath?: string,
): Promise<WasmGifDecoder | null> => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.createWasmGifDecoder(wasmPath) : null;
};

const createWasmWorkerPool = async (
  wasmPath?: string,
): Promise<WasmWorkerPool | null> => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.createWasmWorkerPool(wasmPath) : null;
};

const isWasmSupported = (): boolean => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.isWasmSupported() : false;
};

const isWasmSIMDSupported = (): boolean => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.isWasmSIMDSupported() : false;
};

const isWasmThreadsSupported = (): boolean => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.isWasmThreadsSupported() : false;
};

let globalWasmDecoder: WasmGifDecoder | null = null;
let globalWasmWorkerPool: WasmWorkerPool | null = null;
let wasmInitPromise: Promise<void> | null = null;

let wasmFeatures: {
  supported: boolean;
  simd: boolean;
  threads: boolean;
} | null = null;

export function getWasmFeatures() {
  if (!wasmFeatures) {
    wasmFeatures = {
      supported: isWasmSupported(),
      simd: isWasmSIMDSupported(),
      threads: isWasmThreadsSupported(),
    };
  }
  return wasmFeatures;
}

export const initializeGlobalWasm = async (
  wasmPath?: string,
): Promise<void> => {
  try {
    const features = getWasmFeatures();
    if (!features.supported) {
      return;
    }

    globalWasmDecoder = await createWasmGifDecoder(wasmPath);
    if (features.threads) {
      globalWasmWorkerPool = await createWasmWorkerPool(wasmPath);
    }
    console.log("WebAssembly GIF decoder initialized:", {
      decoder: !!globalWasmDecoder,
      workerPool: !!globalWasmWorkerPool,
      features,
    });
  } catch (error) {
    console.warn("Failed to initialize WebAssembly decoder:", error);
    globalWasmDecoder = null;
    globalWasmWorkerPool = null;
  }
};
export function getWasmDecoder() {
  return globalWasmDecoder;
}
export function getWasmWorkerPool() {
  return globalWasmWorkerPool;
}
export function getWasmInitPromise() {
  return wasmInitPromise;
}
export function setWasmInitPromise(p: Promise<void> | null) {
  wasmInitPromise = p;
}
export function isWasmReady() {
  return globalWasmDecoder !== null;
}
export function getWasmStatus() {
  const features = getWasmFeatures();
  return {
    ...features,
    initialized: globalWasmDecoder !== null,
    workerPoolAvailable: globalWasmWorkerPool !== null,
  };
}
export function cleanupWasm() {
  if (globalWasmWorkerPool) {
    globalWasmWorkerPool.terminate();
    globalWasmWorkerPool = null;
  }
  globalWasmDecoder = null;
  wasmInitPromise = null;
}
