module.exports = {
  async createWasmGifDecoder() {
    return {};
  },
  async createWasmWorkerPool() {
    return {
      decodeFrame: async () => ({ pixels: new Uint32Array(), delay: 0 }),
      decodeFrames: async () => [],
      terminate: () => {},
      getStats: () => ({ activeWorkers: 0, completedJobs: 0, avgDecodeTime: 0 }),
    };
  },
  isWasmSupported() {
    return true;
  },
  isWasmSIMDSupported() {
    return true;
  },
  isWasmThreadsSupported() {
    return true;
  },
};
