import { beforeEach, describe, expect, test, vi } from "vitest";

beforeEach(() => {
  vi.resetModules();
  vi.clearAllMocks();
});

describe("WebAssembly runtime integration", () => {
  test("initializes and cleans up with available wasm module", async () => {
    const runtime = await import("../src/wasm/runtime");
    const {
      initializeGlobalWasm,
      getWasmDecoder,
      getWasmWorkerPool,
      getWasmStatus,
      cleanupWasm,
    } = runtime;

    await initializeGlobalWasm();
    const workerPool = getWasmWorkerPool();
    expect(getWasmDecoder()).not.toBeNull();
    expect(workerPool).not.toBeNull();
    expect(getWasmStatus()).toMatchObject({
      supported: true,
      simd: true,
      threads: true,
      initialized: true,
      workerPoolAvailable: true,
    });

    const terminateSpy = vi.spyOn(workerPool!, "terminate");
    cleanupWasm();
    expect(terminateSpy).toHaveBeenCalled();
    expect(getWasmDecoder()).toBeNull();
    expect(getWasmWorkerPool()).toBeNull();
    expect(getWasmStatus()).toMatchObject({
      initialized: false,
      workerPoolAvailable: false,
    });
  });
});

