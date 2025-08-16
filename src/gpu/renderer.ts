import { UnifiedGPUGifRenderer } from "../types";

export function loadGPUModule(): any {
  try {
    return null;
  } catch (e) {
    return null;
  }
}

export function createGpuRenderer(
  gpuModule: any
): UnifiedGPUGifRenderer | null {
  if (!gpuModule || !gpuModule.UnifiedGPUGifRenderer) return null;
  return new gpuModule.UnifiedGPUGifRenderer();
}
