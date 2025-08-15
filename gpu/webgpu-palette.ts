// gpu/webgpu-palette.ts
// WebGPU compute shader for ultra-fast palette expansion

export interface WebGPUPaletteRenderer {
  render: (indexData: Uint8Array, palette: Uint32Array, width: number, height: number) => Promise<HTMLCanvasElement>;
  renderToCanvas: (indexData: Uint8Array, palette: Uint32Array, width: number, height: number, targetCanvas: HTMLCanvasElement) => Promise<void>;
  updatePalette: (palette: Uint32Array) => void;
  dispose: () => void;
  isSupported: () => boolean;
}

export class WebGPUGifPaletteRenderer implements WebGPUPaletteRenderer {
  private device: GPUDevice;
  private computePipeline: GPUComputePipeline;
  private renderPipeline: GPURenderPipeline;
  private canvas: HTMLCanvasElement;
  private context: GPUCanvasContext;
  
  // Buffers and textures
  private paletteBuffer: GPUBuffer;
  private indexTexture: GPUTexture | null = null;
  private outputTexture: GPUTexture | null = null;
  private uniformBuffer: GPUBuffer;
  
  // Bind groups
  private computeBindGroup: GPUBindGroup | null = null;
  
  // Cached data
  private paletteData = new Uint32Array(256);
  private currentWidth = 0;
  private currentHeight = 0;
  
  constructor(device: GPUDevice, canvas?: HTMLCanvasElement) {
    this.device = device;
    this.canvas = canvas || document.createElement('canvas');
    
    // Get WebGPU canvas context
    this.context = this.canvas.getContext('webgpu')!;
    this.context.configure({
      device: this.device,
      format: 'bgra8unorm',
      alphaMode: 'opaque',
      usage: GPUTextureUsage.RENDER_ATTACHMENT
    });
    
    this.initPipelines();
    this.initBuffers();
    
    console.log('WebGPU GPU palette renderer initialized');
  }
  
  private initPipelines(): void {
    const device = this.device;
    
    // Compute shader for palette expansion
    const computeShaderModule = device.createShaderModule({
      label: 'Palette Expand Compute Shader',
      code: `
// Palette expansion compute shader
@group(0) @binding(0) var indexTexture: texture_2d<u32>;
@group(0) @binding(1) var outputTexture: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> palette: array<vec4<f32>, 256>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
  let coords = vec2<i32>(i32(global_id.x), i32(global_id.y));
  let dimensions = textureDimensions(indexTexture);
  
  // Bounds check
  if (coords.x >= i32(dimensions.x) || coords.y >= i32(dimensions.y)) {
    return;
  }
  
  // Load palette index from texture (0-255)
  let indexValue = textureLoad(indexTexture, coords, 0).r;
  let paletteIndex = min(indexValue, 255u);
  
  // Look up color in palette
  let color = palette[paletteIndex];
  
  // Write RGBA color to output texture
  textureStore(outputTexture, coords, color);
}`
    });
    
    // Create compute pipeline
    this.computePipeline = device.createComputePipeline({
      label: 'Palette Expansion Pipeline',
      layout: 'auto',
      compute: {
        module: computeShaderModule,
        entryPoint: 'main'
      }
    });
    
    // Vertex shader for display
    const vertexShaderModule = device.createShaderModule({
      label: 'Display Vertex Shader',
      code: `
struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) texCoord: vec2<f32>,
}

@vertex
fn main(@builtin(vertex_index) vertexIndex: u32) -> VertexOutput {
  var pos = array<vec2<f32>, 3>(
    vec2(-1.0, -1.0),
    vec2( 3.0, -1.0),
    vec2(-1.0,  3.0)
  );
  
  var out: VertexOutput;
  out.position = vec4<f32>(pos[vertexIndex], 0.0, 1.0);
  out.texCoord = pos[vertexIndex] * 0.5 + 0.5;
  return out;
}`
    });
    
    // Fragment shader for display
    const fragmentShaderModule = device.createShaderModule({
      label: 'Display Fragment Shader', 
      code: `
@group(0) @binding(0) var displayTexture: texture_2d<f32>;
@group(0) @binding(1) var displaySampler: sampler;

@fragment
fn main(@location(0) texCoord: vec2<f32>) -> @location(0) vec4<f32> {
  return textureSample(displayTexture, displaySampler, texCoord);
}`
    });
    
    // Create render pipeline
    this.renderPipeline = device.createRenderPipeline({
      label: 'Display Pipeline',
      layout: 'auto',
      vertex: {
        module: vertexShaderModule,
        entryPoint: 'main'
      },
      fragment: {
        module: fragmentShaderModule,
        entryPoint: 'main',
        targets: [{
          format: 'bgra8unorm'
        }]
      },
      primitive: {
        topology: 'triangle-list'
      }
    });
  }
  
  private initBuffers(): void {
    const device = this.device;
    
    // Create palette buffer (256 * 16 bytes = 4KB)
    this.paletteBuffer = device.createBuffer({
      label: 'Palette Buffer',
      size: 256 * 16, // 256 colors * 4 components * 4 bytes
      usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST
    });
    
    // Create uniforms buffer for compute shader metadata
    this.uniformBuffer = device.createBuffer({
      label: 'Uniform Buffer',
      size: 32, // Extra space for alignment
      usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST
    });
  }
  
  private ensureTextures(width: number, height: number): void {
    const device = this.device;
    
    if (this.currentWidth === width && this.currentHeight === height) {
      return; // Textures already correct size
    }
    
    // Dispose old textures
    this.indexTexture?.destroy();
    this.outputTexture?.destroy();
    
    // Create index texture (R8Uint format for palette indices)
    this.indexTexture = device.createTexture({
      label: 'Index Texture',
      size: { width, height },
      format: 'r8uint',
      usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST
    });
    
    // Create output texture (RGBA8 for final colors)
    this.outputTexture = device.createTexture({
      label: 'Output Texture',
      size: { width, height },
      format: 'rgba8unorm',
      usage: GPUTextureUsage.STORAGE_BINDING | GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_SRC
    });
    
    // Create compute bind group
    this.computeBindGroup = device.createBindGroup({
      label: 'Compute Bind Group',
      layout: this.computePipeline.getBindGroupLayout(0),
      entries: [
        { binding: 0, resource: this.indexTexture.createView() },
        { binding: 1, resource: this.outputTexture.createView() },
        { binding: 2, resource: { buffer: this.paletteBuffer } }
      ]
    });
    
    this.currentWidth = width;
    this.currentHeight = height;
  }
  
  updatePalette(palette: Uint32Array): void {
    // Cache palette data
    this.paletteData.set(palette.subarray(0, Math.min(palette.length, 256)));
    
    // Convert to Float32 array for GPU (RGBA components 0.0-1.0)  
    const paletteFloat = new Float32Array(256 * 4);
    
    for (let i = 0; i < 256; i++) {
      const color = i < palette.length ? palette[i] : 0;
      
      // Extract RGBA components
      const r = (color & 0xFF) / 255.0;
      const g = ((color >> 8) & 0xFF) / 255.0;
      const b = ((color >> 16) & 0xFF) / 255.0;
      const a = ((color >> 24) & 0xFF) / 255.0;
      
      const baseIndex = i * 4;
      paletteFloat[baseIndex] = r;
      paletteFloat[baseIndex + 1] = g;
      paletteFloat[baseIndex + 2] = b;
      paletteFloat[baseIndex + 3] = a;
    }
    
    // Upload to GPU
    this.device.queue.writeBuffer(this.paletteBuffer, 0, paletteFloat);
  }
  
  async renderToCanvas(indexData: Uint8Array, palette: Uint32Array, width: number, height: number, targetCanvas: HTMLCanvasElement): Promise<void> {
    const device = this.device;
    
    // Update palette
    this.updatePalette(palette);
    
    // Ensure textures are correct size
    this.ensureTextures(width, height);
    
    // Resize target canvas
    if (targetCanvas.width !== width || targetCanvas.height !== height) {
      targetCanvas.width = width;
      targetCanvas.height = height;
    }
    
    // Upload index data
    device.queue.writeTexture(
      { texture: this.indexTexture! },
      indexData,
      { bytesPerRow: width },
      { width, height }
    );
    
    // Create command encoder
    const commandEncoder = device.createCommandEncoder({ label: 'Palette Expansion Commands' });
    
    // Compute pass - expand palette
    const computePass = commandEncoder.beginComputePass({ label: 'Palette Expansion Pass' });
    computePass.setPipeline(this.computePipeline);
    computePass.setBindGroup(0, this.computeBindGroup!);
    
    // Dispatch compute shader (8x8 workgroups)
    const workgroupsX = Math.ceil(width / 8);
    const workgroupsY = Math.ceil(height / 8);
    computePass.dispatchWorkgroups(workgroupsX, workgroupsY);
    computePass.end();
    
    // Copy result to canvas if different from internal canvas
    if (targetCanvas !== this.canvas) {
      // Render pass - display to target canvas
      this.canvas.width = width;
      this.canvas.height = height;
      
      const renderPassDescriptor: GPURenderPassDescriptor = {
        label: 'Display Pass',
        colorAttachments: [{
          view: this.context.getCurrentTexture().createView(),
          clearValue: { r: 0, g: 0, b: 0, a: 1 },
          loadOp: 'clear',
          storeOp: 'store'
        }]
      };
      
      const renderPass = commandEncoder.beginRenderPass(renderPassDescriptor);
      renderPass.setPipeline(this.renderPipeline);
      
      // Create bind group for display
      const displayBindGroup = device.createBindGroup({
        layout: this.renderPipeline.getBindGroupLayout(0),
        entries: [
          { binding: 0, resource: this.outputTexture!.createView() },
          { binding: 1, resource: device.createSampler({ magFilter: 'nearest', minFilter: 'nearest' }) }
        ]
      });
      
      renderPass.setBindGroup(0, displayBindGroup);
      renderPass.draw(3); // Full-screen triangle
      renderPass.end();
    }
    
    // Submit commands
    device.queue.submit([commandEncoder.finish()]);
    
    // Wait for completion and copy to target canvas
    await device.queue.onSubmittedWorkDone();
    
    if (targetCanvas !== this.canvas) {
      const targetCtx = targetCanvas.getContext('2d')!;
      targetCtx.drawImage(this.canvas, 0, 0);
    }
  }
  
  async render(indexData: Uint8Array, palette: Uint32Array, width: number, height: number): Promise<HTMLCanvasElement> {
    const outputCanvas = document.createElement('canvas');
    await this.renderToCanvas(indexData, palette, width, height, outputCanvas);
    return outputCanvas;
  }
  
  isSupported(): boolean {
    return !!this.device;
  }
  
  dispose(): void {
    this.indexTexture?.destroy();
    this.outputTexture?.destroy();
    this.paletteBuffer?.destroy();
    this.uniformBuffer?.destroy();
    
    this.indexTexture = null;
    this.outputTexture = null;
    this.computeBindGroup = null;
  }
}

// Factory function with feature detection
export async function createWebGPUPaletteRenderer(canvas?: HTMLCanvasElement): Promise<WebGPUPaletteRenderer | null> {
  try {
    if (!navigator.gpu) {
      return null;
    }
    
    const adapter = await navigator.gpu.requestAdapter({
      powerPreference: 'high-performance'
    });
    
    if (!adapter) {
      return null;
    }
    
    const device = await adapter.requestDevice({
      requiredFeatures: [],
      requiredLimits: {}
    });
    
    return new WebGPUGifPaletteRenderer(device, canvas);
    
  } catch (error) {
    console.warn('WebGPU palette renderer not available:', error);
    return null;
  }
}

// Feature detection
export function isWebGPUSupported(): boolean {
  return typeof navigator !== 'undefined' && !!navigator.gpu;
}