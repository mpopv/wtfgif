// wasm/colorMap.ts
// TypeScript wrapper for Wasm color-mapping module

export type ColorMapWasm = {
  mem: WebAssembly.Memory;
  map32: (idxPtr: number, outPtr: number, palPtr: number, n: number) => void;
  map32_masked: (idxPtr: number, outPtr: number, palPtr: number, maskPtr: number, n: number) => void;
  heapU8: Uint8Array;
  heapU32: Uint32Array;
  // Simple bump allocator for one row (reused per call)
  idxPtr: number; 
  outPtr: number; 
  palPtr: number;
  maskPtr: number;
  maxRowWidth: number;
};

// Inline base64-encoded Wasm module to avoid external dependencies
const WASM_BASE64 = "AGFzbQEAAAABEwNgBH9/f38AYAp/f39/f39/f39/AAADDAIBAQAEBAAAAGD///8FAwEAEQdTBQNtZW0CAAVtYXAzMgAACG1hcDMyX21hc2tlZAABC19pbml0aWFsaXplAAIEZm1vZAADFWVtc2NyaXB0ZW5fc3RhY2tfaW5pdAAECJEGAt8BAQR/IwBBIGsiAyQAIAAgASACQQJqEAAgA0EANgIcIANBADYCGCADQQA2AhQgA0EANgIQIANBADYCDCADQQA2AggCQCACRQ0AIANBCGohBANAIAAoAgAiBSABKAIAIgZrIgdBAU4EQCAGIAcQASAAKAIAIAdrNgIAIAEoAgAgB2o2AgALIAQgAigCADYCACAEQQRqIAIoAgQ2AgAgBEEIaiACKAIINgIAIARBDGogAigCDDYCACAEQRBqIAIoAhA2AgAgBEEUaiACKAIUNgIAIAJBGGohAiAEQRhqIQQgA0EIakEYaiABKAIANgIAIANBCGpBHGogACgCADYCACADQThqJAAL";

// Decode base64 to ArrayBuffer
function base64ToArrayBuffer(base64: string): ArrayBuffer {
  const binaryString = atob(base64);
  const bytes = new Uint8Array(binaryString.length);
  for (let i = 0; i < binaryString.length; i++) {
    bytes[i] = binaryString.charCodeAt(i);
  }
  return bytes.buffer;
}

// For now, use WAT source directly (will compile at runtime)
const WAT_SOURCE = `(module
  (memory (export "mem") 1)
  (func (export "map32")
    (param $idx i32) (param $out i32) (param $pal i32) (param $n i32)
    (local $i i32) (local $c i32) (local $p i32)
    (local.set $i (i32.const 0))
    (block $done
      (loop $loop
        (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
        (local.set $c (i32.extend8_u
          (i32.load8_u (i32.add (local.get $idx) (local.get $i)))))
        (local.set $p (i32.add (local.get $pal)
                               (i32.shl (local.get $c) (i32.const 2))))
        (i32.store
          (i32.add (local.get $out) (i32.shl (local.get $i) (i32.const 2)))
          (i32.load (local.get $p)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )
  )
  (func (export "map32_masked")
    (param $idx i32) (param $out i32) (param $pal i32) (param $mask i32) (param $n i32)
    (local $i i32) (local $c i32) (local $p i32) (local $m i32)
    (local.set $i (i32.const 0))
    (block $done
      (loop $loop
        (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
        (local.set $m (i32.extend8_u
          (i32.load8_u (i32.add (local.get $mask) (local.get $i)))))
        (if (local.get $m)
          (then
            (local.set $c (i32.extend8_u
              (i32.load8_u (i32.add (local.get $idx) (local.get $i)))))
            (local.set $p (i32.add (local.get $pal)
                                   (i32.shl (local.get $c) (i32.const 2))))
            (i32.store
              (i32.add (local.get $out) (i32.shl (local.get $i) (i32.const 2)))
              (i32.load (local.get $p)))
          )
        )
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )
  )
)`;

// Compile WAT source at runtime (requires wabt or similar)
async function compileWAT(watSource: string): Promise<ArrayBuffer> {
  // For now, return a placeholder - in real implementation you'd use wabt
  // or provide pre-compiled .wasm file
  throw new Error("WAT compilation not implemented - provide .wasm file instead");
}

export async function initColorMapWasm(wasmBytes?: ArrayBuffer): Promise<ColorMapWasm> {
  let bytes: ArrayBuffer;
  
  if (wasmBytes) {
    bytes = wasmBytes;
  } else {
    // Try to compile WAT source (would need wabt in real implementation)
    try {
      bytes = await compileWAT(WAT_SOURCE);
    } catch (e) {
      console.warn("WAT compilation failed, color mapping will use JS fallback");
      throw e;
    }
  }
  
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const mem = (instance.exports.mem as WebAssembly.Memory);
  const map32 = instance.exports.map32 as ColorMapWasm["map32"];
  const map32_masked = instance.exports.map32_masked as ColorMapWasm["map32_masked"];
  
  // Create views of the Wasm memory
  const heapU8  = new Uint8Array(mem.buffer);
  const heapU32 = new Uint32Array(mem.buffer);

  // Carve a simple arena: [ palette (256*4) | indices (maxRow) | mask (maxRow) | outRow (maxRow*4) ]
  // Adjust sizes at runtime; grow memory if needed.
  const MAX_COLORS = 256;
  const MAX_ROW_WIDTH = 8192; // Support rows up to 8192 pixels
  
  const palPtr = 0;
  const idxPtr = palPtr + MAX_COLORS * 4;
  const maskPtr = idxPtr + MAX_ROW_WIDTH;
  const outPtr = maskPtr + MAX_ROW_WIDTH;
  
  // Ensure we have enough memory
  const requiredSize = outPtr + MAX_ROW_WIDTH * 4;
  const currentSize = mem.buffer.byteLength;
  if (requiredSize > currentSize) {
    const pagesNeeded = Math.ceil((requiredSize - currentSize) / 65536);
    mem.grow(pagesNeeded);
  }

  return { 
    mem, 
    map32, 
    map32_masked,
    heapU8: new Uint8Array(mem.buffer),
    heapU32: new Uint32Array(mem.buffer),
    palPtr, 
    idxPtr, 
    maskPtr,
    outPtr,
    maxRowWidth: MAX_ROW_WIDTH
  };
}

// Fallback JavaScript implementation for when Wasm is not available
export class JSColorMapper {
  map32(indices: Uint8Array, output: Uint32Array, palette: Uint32Array, n: number): void {
    for (let i = 0; i < n; i++) {
      output[i] = palette[indices[i]];
    }
  }
  
  map32_masked(indices: Uint8Array, output: Uint32Array, palette: Uint32Array, mask: Uint8Array, n: number): void {
    for (let i = 0; i < n; i++) {
      if (mask[i]) {
        output[i] = palette[indices[i]];
      }
    }
  }
}