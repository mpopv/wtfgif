// reference-architecture.ts
// Complete reference architecture: Main thread + Worker(s) + GPU pipeline

/*
Reference Architecture:
┌───────── Main Thread ─────────┐
│ requestAnimationFrame         │
│   drawImage(bitmap)           │
│   schedule next frame index   │
└────────┬──────────────────────┘
         │ ImageBitmap (transfer)
┌────────▼── Worker(s) ─────────┐
│ Wasm.decode_to_index_bytes()  │  ← threaded or sharded
│ GPU compute map→RGBA          │
│ offscreenCanvas.transferToIB()│
│ postMessage(bitmap)           │
└───────────────────────────────┘

All heavy lifting (LZW, palette, interlace, disposal) is off the main thread.
The main thread just presents.
*/

export interface FrameRequest {
  frameIndex: number;
  requestId: string;
  priority: 'high' | 'normal' | 'low';
}

export interface FrameResponse {
  frameIndex: number;
  requestId: string;
  bitmap: ImageBitmap;
  delay: number;
  decodeTime: number;
  renderTime: number;
}

export interface ReferenceArchitectureConfig {
  workerCount?: number;
  enableGPU?: boolean;
  preloadFrames?: number;
  maxQueueSize?: number;
}

export class ReferenceArchitectureGifPlayer {
  private workers: Worker[] = [];
  private gifData: Uint8Array;
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  
  private currentFrame = 0;
  private totalFrames = 0;
  private isPlaying = false;
  private animationId: number | null = null;
  
  // Frame scheduling and preloading
  private frameCache = new Map<number, ImageBitmap>();
  private pendingRequests = new Map<string, {
    resolve: (response: FrameResponse) => void;
    reject: (error: Error) => void;
    frameIndex: number;
  }>();
  
  private preloadQueue: number[] = [];
  private nextRequestId = 0;
  private config: Required<ReferenceArchitectureConfig>;
  
  // Performance tracking
  private stats = {
    framesDecoded: 0,
    totalDecodeTime: 0,
    totalRenderTime: 0,
    cacheHits: 0,
    cacheMisses: 0
  };

  constructor(
    gifData: Uint8Array, 
    canvas: HTMLCanvasElement, 
    config: ReferenceArchitectureConfig = {}
  ) {
    this.gifData = gifData;
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d')!;
    
    this.config = {
      workerCount: config.workerCount || navigator.hardwareConcurrency || 4,
      enableGPU: config.enableGPU ?? true,
      preloadFrames: config.preloadFrames || 5,
      maxQueueSize: config.maxQueueSize || 20
    };
    
    // Get total frame count from GIF header (simplified)
    this.totalFrames = this.parseFrameCount(gifData);
  }

  async initialize(): Promise<void> {
    console.log('🏗️ Initializing reference architecture...');
    
    // Create worker pool with reference architecture worker
    const workerPromises = Array.from({ length: this.config.workerCount }, (_, i) =>
      this.createWorker(i)
    );
    
    await Promise.all(workerPromises);
    
    console.log(`✅ Reference architecture ready: ${this.workers.length} workers, GPU: ${this.config.enableGPU}`);
  }

  private async createWorker(workerId: number): Promise<void> {
    return new Promise((resolve, reject) => {
      const worker = new Worker('./reference-worker.js');
      
      worker.onmessage = (event) => {
        const { type, data } = event.data;
        
        if (type === 'init-complete') {
          this.workers[workerId] = worker;
          console.log(`Worker ${workerId} initialized`);
          resolve();
        } else if (type === 'frame-complete') {
          this.handleFrameComplete(data);
        } else if (type === 'error') {
          console.error(`Worker ${workerId} error:`, data.error);
        }
      };
      
      worker.onerror = (error) => {
        console.error(`Worker ${workerId} failed:`, error);
        reject(error);
      };
      
      // Initialize worker with configuration
      worker.postMessage({
        type: 'init',
        data: {
          workerId,
          enableGPU: this.config.enableGPU,
          gifData: this.gifData.slice() // Copy for worker
        }
      }, [this.gifData.slice().buffer]);
      
      setTimeout(() => reject(new Error(`Worker ${workerId} timeout`)), 10000);
    });
  }

  private handleFrameComplete(data: FrameResponse): void {
    const { requestId, frameIndex, bitmap, delay, decodeTime, renderTime } = data;
    
    // Update statistics
    this.stats.framesDecoded++;
    this.stats.totalDecodeTime += decodeTime;
    this.stats.totalRenderTime += renderTime;
    
    // Cache the frame bitmap
    this.frameCache.set(frameIndex, bitmap);
    
    // Resolve pending request if exists
    const pending = this.pendingRequests.get(requestId);
    if (pending) {
      pending.resolve(data);
      this.pendingRequests.delete(requestId);
    }
    
    // Continue preloading if this was a preload request
    this.schedulePreload();
  }

  private schedulePreload(): void {
    // Preload upcoming frames for smooth playback
    const preloadStart = this.currentFrame + 1;
    const preloadEnd = Math.min(preloadStart + this.config.preloadFrames, this.totalFrames);
    
    for (let i = preloadStart; i < preloadEnd; i++) {
      if (!this.frameCache.has(i) && !this.preloadQueue.includes(i)) {
        this.preloadQueue.push(i);
      }
    }
    
    // Process preload queue
    this.processPreloadQueue();
  }

  private processPreloadQueue(): void {
    // Send preload requests to available workers
    while (this.preloadQueue.length > 0 && this.pendingRequests.size < this.config.maxQueueSize) {
      const frameIndex = this.preloadQueue.shift()!;
      this.requestFrame(frameIndex, 'low'); // Low priority for preload
    }
  }

  private async requestFrame(frameIndex: number, priority: 'high' | 'normal' | 'low' = 'normal'): Promise<FrameResponse> {
    return new Promise((resolve, reject) => {
      const requestId = `${Date.now()}-${this.nextRequestId++}`;
      
      // Track pending request
      this.pendingRequests.set(requestId, {
        resolve,
        reject,
        frameIndex
      });
      
      // Select worker based on load balancing
      const workerId = this.selectWorker();
      const worker = this.workers[workerId];
      
      // Send frame request to worker
      worker.postMessage({
        type: 'decode-frame',
        data: {
          frameIndex,
          requestId,
          priority
        }
      });
    });
  }

  private selectWorker(): number {
    // Simple round-robin selection
    // In production, could use more sophisticated load balancing
    return this.stats.framesDecoded % this.workers.length;
  }

  // Main animation loop - runs on main thread
  async play(): Promise<void> {
    if (this.isPlaying) return;
    
    this.isPlaying = true;
    console.log('▶️ Starting playback with reference architecture');
    
    // Start preloading
    this.schedulePreload();
    
    // Main animation loop
    const animate = async () => {
      if (!this.isPlaying) return;
      
      // Check if current frame is ready
      let bitmap = this.frameCache.get(this.currentFrame);
      
      if (bitmap) {
        // Cache hit - immediate display
        const renderStart = performance.now();
        this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
        this.ctx.drawImage(bitmap, 0, 0);
        const renderTime = performance.now() - renderStart;
        
        this.stats.cacheHits++;
        this.stats.totalRenderTime += renderTime;
        
        // Schedule next frame
        this.currentFrame = (this.currentFrame + 1) % this.totalFrames;
        
        // Clean up old frames from cache
        this.cleanupCache();
        
        // Continue preloading
        this.schedulePreload();
        
        // Schedule next animation frame
        this.animationId = requestAnimationFrame(animate);
        
      } else {
        // Cache miss - request frame with high priority
        this.stats.cacheMisses++;
        
        try {
          const response = await this.requestFrame(this.currentFrame, 'high');
          
          // Display the frame immediately
          const renderStart = performance.now();
          this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
          this.ctx.drawImage(response.bitmap, 0, 0);
          const renderTime = performance.now() - renderStart;
          
          this.stats.totalRenderTime += renderTime;
          
          // Continue animation
          this.currentFrame = (this.currentFrame + 1) % this.totalFrames;
          this.animationId = requestAnimationFrame(animate);
          
        } catch (error) {
          console.error('Frame decode failed:', error);
          this.stop();
        }
      }
    };
    
    // Start animation loop
    this.animationId = requestAnimationFrame(animate);
  }

  stop(): void {
    this.isPlaying = false;
    
    if (this.animationId) {
      cancelAnimationFrame(this.animationId);
      this.animationId = null;
    }
    
    console.log('⏹️ Playback stopped');
  }

  private cleanupCache(): void {
    // Keep only recent frames in cache to manage memory
    const keepFrames = this.config.preloadFrames * 2;
    const framesToRemove: number[] = [];
    
    for (const [frameIndex, bitmap] of this.frameCache.entries()) {
      const distance = Math.abs(frameIndex - this.currentFrame);
      if (distance > keepFrames) {
        framesToRemove.push(frameIndex);
        bitmap.close(); // Release GPU memory
      }
    }
    
    framesToRemove.forEach(frameIndex => {
      this.frameCache.delete(frameIndex);
    });
  }

  private parseFrameCount(gifData: Uint8Array): number {
    // Simplified frame counting - in production, use full GIF parser
    let frameCount = 0;
    let offset = 0;
    
    // Skip header
    if (gifData.length < 13) return 1;
    offset = 13;
    
    // Skip global color table if present
    const globalColorTableFlag = gifData[10] & 0x80;
    if (globalColorTableFlag) {
      const colorTableSize = 2 << (gifData[10] & 0x07);
      offset += colorTableSize * 3;
    }
    
    // Count image descriptors
    while (offset < gifData.length - 1) {
      const blockType = gifData[offset];
      
      if (blockType === 0x2C) { // Image descriptor
        frameCount++;
        offset += 10; // Skip image descriptor
        
        // Skip local color table if present
        const localColorTableFlag = gifData[offset - 1] & 0x80;
        if (localColorTableFlag) {
          const localColorTableSize = 2 << (gifData[offset - 1] & 0x07);
          offset += localColorTableSize * 3;
        }
        
        // Skip image data
        offset++; // LZW minimum code size
        while (offset < gifData.length && gifData[offset] !== 0) {
          offset += gifData[offset] + 1;
        }
        offset++; // Block terminator
        
      } else if (blockType === 0x3B) { // Trailer
        break;
      } else {
        offset++;
      }
    }
    
    return Math.max(frameCount, 1);
  }

  getStats() {
    const cacheHitRate = this.stats.cacheHits + this.stats.cacheMisses > 0 
      ? this.stats.cacheHits / (this.stats.cacheHits + this.stats.cacheMisses) 
      : 0;
    
    const avgDecodeTime = this.stats.framesDecoded > 0 
      ? this.stats.totalDecodeTime / this.stats.framesDecoded 
      : 0;
    
    const avgRenderTime = this.stats.framesDecoded > 0 
      ? this.stats.totalRenderTime / this.stats.framesDecoded 
      : 0;
    
    return {
      framesDecoded: this.stats.framesDecoded,
      cacheHitRate: cacheHitRate * 100,
      avgDecodeTime: avgDecodeTime.toFixed(2) + 'ms',
      avgRenderTime: avgRenderTime.toFixed(2) + 'ms',
      cacheSize: this.frameCache.size,
      pendingRequests: this.pendingRequests.size,
      currentFrame: this.currentFrame,
      totalFrames: this.totalFrames
    };
  }

  async dispose(): Promise<void> {
    this.stop();
    
    // Clean up frame cache
    for (const [, bitmap] of this.frameCache.entries()) {
      bitmap.close();
    }
    this.frameCache.clear();
    
    // Terminate workers
    const terminatePromises = this.workers.map(worker => {
      worker.postMessage({ type: 'terminate' });
      return new Promise<void>(resolve => {
        worker.terminate();
        resolve();
      });
    });
    
    await Promise.all(terminatePromises);
    this.workers.length = 0;
    
    console.log('🧹 Reference architecture disposed');
  }
}