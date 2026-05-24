import { beforeEach, describe, expect, test, vi } from "vitest";

beforeEach(() => {
  vi.resetModules();
  vi.clearAllMocks();
});

describe("WebAssembly runtime integration", () => {
  test("reports unavailable when no real wasm module is bundled", async () => {
    const runtime = await import("../src/wasm/runtime");
    const {
      initializeGlobalWasm,
      getWasmDecoder,
      getWasmWorkerPool,
      getWasmStatus,
      cleanupWasm,
    } = runtime;

    await initializeGlobalWasm();
    expect(getWasmDecoder()).toBeNull();
    expect(getWasmWorkerPool()).toBeNull();
    expect(getWasmStatus()).toMatchObject({
      supported: false,
      simd: false,
      threads: false,
      initialized: false,
      workerPoolAvailable: false,
    });

    cleanupWasm();
    expect(getWasmDecoder()).toBeNull();
    expect(getWasmWorkerPool()).toBeNull();
    expect(getWasmStatus()).toMatchObject({
      initialized: false,
      workerPoolAvailable: false,
    });
  });
});
