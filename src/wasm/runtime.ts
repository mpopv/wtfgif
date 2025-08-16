import { WasmGifDecoder, WasmWorkerPool } from "../types";

let createWasmGifDecoder: any;
let createWasmWorkerPool: any;
let isWasmSupported: any;
let isWasmSIMDSupported: any;
let isWasmThreadsSupported: any;

const loadWasmModule = () => {
  try {
    return require("../wasm-full/wasmDecoder");
  } catch (error) {
    return null;
  }
};

createWasmGifDecoder = async (...args: any[]) => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.createWasmGifDecoder(...args) : null;
};
createWasmWorkerPool = async (...args: any[]) => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.createWasmWorkerPool(...args) : null;
};
isWasmSupported = () => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.isWasmSupported() : false;
};
isWasmSIMDSupported = () => {
  const wasmModule = loadWasmModule();
  return wasmModule ? wasmModule.isWasmSIMDSupported() : false;
};
isWasmThreadsSupported = () => {
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
