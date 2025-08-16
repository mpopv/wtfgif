import { WasmGifDecoder, WasmWorkerPool } from "../types";

interface WasmModule {
  createWasmGifDecoder: (
    wasmPath?: string
  ) => Promise<WasmGifDecoder | null>;
  createWasmWorkerPool: (
    wasmPath?: string
  ) => Promise<WasmWorkerPool | null>;
  isWasmSupported: () => boolean;
  isWasmSIMDSupported: () => boolean;
  isWasmThreadsSupported: () => boolean;
}

const loadWasmModule = (): WasmModule | null => {
  try {
    return require("../wasm-full/wasmDecoder") as WasmModule;
  } catch {
    return null;
  }
};

const createWasmGifDecoder = async (
  wasmPath?: string
): Promise<WasmGifDecoder | null> => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.createWasmGifDecoder(wasmPath) : null;
};

const createWasmWorkerPool = async (
  wasmPath?: string
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

const WASM_FEATURES = {
  supported: isWasmSupported(),
  simd: isWasmSIMDSupported(),
  threads: isWasmThreadsSupported(),
};

export const initializeGlobalWasm = async (
  wasmPath?: string
): Promise<void> => {
  try {
    globalWasmDecoder = await createWasmGifDecoder(wasmPath);
    if (WASM_FEATURES.threads) {
      globalWasmWorkerPool = await createWasmWorkerPool(wasmPath);
    }
    console.log("WebAssembly GIF decoder initialized:", {
      decoder: !!globalWasmDecoder,
      workerPool: !!globalWasmWorkerPool,
      features: WASM_FEATURES,
    });
  } catch (error) {
    console.warn("Failed to initialize WebAssembly decoder:", error);
    globalWasmDecoder = null;
    globalWasmWorkerPool = null;
  }
};

export function getWasmFeatures() {
  return WASM_FEATURES;
}
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
  return {
    ...WASM_FEATURES,
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
