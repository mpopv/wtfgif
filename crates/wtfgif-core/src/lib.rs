#![cfg_attr(feature = "quality-only", allow(dead_code))]

use wasm_bindgen::prelude::*;

/// Reject from a Wasm export with a JavaScript `Error` rather than a bare
/// string, so callers get `instanceof Error`, `message`, and a stack.
fn js_error(message: &str) -> JsValue {
    JsError::new(message).into()
}

#[cfg(all(
    feature = "quality-only",
    target_arch = "wasm32",
    not(target_feature = "atomics")
))]
struct WasmSingleThreadCell<T>(std::cell::UnsafeCell<T>);

#[cfg(all(
    feature = "quality-only",
    target_arch = "wasm32",
    not(target_feature = "atomics")
))]
struct WasmSingleThreadBorrowMut<'a, T>(&'a mut T);

#[cfg(all(
    feature = "quality-only",
    target_arch = "wasm32",
    not(target_feature = "atomics")
))]
impl<T> std::ops::Deref for WasmSingleThreadBorrowMut<'_, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

#[cfg(all(
    feature = "quality-only",
    target_arch = "wasm32",
    not(target_feature = "atomics")
))]
impl<T> std::ops::DerefMut for WasmSingleThreadBorrowMut<'_, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
    }
}

#[cfg(all(
    feature = "quality-only",
    target_arch = "wasm32",
    not(target_feature = "atomics")
))]
unsafe impl<T> Sync for WasmSingleThreadCell<T> {}

#[cfg(all(
    feature = "quality-only",
    target_arch = "wasm32",
    not(target_feature = "atomics")
))]
impl<T> WasmSingleThreadCell<T> {
    const fn new(value: T) -> Self {
        Self(std::cell::UnsafeCell::new(value))
    }

    #[inline(always)]
    fn with<R>(&self, callback: impl FnOnce(&Self) -> R) -> R {
        callback(self)
    }

    #[inline(always)]
    fn borrow(&self) -> &T {
        // The quality-only Wasm build has neither threads nor callbacks into
        // JavaScript while a scratch value is borrowed.
        unsafe { &*self.0.get() }
    }

    #[inline(always)]
    fn borrow_mut(&self) -> WasmSingleThreadBorrowMut<'_, T> {
        // See `borrow`: exports use each scratch cell synchronously and never
        // retain a reference across another access to the same cell.
        WasmSingleThreadBorrowMut(unsafe { &mut *self.0.get() })
    }

    #[inline(always)]
    fn replace(&self, value: T) -> T {
        std::mem::replace(&mut *self.borrow_mut(), value)
    }
}

#[cfg(all(
    feature = "quality-only",
    target_arch = "wasm32",
    not(target_feature = "atomics")
))]
macro_rules! reusable_cells {
    ($(static $name:ident: $value_type:ty = $value:expr;)+) => {
        $(static $name: WasmSingleThreadCell<$value_type> =
            WasmSingleThreadCell::new($value);)+
    };
}

#[cfg(not(all(
    feature = "quality-only",
    target_arch = "wasm32",
    not(target_feature = "atomics")
)))]
macro_rules! reusable_cells {
    ($(static $name:ident: $value_type:ty = $value:expr;)+) => {
        std::thread_local! {
            $(static $name: std::cell::RefCell<$value_type> =
                const { std::cell::RefCell::new($value) };)+
        }
    };
}

#[cfg(all(not(target_feature = "atomics"), target_family = "wasm"))]
const WASM_ALLOCATOR_ARENA_BYTES: usize = 1024 * 1024;

#[cfg(all(not(target_feature = "atomics"), target_family = "wasm"))]
static mut WASM_ALLOCATOR_ARENA: [std::mem::MaybeUninit<u8>; WASM_ALLOCATOR_ARENA_BYTES] =
    [std::mem::MaybeUninit::uninit(); WASM_ALLOCATOR_ARENA_BYTES];

#[cfg(all(not(target_feature = "atomics"), target_family = "wasm"))]
#[derive(Debug)]

struct WasmArenaThenGrow {
    arena_available: bool,
}

#[cfg(all(not(target_feature = "atomics"), target_family = "wasm"))]
unsafe impl talc::source::Source for WasmArenaThenGrow {
    fn acquire<B: talc::base::binning::Binning>(
        talc: &mut talc::base::Talc<Self, B>,
        layout: std::alloc::Layout,
    ) -> Result<(), ()> {
        if talc.source.arena_available {
            talc.source.arena_available = false;
            let arena = std::ptr::addr_of_mut!(WASM_ALLOCATOR_ARENA).cast::<u8>();
            if unsafe { talc.claim(arena, WASM_ALLOCATOR_ARENA_BYTES) }.is_some() {
                return Ok(());
            }
        }

        const WASM_PAGE_BYTES: usize = 64 * 1024;
        let required = layout
            .size()
            .saturating_add(layout.align())
            .saturating_add(talc::base::CHUNK_UNIT);
        let pages = required.div_ceil(WASM_PAGE_BYTES).max(1);
        let previous_pages = core::arch::wasm32::memory_grow::<0>(pages);
        if previous_pages == usize::MAX {
            return Err(());
        }
        let base = (previous_pages * WASM_PAGE_BYTES) as *mut u8;
        unsafe { talc.claim(base, pages * WASM_PAGE_BYTES) }
            .map(|_| ())
            .ok_or(())
    }
}

#[cfg(all(not(target_feature = "atomics"), target_family = "wasm"))]
struct WasmAllocator(talc::cell::TalcCell<WasmArenaThenGrow, talc::wasm::WasmBinning>);

#[cfg(all(not(target_feature = "atomics"), target_family = "wasm"))]
unsafe impl Sync for WasmAllocator {}

#[cfg(all(not(target_feature = "atomics"), target_family = "wasm"))]
impl WasmAllocator {
    const fn new() -> Self {
        Self(talc::cell::TalcCell::new(WasmArenaThenGrow {
            arena_available: true,
        }))
    }

    fn initialize(&self) {
        let source = self.0.replace_source(WasmArenaThenGrow {
            arena_available: false,
        });
        if !source.arena_available {
            return;
        }
        let arena = std::ptr::addr_of_mut!(WASM_ALLOCATOR_ARENA).cast::<u8>();
        if unsafe { self.0.claim(arena, WASM_ALLOCATOR_ARENA_BYTES) }.is_none() {
            self.0.replace_source(source);
        }
    }
}

#[cfg(all(not(target_feature = "atomics"), target_family = "wasm"))]
unsafe impl std::alloc::GlobalAlloc for WasmAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        std::alloc::GlobalAlloc::alloc(&self.0, layout)
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: std::alloc::Layout) {
        std::alloc::GlobalAlloc::dealloc(&self.0, pointer, layout);
    }

    unsafe fn realloc(
        &self,
        pointer: *mut u8,
        layout: std::alloc::Layout,
        new_size: usize,
    ) -> *mut u8 {
        std::alloc::GlobalAlloc::realloc(&self.0, pointer, layout, new_size)
    }
}

#[cfg(all(not(target_feature = "atomics"), target_family = "wasm"))]
#[global_allocator]
static TALC: WasmAllocator = WasmAllocator::new();

#[cfg(all(not(target_feature = "atomics"), target_family = "wasm"))]
#[wasm_bindgen(start)]
pub fn initialize_wasm_allocator() {
    TALC.initialize();
}

#[cfg(any(not(feature = "encode-only"), not(target_arch = "wasm32")))]
mod buffers;

#[cfg(not(target_arch = "wasm32"))]
mod native_ffi;

#[cfg(any(not(feature = "encode-only"), not(target_arch = "wasm32")))]
use buffers::{assume_initialized, uninitialized};

#[cfg(any(not(feature = "encode-only"), not(target_arch = "wasm32")))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GraphicControl {
    delay: u16,
    disposal: u8,
    transparent_index: Option<u8>,
}

#[cfg(any(not(feature = "encode-only"), not(target_arch = "wasm32")))]
#[derive(Debug, PartialEq, Eq)]
struct FrameMetadata {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    has_local_palette: bool,
    palette_offset: usize,
    palette_size: usize,
    data_offset: usize,
    data_length: usize,
    transparent_index: Option<u8>,
    interlaced: bool,
    delay: u16,
    disposal: u8,
    min_code_size: u8,
}

#[cfg(any(not(feature = "encode-only"), not(target_arch = "wasm32")))]
#[derive(Debug, PartialEq, Eq)]
struct GifMetadata {
    version: &'static str,
    width: u16,
    height: u16,
    global_palette_offset: Option<usize>,
    global_palette_size: usize,
    loop_count: Option<u16>,
    frames: Vec<FrameMetadata>,
}

#[cfg(any(not(feature = "encode-only"), not(target_arch = "wasm32")))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PixelFormat {
    Rgba,
    #[cfg(not(feature = "encode-only"))]
    Bgra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RgbaQuantization {
    Exact,
    Fast,
    Quality,
}

impl RgbaQuantization {
    fn from_u8(value: u8) -> Result<Self, String> {
        match value {
            0 => Ok(Self::Exact),
            1 => Ok(Self::Fast),
            2 => Ok(Self::Quality),
            _ => Err("Invalid RGBA quantization mode".to_string()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RgbaPaletteMode {
    Global,
    Local,
}

type IndexedPalette = (Vec<u32>, Vec<u8>, Option<u8>);

impl RgbaPaletteMode {
    fn from_u8(value: u8) -> Result<Self, String> {
        match value {
            0 => Ok(Self::Global),
            1 => Ok(Self::Local),
            _ => Err("Invalid RGBA palette mode".to_string()),
        }
    }
}

#[cfg(not(feature = "encode-only"))]
const COMPOSITED_DELTA_MAGIC: u32 = 0x3144_4757;

#[cfg(not(feature = "encode-only"))]
const COMPOSITED_DELTA_VERSION: u32 = 1;

#[cfg(not(feature = "encode-only"))]
const COMPOSITED_DELTA_HEADER_LEN: usize = 4;

#[cfg(not(feature = "encode-only"))]
const COMPOSITED_DELTA_ENTRY_LEN: usize = 9;

const COLOR_INDEX_CAP: usize = 512;

const TRANSPARENT_ALPHA_THRESHOLD: u8 = 128;

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
#[link(name = "System")]
extern "C" {
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut std::ffi::c_void;
    fn dispatch_apply_f(
        iterations: usize,
        queue: *mut std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void, usize),
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DelaySource<'a> {
    Constant(u16),
    PerFrame(&'a [u16]),
}

impl<'a> DelaySource<'a> {
    fn validate(self, frame_count: usize) -> Result<(), String> {
        match self {
            DelaySource::Constant(_) => Ok(()),
            DelaySource::PerFrame(delays) if delays.len() == frame_count => Ok(()),
            DelaySource::PerFrame(_) => Err("Delay count does not match frame count".to_string()),
        }
    }

    #[inline]
    fn get(self, frame_index: usize) -> u16 {
        match self {
            DelaySource::Constant(delay) => delay,
            DelaySource::PerFrame(delays) => delays[frame_index],
        }
    }
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub struct WtfGifCore {
    data: Vec<u8>,
    metadata: GifMetadata,
    decode_scratch: std::cell::RefCell<FrameDecodeScratch>,
}

#[cfg(not(feature = "encode-only"))]
#[derive(Default)]
struct FrameDecodeScratch {
    image_data: Vec<u8>,
    lzw: LzwStackScratch,
    indices: Vec<u8>,
    output: Vec<u8>,
    composited_output: Vec<u32>,
    palette: Vec<u32>,
    palette_offset: usize,
    palette_size: usize,
    palette_format: Option<PixelFormat>,
}

mod compact;

#[cfg(any(test, all(target_arch = "wasm32", target_feature = "simd128")))]
mod sorted_palette;

mod wasm_api;

#[cfg(feature = "fuzzing")]
mod fuzzing;

#[cfg(feature = "fuzzing")]
pub use fuzzing::{fuzz_decode, fuzz_encode};

pub use wasm_api::*;

// Codec modules. Each starts with `use super::*;` and is glob-imported here,
// so items keep crate-wide names across modules. Decoder modules drop out of
// encode-only Wasm builds, and the threaded decoder out of every Wasm build.
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
mod blit;
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
use blit::*;
mod changed_rect;
use changed_rect::*;
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
mod composite;
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
use composite::*;
#[cfg(not(feature = "encode-only"))]
mod core_api;
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
mod decode;
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
use decode::*;
mod encode_rgba;
use encode_rgba::*;
mod histogram;
use histogram::*;
mod indexed_encode;
use indexed_encode::*;
mod kd_tree;
use kd_tree::*;
mod literal_lzw;
use literal_lzw::*;
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
mod lzw_decode;
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
use lzw_decode::*;
mod median_cut;
use median_cut::*;
#[cfg(not(target_arch = "wasm32"))]
mod native_decode;
#[cfg(not(target_arch = "wasm32"))]
use native_decode::*;
mod palette;
use palette::*;
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
mod parse;
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
use parse::*;
mod quality;
use quality::*;
mod quality_map;
use quality_map::*;
mod quality_plan;
use quality_plan::*;
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
mod reencode;
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
use reencode::*;
mod wu;
use wu::*;

// Scratch buffers reused across encodes and shared by several modules.
reusable_cells! {
    static REUSABLE_QUANTIZED_COLOR_BOXES: Vec<QuantizedColorArenaBox> = Vec::new();
}

reusable_cells! {
    static REUSABLE_PALETTE_KD_NODES: Vec<PaletteKdNode> = Vec::new();
}

reusable_cells! {
    static REUSABLE_LZW_SCRATCH: LzwEncodeScratch = LzwEncodeScratch {
            input: Vec::new(),
            output: Vec::new(),
        };
    static REUSABLE_GIF_OUTPUT: Vec<u8> = Vec::new();
    static REUSABLE_QUALITY_HISTOGRAM_U32: Vec<RgbHistogramBin32> = Vec::new();
    static REUSABLE_QUALITY_HISTOGRAM_U32_CLEAN: bool = true;
    static REUSABLE_QUALITY_HISTOGRAM_U64: Vec<RgbHistogramBin> = Vec::new();
    static REUSABLE_QUALITY_HISTOGRAM_U64_CLEAN: bool = true;
    static REUSABLE_QUALITY_HISTOGRAM_TO_PALETTE: Vec<u8> = Vec::new();
    static REUSABLE_QUALITY_COLORS: Vec<QuantizedColor> = Vec::new();
    static REUSABLE_QUALITY_PALETTE: Vec<u32> = Vec::new();
    static REUSABLE_QUALITY_COLOR_INDEX: ColorIndexTable = ColorIndexTable::empty();
    static REUSABLE_QUANTIZED_BYTES: Vec<u8> = Vec::new();
}

#[cfg(all(test, not(feature = "encode-only")))]
mod tests;

#[cfg(all(test, feature = "encode-only"))]
mod encode_only_tests;
