// memory-pool.ts
// TypedArray pool for allocation hygiene - never new Uint32Array in hot loops

export interface PooledBuffer {
  buffer: Uint32Array;
  release: () => void;
}

export interface TypedArrayPool {
  acquire: (size: number) => PooledBuffer;
  getStats: () => PoolStats;
  cleanup: () => void;
}

export interface PoolStats {
  poolSize: number;
  activeBuffers: number;
  totalAllocations: number;
  reuseRate: number;
  memoryUsed: number; // bytes
}

class Uint32ArrayPool implements TypedArrayPool {
  private pool: Map<number, Uint32Array[]> = new Map();
  private activeBuffers = new Set<Uint32Array>();
  private stats = {
    totalAllocations: 0,
    totalReuses: 0,
    maxPoolSize: 0
  };
  
  private readonly maxPoolSize: number;
  private readonly maxBufferSize: number;

  constructor(maxPoolSize = 50, maxBufferSize = 16 * 1024 * 1024) {
    this.maxPoolSize = maxPoolSize;
    this.maxBufferSize = maxBufferSize;
  }

  acquire(size: number): PooledBuffer {
    this.stats.totalAllocations++;
    
    // Don't pool very large buffers
    if (size > this.maxBufferSize) {
      const buffer = new Uint32Array(size);
      return {
        buffer,
        release: () => {} // No pooling for large buffers
      };
    }
    
    // Round up to next power of 2 for better pooling
    const poolSize = this.nextPowerOf2(size);
    
    // Try to reuse from pool
    const poolArray = this.pool.get(poolSize);
    if (poolArray && poolArray.length > 0) {
      const buffer = poolArray.pop()!;
      this.activeBuffers.add(buffer);
      this.stats.totalReuses++;
      
      return {
        buffer,
        release: () => this.release(buffer, poolSize)
      };
    }
    
    // Allocate new buffer
    const buffer = new Uint32Array(poolSize);
    this.activeBuffers.add(buffer);
    
    return {
      buffer,
      release: () => this.release(buffer, poolSize)
    };
  }

  private release(buffer: Uint32Array, poolSize: number): void {
    this.activeBuffers.delete(buffer);
    
    // Don't exceed max pool size
    if (!this.pool.has(poolSize)) {
      this.pool.set(poolSize, []);
    }
    
    const poolArray = this.pool.get(poolSize)!;
    if (poolArray.length < this.maxPoolSize) {
      poolArray.push(buffer);
    }
  }

  private nextPowerOf2(n: number): number {
    return Math.pow(2, Math.ceil(Math.log2(n)));
  }

  getStats(): PoolStats {
    let poolSize = 0;
    let memoryUsed = 0;
    
    for (const [size, buffers] of this.pool.entries()) {
      poolSize += buffers.length;
      memoryUsed += buffers.length * size * 4; // 4 bytes per Uint32
    }
    
    // Add active buffer memory
    memoryUsed += this.activeBuffers.size * 4 * 1024; // Estimate
    
    return {
      poolSize,
      activeBuffers: this.activeBuffers.size,
      totalAllocations: this.stats.totalAllocations,
      reuseRate: this.stats.totalAllocations > 0 ? this.stats.totalReuses / this.stats.totalAllocations : 0,
      memoryUsed
    };
  }

  cleanup(): void {
    this.pool.clear();
    this.activeBuffers.clear();
    this.stats = { totalAllocations: 0, totalReuses: 0, maxPoolSize: 0 };
  }
}

// Global pool instances
let globalUint32Pool: Uint32ArrayPool | null = null;
let globalUint8Pool: Uint8ArrayPool | null = null;

export function getGlobalUint32Pool(): Uint32ArrayPool {
  if (!globalUint32Pool) {
    globalUint32Pool = new Uint32ArrayPool();
  }
  return globalUint32Pool;
}

// Similar pool for Uint8Array (for index data)
class Uint8ArrayPool {
  private pool: Map<number, Uint8Array[]> = new Map();
  private activeBuffers = new Set<Uint8Array>();
  private stats = { totalAllocations: 0, totalReuses: 0 };
  
  private readonly maxPoolSize: number;
  private readonly maxBufferSize: number;

  constructor(maxPoolSize = 50, maxBufferSize = 8 * 1024 * 1024) {
    this.maxPoolSize = maxPoolSize;
    this.maxBufferSize = maxBufferSize;
  }

  acquire(size: number): { buffer: Uint8Array; release: () => void } {
    this.stats.totalAllocations++;
    
    if (size > this.maxBufferSize) {
      const buffer = new Uint8Array(size);
      return { buffer, release: () => {} };
    }
    
    const poolSize = this.nextPowerOf2(size);
    const poolArray = this.pool.get(poolSize);
    
    if (poolArray && poolArray.length > 0) {
      const buffer = poolArray.pop()!;
      this.activeBuffers.add(buffer);
      this.stats.totalReuses++;
      
      return {
        buffer,
        release: () => this.release(buffer, poolSize)
      };
    }
    
    const buffer = new Uint8Array(poolSize);
    this.activeBuffers.add(buffer);
    
    return {
      buffer,
      release: () => this.release(buffer, poolSize)
    };
  }

  private release(buffer: Uint8Array, poolSize: number): void {
    this.activeBuffers.delete(buffer);
    
    if (!this.pool.has(poolSize)) {
      this.pool.set(poolSize, []);
    }
    
    const poolArray = this.pool.get(poolSize)!;
    if (poolArray.length < this.maxPoolSize) {
      poolArray.push(buffer);
    }
  }

  private nextPowerOf2(n: number): number {
    return Math.pow(2, Math.ceil(Math.log2(n)));
  }

  getStats(): PoolStats {
    let poolSize = 0;
    let memoryUsed = 0;
    
    for (const [size, buffers] of this.pool.entries()) {
      poolSize += buffers.length;
      memoryUsed += buffers.length * size; // 1 byte per Uint8
    }
    
    memoryUsed += this.activeBuffers.size * 1024; // Estimate
    
    return {
      poolSize,
      activeBuffers: this.activeBuffers.size,
      totalAllocations: this.stats.totalAllocations,
      reuseRate: this.stats.totalAllocations > 0 ? this.stats.totalReuses / this.stats.totalAllocations : 0,
      memoryUsed
    };
  }

  cleanup(): void {
    this.pool.clear();
    this.activeBuffers.clear();
    this.stats = { totalAllocations: 0, totalReuses: 0 };
  }
}

export function getGlobalUint8Pool(): Uint8ArrayPool {
  if (!globalUint8Pool) {
    globalUint8Pool = new Uint8ArrayPool();
  }
  return globalUint8Pool;
}

// Arena allocator interface for Wasm
export interface ArenaAllocator {
  malloc: (size: number) => number;
  reset: () => void;
  getUsage: () => { used: number; total: number };
}

// Wasm arena allocator implementation
export function createWasmArena(wasmInstance: any, totalSize = 64 * 1024 * 1024): ArenaAllocator {
  let arenaBase = 0;
  let arenaSize = totalSize;
  let currentOffset = 0;
  
  // Initialize arena in Wasm memory
  if (wasmInstance.wasm_init_arena) {
    arenaBase = wasmInstance.wasm_init_arena(arenaSize);
  } else {
    // Fallback: use regular malloc for base
    arenaBase = wasmInstance.wasm_malloc(arenaSize);
  }
  
  return {
    malloc: (size: number): number => {
      // Align to 8-byte boundary
      const alignedSize = (size + 7) & ~7;
      
      if (currentOffset + alignedSize > arenaSize) {
        throw new Error(`Arena exhausted: requested ${alignedSize}, available ${arenaSize - currentOffset}`);
      }
      
      const ptr = arenaBase + currentOffset;
      currentOffset += alignedSize;
      return ptr;
    },
    
    reset: (): void => {
      currentOffset = 0;
      // Zero out the arena for clean state
      if (wasmInstance.wasm_zero_arena) {
        wasmInstance.wasm_zero_arena(arenaBase, arenaSize);
      }
    },
    
    getUsage: () => ({
      used: currentOffset,
      total: arenaSize
    })
  };
}

// Memory hygiene utilities
export class MemoryHygiene {
  private uint32Pool = getGlobalUint32Pool();
  private uint8Pool = getGlobalUint8Pool();
  
  // Acquire pooled pixel buffer
  acquirePixelBuffer(width: number, height: number): PooledBuffer {
    return this.uint32Pool.acquire(width * height);
  }
  
  // Acquire pooled index buffer
  acquireIndexBuffer(width: number, height: number): { buffer: Uint8Array; release: () => void } {
    return this.uint8Pool.acquire(width * height);
  }
  
  // Get memory statistics
  getMemoryStats(): { uint32Pool: PoolStats; uint8Pool: PoolStats } {
    return {
      uint32Pool: this.uint32Pool.getStats(),
      uint8Pool: this.uint8Pool.getStats()
    };
  }
  
  // Cleanup all pools
  cleanup(): void {
    this.uint32Pool.cleanup();
    this.uint8Pool.cleanup();
  }
}

// Global memory hygiene instance
let globalMemoryHygiene: MemoryHygiene | null = null;

export function getGlobalMemoryHygiene(): MemoryHygiene {
  if (!globalMemoryHygiene) {
    globalMemoryHygiene = new MemoryHygiene();
  }
  return globalMemoryHygiene;
}