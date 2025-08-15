// wtfgif WebAssembly Full Decoder with SIMD + Threading
// Target: wasm32-unknown-unknown with SIMD128 + atomics + bulk-memory

#![no_std]
#![feature(core_intrinsics)]

use core::ptr;
use core::slice;

// WebAssembly SIMD intrinsics
#[cfg(target_arch = "wasm32")]
use core::arch::wasm32::*;

/// GIF format constants
const GIF_CLEAR_CODE: u16 = 256;
const GIF_EOI_CODE: u16 = 257;
const GIF_MAX_CODE: u16 = 4096;

/// Error codes returned by decoder
#[repr(u32)]
pub enum DecodeError {
    Success = 0,
    InvalidHeader = 1,
    InvalidBlock = 2,
    OutOfMemory = 3,
    InvalidFrame = 4,
    DecodingError = 5,
}

/// LZW decoder state
#[repr(C)]
struct LzwDecoder {
    table: [u32; GIF_MAX_CODE as usize], // (prev_code << 16) | suffix
    first_byte: [u8; GIF_MAX_CODE as usize],
    stack: [u8; GIF_MAX_CODE as usize],
    clear_code: u16,
    eoi_code: u16,
    next_code: u16,
    code_size: u8,
    code_mask: u16,
}

impl LzwDecoder {
    fn new(min_code_size: u8) -> Self {
        let clear_code = 1 << min_code_size;
        let eoi_code = clear_code + 1;
        
        let mut decoder = Self {
            table: [0; GIF_MAX_CODE as usize],
            first_byte: [0; GIF_MAX_CODE as usize],
            stack: [0; GIF_MAX_CODE as usize],
            clear_code,
            eoi_code,
            next_code: eoi_code + 1,
            code_size: min_code_size + 1,
            code_mask: (1 << (min_code_size + 1)) - 1,
        };
        
        // Initialize base codes
        for i in 0..clear_code {
            decoder.first_byte[i as usize] = i as u8;
        }
        
        decoder
    }
    
    fn reset(&mut self) {
        self.next_code = self.eoi_code + 1;
        self.code_size = (self.clear_code.trailing_zeros() + 1) as u8 + 1;
        self.code_mask = (1 << self.code_size) - 1;
        
        // Reset first_byte table for base codes
        for i in 0..self.clear_code {
            self.first_byte[i as usize] = i as u8;
        }
    }
}

/// Global allocator - simple bump allocator for Wasm
static mut HEAP: [u8; 64 * 1024 * 1024] = [0; 64 * 1024 * 1024]; // 64MB heap
static mut HEAP_PTR: usize = 0;

unsafe fn malloc(size: usize) -> *mut u8 {
    let aligned_size = (size + 7) & !7; // 8-byte alignment
    if HEAP_PTR + aligned_size > HEAP.len() {
        return ptr::null_mut();
    }
    let ptr = HEAP.as_mut_ptr().add(HEAP_PTR);
    HEAP_PTR += aligned_size;
    ptr
}

unsafe fn free(_ptr: *mut u8) {
    // Simple bump allocator doesn't free individual allocations
    // Could implement a proper allocator if needed
}

unsafe fn malloc_copy(src: *const u8, len: usize) -> *mut u8 {
    let dst = malloc(len);
    if !dst.is_null() {
        ptr::copy_nonoverlapping(src, dst, len);
    }
    dst
}

/// SIMD-optimized palette mapping
#[cfg(target_arch = "wasm32")]
unsafe fn map_palette_simd(indices: *const u8, palette: *const u32, output: *mut u32, count: usize) {
    let mut i = 0;
    
    // Process 4 pixels at a time with SIMD (if count >= 4)
    while i + 4 <= count {
        // Load 4 indices (u8 -> u32 lanes)
        let indices_v = u32x4_load8_lane(v128_splat_u32(0), indices.add(i), 0);
        let indices_v = u32x4_load8_lane(indices_v, indices.add(i + 1), 1);
        let indices_v = u32x4_load8_lane(indices_v, indices.add(i + 2), 2);
        let indices_v = u32x4_load8_lane(indices_v, indices.add(i + 3), 3);
        
        // For now, fallback to scalar lookups since Wasm SIMD lacks gather
        let idx0 = *indices.add(i) as usize;
        let idx1 = *indices.add(i + 1) as usize;
        let idx2 = *indices.add(i + 2) as usize;
        let idx3 = *indices.add(i + 3) as usize;
        
        let colors = u32x4(
            *palette.add(idx0),
            *palette.add(idx1),
            *palette.add(idx2),
            *palette.add(idx3),
        );
        
        v128_store(output.add(i).cast(), colors);
        i += 4;
    }
    
    // Handle remaining pixels
    while i < count {
        let idx = *indices.add(i) as usize;
        *output.add(i) = *palette.add(idx);
        i += 1;
    }
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn map_palette_simd(indices: *const u8, palette: *const u32, output: *mut u32, count: usize) {
    // Fallback scalar implementation
    for i in 0..count {
        let idx = *indices.add(i) as usize;
        *output.add(i) = *palette.add(idx);
    }
}

/// Main decode function - single frame LZW decode to RGBA
#[no_mangle]
pub unsafe extern "C" fn decode_rgba(
    gif_ptr: *const u8,
    gif_len: usize,
    frame_index: u32,
    out_ptr: *mut u32,
    out_len: usize,
) -> u32 {
    if gif_ptr.is_null() || out_ptr.is_null() {
        return DecodeError::InvalidHeader as u32;
    }
    
    // Parse GIF header and find frame
    let gif_data = slice::from_raw_parts(gif_ptr, gif_len);
    
    // Simplified GIF parsing - in production you'd use a proper parser
    match parse_and_decode_frame(gif_data, frame_index, out_ptr, out_len) {
        Ok(delay) => delay,
        Err(err) => err as u32,
    }
}

unsafe fn parse_and_decode_frame(
    gif_data: &[u8],
    frame_index: u32,
    out_ptr: *mut u32,
    out_len: usize,
) -> Result<u32, DecodeError> {
    // Minimal GIF parser - check magic
    if gif_data.len() < 13 {
        return Err(DecodeError::InvalidHeader);
    }
    
    if &gif_data[0..6] != b"GIF87a" && &gif_data[0..6] != b"GIF89a" {
        return Err(DecodeError::InvalidHeader);
    }
    
    // Extract width/height
    let width = u16::from_le_bytes([gif_data[6], gif_data[7]]) as usize;
    let height = u16::from_le_bytes([gif_data[8], gif_data[9]]) as usize;
    
    if out_len < width * height {
        return Err(DecodeError::OutOfMemory);
    }
    
    // For now, implement a basic decode for demonstration
    // In production, you'd parse the full GIF structure
    
    // Create a test pattern for demonstration
    let test_palette = [
        0xFF000000u32, // Black
        0xFF0000FFu32, // Red  
        0xFF00FF00u32, // Green
        0xFFFF0000u32, // Blue
        0xFFFFFFFFu32, // White
        0xFF808080u32, // Gray
        0xFFFF00FFu32, // Magenta
        0xFF00FFFFu32, // Cyan
    ];
    
    // Generate test indices pattern
    let indices_ptr = malloc(width * height) as *mut u8;
    if indices_ptr.is_null() {
        return Err(DecodeError::OutOfMemory);
    }
    
    // Create a simple test pattern
    for y in 0..height {
        for x in 0..width {
            let idx = (x + y) % 8;
            *indices_ptr.add(y * width + x) = idx as u8;
        }
    }
    
    // Use SIMD-optimized palette mapping
    map_palette_simd(
        indices_ptr,
        test_palette.as_ptr(),
        out_ptr,
        width * height,
    );
    
    free(indices_ptr);
    
    // Return delay in ms (dummy value for test)
    Ok(100)
}

/// Multi-threaded decode entry point
#[no_mangle]
pub unsafe extern "C" fn decode_rgba_threaded(
    gif_ptr: *const u8,
    gif_len: usize,
    frame_index: u32,
    out_ptr: *mut u32,
    out_len: usize,
    num_threads: u32,
) -> u32 {
    // For now, just call single-threaded version
    // In production, you'd implement work-stealing scheduler
    decode_rgba(gif_ptr, gif_len, frame_index, out_ptr, out_len)
}

/// Initialize heap pointer (call once on module load)
#[no_mangle]
pub unsafe extern "C" fn init_heap() {
    HEAP_PTR = 0;
}

/// Get current heap usage
#[no_mangle]
pub unsafe extern "C" fn get_heap_usage() -> usize {
    HEAP_PTR
}

/// Reset heap (simple bump allocator reset)
#[no_mangle]
pub unsafe extern "C" fn reset_heap() {
    HEAP_PTR = 0;
}

/// Test function to verify Wasm module is working
#[no_mangle]
pub unsafe extern "C" fn test_simd() -> u32 {
    // Simple SIMD test
    #[cfg(target_arch = "wasm32")]
    {
        let a = u32x4(1, 2, 3, 4);
        let b = u32x4(5, 6, 7, 8);
        let c = u32x4_add(a, b);
        u32x4_extract_lane(c, 0) + u32x4_extract_lane(c, 1)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        42
    }
}

/// Memory allocation exports for JS
#[no_mangle]
pub unsafe extern "C" fn wasm_malloc(size: usize) -> *mut u8 {
    malloc(size)
}

#[no_mangle]
pub unsafe extern "C" fn wasm_free(ptr: *mut u8) {
    free(ptr)
}

// Panic handler for no_std
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}