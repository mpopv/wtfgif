import { UnifiedGPUGifRenderer } from "../types";

export interface GPUModule {
  UnifiedGPUGifRenderer: new () => UnifiedGPUGifRenderer;
}

export function loadGPUModule(): GPUModule | null {
  try {
    return null;
  } catch {
    return null;
  }
}

export function createGpuRenderer(
  gpuModule: GPUModule | null
): UnifiedGPUGifRenderer | null {
  if (!gpuModule || !gpuModule.UnifiedGPUGifRenderer) return null;
  return new gpuModule.UnifiedGPUGifRenderer();
}
