// gpu/webgl2-palette.ts
// WebGL 2 GPU palette expansion for ultra-fast RGBA conversion

export interface WebGL2PaletteRenderer {
  render: (indexData: Uint8Array, palette: Uint32Array, width: number, height: number) => HTMLCanvasElement;
  renderToCanvas: (indexData: Uint8Array, palette: Uint32Array, width: number, height: number, targetCanvas: HTMLCanvasElement) => void;
  updatePalette: (palette: Uint32Array) => void;
  dispose: () => void;
  isSupported: () => boolean;
}

export class WebGL2GifPaletteRenderer implements WebGL2PaletteRenderer {
  private gl: WebGL2RenderingContext;
  private program: WebGLProgram;
  private indexTexture: WebGLTexture;
  private canvas: HTMLCanvasElement;
  private vao: WebGLVertexArrayObject;
  
  // Shader uniforms
  private uIndexTexLoc: WebGLUniformLocation;
  private uPaletteLoc: WebGLUniformLocation;
  private uResolutionLoc: WebGLUniformLocation;
  
  // Cached palette data
  private paletteFloat32 = new Float32Array(256 * 4); // 256 RGBA colors
  
  constructor(canvas?: HTMLCanvasElement) {
    this.canvas = canvas || document.createElement('canvas');
    
    // Try to get WebGL 2 context
    const gl = this.canvas.getContext('webgl2', {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      preserveDrawingBuffer: false,
      powerPreference: 'high-performance'
    });
    
    if (!gl) {
      throw new Error('WebGL 2 not supported');
    }
    
    this.gl = gl;
    this.initShaders();
    this.initGeometry();
    this.initTextures();
    
    console.log('WebGL2 GPU palette renderer initialized');
  }
  
  private initShaders(): void {
    const gl = this.gl;
    
    // Vertex shader - simple full-screen triangle
    const vertexShaderSource = `#version 300 es
precision highp float;

layout(location = 0) in vec2 a_position;
out vec2 v_texCoord;

void main() {
  gl_Position = vec4(a_position, 0.0, 1.0);
  v_texCoord = a_position * 0.5 + 0.5;
}`;

    // Fragment shader - palette lookup
    const fragmentShaderSource = `#version 300 es
precision mediump float;

uniform sampler2D uIndexTex;     // 8-bit index texture
uniform vec4 uPalette[256];      // 256 RGBA colors  
uniform vec2 uResolution;        // Frame dimensions

in vec2 v_texCoord;
out vec4 fragColor;

void main() {
  // Sample index from texture (0-1 range)
  float indexFloat = texture(uIndexTex, v_texCoord).r;
  
  // Convert to palette index (0-255)
  int idx = int(indexFloat * 255.0 + 0.5);
  
  // Clamp to valid range
  idx = clamp(idx, 0, 255);
  
  // Output palette color
  fragColor = uPalette[idx];
}`;

    const vertexShader = this.createShader(gl.VERTEX_SHADER, vertexShaderSource);
    const fragmentShader = this.createShader(gl.FRAGMENT_SHADER, fragmentShaderSource);
    
    this.program = gl.createProgram()!;
    gl.attachShader(this.program, vertexShader);
    gl.attachShader(this.program, fragmentShader);
    gl.linkProgram(this.program);
    
    if (!gl.getProgramParameter(this.program, gl.LINK_STATUS)) {
      const error = gl.getProgramInfoLog(this.program);
      throw new Error('Shader program failed to link: ' + error);
    }
    
    // Get uniform locations
    this.uIndexTexLoc = gl.getUniformLocation(this.program, 'uIndexTex')!;
    this.uPaletteLoc = gl.getUniformLocation(this.program, 'uPalette')!;
    this.uResolutionLoc = gl.getUniformLocation(this.program, 'uResolution')!;
    
    // Cleanup
    gl.deleteShader(vertexShader);
    gl.deleteShader(fragmentShader);
  }
  
  private createShader(type: number, source: string): WebGLShader {
    const gl = this.gl;
    const shader = gl.createShader(type)!;
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      const error = gl.getShaderInfoLog(shader);
      gl.deleteShader(shader);
      throw new Error('Shader compilation failed: ' + error);
    }
    
    return shader;
  }
  
  private initGeometry(): void {
    const gl = this.gl;
    
    // Full-screen triangle vertices (clip space)
    const vertices = new Float32Array([
      -1, -1,  // Bottom-left
       3, -1,  // Bottom-right (extends beyond screen)
      -1,  3   // Top-left (extends beyond screen)
    ]);
    
    const vbo = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
    gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);
    
    // Create VAO
    this.vao = gl.createVertexArrayObject()!;
    gl.bindVertexArrayObject(this.vao);
    
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    
    gl.bindVertexArrayObject(null);
    gl.bindBuffer(gl.ARRAY_BUFFER, null);
  }
  
  private initTextures(): void {
    const gl = this.gl;
    
    // Create index texture
    this.indexTexture = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, this.indexTexture);
    
    // Set texture parameters for pixel-perfect sampling
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    
    gl.bindTexture(gl.TEXTURE_2D, null);
  }
  
  updatePalette(palette: Uint32Array): void {
    // Convert Uint32 palette to Float32 vec4 array
    for (let i = 0; i < 256; i++) {
      const color = i < palette.length ? palette[i] : 0;
      
      // Extract RGBA components (assuming little-endian RGBA format)
      const r = (color & 0xFF) / 255.0;
      const g = ((color >> 8) & 0xFF) / 255.0;  
      const b = ((color >> 16) & 0xFF) / 255.0;
      const a = ((color >> 24) & 0xFF) / 255.0;
      
      const baseIndex = i * 4;
      this.paletteFloat32[baseIndex] = r;
      this.paletteFloat32[baseIndex + 1] = g;
      this.paletteFloat32[baseIndex + 2] = b;
      this.paletteFloat32[baseIndex + 3] = a;
    }
  }
  
  renderToCanvas(indexData: Uint8Array, palette: Uint32Array, width: number, height: number, targetCanvas: HTMLCanvasElement): void {
    const gl = this.gl;
    
    // Update palette if needed
    this.updatePalette(palette);
    
    // Resize target canvas if needed
    if (targetCanvas.width !== width || targetCanvas.height !== height) {
      targetCanvas.width = width;
      targetCanvas.height = height;
    }
    
    // Resize internal canvas if needed
    if (this.canvas.width !== width || this.canvas.height !== height) {
      this.canvas.width = width;
      this.canvas.height = height;
      gl.viewport(0, 0, width, height);
    }
    
    // Upload index data as R8 texture
    gl.bindTexture(gl.TEXTURE_2D, this.indexTexture);
    gl.texImage2D(
      gl.TEXTURE_2D, 0, gl.R8,           // Internal format: single 8-bit channel
      width, height, 0,                  // Size and border
      gl.RED, gl.UNSIGNED_BYTE,          // Format and type
      indexData                          // Data
    );
    
    // Render
    gl.useProgram(this.program);
    gl.bindVertexArrayObject(this.vao);
    
    // Set uniforms
    gl.uniform1i(this.uIndexTexLoc, 0); // Texture unit 0
    gl.uniform4fv(this.uPaletteLoc, this.paletteFloat32);
    gl.uniform2f(this.uResolutionLoc, width, height);
    
    // Bind texture to unit 0
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.indexTexture);
    
    // Draw full-screen triangle
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    
    // Copy result to target canvas
    if (targetCanvas !== this.canvas) {
      const targetCtx = targetCanvas.getContext('2d')!;
      targetCtx.drawImage(this.canvas, 0, 0);
    }
    
    // Cleanup
    gl.bindVertexArrayObject(null);
    gl.useProgram(null);
  }
  
  render(indexData: Uint8Array, palette: Uint32Array, width: number, height: number): HTMLCanvasElement {
    const outputCanvas = document.createElement('canvas');
    this.renderToCanvas(indexData, palette, width, height, outputCanvas);
    return outputCanvas;
  }
  
  isSupported(): boolean {
    return !!this.gl;
  }
  
  dispose(): void {
    const gl = this.gl;
    
    if (this.program) {
      gl.deleteProgram(this.program);
    }
    if (this.indexTexture) {
      gl.deleteTexture(this.indexTexture);
    }
    if (this.vao) {
      gl.deleteVertexArrayObject(this.vao);
    }
    
    // Lose WebGL context to free GPU memory
    const loseContext = gl.getExtension('WEBGL_lose_context');
    if (loseContext) {
      loseContext.loseContext();
    }
  }
}

// Factory function with feature detection
export async function createWebGL2PaletteRenderer(canvas?: HTMLCanvasElement): Promise<WebGL2PaletteRenderer | null> {
  try {
    return new WebGL2GifPaletteRenderer(canvas);
  } catch (error) {
    console.warn('WebGL2 palette renderer not available:', error);
    return null;
  }
}

// Feature detection
export function isWebGL2Supported(): boolean {
  try {
    const testCanvas = document.createElement('canvas');
    const gl = testCanvas.getContext('webgl2');
    return !!gl;
  } catch {
    return false;
  }
}