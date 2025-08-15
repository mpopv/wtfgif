// threaded-worker-pool.ts
// Threaded worker pool for parallel frame decode when Wasm threads aren't available

export interface WorkerPoolConfig {
  workerCount?: number;
  maxQueueSize?: number;
  workerScript?: string;
}

export interface FrameDecodeRequest {
  gifData: Uint8Array;
  frameIndex: number;
  requestId: string;
}

export interface FrameDecodeResult {
  requestId: string;
  frameIndex: number;
  pixels: Uint32Array;
  delay: number;
  decodeTime: number;
  workerId: number;
  wasmUsed: boolean;
}

export interface WorkerPoolStats {
  activeWorkers: number;
  queuedRequests: number;
  completedJobs: number;
  avgDecodeTime: number;
  totalDecodeTime: number;
  workerUtilization: number[];
}

class WorkerPoolManager {
  private workers: Worker[] = [];
  private workerQueue: number[] = []; // Available worker indices
  private requestQueue: Array<{
    request: FrameDecodeRequest;
    resolve: (result: FrameDecodeResult) => void;
    reject: (error: Error) => void;
  }> = [];
  
  private pendingRequests = new Map<string, {
    resolve: (result: FrameDecodeResult) => void;
    reject: (error: Error) => void;
    startTime: number;
  }>();
  
  private stats = {
    completedJobs: 0,
    totalDecodeTime: 0,
    workerUtilization: [] as number[]
  };
  
  private nextWorkerId = 0;
  private nextRequestId = 0;
  private config: Required<WorkerPoolConfig>;

  constructor(config: WorkerPoolConfig = {}) {
    this.config = {
      workerCount: config.workerCount || navigator.hardwareConcurrency || 4,
      maxQueueSize: config.maxQueueSize || 100,
      workerScript: config.workerScript || './decoder-worker.js'
    };
    
    // Initialize worker utilization tracking
    this.stats.workerUtilization = new Array(this.config.workerCount).fill(0);
  }

  async initialize(): Promise<void> {
    console.log(`Initializing threaded worker pool with ${this.config.workerCount} workers`);
    
    // Create worker pool
    const initPromises = Array.from({ length: this.config.workerCount }, (_, i) => 
      this.createWorker(i)
    );
    
    await Promise.all(initPromises);
    
    console.log(`Worker pool initialized: ${this.workers.length} workers ready`);
  }

  private async createWorker(workerId: number): Promise<void> {
    return new Promise((resolve, reject) => {
      const worker = new Worker(this.config.workerScript);
      let initTimeout: NodeJS.Timeout;
      
      const handleInitComplete = (event: MessageEvent) => {
        const { type, workerId: responseWorkerId, wasmAvailable } = event.data;
        
        if (type === 'init-complete' && responseWorkerId === workerId) {
          clearTimeout(initTimeout);
          worker.removeEventListener('message', handleInitComplete);
          
          // Set up main message handler
          worker.onmessage = (e) => this.handleWorkerMessage(e, workerId);
          worker.onerror = (e) => this.handleWorkerError(e, workerId);
          
          this.workers[workerId] = worker;
          this.workerQueue.push(workerId); // Worker is available
          
          console.log(`Worker ${workerId}: Ready (Wasm: ${wasmAvailable ? 'Yes' : 'No'})`);
          resolve();
        }
      };
      
      // Set up initialization handler
      worker.addEventListener('message', handleInitComplete);
      
      // Initialize worker with unique ID
      worker.postMessage({ 
        type: 'init', 
        data: { workerId } 
      });
      
      // Timeout after 10 seconds
      initTimeout = setTimeout(() => {
        worker.removeEventListener('message', handleInitComplete);
        reject(new Error(`Worker ${workerId} initialization timeout`));
      }, 10000);
    });
  }

  private handleWorkerMessage(event: MessageEvent, workerId: number): void {
    const { type, data } = event.data;
    
    if (type === 'decode-complete') {
      this.handleDecodeComplete(data, workerId);
    } else if (type === 'decode-error') {
      this.handleDecodeError(data, workerId);
    }
  }

  private handleDecodeComplete(data: any, workerId: number): void {
    const { requestId, frameIndex, pixels, delay, decodeTime, wasmUsed } = data;
    
    const pendingRequest = this.pendingRequests.get(requestId);
    if (pendingRequest) {
      // Update statistics
      this.stats.completedJobs++;
      this.stats.totalDecodeTime += decodeTime;
      this.stats.workerUtilization[workerId]++;
      
      // Convert transferred buffer back to Uint32Array
      const pixelsArray = new Uint32Array(pixels);
      
      // Resolve the request
      pendingRequest.resolve({
        requestId,
        frameIndex,
        pixels: pixelsArray,
        delay,
        decodeTime,
        workerId,
        wasmUsed
      });
      
      this.pendingRequests.delete(requestId);
    }
    
    // Worker is now available
    this.workerQueue.push(workerId);
    this.processQueue();
  }

  private handleDecodeError(data: any, workerId: number): void {
    const { requestId, error } = data;
    
    const pendingRequest = this.pendingRequests.get(requestId);
    if (pendingRequest) {
      pendingRequest.reject(new Error(`Worker ${workerId}: ${error}`));
      this.pendingRequests.delete(requestId);
    }
    
    // Worker is available again
    this.workerQueue.push(workerId);
    this.processQueue();
  }

  private handleWorkerError(error: ErrorEvent, workerId: number): void {
    console.error(`Worker ${workerId} error:`, error);
    
    // Reject any pending requests for this worker
    for (const [requestId, pending] of this.pendingRequests.entries()) {
      if (requestId.includes(`-${workerId}-`)) {
        pending.reject(new Error(`Worker ${workerId} crashed`));
        this.pendingRequests.delete(requestId);
      }
    }
  }

  // Main API: Decode frame asynchronously using worker pool
  async decodeFrame(gifData: Uint8Array, frameIndex: number): Promise<FrameDecodeResult> {
    return new Promise((resolve, reject) => {
      const requestId = `${Date.now()}-${this.nextRequestId++}`;
      
      const request: FrameDecodeRequest = {
        gifData: gifData.slice(), // Copy to ensure transferable
        frameIndex,
        requestId
      };
      
      if (this.requestQueue.length >= this.config.maxQueueSize) {
        reject(new Error('Worker pool queue is full'));
        return;
      }
      
      this.requestQueue.push({ request, resolve, reject });
      this.processQueue();
    });
  }

  // Parallel decode multiple frames
  async decodeFrames(gifData: Uint8Array, frameIndices: number[]): Promise<FrameDecodeResult[]> {
    const decodePromises = frameIndices.map(frameIndex => 
      this.decodeFrame(gifData, frameIndex)
    );
    
    return Promise.all(decodePromises);
  }

  private processQueue(): void {
    // Process as many requests as we have available workers
    while (this.requestQueue.length > 0 && this.workerQueue.length > 0) {
      const { request, resolve, reject } = this.requestQueue.shift()!;
      const workerId = this.workerQueue.shift()!;
      
      // Track the pending request
      this.pendingRequests.set(request.requestId, {
        resolve,
        reject,
        startTime: performance.now()
      });
      
      // Send request to worker with transferable buffer
      this.workers[workerId].postMessage({
        type: 'decode',
        data: request
      }, [request.gifData.buffer]);
    }
  }

  getStats(): WorkerPoolStats {
    const activeWorkers = this.workers.length - this.workerQueue.length;
    const avgDecodeTime = this.stats.completedJobs > 0 
      ? this.stats.totalDecodeTime / this.stats.completedJobs 
      : 0;
    
    return {
      activeWorkers,
      queuedRequests: this.requestQueue.length,
      completedJobs: this.stats.completedJobs,
      avgDecodeTime,
      totalDecodeTime: this.stats.totalDecodeTime,
      workerUtilization: this.stats.workerUtilization.slice()
    };
  }

  // Clean shutdown
  async terminate(): Promise<void> {
    console.log('Terminating worker pool...');
    
    // Reject any pending requests
    for (const [requestId, pending] of this.pendingRequests.entries()) {
      pending.reject(new Error('Worker pool terminated'));
    }
    this.pendingRequests.clear();
    this.requestQueue.length = 0;
    
    // Terminate all workers
    const terminatePromises = this.workers.map((worker, i) => {
      return new Promise<void>(resolve => {
        worker.postMessage({ type: 'terminate' });
        worker.terminate();
        resolve();
      });
    });
    
    await Promise.all(terminatePromises);
    
    this.workers.length = 0;
    this.workerQueue.length = 0;
    
    console.log('Worker pool terminated');
  }
}

// Global worker pool instance
let globalWorkerPool: WorkerPoolManager | null = null;

export async function createWorkerPool(config?: WorkerPoolConfig): Promise<WorkerPoolManager> {
  const pool = new WorkerPoolManager(config);
  await pool.initialize();
  return pool;
}

export async function getGlobalWorkerPool(config?: WorkerPoolConfig): Promise<WorkerPoolManager> {
  if (!globalWorkerPool) {
    globalWorkerPool = await createWorkerPool(config);
  }
  return globalWorkerPool;
}

export async function terminateGlobalWorkerPool(): Promise<void> {
  if (globalWorkerPool) {
    await globalWorkerPool.terminate();
    globalWorkerPool = null;
  }
}

export { WorkerPoolManager };