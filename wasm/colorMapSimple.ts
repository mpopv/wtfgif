// wasm/colorMapSimple.ts
// Simplified Wasm color-mapping with inline binary module

export type ColorMapWasm = {
  mem: WebAssembly.Memory;
  map32: (idxPtr: number, outPtr: number, palPtr: number, n: number) => void;
  heapU8: Uint8Array;
  heapU32: Uint32Array;
  palPtr: number; 
  idxPtr: number; 
  outPtr: number;
  maxRowWidth: number;
};

// Hand-crafted minimal Wasm module (hex-encoded)
// This implements the map32 function from color-map.wat
const WASM_HEX = `
0061736d0100000001070160047f7f7f7f00030201000504010001100605017f01418080040b071102046d656d0200056d61703332000009040100418080040a2e012c00200041004a044040034020002d00002201410274200241027420036a28020036020020004101460440200141016a21010c010b200141016a210120004101400b0b0b001a046e616d6501131100046d61703302000103696478010170616c0200016e
`.replace(/\s+/g, '');

function hexToArrayBuffer(hex: string): ArrayBuffer {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = parseInt(hex.substr(i, 2), 16);
  }
  return bytes.buffer;
}

// Simple manual Wasm module creation
function createMinimalWasmModule(): ArrayBuffer {
  // Manually craft a minimal Wasm module
  const magicNumber = [0x00, 0x61, 0x73, 0x6d]; // '\0asm'
  const version = [0x01, 0x00, 0x00, 0x00];     // version 1
  
  // Type section (function signatures)
  const typeSection = [
    0x01,           // section id
    0x07,           // section size
    0x01,           // number of types
    0x60,           // func type
    0x04,           // 4 parameters
    0x7f, 0x7f, 0x7f, 0x7f, // i32, i32, i32, i32
    0x00            // 0 results
  ];
  
  // Function section (declare functions)
  const funcSection = [
    0x03,           // section id  
    0x02,           // section size
    0x01,           // number of functions
    0x00            // function 0 uses type 0
  ];
  
  // Memory section
  const memorySection = [
    0x05,           // section id
    0x04,           // section size
    0x01,           // number of memories
    0x01,           // memory type (has max)
    0x10, 0x00      // initial pages = 16 (1MB)
  ];
  
  // Export section
  const exportSection = [
    0x07,           // section id
    0x11,           // section size
    0x02,           // number of exports
    // Export memory
    0x03, 0x6d, 0x65, 0x6d, // "mem"
    0x02,           // memory export
    0x00,           // memory index 0
    // Export function
    0x05, 0x6d, 0x61, 0x70, 0x33, 0x32, // "map32"
    0x00,           // function export  
    0x00            // function index 0
  ];
  
  // Code section (function implementations)
  const codeSection = [
    0x0a,           // section id
    0x20,           // section size
    0x01,           // number of functions
    0x1e,           // function 0 size
    0x03,           // number of locals
    0x01, 0x7f,     // 1 local of type i32 (i)
    0x01, 0x7f,     // 1 local of type i32 (c) 
    0x01, 0x7f,     // 1 local of type i32 (p)
    // Function body: simple loop that maps indices to colors
    0x41, 0x00,     // i32.const 0
    0x21, 0x04,     // local.set $i
    0x02, 0x40,     // block
    0x03, 0x40,     // loop
    0x20, 0x04,     // local.get $i
    0x20, 0x03,     // local.get $n
    0x4f,           // i32.ge_u
    0x0d, 0x01,     // br_if 1 (exit block)
    0x20, 0x00,     // local.get $idx
    0x20, 0x04,     // local.get $i
    0x6a,           // i32.add
    0x2d, 0x00, 0x00, // i32.load8_u
    0x21, 0x05,     // local.set $c
    0x20, 0x01,     // local.get $out
    0x20, 0x04,     // local.get $i
    0x41, 0x02,     // i32.const 2
    0x74,           // i32.shl
    0x6a,           // i32.add
    0x20, 0x02,     // local.get $pal
    0x20, 0x05,     // local.get $c
    0x41, 0x02,     // i32.const 2
    0x74,           // i32.shl
    0x6a,           // i32.add
    0x28, 0x02, 0x00, // i32.load
    0x36, 0x02, 0x00, // i32.store
    0x20, 0x04,     // local.get $i
    0x41, 0x01,     // i32.const 1
    0x6a,           // i32.add
    0x21, 0x04,     // local.set $i
    0x0c, 0x00,     // br 0 (continue loop)
    0x0b,           // end loop
    0x0b,           // end block
    0x0b            // end function
  ];
  
  const bytes = [
    ...magicNumber,
    ...version,
    ...typeSection,
    ...funcSection,
    ...memorySection,
    ...exportSection,
    ...codeSection
  ];
  
  return new Uint8Array(bytes).buffer;
}

export async function initColorMapWasm(): Promise<ColorMapWasm> {
  const wasmBytes = createMinimalWasmModule();
  
  const { instance } = await WebAssembly.instantiate(wasmBytes, {});
  const mem = (instance.exports.mem as WebAssembly.Memory);
  const map32 = instance.exports.map32 as ColorMapWasm["map32"];
  
  // Create views of the Wasm memory
  const heapU8  = new Uint8Array(mem.buffer);
  const heapU32 = new Uint32Array(mem.buffer);

  // Carve a simple arena: [ palette (256*4) | indices (maxRow) | outRow (maxRow*4) ]
  const MAX_COLORS = 256;
  const MAX_ROW_WIDTH = 4096; // Support rows up to 4096 pixels
  
  const palPtr = 0;
  const idxPtr = palPtr + MAX_COLORS * 4;
  const outPtr = idxPtr + MAX_ROW_WIDTH;

  return { 
    mem, 
    map32, 
    heapU8,
    heapU32,
    palPtr, 
    idxPtr, 
    outPtr,
    maxRowWidth: MAX_ROW_WIDTH
  };
}

// Fallback JavaScript implementation 
export class JSColorMapper {
  map32(indices: Uint8Array, output: Uint32Array, palette: Uint32Array, n: number): void {
    for (let i = 0; i < n; i++) {
      output[i] = palette[indices[i]];
    }
  }
}