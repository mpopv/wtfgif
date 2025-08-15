// canvas/zerocopydisplay.ts
// Zero-copy streaming from GIF decoder to canvas display

export class ZeroCopyCanvasDisplay {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D;
  private imageData: ImageData | null = null;
  private lastWidth = 0;
  private lastHeight = 0;
  
  constructor(canvas: HTMLCanvasElement) {
    this.canvas = canvas;
    const ctx = canvas.getContext('2d');
    if (!ctx) {
      throw new Error('Failed to get 2D context from canvas');
    }
    this.ctx = ctx;
  }
  
  /**
   * Display frame pixels with zero-copy optimization.
   * Creates ImageData directly from Uint32Array backing store to eliminate one copy.
   */
  displayFrame(pixels: Uint32Array, width: number, height: number): void {
    // Resize canvas if needed
    if (this.canvas.width !== width || this.canvas.height !== height) {
      this.canvas.width = width;
      this.canvas.height = height;
    }
    
    // Create or reuse ImageData with zero-copy view
    if (!this.imageData || this.lastWidth !== width || this.lastHeight !== height) {
      // Create Uint8ClampedArray view directly from Uint32Array backing store (zero-copy)
      const pixelBytes = new Uint8ClampedArray(
        pixels.buffer, 
        pixels.byteOffset, 
        pixels.length * 4
      );
      this.imageData = new ImageData(pixelBytes, width, height);
      this.lastWidth = width;
      this.lastHeight = height;
    } else {
      // Reuse existing ImageData and update its data (still zero-copy view)
      const pixelBytes = new Uint8ClampedArray(
        pixels.buffer, 
        pixels.byteOffset, 
        pixels.length * 4
      );
      this.imageData.data.set(pixelBytes);
    }
    
    // Single unavoidable copy into canvas
    this.ctx.putImageData(this.imageData, 0, 0);
  }
  
  /**
   * Alternative method that reuses the same ImageData object for maximum efficiency
   */
  displayFrameReuse(pixels: Uint32Array, width: number, height: number): void {
    if (this.canvas.width !== width || this.canvas.height !== height) {
      this.canvas.width = width;
      this.canvas.height = height;
      this.imageData = null; // Force recreation
    }
    
    if (!this.imageData) {
      // Create ImageData with backing store shared with our pixel buffer
      const pixelBytes = new Uint8ClampedArray(
        pixels.buffer, 
        pixels.byteOffset, 
        width * height * 4
      );
      this.imageData = new ImageData(pixelBytes, width, height);
    } else {
      // Update existing ImageData by setting its data array
      // This creates a zero-copy view then does one copy into ImageData store  
      const pixelBytes = new Uint8ClampedArray(
        pixels.buffer, 
        pixels.byteOffset, 
        width * height * 4
      );
      this.imageData.data.set(pixelBytes);
    }
    
    this.ctx.putImageData(this.imageData, 0, 0);
  }
  
  /**
   * Clear the canvas to a specific background color
   */
  clear(r: number = 0, g: number = 0, b: number = 0, a: number = 1): void {
    this.ctx.fillStyle = `rgba(${r},${g},${b},${a})`;
    this.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
  }
  
  /**
   * Clear a specific rectangle (useful for disposal operations)
   */
  clearRect(x: number, y: number, width: number, height: number): void {
    this.ctx.clearRect(x, y, width, height);
  }
  
  /**
   * Fill a specific rectangle with a color
   */
  fillRect(x: number, y: number, width: number, height: number, 
           r: number, g: number, b: number, a: number = 1): void {
    this.ctx.fillStyle = `rgba(${r},${g},${b},${a})`;
    this.ctx.fillRect(x, y, width, height);
  }
  
  /**
   * Get canvas size
   */
  getSize(): { width: number; height: number } {
    return { width: this.canvas.width, height: this.canvas.height };
  }
  
  /**
   * Set canvas size
   */
  setSize(width: number, height: number): void {
    this.canvas.width = width;
    this.canvas.height = height;
    this.imageData = null; // Force recreation on next display
  }
}