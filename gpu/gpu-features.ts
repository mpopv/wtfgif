// gpu/gpu-features.ts  
// Unified GPU feature detection and renderer selection

import { createWebGL2PaletteRenderer, isWebGL2Supported, type WebGL2PaletteRenderer } from './webgl2-palette';
import { createWebGPUPaletteRenderer, isWebGPUSupported, type WebGPUPaletteRenderer } from './webgpu-palette';

export type GPUPaletteRenderer = WebGL2PaletteRenderer | WebGPUPaletteRenderer;

export enum GPUBackend {
  None = 'none',
  WebGL2 = 'webgl2', 
  WebGPU = 'webgpu'
}

export interface GPUCapabilities {
  webgl2: boolean;
  webgpu: boolean;
  preferredBackend: GPUBackend;
  maxTextureSize?: number;
  maxComputeWorkgroupSize?: number;
}

export interface GPUPerformanceStats {
  backend: GPUBackend;
  paletteExpandTime: number;
  pixelsPerSecond: number;
  gpuMemoryUsed: number;
}

/**
 * Detect GPU capabilities and recommend optimal backend
 */
export function detectGPUCapabilities(): GPUCapabilities {
  const webgl2 = isWebGL2Supported();
  const webgpu = isWebGPUSupported();
  
  // Prefer WebGPU for compute shaders, fallback to WebGL2
  let preferredBackend = GPUBackend.None;
  if (webgpu) {
    preferredBackend = GPUBackend.WebGPU;
  } else if (webgl2) {
    preferredBackend = GPUBackend.WebGL2;
  }
  
  return {
    webgl2,
    webgpu,
    preferredBackend
  };
}

/**
 * Create the best available GPU palette renderer
 */
export async function createOptimalGPURenderer(canvas?: HTMLCanvasElement): Promise<{
  renderer: GPUPaletteRenderer | null;
  backend: GPUBackend;
}> {
  const capabilities = detectGPUCapabilities();
  
  // Try WebGPU first (best performance)
  if (capabilities.webgpu) {
    const renderer = await createWebGPUPaletteRenderer(canvas);
    if (renderer) {
      return { renderer, backend: GPUBackend.WebGPU };
    }
  }
  
  // Fallback to WebGL2
  if (capabilities.webgl2) {
    const renderer = await createWebGL2PaletteRenderer(canvas);
    if (renderer) {
      return { renderer, backend: GPUBackend.WebGL2 };
    }
  }
  
  // No GPU acceleration available
  return { renderer: null, backend: GPUBackend.None };
}

/**
 * Benchmark GPU palette expansion performance
 */
export async function benchmarkGPUPalette(
  renderer: GPUPaletteRenderer,
  backend: GPUBackend,
  width = 512,
  height = 512
): Promise<GPUPerformanceStats> {
  // Create test data
  const indexData = new Uint8Array(width * height);
  const palette = new Uint32Array(256);
  
  // Fill with test pattern
  for (let i = 0; i < indexData.length; i++) {
    indexData[i] = i % 256;
  }
  
  for (let i = 0; i < 256; i++) {
    palette[i] = 0xFF000000 | (i << 16) | (i << 8) | i; // Grayscale
  }
  
  const iterations = 10;
  const totalPixels = width * height * iterations;
  
  // Warm up
  await performRender(renderer, indexData, palette, width, height);
  
  // Benchmark
  const startTime = performance.now();
  
  for (let i = 0; i < iterations; i++) {
    await performRender(renderer, indexData, palette, width, height);
  }
  
  const endTime = performance.now();
  const totalTime = endTime - startTime;
  const avgTime = totalTime / iterations;
  const pixelsPerSecond = totalPixels / (totalTime / 1000);
  
  return {
    backend,
    paletteExpandTime: avgTime,
    pixelsPerSecond,
    gpuMemoryUsed: estimateGPUMemory(width, height)
  };
}

async function performRender(
  renderer: GPUPaletteRenderer,
  indexData: Uint8Array,
  palette: Uint32Array,
  width: number,
  height: number
): Promise<void> {
  const canvas = document.createElement('canvas');
  
  if ('renderToCanvas' in renderer) {
    if (renderer.renderToCanvas.constructor.name === 'AsyncFunction') {
      await (renderer as WebGPUPaletteRenderer).renderToCanvas(indexData, palette, width, height, canvas);
    } else {
      (renderer as WebGL2PaletteRenderer).renderToCanvas(indexData, palette, width, height, canvas);
    }
  }
}

function estimateGPUMemory(width: number, height: number): number {
  return (
    width * height * 1 +  // Index texture (R8)
    width * height * 4 +  // Output texture (RGBA8) 
    256 * 4 * 4           // Palette buffer (256 * vec4)
  );
}

/**
 * GPU-accelerated GIF frame renderer with automatic backend selection
 */
export class UnifiedGPUGifRenderer {
  private renderer: GPUPaletteRenderer | null = null;
  private backend: GPUBackend = GPUBackend.None;
  private initPromise: Promise<void> | null = null;
  
  constructor(private preferredBackend?: GPUBackend) {}
  
  async initialize(canvas?: HTMLCanvasElement): Promise<boolean> {
    if (this.initPromise) {
      await this.initPromise;
      return this.renderer !== null;
    }
    
    this.initPromise = this.doInitialize(canvas);
    await this.initPromise;
    return this.renderer !== null;
  }
  
  private async doInitialize(canvas?: HTMLCanvasElement): Promise<void> {
    // Use preferred backend if specified and available
    if (this.preferredBackend) {
      if (this.preferredBackend === GPUBackend.WebGPU && isWebGPUSupported()) {
        const renderer = await createWebGPUPaletteRenderer(canvas);
        if (renderer) {
          this.renderer = renderer;
          this.backend = GPUBackend.WebGPU;
          return;
        }
      } else if (this.preferredBackend === GPUBackend.WebGL2 && isWebGL2Supported()) {
        const renderer = await createWebGL2PaletteRenderer(canvas);
        if (renderer) {
          this.renderer = renderer;
          this.backend = GPUBackend.WebGL2;
          return;
        }
      }
    }
    
    // Auto-select optimal backend
    const { renderer, backend } = await createOptimalGPURenderer(canvas);
    this.renderer = renderer;
    this.backend = backend;
  }
  
  async renderFrame(indexData: Uint8Array, palette: Uint32Array, width: number, height: number): Promise<HTMLCanvasElement | null> {
    if (!await this.initialize()) {
      return null; // No GPU acceleration available
    }
    
    if (this.backend === GPUBackend.WebGPU) {
      return await (this.renderer as WebGPUPaletteRenderer).render(indexData, palette, width, height);
    } else {
      return (this.renderer as WebGL2PaletteRenderer).render(indexData, palette, width, height);
    }
  }
  
  async renderToCanvas(indexData: Uint8Array, palette: Uint32Array, width: number, height: number, targetCanvas: HTMLCanvasElement): Promise<boolean> {
    if (!await this.initialize()) {
      return false; // No GPU acceleration available
    }
    
    if (this.backend === GPUBackend.WebGPU) {
      await (this.renderer as WebGPUPaletteRenderer).renderToCanvas(indexData, palette, width, height, targetCanvas);
    } else {
      (this.renderer as WebGL2PaletteRenderer).renderToCanvas(indexData, palette, width, height, targetCanvas);
    }
    
    return true;
  }
  
  updatePalette(palette: Uint32Array): void {
    this.renderer?.updatePalette(palette);
  }
  
  getBackend(): GPUBackend {
    return this.backend;
  }
  
  isGPUAccelerated(): boolean {
    return this.backend !== GPUBackend.None;
  }
  
  async benchmark(width = 512, height = 512): Promise<GPUPerformanceStats | null> {
    if (!await this.initialize() || !this.renderer) {
      return null;
    }
    
    return benchmarkGPUPalette(this.renderer, this.backend, width, height);
  }
  
  dispose(): void {
    this.renderer?.dispose();
    this.renderer = null;
    this.backend = GPUBackend.None;
  }
}

// Utility functions for performance comparison
export async function compareGPUBackends(): Promise<{
  webgl2?: GPUPerformanceStats;
  webgpu?: GPUPerformanceStats;
  recommendation: GPUBackend;
}> {
  const capabilities = detectGPUCapabilities();
  const results: any = {};
  
  // Benchmark WebGL2
  if (capabilities.webgl2) {
    const renderer = await createWebGL2PaletteRenderer();
    if (renderer) {
      results.webgl2 = await benchmarkGPUPalette(renderer, GPUBackend.WebGL2);
      renderer.dispose();
    }
  }
  
  // Benchmark WebGPU  
  if (capabilities.webgpu) {
    const renderer = await createWebGPUPaletteRenderer();
    if (renderer) {
      results.webgpu = await benchmarkGPUPalette(renderer, GPUBackend.WebGPU);
      renderer.dispose();
    }
  }
  
  // Recommend faster backend
  let recommendation = capabilities.preferredBackend;
  if (results.webgl2 && results.webgpu) {
    recommendation = results.webgpu.pixelsPerSecond > results.webgl2.pixelsPerSecond 
      ? GPUBackend.WebGPU 
      : GPUBackend.WebGL2;
  }
  
  return { ...results, recommendation };
}