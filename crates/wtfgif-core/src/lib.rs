#![cfg_attr(feature = "encode-only", allow(dead_code))]

use wasm_bindgen::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GraphicControl {
    delay: u16,
    disposal: u8,
    transparent_index: Option<u8>,
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PixelFormat {
    Rgba,
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

impl RgbaPaletteMode {
    fn from_u8(value: u8) -> Result<Self, String> {
        match value {
            0 => Ok(Self::Global),
            1 => Ok(Self::Local),
            _ => Err("Invalid RGBA palette mode".to_string()),
        }
    }
}

const COMPOSITED_DELTA_MAGIC: u32 = 0x3144_4757;
const COMPOSITED_DELTA_VERSION: u32 = 1;
const COMPOSITED_DELTA_HEADER_LEN: usize = 4;
const COMPOSITED_DELTA_ENTRY_LEN: usize = 9;
const LZW_DIRECT_ENTRY_COUNT: usize = 1 << 20;
const LZW_ENTRY_CODE_MASK: u32 = 0x0fff;
const LZW_ENTRY_EPOCH_SHIFT: u32 = 12;
const LZW_ENTRY_EPOCH_MAX: u32 = (1 << (32 - LZW_ENTRY_EPOCH_SHIFT)) - 1;
const COLOR_INDEX_CAP: usize = 512;
const TRANSPARENT_ALPHA_THRESHOLD: u8 = 128;

#[cfg(not(target_arch = "wasm32"))]
#[repr(C)]
pub struct NativeDecodedGif {
    pixels: *mut u8,
    byte_len: usize,
    width: u32,
    height: u32,
    frame_count: u32,
    parse_nanos: u64,
    decode_nanos: u64,
    compose_nanos: u64,
    host_owned: i32,
}

#[cfg(not(target_arch = "wasm32"))]
type NativeRgbaAllocator =
    unsafe extern "C" fn(context: *mut std::ffi::c_void, byte_len: usize) -> *mut u8;

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

#[cfg(not(target_arch = "wasm32"))]
#[repr(C)]
pub struct NativeEncodedGif {
    bytes: *mut u8,
    byte_len: usize,
    byte_capacity: usize,
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub unsafe extern "C" fn wtfgif_decode_all_rgba(
    data: *const u8,
    data_len: usize,
    decoded: *mut NativeDecodedGif,
) -> i32 {
    wtfgif_decode_all_rgba_inner(data, data_len, None, std::ptr::null_mut(), decoded)
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub unsafe extern "C" fn wtfgif_decode_all_rgba_host(
    data: *const u8,
    data_len: usize,
    allocate: NativeRgbaAllocator,
    allocate_context: *mut std::ffi::c_void,
    decoded: *mut NativeDecodedGif,
) -> i32 {
    wtfgif_decode_all_rgba_inner(data, data_len, Some(allocate), allocate_context, decoded)
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn wtfgif_decode_all_rgba_inner(
    data: *const u8,
    data_len: usize,
    allocate: Option<NativeRgbaAllocator>,
    allocate_context: *mut std::ffi::c_void,
    decoded: *mut NativeDecodedGif,
) -> i32 {
    if data.is_null() || decoded.is_null() {
        return 0;
    }
    let data = std::slice::from_raw_parts(data, data_len);
    let Ok(metadata) = parse_metadata(data) else {
        return 0;
    };
    let canvas_pixels = usize::from(metadata.width).checked_mul(usize::from(metadata.height));
    let output_pixels = canvas_pixels.and_then(|pixels| pixels.checked_mul(metadata.frames.len()));
    if let (Some(allocate), Some(canvas_pixels), Some(output_pixels)) =
        (allocate, canvas_pixels, output_pixels)
    {
        if output_pixels < 50_000 && frames_are_independent_native(&metadata) {
            let byte_len = match output_pixels.checked_mul(std::mem::size_of::<u32>()) {
                Some(byte_len) => byte_len,
                None => return 0,
            };
            let pixels_ptr = allocate(allocate_context, byte_len);
            if pixels_ptr.is_null() {
                return 0;
            }
            let output = std::slice::from_raw_parts_mut(
                pixels_ptr.cast::<std::mem::MaybeUninit<u32>>(),
                output_pixels,
            );
            if decode_small_independent_frames_into_native(data, &metadata, canvas_pixels, output)
                .is_err()
            {
                return 0;
            }
            decoded.write(NativeDecodedGif {
                pixels: pixels_ptr,
                byte_len,
                width: u32::from(metadata.width),
                height: u32::from(metadata.height),
                frame_count: metadata.frames.len() as u32,
                parse_nanos: 0,
                decode_nanos: 0,
                compose_nanos: 0,
                host_owned: 1,
            });
            return 1;
        }
        if let Some(segments) = independent_frame_segments_native(&metadata) {
            let byte_len = match output_pixels.checked_mul(std::mem::size_of::<u32>()) {
                Some(byte_len) => byte_len,
                None => return 0,
            };
            let pixels_ptr = allocate(allocate_context, byte_len);
            if pixels_ptr.is_null() {
                return 0;
            }
            let output = std::slice::from_raw_parts_mut(
                pixels_ptr.cast::<std::mem::MaybeUninit<u32>>(),
                output_pixels,
            );
            if decode_segmented_frames_into_native(data, &metadata, &segments, output).is_err() {
                return 0;
            }
            decoded.write(NativeDecodedGif {
                pixels: pixels_ptr,
                byte_len,
                width: u32::from(metadata.width),
                height: u32::from(metadata.height),
                frame_count: metadata.frames.len() as u32,
                parse_nanos: 0,
                decode_nanos: 0,
                compose_nanos: 0,
                host_owned: 1,
            });
            return 1;
        }
        let mixed_pipeline_output = output_pixels >= 2_000_000
            && metadata
                .frames
                .iter()
                .any(|frame| frame.disposal > 1 || !frame_covers_canvas(frame, &metadata));
        if mixed_pipeline_output && should_pipeline_decode_native(&metadata) {
            let byte_len = match output_pixels.checked_mul(std::mem::size_of::<u32>()) {
                Some(byte_len) => byte_len,
                None => return 0,
            };
            let pixels_ptr = allocate(allocate_context, byte_len);
            if pixels_ptr.is_null() {
                return 0;
            }
            let output = std::slice::from_raw_parts_mut(
                pixels_ptr.cast::<std::mem::MaybeUninit<u32>>(),
                output_pixels,
            );
            if decode_and_compose_pipeline_into_native(data, &metadata, output).is_err() {
                return 0;
            }
            decoded.write(NativeDecodedGif {
                pixels: pixels_ptr,
                byte_len,
                width: u32::from(metadata.width),
                height: u32::from(metadata.height),
                frame_count: metadata.frames.len() as u32,
                parse_nanos: 0,
                decode_nanos: 0,
                compose_nanos: 0,
                host_owned: 1,
            });
            return 1;
        }
    }

    let result = (|| {
        let pixels = if frames_are_independent_native(&metadata) {
            decode_independent_frames_native(data, &metadata)?
        } else if let Some(segments) = independent_frame_segments_native(&metadata) {
            decode_segmented_frames_native(data, &metadata, &segments)?
        } else if should_fuse_sequential_decode_native(&metadata) {
            prepare_all_composited_frames_inner(data, &metadata, PixelFormat::Rgba)?
        } else if should_pipeline_decode_native(&metadata) {
            decode_and_compose_pipeline_native(data, &metadata)?
        } else {
            let decoded_indices = decode_frame_indices_parallel_native(data, &metadata)?;
            compose_decoded_frames_native(data, &metadata, &decoded_indices)?
        };
        Ok::<_, String>(pixels)
    })();
    let Ok(pixels) = result else {
        return 0;
    };
    let mut pixels = pixels.into_boxed_slice();
    let byte_len = pixels.len() * std::mem::size_of::<u32>();
    let pixels_ptr = pixels.as_mut_ptr().cast::<u8>();
    std::mem::forget(pixels);
    decoded.write(NativeDecodedGif {
        pixels: pixels_ptr,
        byte_len,
        width: u32::from(metadata.width),
        height: u32::from(metadata.height),
        frame_count: metadata.frames.len() as u32,
        parse_nanos: 0,
        decode_nanos: 0,
        compose_nanos: 0,
        host_owned: 0,
    });
    1
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub unsafe extern "C" fn wtfgif_free_rgba(pixels: *mut u8, byte_len: usize) {
    if pixels.is_null() || byte_len == 0 || byte_len % std::mem::size_of::<u32>() != 0 {
        return;
    }
    let pixel_len = byte_len / std::mem::size_of::<u32>();
    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
        pixels.cast::<u32>(),
        pixel_len,
    )));
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub unsafe extern "C" fn wtfgif_encode_rgba_fast(
    rgba_stream: *const u8,
    rgba_len: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: *const u32,
    palette_len: usize,
    delays: *const u16,
    delay_count: usize,
    loop_count: i32,
    deltas: i32,
    encoded: *mut NativeEncodedGif,
) -> i32 {
    if rgba_stream.is_null()
        || encoded.is_null()
        || (palette_len != 0 && palette_rgb.is_null())
        || (delay_count != 0 && delays.is_null())
    {
        return 0;
    }

    let rgba_stream = std::slice::from_raw_parts(rgba_stream, rgba_len);
    let palette_rgb = if palette_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(palette_rgb, palette_len)
    };
    let delays = if delay_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(delays, delay_count)
    };
    let result = encode_rgba_gif_inner(
        rgba_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::PerFrame(delays),
        loop_count,
        deltas != 0,
        TRANSPARENT_ALPHA_THRESHOLD,
        true,
    );
    let Ok(bytes) = result else {
        return 0;
    };

    let mut bytes = bytes;
    let byte_len = bytes.len();
    let byte_capacity = bytes.capacity();
    let bytes_ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    encoded.write(NativeEncodedGif {
        bytes: bytes_ptr,
        byte_len,
        byte_capacity,
    });
    1
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub unsafe extern "C" fn wtfgif_encode_indexed_fast(
    index_stream: *const u8,
    index_len: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: *const u32,
    palette_len: usize,
    delays: *const u16,
    delay_count: usize,
    loop_count: i32,
    deltas: i32,
    encoded: *mut NativeEncodedGif,
) -> i32 {
    if index_stream.is_null() || encoded.is_null() || palette_rgb.is_null() || delays.is_null() {
        return 0;
    }
    let index_stream = std::slice::from_raw_parts(index_stream, index_len);
    let palette_rgb = std::slice::from_raw_parts(palette_rgb, palette_len);
    let delays = std::slice::from_raw_parts(delays, delay_count);
    let delay_source = DelaySource::PerFrame(delays);
    let result = if deltas != 0 {
        encode_indexed_literal_delta_gif_inner(
            index_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delay_source,
            loop_count,
        )
    } else {
        encode_indexed_literal_gif_inner(
            index_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delay_source,
            loop_count,
            None,
        )
    };
    let Ok(mut bytes) = result else {
        return 0;
    };
    let byte_len = bytes.len();
    let byte_capacity = bytes.capacity();
    let bytes_ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    encoded.write(NativeEncodedGif {
        bytes: bytes_ptr,
        byte_len,
        byte_capacity,
    });
    1
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub unsafe extern "C" fn wtfgif_reencode_gif_fast(
    data: *const u8,
    data_len: usize,
    encoded: *mut NativeEncodedGif,
) -> i32 {
    if data.is_null() || encoded.is_null() {
        return 0;
    }
    let data = std::slice::from_raw_parts(data, data_len);
    let result = (|| {
        let metadata = parse_metadata(data)?;
        let loop_count = metadata.loop_count.map(i32::from).unwrap_or(-1);
        reencode_gif_literal_parallel_native(data, &metadata, loop_count)
    })();
    let Ok(bytes) = result else {
        return 0;
    };

    let mut bytes = bytes;
    let byte_len = bytes.len();
    let byte_capacity = bytes.capacity();
    let bytes_ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    encoded.write(NativeEncodedGif {
        bytes: bytes_ptr,
        byte_len,
        byte_capacity,
    });
    1
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub unsafe extern "C" fn wtfgif_reencode_gif_fast_host(
    data: *const u8,
    data_len: usize,
    allocate: NativeRgbaAllocator,
    allocate_context: *mut std::ffi::c_void,
    encoded: *mut NativeEncodedGif,
) -> i32 {
    if data.is_null() || encoded.is_null() {
        return 0;
    }
    let data_slice = std::slice::from_raw_parts(data, data_len);
    match try_reencode_small_gif_into_host(data_slice, allocate, allocate_context) {
        Ok(Some((bytes, byte_len))) => {
            encoded.write(NativeEncodedGif {
                bytes,
                byte_len,
                byte_capacity: 0,
            });
            2
        }
        Ok(None) => {
            let Ok(metadata) = parse_metadata(data_slice) else {
                return 0;
            };
            let loop_count = metadata.loop_count.map(i32::from).unwrap_or(-1);
            match try_reencode_parallel_gif_into_host(
                data_slice,
                &metadata,
                loop_count,
                allocate,
                allocate_context,
            ) {
                Ok(Some((bytes, byte_len))) => {
                    encoded.write(NativeEncodedGif {
                        bytes,
                        byte_len,
                        byte_capacity: 0,
                    });
                    2
                }
                Ok(None) => {
                    let Ok(mut bytes) =
                        reencode_gif_literal_parallel_native(data_slice, &metadata, loop_count)
                    else {
                        return 0;
                    };
                    let byte_len = bytes.len();
                    let byte_capacity = bytes.capacity();
                    let bytes_pointer = bytes.as_mut_ptr();
                    std::mem::forget(bytes);
                    encoded.write(NativeEncodedGif {
                        bytes: bytes_pointer,
                        byte_len,
                        byte_capacity,
                    });
                    1
                }
                Err(_) => 0,
            }
        }
        Err(_) => 0,
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub unsafe extern "C" fn wtfgif_free_bytes(bytes: *mut u8, byte_len: usize, byte_capacity: usize) {
    if bytes.is_null() {
        return;
    }
    drop(Vec::from_raw_parts(bytes, byte_len, byte_capacity));
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

#[derive(Default)]
struct FrameDecodeScratch {
    image_data: Vec<u8>,
    lzw: LzwStackScratch,
    indices: Vec<u8>,
    palette: Vec<u32>,
    palette_offset: usize,
    palette_size: usize,
    palette_format: Option<PixelFormat>,
}

#[wasm_bindgen]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn parse_metadata_json(data: &[u8]) -> Result<String, JsValue> {
    parse_metadata(data)
        .map(|metadata| metadata.to_json())
        .map_err(|message| JsValue::from_str(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn decode_frame_indices(data: &[u8], frame_index: usize) -> Result<Vec<u8>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    let frame = metadata
        .frames
        .get(frame_index)
        .ok_or_else(|| JsValue::from_str("Frame index out of range"))?;
    decode_frame_indices_inner(data, frame).map_err(|message| JsValue::from_str(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn decode_frame_rgba(data: &[u8], frame_index: usize) -> Result<Vec<u8>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    decode_frame_pixels_inner(data, &metadata, frame_index, PixelFormat::Rgba)
        .map_err(|message| JsValue::from_str(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn decode_frame_bgra(data: &[u8], frame_index: usize) -> Result<Vec<u8>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    decode_frame_pixels_inner(data, &metadata, frame_index, PixelFormat::Bgra)
        .map_err(|message| JsValue::from_str(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn decode_all_rgba(data: &[u8]) -> Result<Vec<u32>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    prepare_all_composited_frames_inner(data, &metadata, PixelFormat::Rgba)
        .map_err(|message| JsValue::from_str(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn reencode_gif_pixel_perfect(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    let loop_count = metadata.loop_count.map(i32::from).unwrap_or(-1);
    let total_frame_pixels = metadata.frames.iter().try_fold(0usize, |total, frame| {
        usize::from(frame.width)
            .checked_mul(usize::from(frame.height))
            .and_then(|pixels| total.checked_add(pixels))
            .ok_or_else(|| "Decoded frame size overflow".to_string())
    });
    let total_frame_pixels = total_frame_pixels.map_err(|message| JsValue::from_str(&message))?;
    reencode_gif_literal_sequential(data, &metadata, loop_count, total_frame_pixels)
        .map_err(|message| JsValue::from_str(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn remux_gif_pixel_perfect(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    if data.len() <= 4_096 {
        validate_gif_structure_no_alloc(data).map_err(|message| JsValue::from_str(&message))?;
        return Ok(data.to_vec());
    }
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    let loop_count = metadata.loop_count.map(i32::from).unwrap_or(-1);
    remux_gif_pixel_perfect_inner(data, &metadata, loop_count)
        .map_err(|message| JsValue::from_str(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn prepare_composited_rgba(data: &[u8], requested_frames: &[u8]) -> Result<Vec<u32>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    prepare_composited_frames_inner(data, &metadata, requested_frames, PixelFormat::Rgba)
        .map_err(|message| JsValue::from_str(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn prepare_composited_bgra(data: &[u8], requested_frames: &[u8]) -> Result<Vec<u32>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    prepare_composited_frames_inner(data, &metadata, requested_frames, PixelFormat::Bgra)
        .map_err(|message| JsValue::from_str(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn prepare_composited_delta_rgba(
    data: &[u8],
    requested_frames: &[u8],
) -> Result<Vec<u32>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    prepare_composited_delta_frames_inner(data, &metadata, requested_frames, PixelFormat::Rgba)
        .map_err(|message| JsValue::from_str(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn prepare_composited_delta_bgra(
    data: &[u8],
    requested_frames: &[u8],
) -> Result<Vec<u32>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    prepare_composited_delta_frames_inner(data, &metadata, requested_frames, PixelFormat::Bgra)
        .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_indexed_lzw(
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<Vec<u8>, JsValue> {
    encode_indexed_lzw_inner(index_stream, min_code_size, color_count)
        .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_indexed_lzw_scratch(
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<usize, JsValue> {
    encode_indexed_lzw_scratch_inner(index_stream, min_code_size, color_count, false)
        .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_indexed_literal_lzw_scratch(
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<usize, JsValue> {
    encode_indexed_lzw_scratch_inner(index_stream, min_code_size, color_count, true)
        .map_err(|message| JsValue::from_str(&message))
}

/// Reserves raw input scratch storage. Callers must overwrite the complete
/// requested range before invoking an encoder that reads it.
#[wasm_bindgen]
pub fn indexed_lzw_input_scratch_reserve(length: usize) -> usize {
    REUSABLE_LZW_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let units = length.div_ceil(std::mem::size_of::<AlignedByte>());
        if units > scratch.input.len() {
            let additional = units - scratch.input.len();
            scratch.input.reserve(additional);
            // All current callers immediately overwrite the entire exposed
            // range before asking the encoder to read it.
            unsafe { scratch.input.set_len(units) };
        }
        scratch.input.as_mut_ptr().cast::<u8>() as usize
    })
}

#[wasm_bindgen]
pub fn encode_indexed_lzw_scratch_from_input(
    length: usize,
    min_code_size: u8,
    color_count: usize,
    literal: bool,
) -> Result<usize, JsValue> {
    encode_indexed_lzw_scratch_from_input_inner(length, min_code_size, color_count, literal)
        .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn indexed_lzw_scratch_ptr() -> usize {
    REUSABLE_LZW_SCRATCH.with(|scratch| scratch.borrow().output.as_ptr() as usize)
}

#[wasm_bindgen]
pub fn wasm_memory() -> JsValue {
    wasm_bindgen::memory()
}

#[wasm_bindgen]
pub fn encode_indexed_gif(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delay: u16,
    loop_count: i32,
) -> Result<Vec<u8>, JsValue> {
    encode_indexed_gif_inner(
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::Constant(delay),
        loop_count,
        None,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_indexed_literal_gif(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delay: u16,
    loop_count: i32,
) -> Result<Vec<u8>, JsValue> {
    encode_indexed_literal_gif_inner(
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::Constant(delay),
        loop_count,
        None,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_indexed_literal_gif_with_delays(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: &[u16],
    loop_count: i32,
) -> Result<Vec<u8>, JsValue> {
    encode_indexed_literal_gif_inner(
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::PerFrame(delays),
        loop_count,
        None,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_indexed_gif_with_delays(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: &[u16],
    loop_count: i32,
) -> Result<Vec<u8>, JsValue> {
    encode_indexed_gif_inner(
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::PerFrame(delays),
        loop_count,
        None,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_indexed_delta_gif(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delay: u16,
    loop_count: i32,
) -> Result<Vec<u8>, JsValue> {
    encode_indexed_delta_gif_inner(
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::Constant(delay),
        loop_count,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_indexed_delta_gif_with_delays(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: &[u16],
    loop_count: i32,
) -> Result<Vec<u8>, JsValue> {
    encode_indexed_delta_gif_inner(
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::PerFrame(delays),
        loop_count,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_indexed_literal_delta_gif(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delay: u16,
    loop_count: i32,
) -> Result<Vec<u8>, JsValue> {
    encode_indexed_literal_delta_gif_inner(
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::Constant(delay),
        loop_count,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_indexed_literal_delta_gif_with_delays(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: &[u16],
    loop_count: i32,
) -> Result<Vec<u8>, JsValue> {
    encode_indexed_literal_delta_gif_inner(
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::PerFrame(delays),
        loop_count,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_rgba_gif(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delay: u16,
    loop_count: i32,
    deltas: bool,
) -> Result<Vec<u8>, JsValue> {
    encode_rgba_gif_inner(
        rgba_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::Constant(delay),
        loop_count,
        deltas,
        TRANSPARENT_ALPHA_THRESHOLD,
        false,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_rgba_literal_gif(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delay: u16,
    loop_count: i32,
) -> Result<Vec<u8>, JsValue> {
    encode_rgba_gif_inner(
        rgba_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::Constant(delay),
        loop_count,
        false,
        TRANSPARENT_ALPHA_THRESHOLD,
        true,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_rgba_literal_gif_with_options(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: &[u16],
    loop_count: i32,
    alpha_threshold: u8,
) -> Result<Vec<u8>, JsValue> {
    encode_rgba_gif_inner(
        rgba_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::PerFrame(delays),
        loop_count,
        false,
        alpha_threshold,
        true,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_rgba_literal_delta_gif(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delay: u16,
    loop_count: i32,
) -> Result<Vec<u8>, JsValue> {
    encode_rgba_gif_inner(
        rgba_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::Constant(delay),
        loop_count,
        true,
        TRANSPARENT_ALPHA_THRESHOLD,
        true,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_rgba_literal_delta_gif_with_options(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: &[u16],
    loop_count: i32,
    alpha_threshold: u8,
) -> Result<Vec<u8>, JsValue> {
    encode_rgba_gif_inner(
        rgba_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::PerFrame(delays),
        loop_count,
        true,
        alpha_threshold,
        true,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_rgba_gif_with_options(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: &[u16],
    loop_count: i32,
    deltas: bool,
    alpha_threshold: u8,
) -> Result<Vec<u8>, JsValue> {
    encode_rgba_gif_inner(
        rgba_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::PerFrame(delays),
        loop_count,
        deltas,
        alpha_threshold,
        false,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_rgba_gif_advanced(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: &[u16],
    loop_count: i32,
    deltas: bool,
    alpha_threshold: u8,
    literal: bool,
    quantization: u8,
    palette_mode: u8,
) -> Result<Vec<u8>, JsValue> {
    let quantization =
        RgbaQuantization::from_u8(quantization).map_err(|message| JsValue::from_str(&message))?;
    let palette_mode =
        RgbaPaletteMode::from_u8(palette_mode).map_err(|message| JsValue::from_str(&message))?;
    encode_rgba_gif_advanced_inner(
        rgba_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::PerFrame(delays),
        loop_count,
        deltas,
        alpha_threshold,
        literal,
        quantization,
        palette_mode,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn encode_rgba_gif_advanced_from_input(
    length: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: &[u16],
    loop_count: i32,
    deltas: bool,
    alpha_threshold: u8,
    literal: bool,
    quantization: u8,
    palette_mode: u8,
) -> Result<Vec<u8>, JsValue> {
    let quantization =
        RgbaQuantization::from_u8(quantization).map_err(|message| JsValue::from_str(&message))?;
    let palette_mode =
        RgbaPaletteMode::from_u8(palette_mode).map_err(|message| JsValue::from_str(&message))?;
    REUSABLE_LZW_SCRATCH.with(|scratch| {
        // Release the RefCell borrow before encoding: the encoder reuses the
        // same scratch object for its indexed/LZW output buffers. The input
        // Vec is not resized while this call runs, so its pointer remains
        // stable for the duration of the encode.
        let input_ptr = {
            let scratch = scratch.borrow();
            if length > scratch.input.len() * std::mem::size_of::<AlignedByte>() {
                return Err(JsValue::from_str("RGBA input scratch buffer is too short"));
            }
            scratch.input.as_ptr().cast::<u8>()
        };
        let rgba_stream = unsafe { std::slice::from_raw_parts(input_ptr, length) };
        encode_rgba_gif_advanced_inner(
            rgba_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            DelaySource::PerFrame(delays),
            loop_count,
            deltas,
            alpha_threshold,
            literal,
            quantization,
            palette_mode,
        )
        .map_err(|message| JsValue::from_str(&message))
    })
}

/// Encodes from the reusable input buffer and retains the GIF output in Wasm
/// memory. JavaScript callers copy the returned range before the next encode,
/// which lets repeated encodes reuse the output allocation too.
#[wasm_bindgen]
pub fn encode_rgba_gif_advanced_scratch_from_input(
    length: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: &[u16],
    loop_count: i32,
    deltas: bool,
    alpha_threshold: u8,
    literal: bool,
    quantization: u8,
    palette_mode: u8,
) -> Result<usize, JsValue> {
    let quantization =
        RgbaQuantization::from_u8(quantization).map_err(|message| JsValue::from_str(&message))?;
    let palette_mode =
        RgbaPaletteMode::from_u8(palette_mode).map_err(|message| JsValue::from_str(&message))?;
    let input_ptr = REUSABLE_LZW_SCRATCH.with(|scratch| {
        let scratch = scratch.borrow();
        if length > scratch.input.len() * std::mem::size_of::<AlignedByte>() {
            return Err(JsValue::from_str("RGBA input scratch buffer is too short"));
        }
        Ok(scratch.input.as_ptr().cast::<u8>())
    })?;
    let rgba_stream = unsafe { std::slice::from_raw_parts(input_ptr, length) };
    let output = REUSABLE_GIF_OUTPUT.with(|scratch| std::mem::take(&mut *scratch.borrow_mut()));
    let encoded = encode_rgba_gif_advanced_inner_with_output(
        rgba_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::PerFrame(delays),
        loop_count,
        deltas,
        alpha_threshold,
        literal,
        quantization,
        palette_mode,
        output,
    )
    .map_err(|message| JsValue::from_str(&message))?;
    let length = encoded.len();
    REUSABLE_GIF_OUTPUT.with(|scratch| {
        *scratch.borrow_mut() = encoded;
    });
    Ok(length)
}

/// Quality/global/literal RGBA encoding with the mode decisions removed from
/// the hot call graph. The public TypeScript API selects this only for the
/// normal arbitrary-image contract; other combinations use the generic
/// advanced entry above.
#[wasm_bindgen]
pub fn encode_rgba_quality_gif_from_input(
    length: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    delays: &[u16],
    loop_count: i32,
    alpha_threshold: u8,
) -> Result<Vec<u8>, JsValue> {
    let input_ptr = REUSABLE_LZW_SCRATCH.with(|scratch| {
        let scratch = scratch.borrow();
        if length > scratch.input.len() * std::mem::size_of::<AlignedByte>() {
            return Err(JsValue::from_str("RGBA input scratch buffer is too short"));
        }
        Ok(scratch.input.as_ptr().cast::<u8>())
    })?;
    let rgba_stream = unsafe { std::slice::from_raw_parts(input_ptr, length) };
    encode_rgba_quality_gif_inner_with_output(
        rgba_stream,
        width,
        height,
        frame_count,
        delays,
        loop_count,
        alpha_threshold,
        Vec::new(),
    )
    .map_err(|message| JsValue::from_str(&message))
}

/// Scratch-output form of the specialized quality encoder. The returned
/// length refers to `gif_output_scratch_ptr()` in Wasm memory.
#[wasm_bindgen]
pub fn encode_rgba_quality_gif_scratch_from_input(
    length: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    delays: &[u16],
    loop_count: i32,
    alpha_threshold: u8,
) -> Result<usize, JsValue> {
    let input_ptr = REUSABLE_LZW_SCRATCH.with(|scratch| {
        let scratch = scratch.borrow();
        if length > scratch.input.len() * std::mem::size_of::<AlignedByte>() {
            return Err(JsValue::from_str("RGBA input scratch buffer is too short"));
        }
        Ok(scratch.input.as_ptr().cast::<u8>())
    })?;
    let rgba_stream = unsafe { std::slice::from_raw_parts(input_ptr, length) };
    let output = REUSABLE_GIF_OUTPUT.with(|scratch| std::mem::take(&mut *scratch.borrow_mut()));
    let encoded = encode_rgba_quality_gif_inner_with_output(
        rgba_stream,
        width,
        height,
        frame_count,
        delays,
        loop_count,
        alpha_threshold,
        output,
    )
    .map_err(|message| JsValue::from_str(&message))?;
    let length = encoded.len();
    REUSABLE_GIF_OUTPUT.with(|scratch| {
        *scratch.borrow_mut() = encoded;
    });
    Ok(length)
}

#[wasm_bindgen]
pub fn gif_output_scratch_ptr() -> usize {
    REUSABLE_GIF_OUTPUT.with(|scratch| scratch.borrow().as_ptr() as usize)
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
impl WtfGifCore {
    #[wasm_bindgen(constructor)]
    pub fn new(data: &[u8]) -> Result<WtfGifCore, JsValue> {
        let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
        Ok(WtfGifCore {
            data: data.to_vec(),
            metadata,
            decode_scratch: std::cell::RefCell::new(FrameDecodeScratch::default()),
        })
    }

    pub fn width(&self) -> u16 {
        self.metadata.width
    }

    pub fn height(&self) -> u16 {
        self.metadata.height
    }

    pub fn frame_count(&self) -> usize {
        self.metadata.frames.len()
    }

    pub fn metadata_json(&self) -> String {
        self.metadata.to_json()
    }

    pub fn decode_frame_indices(&self, frame_index: usize) -> Result<Vec<u8>, JsValue> {
        let frame = self
            .metadata
            .frames
            .get(frame_index)
            .ok_or_else(|| JsValue::from_str("Frame index out of range"))?;
        decode_frame_indices_inner(&self.data, frame).map_err(|message| JsValue::from_str(&message))
    }

    pub fn decode_frame_rgba(&self, frame_index: usize) -> Result<Vec<u8>, JsValue> {
        decode_frame_pixels_inner(&self.data, &self.metadata, frame_index, PixelFormat::Rgba)
            .map_err(|message| JsValue::from_str(&message))
    }

    pub fn decode_frame_bgra(&self, frame_index: usize) -> Result<Vec<u8>, JsValue> {
        decode_frame_pixels_inner(&self.data, &self.metadata, frame_index, PixelFormat::Bgra)
            .map_err(|message| JsValue::from_str(&message))
    }

    pub fn decode_and_blit_frame_rgba(
        &self,
        frame_index: usize,
        pixels: &mut [u8],
    ) -> Result<(), JsValue> {
        decode_and_blit_frame_reusing_scratch(
            &self.data,
            &self.metadata,
            frame_index,
            PixelFormat::Rgba,
            pixels,
            &mut self.decode_scratch.borrow_mut(),
        )
        .map_err(|message| JsValue::from_str(&message))
    }

    pub fn decode_and_blit_frame_bgra(
        &self,
        frame_index: usize,
        pixels: &mut [u8],
    ) -> Result<(), JsValue> {
        decode_and_blit_frame_reusing_scratch(
            &self.data,
            &self.metadata,
            frame_index,
            PixelFormat::Bgra,
            pixels,
            &mut self.decode_scratch.borrow_mut(),
        )
        .map_err(|message| JsValue::from_str(&message))
    }

    pub fn decode_all_rgba(&self) -> Result<Vec<u32>, JsValue> {
        prepare_all_composited_frames_inner(&self.data, &self.metadata, PixelFormat::Rgba)
            .map_err(|message| JsValue::from_str(&message))
    }

    pub fn reencode_gif_pixel_perfect(&self) -> Result<Vec<u8>, JsValue> {
        let loop_count = self.metadata.loop_count.map(i32::from).unwrap_or(-1);
        let total_frame_pixels = self
            .metadata
            .frames
            .iter()
            .try_fold(0usize, |total, frame| {
                usize::from(frame.width)
                    .checked_mul(usize::from(frame.height))
                    .and_then(|pixels| total.checked_add(pixels))
                    .ok_or_else(|| "Decoded frame size overflow".to_string())
            });
        let total_frame_pixels =
            total_frame_pixels.map_err(|message| JsValue::from_str(&message))?;
        reencode_gif_literal_sequential(&self.data, &self.metadata, loop_count, total_frame_pixels)
            .map_err(|message| JsValue::from_str(&message))
    }

    pub fn prepare_composited_rgba(&self, requested_frames: &[u8]) -> Result<Vec<u32>, JsValue> {
        prepare_composited_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Rgba,
        )
        .map_err(|message| JsValue::from_str(&message))
    }

    pub fn prepare_composited_bgra(&self, requested_frames: &[u8]) -> Result<Vec<u32>, JsValue> {
        prepare_composited_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Bgra,
        )
        .map_err(|message| JsValue::from_str(&message))
    }

    pub fn prepare_composited_delta_rgba(
        &self,
        requested_frames: &[u8],
    ) -> Result<Vec<u32>, JsValue> {
        prepare_composited_delta_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Rgba,
        )
        .map_err(|message| JsValue::from_str(&message))
    }

    pub fn prepare_composited_delta_bgra(
        &self,
        requested_frames: &[u8],
    ) -> Result<Vec<u32>, JsValue> {
        prepare_composited_delta_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Bgra,
        )
        .map_err(|message| JsValue::from_str(&message))
    }
}

fn parse_metadata(data: &[u8]) -> Result<GifMetadata, String> {
    if data.len() < 13 {
        return Err("GIF data is too short for a header".to_string());
    }

    let version = match &data[0..6] {
        b"GIF87a" => "GIF87a",
        b"GIF89a" => "GIF89a",
        _ => return Err("Invalid GIF signature".to_string()),
    };

    let width = read_u16(data, 6, "logical screen width")?;
    let height = read_u16(data, 8, "logical screen height")?;
    let packed = data[10];
    let has_global_palette = (packed & 0x80) != 0;
    let global_palette_size = if has_global_palette {
        2usize << usize::from(packed & 0x07)
    } else {
        0
    };
    let global_palette_offset = has_global_palette.then_some(13);
    let mut offset = 13usize;

    if has_global_palette {
        offset = checked_add(
            offset,
            global_palette_size * 3,
            data.len(),
            "global color table",
        )?;
    }

    let mut frames = Vec::new();
    let mut graphic_control = GraphicControl::default();
    let mut loop_count = None;

    while offset < data.len() {
        let byte = data[offset];
        offset += 1;

        match byte {
            0x2c => {
                let (frame, next_offset) = parse_image_descriptor(
                    data,
                    offset,
                    global_palette_offset,
                    global_palette_size,
                    graphic_control,
                )?;
                frames.push(frame);
                offset = next_offset;
                graphic_control = GraphicControl::default();
            }
            0x21 => {
                if offset >= data.len() {
                    return Err("Truncated extension block".to_string());
                }
                let label = data[offset];
                offset += 1;

                if label == 0xf9 {
                    let (gce, next_offset) = parse_graphic_control(data, offset)?;
                    graphic_control = gce;
                    offset = next_offset;
                } else {
                    if label == 0xff {
                        loop_count = read_loop_count_extension(data, offset).or(loop_count);
                    }
                    offset = skip_sub_blocks(data, offset, "extension data")?;
                }
            }
            0x3b => break,
            _ => {
                return Err(format!(
                    "Unexpected GIF block byte 0x{byte:02x} at offset {}",
                    offset - 1
                ));
            }
        }
    }

    Ok(GifMetadata {
        version,
        width,
        height,
        global_palette_offset,
        global_palette_size,
        loop_count,
        frames,
    })
}

fn validate_gif_structure_no_alloc(data: &[u8]) -> Result<(), String> {
    if data.len() < 13 {
        return Err("GIF data is too short for a header".to_string());
    }
    if !matches!(&data[0..6], b"GIF87a" | b"GIF89a") {
        return Err("Invalid GIF signature".to_string());
    }

    let packed = data[10];
    let has_global_palette = (packed & 0x80) != 0;
    let global_palette_size = if has_global_palette {
        2usize << usize::from(packed & 0x07)
    } else {
        0
    };
    let global_palette_offset = has_global_palette.then_some(13);
    let mut offset = 13usize;
    if has_global_palette {
        offset = checked_add(
            offset,
            global_palette_size * 3,
            data.len(),
            "global color table",
        )?;
    }

    let mut graphic_control = GraphicControl::default();
    while offset < data.len() {
        let byte = data[offset];
        offset += 1;
        match byte {
            0x2c => {
                let (_, next_offset) = parse_image_descriptor(
                    data,
                    offset,
                    global_palette_offset,
                    global_palette_size,
                    graphic_control,
                )?;
                offset = next_offset;
                graphic_control = GraphicControl::default();
            }
            0x21 => {
                if offset >= data.len() {
                    return Err("Truncated extension block".to_string());
                }
                let label = data[offset];
                offset += 1;
                if label == 0xf9 {
                    let (gce, next_offset) = parse_graphic_control(data, offset)?;
                    graphic_control = gce;
                    offset = next_offset;
                } else {
                    offset = skip_sub_blocks(data, offset, "extension data")?;
                }
            }
            0x3b => return Ok(()),
            _ => {
                return Err(format!(
                    "Unexpected GIF block byte 0x{byte:02x} at offset {}",
                    offset - 1
                ));
            }
        }
    }
    Ok(())
}

fn read_loop_count_extension(data: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(16)?;
    if end > data.len()
        || data[offset] != 11
        || (&data[offset + 1..offset + 12] != b"NETSCAPE2.0"
            && &data[offset + 1..offset + 12] != b"ANIMEXTS1.0")
        || data[offset + 12] != 3
        || data[offset + 13] != 1
    {
        return None;
    }
    Some(u16::from_le_bytes([data[offset + 14], data[offset + 15]]))
}

fn parse_graphic_control(data: &[u8], offset: usize) -> Result<(GraphicControl, usize), String> {
    let block_end = checked_add(offset, 6, data.len(), "graphic control extension")?;
    if data[offset] != 4 {
        return Err(format!(
            "Invalid graphic control extension length {} at offset {offset}",
            data[offset]
        ));
    }
    if data[offset + 5] != 0 {
        return Err(format!(
            "Graphic control extension missing terminator at offset {}",
            offset + 5
        ));
    }

    let packed = data[offset + 1];
    let delay = read_u16(data, offset + 2, "graphic control delay")?;
    let transparent_index = ((packed & 0x01) != 0).then_some(data[offset + 4]);

    Ok((
        GraphicControl {
            delay,
            disposal: (packed >> 2) & 0x07,
            transparent_index,
        },
        block_end,
    ))
}

fn parse_image_descriptor(
    data: &[u8],
    offset: usize,
    global_palette_offset: Option<usize>,
    global_palette_size: usize,
    graphic_control: GraphicControl,
) -> Result<(FrameMetadata, usize), String> {
    let descriptor_end = checked_add(offset, 9, data.len(), "image descriptor")?;
    let x = read_u16(data, offset, "image x")?;
    let y = read_u16(data, offset + 2, "image y")?;
    let width = read_u16(data, offset + 4, "image width")?;
    let height = read_u16(data, offset + 6, "image height")?;
    let packed = data[offset + 8];
    let has_local_palette = (packed & 0x80) != 0;
    let interlaced = (packed & 0x40) != 0;

    let mut data_offset = descriptor_end;
    let (palette_offset, palette_size) = if has_local_palette {
        let palette_size = 2usize << usize::from(packed & 0x07);
        let palette_offset = data_offset;
        data_offset = checked_add(
            data_offset,
            palette_size * 3,
            data.len(),
            "local color table",
        )?;
        (palette_offset, palette_size)
    } else {
        (global_palette_offset.unwrap_or(0), global_palette_size)
    };

    if data_offset >= data.len() {
        return Err("Image data is missing an LZW minimum code size".to_string());
    }
    let min_code_size = data[data_offset];
    let next_offset = skip_sub_blocks(data, data_offset + 1, "image data")?;
    let data_length = next_offset - data_offset;

    Ok((
        FrameMetadata {
            x,
            y,
            width,
            height,
            has_local_palette,
            palette_offset,
            palette_size,
            data_offset,
            data_length,
            transparent_index: graphic_control.transparent_index,
            interlaced,
            delay: graphic_control.delay,
            disposal: graphic_control.disposal,
            min_code_size,
        },
        next_offset,
    ))
}

fn decode_frame_indices_inner(data: &[u8], frame: &FrameMetadata) -> Result<Vec<u8>, String> {
    let mut image_data = Vec::new();
    decode_frame_indices_with_image_scratch(data, frame, &mut image_data)
}

fn decode_frame_indices_with_image_scratch(
    data: &[u8],
    frame: &FrameMetadata,
    image_data: &mut Vec<u8>,
) -> Result<Vec<u8>, String> {
    let frame_size = usize::from(frame.width) * usize::from(frame.height);
    if should_decode_lzw_direct(frame, frame_size) {
        if image_data.capacity() < frame.data_length {
            image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
        }
        collect_image_data_into(data, frame.data_offset, image_data)?;
        let mut output = vec![0; frame_size];
        lzw_decode_to_indices_direct(frame.min_code_size, image_data, &mut output)?;
        return deinterlace_frame_indices(output, frame);
    }
    let mut lzw_scratch = LzwStackScratch::default();
    decode_frame_indices_with_scratches(data, frame, image_data, &mut lzw_scratch)
}

fn decode_frame_indices_with_scratches(
    data: &[u8],
    frame: &FrameMetadata,
    image_data: &mut Vec<u8>,
    lzw_scratch: &mut LzwStackScratch,
) -> Result<Vec<u8>, String> {
    if image_data.capacity() < frame.data_length {
        image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
    }
    collect_image_data_into(data, frame.data_offset, image_data)?;
    let frame_size = usize::from(frame.width) * usize::from(frame.height);
    let linear = if should_decode_lzw_direct(frame, frame_size) {
        let mut output = vec![0; frame_size];
        if should_decode_lzw_copy(frame, frame_size) {
            lzw_decode_to_indices_copy_with_scratch(
                frame.min_code_size,
                image_data,
                &mut output,
                lzw_scratch,
            )?;
        } else {
            lzw_decode_to_indices_direct_with_scratch(
                frame.min_code_size,
                image_data,
                &mut output,
                lzw_scratch,
            )?;
        }
        output
    } else {
        let mut output = vec![0; frame_size];
        lzw_decode_to_indices_stack_with_scratch(
            frame.min_code_size,
            image_data,
            &mut output,
            lzw_scratch,
        )?;
        output
    };
    deinterlace_frame_indices(linear, frame)
}

fn decode_frame_indices_reusing_output(
    data: &[u8],
    frame: &FrameMetadata,
    image_data: &mut Vec<u8>,
    lzw_scratch: &mut LzwStackScratch,
    output: &mut Vec<u8>,
) -> Result<(), String> {
    if frame.interlaced {
        *output = decode_frame_indices_with_scratches(data, frame, image_data, lzw_scratch)?;
        return Ok(());
    }
    if image_data.capacity() < frame.data_length {
        image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
    }
    collect_image_data_into(data, frame.data_offset, image_data)?;
    let frame_size = usize::from(frame.width) * usize::from(frame.height);
    output.resize(frame_size, 0);
    if should_decode_lzw_direct(frame, frame_size) {
        if should_decode_lzw_copy(frame, frame_size) {
            lzw_decode_to_indices_copy_with_scratch(
                frame.min_code_size,
                image_data,
                output,
                lzw_scratch,
            )
        } else {
            lzw_decode_to_indices_direct_with_scratch(
                frame.min_code_size,
                image_data,
                output,
                lzw_scratch,
            )
        }
    } else {
        lzw_decode_to_indices_stack_with_scratch(
            frame.min_code_size,
            image_data,
            output,
            lzw_scratch,
        )
    }
}

#[inline]
fn should_decode_lzw_direct(frame: &FrameMetadata, frame_size: usize) -> bool {
    (frame.min_code_size == 8 && frame_size >= 256)
        || frame_size >= 40_000
        || (frame_size >= 1_000
            && frame.data_length.saturating_mul(20) < frame_size.saturating_mul(13))
        || (frame_size >= 256 && frame.data_length.saturating_mul(5) < frame_size.saturating_mul(2))
}

#[inline]
fn should_decode_lzw_copy(frame: &FrameMetadata, frame_size: usize) -> bool {
    frame.min_code_size <= 6
        || frame_size >= 10_000
        || frame.data_length.saturating_mul(5) < frame_size.saturating_mul(2)
}

fn deinterlace_frame_indices(linear: Vec<u8>, frame: &FrameMetadata) -> Result<Vec<u8>, String> {
    if !frame.interlaced {
        return Ok(linear);
    }

    let frame_size = usize::from(frame.width) * usize::from(frame.height);
    let mut deinterlaced = vec![0; frame_size];
    let width = usize::from(frame.width);
    let height = usize::from(frame.height);
    let mut src = 0usize;

    for (y_start, y_stride) in [(0usize, 8usize), (4, 8), (2, 4), (1, 2)] {
        let mut row = y_start;
        while row < height {
            let dst = row * width;
            for x in 0..width {
                if src >= linear.len() {
                    return Ok(deinterlaced);
                }
                deinterlaced[dst + x] = linear[src];
                src += 1;
            }
            row += y_stride;
        }
    }

    Ok(deinterlaced)
}

fn decode_frame_pixels_inner(
    data: &[u8],
    metadata: &GifMetadata,
    frame_index: usize,
    format: PixelFormat,
) -> Result<Vec<u8>, String> {
    let frame = metadata
        .frames
        .get(frame_index)
        .ok_or_else(|| "Frame index out of range".to_string())?;
    let canvas_len = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let mut pixels = vec![0; canvas_len];
    let palette = build_palette_pixels(data, frame, format)?;
    let indices = decode_frame_indices_inner(data, frame)?;

    blit_indices_to_pixels(&palette, metadata.width, frame, &indices, &mut pixels)?;

    Ok(pixels)
}

fn decode_and_blit_frame_reusing_scratch(
    data: &[u8],
    metadata: &GifMetadata,
    frame_index: usize,
    format: PixelFormat,
    pixels: &mut [u8],
    scratch: &mut FrameDecodeScratch,
) -> Result<(), String> {
    let expected_length = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .and_then(|pixel_count| pixel_count.checked_mul(4))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    if pixels.len() < expected_length {
        return Err("Pixel buffer is too small".to_string());
    }
    let frame = metadata
        .frames
        .get(frame_index)
        .ok_or_else(|| "Frame index out of range".to_string())?;

    let target = &mut pixels[..expected_length];
    decode_frame_indices_reusing_output(
        data,
        frame,
        &mut scratch.image_data,
        &mut scratch.lzw,
        &mut scratch.indices,
    )?;
    let (prefix, canvas, suffix) = unsafe { target.align_to_mut::<u32>() };
    if prefix.is_empty() && suffix.is_empty() {
        if scratch.palette_offset != frame.palette_offset
            || scratch.palette_size != frame.palette_size
            || scratch.palette_format != Some(format)
        {
            scratch.palette = build_palette_u32(data, frame, format)?;
            scratch.palette_offset = frame.palette_offset;
            scratch.palette_size = frame.palette_size;
            scratch.palette_format = Some(format);
        }
        return blit_indices_to_canvas_u32(
            &scratch.palette,
            metadata.width,
            frame,
            &scratch.indices,
            canvas,
        );
    }
    let palette = build_palette_pixels(data, frame, format)?;
    blit_indices_to_pixels(&palette, metadata.width, frame, &scratch.indices, target)
}

fn prepare_composited_frames_inner(
    data: &[u8],
    metadata: &GifMetadata,
    requested_frames: &[u8],
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    prepare_composited_frames_selected(data, metadata, Some(requested_frames), format)
}

fn prepare_all_composited_frames_inner(
    data: &[u8],
    metadata: &GifMetadata,
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    prepare_composited_frames_selected(data, metadata, None, format)
}

fn prepare_composited_frames_selected(
    data: &[u8],
    metadata: &GifMetadata,
    requested_frames: Option<&[u8]>,
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    if let Some(requested_frames) = requested_frames {
        if requested_frames.len() > metadata.frames.len() {
            return Err("Requested frame flags exceed frame count".to_string());
        }
    }

    let canvas_width = usize::from(metadata.width);
    let canvas_height = usize::from(metadata.height);
    let canvas_pixels = canvas_width
        .checked_mul(canvas_height)
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let requested_count = requested_frames.map_or(metadata.frames.len(), |requested_frames| {
        requested_frames.iter().filter(|flag| **flag != 0).count()
    });
    let output_pixels = requested_count
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    let mut output = Vec::with_capacity(output_pixels);
    let mut canvas = vec![0u32; canvas_pixels];
    let mut image_data = Vec::new();
    let mut lzw_scratch = LzwStackScratch::default();
    let mut indices_scratch = Vec::new();
    let palettes_share_table = metadata.frames.first().is_some_and(|first| {
        metadata.frames.iter().all(|frame| {
            frame.palette_offset == first.palette_offset && frame.palette_size == first.palette_size
        })
    });
    let shared_palette = if palettes_share_table {
        Some(build_palette_u32(data, &metadata.frames[0], format)?)
    } else {
        None
    };

    let frame_limit = requested_frames.map_or(metadata.frames.len(), <[u8]>::len);
    for frame_index in 0..frame_limit {
        let requested = requested_frames
            .and_then(|frames| frames.get(frame_index))
            .copied()
            .unwrap_or(1);
        let frame = metadata
            .frames
            .get(frame_index)
            .ok_or_else(|| "Frame index out of range".to_string())?;
        let restore = (frame.disposal == 3).then(|| canvas.clone());
        let palette_storage;
        let palette = if let Some(palette) = shared_palette.as_ref() {
            palette
        } else {
            palette_storage = build_palette_u32(data, frame, format)?;
            &palette_storage
        };
        let indices = {
            decode_frame_indices_reusing_output(
                data,
                frame,
                &mut image_data,
                &mut lzw_scratch,
                &mut indices_scratch,
            )?;
            &indices_scratch
        };

        blit_indices_to_canvas_u32(palette, metadata.width, frame, indices, &mut canvas)?;

        if requested != 0 {
            output.extend_from_slice(&canvas);
        }

        apply_frame_disposal_u32(&mut canvas, metadata.width, metadata.height, frame, restore);
    }

    Ok(output)
}

#[cfg(not(target_arch = "wasm32"))]
fn compose_decoded_frames_native(
    data: &[u8],
    metadata: &GifMetadata,
    decoded_indices: &[Vec<u8>],
) -> Result<Vec<u32>, String> {
    let canvas_pixels = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_pixels = metadata
        .frames
        .len()
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    if available_threads > 1 && output_pixels >= 200_000 {
        return compose_decoded_frames_by_rows_native(
            data,
            metadata,
            decoded_indices,
            available_threads,
            output_pixels,
        );
    }
    let mut output = Vec::with_capacity(output_pixels);
    let mut canvas = vec![0u32; canvas_pixels];

    for (frame, indices) in metadata.frames.iter().zip(decoded_indices.iter()) {
        let restore = (frame.disposal == 3).then(|| canvas.clone());
        let palette = build_palette_u32(data, frame, PixelFormat::Rgba)?;
        blit_indices_to_canvas_u32(&palette, metadata.width, frame, indices, &mut canvas)?;
        output.extend_from_slice(&canvas);
        apply_frame_disposal_u32(&mut canvas, metadata.width, metadata.height, frame, restore);
    }
    Ok(output)
}

#[cfg(not(target_arch = "wasm32"))]
fn compose_decoded_frames_by_rows_native(
    data: &[u8],
    metadata: &GifMetadata,
    decoded_indices: &[Vec<u8>],
    available_threads: usize,
    output_pixels: usize,
) -> Result<Vec<u32>, String> {
    let canvas_width = usize::from(metadata.width);
    let canvas_height = usize::from(metadata.height);
    let canvas_pixels = canvas_width * canvas_height;
    let thread_count = available_threads.min(canvas_height);
    let rows_per_thread = canvas_height.div_ceil(thread_count);
    let palettes_share_table = metadata.frames.first().is_some_and(|first| {
        metadata.frames.iter().all(|frame| {
            frame.palette_offset == first.palette_offset && frame.palette_size == first.palette_size
        })
    });
    let palettes = if palettes_share_table {
        vec![build_palette_u32(
            data,
            &metadata.frames[0],
            PixelFormat::Rgba,
        )?]
    } else {
        metadata
            .frames
            .iter()
            .map(|frame| build_palette_u32(data, frame, PixelFormat::Rgba))
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut output = Vec::<std::mem::MaybeUninit<u32>>::with_capacity(output_pixels);
    unsafe {
        output.set_len(output_pixels);
    }
    let output_address = output.as_mut_ptr() as usize;

    let result = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..thread_count)
            .map(|thread_index| {
                let row_start = thread_index * rows_per_thread;
                let row_end = (row_start + rows_per_thread).min(canvas_height);
                let palettes = &palettes;
                scope.spawn(move || {
                    let stripe_pixels = (row_end - row_start) * canvas_width;
                    let mut canvas = vec![0u32; stripe_pixels];
                    for (frame_index, (frame, indices)) in metadata
                        .frames
                        .iter()
                        .zip(decoded_indices.iter())
                        .enumerate()
                    {
                        let palette = &palettes[if palettes_share_table { 0 } else { frame_index }];
                        let restore = (frame.disposal == 3).then(|| canvas.clone());
                        blit_indices_to_canvas_stripe_u32(
                            palette,
                            canvas_width,
                            frame,
                            indices,
                            row_start,
                            row_end,
                            &mut canvas,
                        )?;
                        unsafe {
                            let destination = (output_address as *mut std::mem::MaybeUninit<u32>)
                                .add(frame_index * canvas_pixels + row_start * canvas_width)
                                .cast::<u32>();
                            std::ptr::copy_nonoverlapping(
                                canvas.as_ptr(),
                                destination,
                                stripe_pixels,
                            );
                        }
                        if frame.disposal == 2 {
                            clear_frame_rect_stripe_u32(
                                &mut canvas,
                                canvas_width,
                                frame,
                                row_start,
                                row_end,
                            );
                        } else if frame.disposal == 3 {
                            canvas = restore.expect("restore canvas exists");
                        }
                    }
                    Ok::<_, String>(())
                })
            })
            .collect();
        for handle in handles {
            handle
                .join()
                .map_err(|_| "Parallel GIF compositor panicked".to_string())??;
        }
        Ok::<_, String>(())
    });
    result?;

    let pointer = output.as_mut_ptr().cast::<u32>();
    let length = output.len();
    let capacity = output.capacity();
    std::mem::forget(output);
    Ok(unsafe { Vec::from_raw_parts(pointer, length, capacity) })
}

#[cfg(not(target_arch = "wasm32"))]
fn blit_indices_to_canvas_stripe_u32(
    palette: &[u32],
    canvas_width: usize,
    frame: &FrameMetadata,
    indices: &[u8],
    row_start: usize,
    row_end: usize,
    canvas: &mut [u32],
) -> Result<(), String> {
    let frame_y = usize::from(frame.y);
    let frame_bottom = frame_y + usize::from(frame.height);
    let first_row = frame_y.max(row_start);
    let last_row = frame_bottom.min(row_end);
    if first_row >= last_row {
        return Ok(());
    }
    let frame_width = usize::from(frame.width);
    let frame_x = usize::from(frame.x);
    let frame_pixels = frame_width
        .checked_mul(usize::from(frame.height))
        .ok_or_else(|| "Decoded frame size overflow".to_string())?;
    if indices.len() < frame_pixels {
        return Err("Decoded index buffer is too short".to_string());
    }
    if frame_x.saturating_add(frame_width) > canvas_width
        || canvas_width == 0
        || row_end.saturating_sub(row_start) > canvas.len() / canvas_width
    {
        return Err("Frame destination exceeds canvas bounds".to_string());
    }
    let full_byte_palette = palette.len() == 256;
    match frame.transparent_index {
        None => {
            for global_y in first_row..last_row {
                let source = (global_y - frame_y) * frame_width;
                let source_row = &indices[source..source + frame_width];
                let destination = (global_y - row_start) * canvas_width + frame_x;
                let destination_row = &mut canvas[destination..destination + frame_width];
                if full_byte_palette {
                    for (pixel, &index) in destination_row.iter_mut().zip(source_row) {
                        *pixel = unsafe { *palette.get_unchecked(usize::from(index)) };
                    }
                    continue;
                }
                for (pixel, &index) in destination_row.iter_mut().zip(source_row) {
                    *pixel = *palette
                        .get(usize::from(index))
                        .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
                }
            }
        }
        Some(transparent_index) => {
            for global_y in first_row..last_row {
                let source = (global_y - frame_y) * frame_width;
                let source_row = &indices[source..source + frame_width];
                let destination = (global_y - row_start) * canvas_width + frame_x;
                let destination_row = &mut canvas[destination..destination + frame_width];
                if full_byte_palette {
                    let palette_address = palette.as_ptr();
                    let transparent_bytes =
                        u64::from(transparent_index).wrapping_mul(0x0101_0101_0101_0101);
                    let mut pixel_index = 0usize;
                    while pixel_index + 8 <= frame_width {
                        let packed_indices = unsafe {
                            std::ptr::read_unaligned(
                                source_row.as_ptr().add(pixel_index).cast::<u64>(),
                            )
                        };
                        if packed_indices == transparent_bytes {
                            pixel_index += 8;
                            continue;
                        }
                        let compared = packed_indices ^ transparent_bytes;
                        let transparent_lanes = compared.wrapping_sub(0x0101_0101_0101_0101)
                            & !compared
                            & 0x8080_8080_8080_8080;
                        if transparent_lanes == 0 {
                            let colors = unsafe {
                                [
                                    *palette_address.add((packed_indices & 0xff) as usize),
                                    *palette_address.add(((packed_indices >> 8) & 0xff) as usize),
                                    *palette_address.add(((packed_indices >> 16) & 0xff) as usize),
                                    *palette_address.add(((packed_indices >> 24) & 0xff) as usize),
                                    *palette_address.add(((packed_indices >> 32) & 0xff) as usize),
                                    *palette_address.add(((packed_indices >> 40) & 0xff) as usize),
                                    *palette_address.add(((packed_indices >> 48) & 0xff) as usize),
                                    *palette_address.add(((packed_indices >> 56) & 0xff) as usize),
                                ]
                            };
                            unsafe {
                                std::ptr::copy_nonoverlapping(
                                    colors.as_ptr(),
                                    destination_row.as_mut_ptr().add(pixel_index),
                                    colors.len(),
                                );
                            }
                            pixel_index += 8;
                            continue;
                        }
                        for lane in 0..8 {
                            let index = unsafe { *source_row.get_unchecked(pixel_index + lane) };
                            if index != transparent_index {
                                destination_row[pixel_index + lane] =
                                    unsafe { *palette_address.add(usize::from(index)) };
                            }
                        }
                        pixel_index += 8;
                    }
                    while pixel_index < frame_width {
                        let index = source_row[pixel_index];
                        if index != transparent_index {
                            destination_row[pixel_index] =
                                unsafe { *palette_address.add(usize::from(index)) };
                        }
                        pixel_index += 1;
                    }
                    continue;
                }
                for (pixel, &index) in destination_row.iter_mut().zip(source_row) {
                    if index != transparent_index {
                        *pixel = *palette
                            .get(usize::from(index))
                            .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn clear_frame_rect_stripe_u32(
    canvas: &mut [u32],
    canvas_width: usize,
    frame: &FrameMetadata,
    row_start: usize,
    row_end: usize,
) {
    let frame_y = usize::from(frame.y);
    let first_row = frame_y.max(row_start);
    let last_row = (frame_y + usize::from(frame.height)).min(row_end);
    let x = usize::from(frame.x);
    let width = usize::from(frame.width);
    for global_y in first_row..last_row {
        let start = (global_y - row_start) * canvas_width + x;
        canvas[start..start + width].fill(0);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn frames_are_independent_native(metadata: &GifMetadata) -> bool {
    metadata.frames.iter().enumerate().all(|(index, frame)| {
        let starts_clear = index == 0
            || (metadata.frames[index - 1].disposal == 2
                && frame_covers_canvas(&metadata.frames[index - 1], metadata));
        starts_clear || (frame_covers_canvas(frame, metadata) && frame.transparent_index.is_none())
    })
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
fn frame_covers_canvas(frame: &FrameMetadata, metadata: &GifMetadata) -> bool {
    frame.x == 0 && frame.y == 0 && frame.width == metadata.width && frame.height == metadata.height
}

#[cfg(not(target_arch = "wasm32"))]
fn independent_frame_segments_native(metadata: &GifMetadata) -> Option<Vec<(usize, usize)>> {
    let mut segments = Vec::new();
    let mut start = 0usize;
    for (frame_index, frame) in metadata.frames.iter().enumerate() {
        let clears_canvas = frame.disposal == 2
            && frame.x == 0
            && frame.y == 0
            && frame.width == metadata.width
            && frame.height == metadata.height;
        if clears_canvas && frame_index + 1 < metadata.frames.len() {
            segments.push((start, frame_index + 1));
            start = frame_index + 1;
        }
    }
    segments.push((start, metadata.frames.len()));
    let output_pixels = usize::from(metadata.width)
        .saturating_mul(usize::from(metadata.height))
        .saturating_mul(metadata.frames.len());
    let longest_segment = segments
        .iter()
        .map(|(segment_start, segment_end)| segment_end - segment_start)
        .max()
        .unwrap_or(0);
    (segments.len() >= 2 && output_pixels >= 100_000 && longest_segment <= 8).then_some(segments)
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_segments_worker(
    data: &[u8],
    metadata: &GifMetadata,
    segments: &[(usize, usize)],
    palettes: &[Vec<u32>],
    palettes_share_table: bool,
    canvas_pixels: usize,
    output_address: usize,
    next_segment: &std::sync::atomic::AtomicUsize,
) -> Result<(), String> {
    let mut image_data = Vec::new();
    let mut lzw_scratch = LzwStackScratch::default();
    let mut indices = Vec::new();
    loop {
        let segment_index = next_segment.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(&(start, end)) = segments.get(segment_index) else {
            return Ok(());
        };
        if end == start + 1 {
            let frame = &metadata.frames[start];
            if frame_covers_canvas(frame, metadata) && !frame.interlaced {
                let palette = &palettes[if palettes_share_table { 0 } else { start }];
                decode_frame_indices_reusing_output(
                    data,
                    frame,
                    &mut image_data,
                    &mut lzw_scratch,
                    &mut indices,
                )?;
                let destination = unsafe {
                    std::slice::from_raw_parts_mut(
                        (output_address as *mut std::mem::MaybeUninit<u32>)
                            .add(start * canvas_pixels),
                        canvas_pixels,
                    )
                };
                if palette.len() == 256 {
                    let palette_address = palette.as_ptr();
                    let indices_address = indices.as_ptr();
                    let destination_address = destination.as_mut_ptr().cast::<u32>();
                    let mut pixel_index = 0usize;
                    match frame.transparent_index {
                        None => {
                            while pixel_index + 8 <= canvas_pixels {
                                let packed = unsafe {
                                    std::ptr::read_unaligned(
                                        indices_address.add(pixel_index).cast::<u64>(),
                                    )
                                };
                                let colors = unsafe {
                                    [
                                        *palette_address.add((packed & 0xff) as usize),
                                        *palette_address.add(((packed >> 8) & 0xff) as usize),
                                        *palette_address.add(((packed >> 16) & 0xff) as usize),
                                        *palette_address.add(((packed >> 24) & 0xff) as usize),
                                        *palette_address.add(((packed >> 32) & 0xff) as usize),
                                        *palette_address.add(((packed >> 40) & 0xff) as usize),
                                        *palette_address.add(((packed >> 48) & 0xff) as usize),
                                        *palette_address.add(((packed >> 56) & 0xff) as usize),
                                    ]
                                };
                                unsafe {
                                    std::ptr::copy_nonoverlapping(
                                        colors.as_ptr(),
                                        destination_address.add(pixel_index),
                                        colors.len(),
                                    );
                                }
                                pixel_index += 8;
                            }
                        }
                        Some(transparent_index) => {
                            while pixel_index + 8 <= canvas_pixels {
                                let packed = unsafe {
                                    std::ptr::read_unaligned(
                                        indices_address.add(pixel_index).cast::<u64>(),
                                    )
                                };
                                let indices = [
                                    (packed & 0xff) as u8,
                                    ((packed >> 8) & 0xff) as u8,
                                    ((packed >> 16) & 0xff) as u8,
                                    ((packed >> 24) & 0xff) as u8,
                                    ((packed >> 32) & 0xff) as u8,
                                    ((packed >> 40) & 0xff) as u8,
                                    ((packed >> 48) & 0xff) as u8,
                                    ((packed >> 56) & 0xff) as u8,
                                ];
                                let colors = indices.map(|index| {
                                    if index == transparent_index {
                                        0
                                    } else {
                                        unsafe { *palette_address.add(usize::from(index)) }
                                    }
                                });
                                unsafe {
                                    std::ptr::copy_nonoverlapping(
                                        colors.as_ptr(),
                                        destination_address.add(pixel_index),
                                        colors.len(),
                                    );
                                }
                                pixel_index += 8;
                            }
                        }
                    }
                    while pixel_index < canvas_pixels {
                        let index = unsafe { *indices_address.add(pixel_index) };
                        let color = if frame.transparent_index == Some(index) {
                            0
                        } else {
                            unsafe { *palette_address.add(usize::from(index)) }
                        };
                        unsafe {
                            destination_address.add(pixel_index).write(color);
                        }
                        pixel_index += 1;
                    }
                    continue;
                }
                match frame.transparent_index {
                    None => {
                        for (pixel, &index) in destination.iter_mut().zip(&indices) {
                            pixel.write(*palette.get(usize::from(index)).ok_or_else(|| {
                                format!("Palette index {index} exceeds palette size")
                            })?);
                        }
                    }
                    Some(transparent_index) => {
                        for (pixel, &index) in destination.iter_mut().zip(&indices) {
                            pixel.write(if index == transparent_index {
                                0
                            } else {
                                *palette.get(usize::from(index)).ok_or_else(|| {
                                    format!("Palette index {index} exceeds palette size")
                                })?
                            });
                        }
                    }
                }
                continue;
            }
        }
        let mut canvas = vec![0u32; canvas_pixels];
        for frame_index in start..end {
            let frame = &metadata.frames[frame_index];
            let restore = (frame.disposal == 3).then(|| canvas.clone());
            let palette = &palettes[if palettes_share_table { 0 } else { frame_index }];
            decode_frame_indices_reusing_output(
                data,
                frame,
                &mut image_data,
                &mut lzw_scratch,
                &mut indices,
            )?;
            blit_indices_to_canvas_u32(palette, metadata.width, frame, &indices, &mut canvas)?;
            unsafe {
                let destination = (output_address as *mut std::mem::MaybeUninit<u32>)
                    .add(frame_index * canvas_pixels)
                    .cast::<u32>();
                std::ptr::copy_nonoverlapping(canvas.as_ptr(), destination, canvas_pixels);
            }
            apply_frame_disposal_u32(&mut canvas, metadata.width, metadata.height, frame, restore);
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
struct DecodeSegmentsDispatchContext<'a> {
    data: &'a [u8],
    metadata: &'a GifMetadata,
    segments: &'a [(usize, usize)],
    palettes: &'a [Vec<u32>],
    palettes_share_table: bool,
    canvas_pixels: usize,
    output_address: usize,
    next_segment: std::sync::atomic::AtomicUsize,
    error: std::sync::Mutex<Option<String>>,
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
unsafe extern "C" fn decode_segments_dispatch_worker(
    context: *mut std::ffi::c_void,
    _iteration: usize,
) {
    let context = &*(context.cast::<DecodeSegmentsDispatchContext<'_>>());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        decode_segments_worker(
            context.data,
            context.metadata,
            context.segments,
            context.palettes,
            context.palettes_share_table,
            context.canvas_pixels,
            context.output_address,
            &context.next_segment,
        )
    }));
    let error = match result {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error,
        Err(_) => "Parallel GIF segment decoder panicked".to_string(),
    };
    if let Ok(mut stored_error) = context.error.lock() {
        if stored_error.is_none() {
            *stored_error = Some(error);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_segmented_frames_into_native(
    data: &[u8],
    metadata: &GifMetadata,
    segments: &[(usize, usize)],
    output: &mut [std::mem::MaybeUninit<u32>],
) -> Result<(), String> {
    let canvas_pixels = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_pixels = metadata
        .frames
        .len()
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    if output.len() != output_pixels {
        return Err("Host output buffer has the wrong size".to_string());
    }
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let thread_count = available_threads.min(segments.len());
    let palettes_share_table = metadata.frames.first().is_some_and(|first| {
        metadata.frames.iter().all(|frame| {
            frame.palette_offset == first.palette_offset && frame.palette_size == first.palette_size
        })
    });
    let palettes = if palettes_share_table {
        vec![build_palette_u32(
            data,
            &metadata.frames[0],
            PixelFormat::Rgba,
        )?]
    } else {
        metadata
            .frames
            .iter()
            .map(|frame| build_palette_u32(data, frame, PixelFormat::Rgba))
            .collect::<Result<Vec<_>, _>>()?
    };
    let output_address = output.as_mut_ptr() as usize;
    let next_segment = std::sync::atomic::AtomicUsize::new(0);

    #[cfg(target_os = "macos")]
    {
        let mut context = DecodeSegmentsDispatchContext {
            data,
            metadata,
            segments,
            palettes: &palettes,
            palettes_share_table,
            canvas_pixels,
            output_address,
            next_segment,
            error: std::sync::Mutex::new(None),
        };
        unsafe {
            let queue = dispatch_get_global_queue(0, 0);
            if queue.is_null() {
                return Err("Could not acquire the system worker queue".to_string());
            }
            dispatch_apply_f(
                thread_count,
                queue,
                (&mut context as *mut DecodeSegmentsDispatchContext<'_>).cast(),
                decode_segments_dispatch_worker,
            );
        }
        if let Some(error) = context
            .error
            .into_inner()
            .map_err(|_| "Parallel GIF segment decoder error lock was poisoned".to_string())?
        {
            return Err(error);
        }
    }

    #[cfg(not(target_os = "macos"))]
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..thread_count)
            .map(|_| {
                let next_segment = &next_segment;
                let palettes = &palettes;
                scope.spawn(move || {
                    decode_segments_worker(
                        data,
                        metadata,
                        segments,
                        palettes,
                        palettes_share_table,
                        canvas_pixels,
                        output_address,
                        next_segment,
                    )
                })
            })
            .collect();
        for handle in handles {
            handle
                .join()
                .map_err(|_| "Parallel GIF segment decoder panicked".to_string())??;
        }
        Ok::<_, String>(())
    })?;

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_segmented_frames_native(
    data: &[u8],
    metadata: &GifMetadata,
    segments: &[(usize, usize)],
) -> Result<Vec<u32>, String> {
    let canvas_pixels = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_pixels = metadata
        .frames
        .len()
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    let mut output = Vec::<std::mem::MaybeUninit<u32>>::with_capacity(output_pixels);
    unsafe {
        output.set_len(output_pixels);
    }
    decode_segmented_frames_into_native(data, metadata, segments, &mut output)?;
    let pointer = output.as_mut_ptr().cast::<u32>();
    let length = output.len();
    let capacity = output.capacity();
    std::mem::forget(output);
    Ok(unsafe { Vec::from_raw_parts(pointer, length, capacity) })
}

#[cfg(not(target_arch = "wasm32"))]
fn should_fuse_sequential_decode_native(metadata: &GifMetadata) -> bool {
    let total_frame_pixels = metadata.frames.iter().fold(0usize, |total, frame| {
        total.saturating_add(usize::from(frame.width) * usize::from(frame.height))
    });
    metadata.frames.len() < 8 || total_frame_pixels < 30_000
}

#[cfg(not(target_arch = "wasm32"))]
fn should_pipeline_decode_native(metadata: &GifMetadata) -> bool {
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    if available_threads < 4 || metadata.frames.len() < 8 {
        return false;
    }
    let canvas_pixels = usize::from(metadata.width) * usize::from(metadata.height);
    let output_pixels = canvas_pixels.saturating_mul(metadata.frames.len());
    let decoded_pixels = metadata.frames.iter().fold(0usize, |total, frame| {
        total.saturating_add(usize::from(frame.width) * usize::from(frame.height))
    });
    output_pixels >= 50_000 && decoded_pixels >= 50_000
}

#[cfg(not(target_arch = "wasm32"))]
struct PipelineDecodedFrame {
    state: std::sync::atomic::AtomicU8,
    result: std::sync::OnceLock<Result<Vec<u8>, String>>,
    direct_mapped: std::sync::atomic::AtomicBool,
    offset: usize,
    length: usize,
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_pipeline_frame_into(
    data: &[u8],
    frame: &FrameMetadata,
    image_data: &mut Vec<u8>,
    lzw_scratch: &mut LzwStackScratch,
    decoded_indices_address: usize,
    layout: &PipelineDecodedFrame,
) -> Result<(), String> {
    if frame.interlaced {
        let indices = decode_frame_indices_with_scratches(data, frame, image_data, lzw_scratch)?;
        if indices.len() != layout.length {
            return Err("Decoded frame length does not match dimensions".to_string());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                indices.as_ptr(),
                (decoded_indices_address as *mut u8).add(layout.offset),
                layout.length,
            );
        }
        return Ok(());
    }
    if image_data.capacity() < frame.data_length {
        image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
    }
    collect_image_data_into(data, frame.data_offset, image_data)?;
    unsafe {
        std::ptr::write_bytes(
            (decoded_indices_address as *mut u8).add(layout.offset),
            0,
            layout.length,
        );
    }
    let output = unsafe {
        std::slice::from_raw_parts_mut(
            (decoded_indices_address as *mut u8).add(layout.offset),
            layout.length,
        )
    };
    if should_decode_lzw_direct(frame, layout.length) {
        if should_decode_lzw_copy(frame, layout.length) {
            lzw_decode_to_indices_copy_with_scratch(
                frame.min_code_size,
                image_data,
                output,
                lzw_scratch,
            )
        } else {
            lzw_decode_to_indices_direct_with_scratch(
                frame.min_code_size,
                image_data,
                output,
                lzw_scratch,
            )
        }
    } else {
        lzw_decode_to_indices_stack_with_scratch(
            frame.min_code_size,
            image_data,
            output,
            lzw_scratch,
        )
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn decode_pipeline_frame_result(
    data: &[u8],
    metadata: &GifMetadata,
    frame_index: usize,
    frame: &FrameMetadata,
    layout: &PipelineDecodedFrame,
    image_data: &mut Vec<u8>,
    lzw_scratch: &mut LzwStackScratch,
    decoded_indices_address: usize,
    flat_decoded_indices: bool,
    palette: &[u32],
    output_address: usize,
    canvas_pixels: usize,
    parallel_direct_mapping: bool,
) -> Result<Vec<u8>, String> {
    if flat_decoded_indices {
        decode_pipeline_frame_into(
            data,
            frame,
            image_data,
            lzw_scratch,
            decoded_indices_address,
            layout,
        )?;
        return Ok(Vec::new());
    }
    if parallel_direct_mapping && !frame.interlaced {
        if image_data.capacity() < frame.data_length {
            image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
        }
        collect_image_data_into(data, frame.data_offset, image_data)?;
        let destination = unsafe {
            (output_address as *mut std::mem::MaybeUninit<u32>)
                .add(frame_index * canvas_pixels)
                .cast::<u32>()
        };
        unsafe {
            std::ptr::write_bytes(destination.cast::<u8>(), 0, canvas_pixels);
        }
        let destination_bytes =
            unsafe { std::slice::from_raw_parts_mut(destination.cast::<u8>(), canvas_pixels) };
        if should_decode_lzw_direct(frame, canvas_pixels) {
            if should_decode_lzw_copy(frame, canvas_pixels) {
                lzw_decode_to_indices_copy_with_scratch(
                    frame.min_code_size,
                    image_data,
                    destination_bytes,
                    lzw_scratch,
                )?;
            } else {
                lzw_decode_to_indices_direct_with_scratch(
                    frame.min_code_size,
                    image_data,
                    destination_bytes,
                    lzw_scratch,
                )?;
            }
        } else {
            lzw_decode_to_indices_stack_with_scratch(
                frame.min_code_size,
                image_data,
                destination_bytes,
                lzw_scratch,
            )?;
        }
        if let Some(transparent_index) = frame.transparent_index {
            if destination_bytes.contains(&transparent_index) {
                return Ok(destination_bytes.to_vec());
            }
        }
        if palette.len() == 256 {
            let palette_address = palette.as_ptr();
            let indices_address = destination.cast::<u8>();
            let mut pixel_index = canvas_pixels;
            while pixel_index >= 8 {
                pixel_index -= 8;
                let packed_indices = unsafe {
                    std::ptr::read_unaligned(indices_address.add(pixel_index).cast::<u64>())
                };
                let colors = unsafe {
                    [
                        *palette_address.add((packed_indices & 0xff) as usize),
                        *palette_address.add(((packed_indices >> 8) & 0xff) as usize),
                        *palette_address.add(((packed_indices >> 16) & 0xff) as usize),
                        *palette_address.add(((packed_indices >> 24) & 0xff) as usize),
                        *palette_address.add(((packed_indices >> 32) & 0xff) as usize),
                        *palette_address.add(((packed_indices >> 40) & 0xff) as usize),
                        *palette_address.add(((packed_indices >> 48) & 0xff) as usize),
                        *palette_address.add(((packed_indices >> 56) & 0xff) as usize),
                    ]
                };
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        colors.as_ptr(),
                        destination.add(pixel_index),
                        colors.len(),
                    );
                }
            }
            while pixel_index > 0 {
                pixel_index -= 1;
                let index = unsafe { usize::from(*indices_address.add(pixel_index)) };
                unsafe {
                    destination
                        .add(pixel_index)
                        .write(*palette_address.add(index));
                }
            }
        } else {
            for pixel_index in (0..canvas_pixels).rev() {
                let index = destination_bytes[pixel_index];
                let color = *palette
                    .get(usize::from(index))
                    .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
                unsafe {
                    destination.add(pixel_index).write(color);
                }
            }
        }
        layout
            .direct_mapped
            .store(true, std::sync::atomic::Ordering::Release);
        return Ok(Vec::new());
    }
    let indices = decode_frame_indices_with_scratches(data, frame, image_data, lzw_scratch)?;
    if !parallel_direct_mapping {
        return Ok(indices);
    }
    debug_assert!(frame_covers_canvas(frame, metadata));
    if frame
        .transparent_index
        .is_some_and(|transparent_index| indices.contains(&transparent_index))
    {
        return Ok(indices);
    }
    if indices.len() != canvas_pixels {
        return Err("Decoded full frame length does not match canvas".to_string());
    }
    let destination = unsafe {
        (output_address as *mut std::mem::MaybeUninit<u32>)
            .add(frame_index * canvas_pixels)
            .cast::<u32>()
    };
    if palette.len() == 256 {
        for (pixel_index, &index) in indices.iter().enumerate() {
            unsafe {
                destination
                    .add(pixel_index)
                    .write(*palette.get_unchecked(usize::from(index)));
            }
        }
    } else {
        for (pixel_index, &index) in indices.iter().enumerate() {
            let color = *palette
                .get(usize::from(index))
                .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
            unsafe {
                destination.add(pixel_index).write(color);
            }
        }
    }
    layout
        .direct_mapped
        .store(true, std::sync::atomic::Ordering::Release);
    Ok(Vec::new())
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn decode_pipeline_worker(
    data: &[u8],
    metadata: &GifMetadata,
    decoded: &[PipelineDecodedFrame],
    decoded_indices_address: usize,
    flat_decoded_indices: bool,
    palettes: &[Vec<u32>],
    palettes_share_table: bool,
    output_address: usize,
    canvas_pixels: usize,
    parallel_direct_mapping: bool,
    next_frame: &std::sync::atomic::AtomicUsize,
    assist_decode: bool,
) {
    let mut image_data = Vec::new();
    let mut lzw_scratch = LzwStackScratch::default();
    loop {
        let frame_index = next_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(frame) = metadata.frames.get(frame_index) else {
            return;
        };
        let layout = &decoded[frame_index];
        if assist_decode
            && layout
                .state
                .compare_exchange(
                    0,
                    1,
                    std::sync::atomic::Ordering::Acquire,
                    std::sync::atomic::Ordering::Relaxed,
                )
                .is_err()
        {
            continue;
        }
        let palette = &palettes[if palettes_share_table { 0 } else { frame_index }];
        let result = decode_pipeline_frame_result(
            data,
            metadata,
            frame_index,
            frame,
            layout,
            &mut image_data,
            &mut lzw_scratch,
            decoded_indices_address,
            flat_decoded_indices,
            palette,
            output_address,
            canvas_pixels,
            parallel_direct_mapping,
        );
        let _ = layout.result.set(result);
        if assist_decode {
            layout.state.store(2, std::sync::atomic::Ordering::Release);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn compose_pipeline_stripe(
    data: &[u8],
    metadata: &GifMetadata,
    decoded: &[PipelineDecodedFrame],
    decoded_indices_address: usize,
    flat_decoded_indices: bool,
    parallel_direct_mapping: bool,
    palettes: &[Vec<u32>],
    palettes_share_table: bool,
    canvas_width: usize,
    canvas_height: usize,
    canvas_pixels: usize,
    rows_per_thread: usize,
    thread_index: usize,
    output_address: usize,
    next_frame: &std::sync::atomic::AtomicUsize,
    assist_decode: bool,
    direct_overlay_output: bool,
) -> Result<(), String> {
    let row_start = thread_index * rows_per_thread;
    let row_end = (row_start + rows_per_thread).min(canvas_height);
    let stripe_pixels = (row_end - row_start) * canvas_width;
    let mut canvas = if direct_overlay_output {
        Vec::new()
    } else {
        vec![0u32; stripe_pixels]
    };
    let mut image_data = Vec::new();
    let mut lzw_scratch = LzwStackScratch::default();
    for (frame_index, frame) in metadata.frames.iter().enumerate() {
        let palette = &palettes[if palettes_share_table { 0 } else { frame_index }];
        let layout = &decoded[frame_index];
        let frame_result = if assist_decode {
            'frame_ready: loop {
                if let Some(result) = layout.result.get() {
                    break result;
                }
                if layout
                    .state
                    .compare_exchange(
                        0,
                        1,
                        std::sync::atomic::Ordering::Acquire,
                        std::sync::atomic::Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    let result = decode_pipeline_frame_result(
                        data,
                        metadata,
                        frame_index,
                        frame,
                        layout,
                        &mut image_data,
                        &mut lzw_scratch,
                        decoded_indices_address,
                        flat_decoded_indices,
                        palette,
                        output_address,
                        canvas_pixels,
                        parallel_direct_mapping,
                    );
                    let _ = layout.result.set(result);
                    layout.state.store(2, std::sync::atomic::Ordering::Release);
                    break layout.result.get().unwrap();
                }

                let background_index =
                    next_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if let Some(background_frame) = metadata.frames.get(background_index) {
                    let background_layout = &decoded[background_index];
                    if background_layout
                        .state
                        .compare_exchange(
                            0,
                            1,
                            std::sync::atomic::Ordering::Acquire,
                            std::sync::atomic::Ordering::Relaxed,
                        )
                        .is_ok()
                    {
                        let background_palette = &palettes[if palettes_share_table {
                            0
                        } else {
                            background_index
                        }];
                        let result = decode_pipeline_frame_result(
                            data,
                            metadata,
                            background_index,
                            background_frame,
                            background_layout,
                            &mut image_data,
                            &mut lzw_scratch,
                            decoded_indices_address,
                            flat_decoded_indices,
                            background_palette,
                            output_address,
                            canvas_pixels,
                            parallel_direct_mapping,
                        );
                        let _ = background_layout.result.set(result);
                        background_layout
                            .state
                            .store(2, std::sync::atomic::Ordering::Release);
                    }
                    continue;
                }

                let mut spins = 0usize;
                loop {
                    if let Some(result) = layout.result.get() {
                        break 'frame_ready result;
                    }
                    if spins < 64 {
                        std::hint::spin_loop();
                        spins += 1;
                    } else {
                        std::thread::yield_now();
                    }
                }
            }
        } else {
            let wait_for_result = || {
                let mut spins = 0usize;
                loop {
                    if let Some(result) = layout.result.get() {
                        return result;
                    }
                    if spins < 64 {
                        std::hint::spin_loop();
                        spins += 1;
                    } else {
                        std::thread::yield_now();
                    }
                }
            };
            if canvas_pixels >= 16_000 {
                match layout.result.get() {
                    Some(result) => result,
                    None => wait_for_result(),
                }
            } else {
                wait_for_result()
            }
        };
        let stored_indices = frame_result.as_ref().map_err(Clone::clone)?;
        if layout
            .direct_mapped
            .load(std::sync::atomic::Ordering::Acquire)
        {
            continue;
        }
        let indices = if flat_decoded_indices {
            unsafe {
                std::slice::from_raw_parts(
                    (decoded_indices_address as *const u8).add(layout.offset),
                    layout.length,
                )
            }
        } else {
            stored_indices
        };
        if direct_overlay_output {
            let frame_covers_canvas = frame_covers_canvas(frame, metadata);
            let source_start = row_start * canvas_width;
            let destination = unsafe {
                (output_address as *mut std::mem::MaybeUninit<u32>)
                    .add(frame_index * canvas_pixels + source_start)
                    .cast::<u32>()
            };
            let previous = (frame_index != 0).then(|| unsafe {
                (output_address as *const std::mem::MaybeUninit<u32>)
                    .add((frame_index - 1) * canvas_pixels + source_start)
                    .cast::<u32>()
            });
            if !frame_covers_canvas {
                unsafe {
                    if let Some(previous) = previous {
                        std::ptr::copy_nonoverlapping(previous, destination, stripe_pixels);
                    } else {
                        std::ptr::write_bytes(destination, 0, stripe_pixels);
                    }
                }
                let destination =
                    unsafe { std::slice::from_raw_parts_mut(destination, stripe_pixels) };
                blit_indices_to_canvas_stripe_u32(
                    palette,
                    canvas_width,
                    frame,
                    indices,
                    row_start,
                    row_end,
                    destination,
                )?;
                continue;
            }
            let source = indices
                .get(source_start..source_start + stripe_pixels)
                .ok_or_else(|| "Decoded index buffer is too short".to_string())?;
            match frame.transparent_index {
                None => {
                    for (offset, &index) in source.iter().enumerate() {
                        let color = *palette
                            .get(usize::from(index))
                            .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
                        unsafe {
                            destination.add(offset).write(color);
                        }
                    }
                }
                Some(transparent_index) => {
                    for (offset, &index) in source.iter().enumerate() {
                        let color = if index == transparent_index {
                            previous
                                .map(|previous| unsafe { *previous.add(offset) })
                                .unwrap_or(0)
                        } else {
                            *palette.get(usize::from(index)).ok_or_else(|| {
                                format!("Palette index {index} exceeds palette size")
                            })?
                        };
                        unsafe {
                            destination.add(offset).write(color);
                        }
                    }
                }
            }
            continue;
        }
        let restore = (frame.disposal == 3).then(|| canvas.clone());
        blit_indices_to_canvas_stripe_u32(
            palette,
            canvas_width,
            frame,
            indices,
            row_start,
            row_end,
            &mut canvas,
        )?;
        unsafe {
            let destination = (output_address as *mut std::mem::MaybeUninit<u32>)
                .add(frame_index * canvas_pixels + row_start * canvas_width)
                .cast::<u32>();
            std::ptr::copy_nonoverlapping(canvas.as_ptr(), destination, stripe_pixels);
        }
        if frame.disposal == 2 {
            clear_frame_rect_stripe_u32(&mut canvas, canvas_width, frame, row_start, row_end);
        } else if frame.disposal == 3 {
            canvas = restore.expect("restore canvas exists");
        }
    }
    Ok(())
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
struct DecodePipelineDispatchContext<'a> {
    data: &'a [u8],
    metadata: &'a GifMetadata,
    decoded: &'a [PipelineDecodedFrame],
    decoded_indices_address: usize,
    flat_decoded_indices: bool,
    parallel_direct_mapping: bool,
    palettes: &'a [Vec<u32>],
    palettes_share_table: bool,
    canvas_width: usize,
    canvas_height: usize,
    canvas_pixels: usize,
    rows_per_thread: usize,
    decode_thread_count: usize,
    output_address: usize,
    next_frame: &'a std::sync::atomic::AtomicUsize,
    assist_decode: bool,
    direct_overlay_output: bool,
    error: std::sync::Mutex<Option<String>>,
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
unsafe extern "C" fn decode_pipeline_dispatch_worker(
    context: *mut std::ffi::c_void,
    iteration: usize,
) {
    let context = &*(context.cast::<DecodePipelineDispatchContext<'_>>());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if iteration < context.decode_thread_count {
            decode_pipeline_worker(
                context.data,
                context.metadata,
                context.decoded,
                context.decoded_indices_address,
                context.flat_decoded_indices,
                context.palettes,
                context.palettes_share_table,
                context.output_address,
                context.canvas_pixels,
                context.parallel_direct_mapping,
                context.next_frame,
                context.assist_decode,
            );
            Ok(())
        } else {
            compose_pipeline_stripe(
                context.data,
                context.metadata,
                context.decoded,
                context.decoded_indices_address,
                context.flat_decoded_indices,
                context.parallel_direct_mapping,
                context.palettes,
                context.palettes_share_table,
                context.canvas_width,
                context.canvas_height,
                context.canvas_pixels,
                context.rows_per_thread,
                iteration - context.decode_thread_count,
                context.output_address,
                context.next_frame,
                context.assist_decode,
                context.direct_overlay_output,
            )
        }
    }));
    let error = match result {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error,
        Err(_) => "Parallel GIF pipeline panicked".to_string(),
    };
    if let Ok(mut stored_error) = context.error.lock() {
        if stored_error.is_none() {
            *stored_error = Some(error);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_and_compose_pipeline_native(
    data: &[u8],
    metadata: &GifMetadata,
) -> Result<Vec<u32>, String> {
    let canvas_width = usize::from(metadata.width);
    let canvas_height = usize::from(metadata.height);
    let canvas_pixels = canvas_width
        .checked_mul(canvas_height)
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_pixels = metadata
        .frames
        .len()
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    let mut output = Vec::<std::mem::MaybeUninit<u32>>::with_capacity(output_pixels);
    unsafe {
        output.set_len(output_pixels);
    }
    decode_and_compose_pipeline_into_native(data, metadata, &mut output)?;
    let pointer = output.as_mut_ptr().cast::<u32>();
    let length = output.len();
    let capacity = output.capacity();
    std::mem::forget(output);
    Ok(unsafe { Vec::from_raw_parts(pointer, length, capacity) })
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_and_compose_pipeline_into_native(
    data: &[u8],
    metadata: &GifMetadata,
    output: &mut [std::mem::MaybeUninit<u32>],
) -> Result<(), String> {
    let canvas_width = usize::from(metadata.width);
    let canvas_height = usize::from(metadata.height);
    let canvas_pixels = canvas_width
        .checked_mul(canvas_height)
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_pixels = metadata
        .frames
        .len()
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    if output.len() != output_pixels {
        return Err("Host output buffer has the wrong size".to_string());
    }
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let assist_decode = (32..=128).contains(&metadata.frames.len()) && canvas_pixels >= 40_000;
    let decode_thread_count = if assist_decode || canvas_pixels < 10_000 {
        available_threads * 2 / 3
    } else {
        available_threads / 2
    };
    let compose_thread_count = available_threads - decode_thread_count;
    let direct_overlay_output = metadata
        .frames
        .iter()
        .all(|frame| frame.disposal <= 1 && frame_covers_canvas(frame, metadata));
    let rows_per_thread = canvas_height.div_ceil(compose_thread_count);
    let next_frame = std::sync::atomic::AtomicUsize::new(0);
    let mut decoded_indices_length = 0usize;
    let decoded: Vec<PipelineDecodedFrame> = metadata
        .frames
        .iter()
        .map(|frame| {
            let length = usize::from(frame.width)
                .checked_mul(usize::from(frame.height))
                .ok_or_else(|| "Decoded frame size overflow".to_string())?;
            let offset = decoded_indices_length;
            decoded_indices_length = decoded_indices_length
                .checked_add(length)
                .ok_or_else(|| "Decoded frame stream overflow".to_string())?;
            Ok(PipelineDecodedFrame {
                state: std::sync::atomic::AtomicU8::new(0),
                result: std::sync::OnceLock::new(),
                direct_mapped: std::sync::atomic::AtomicBool::new(false),
                offset,
                length,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let flat_decoded_indices = metadata.frames.len() >= 128 && canvas_pixels < 10_000;
    let mut decoded_indices = if flat_decoded_indices {
        let mut storage = Vec::<std::mem::MaybeUninit<u8>>::with_capacity(decoded_indices_length);
        unsafe {
            storage.set_len(decoded_indices_length);
        }
        storage
    } else {
        Vec::new()
    };
    let decoded_indices_address = if flat_decoded_indices {
        decoded_indices.as_mut_ptr() as usize
    } else {
        0
    };
    let palettes_share_table = metadata.frames.first().is_some_and(|first| {
        metadata.frames.iter().all(|frame| {
            frame.palette_offset == first.palette_offset && frame.palette_size == first.palette_size
        })
    });
    let palettes = if palettes_share_table {
        vec![build_palette_u32(
            data,
            &metadata.frames[0],
            PixelFormat::Rgba,
        )?]
    } else {
        metadata
            .frames
            .iter()
            .map(|frame| build_palette_u32(data, frame, PixelFormat::Rgba))
            .collect::<Result<Vec<_>, _>>()?
    };
    let output_address = output.as_mut_ptr() as usize;
    let parallel_direct_mapping = direct_overlay_output && metadata.frames.len() >= 100;

    #[cfg(target_os = "macos")]
    {
        let mut context = DecodePipelineDispatchContext {
            data,
            metadata,
            decoded: &decoded,
            decoded_indices_address,
            flat_decoded_indices,
            parallel_direct_mapping,
            palettes: &palettes,
            palettes_share_table,
            canvas_width,
            canvas_height,
            canvas_pixels,
            rows_per_thread,
            decode_thread_count,
            output_address,
            next_frame: &next_frame,
            assist_decode,
            direct_overlay_output,
            error: std::sync::Mutex::new(None),
        };
        unsafe {
            let queue = dispatch_get_global_queue(0, 0);
            if queue.is_null() {
                return Err("Could not acquire the system worker queue".to_string());
            }
            dispatch_apply_f(
                available_threads,
                queue,
                (&mut context as *mut DecodePipelineDispatchContext<'_>).cast(),
                decode_pipeline_dispatch_worker,
            );
        }
        if let Some(error) = context
            .error
            .into_inner()
            .map_err(|_| "Parallel GIF pipeline error lock was poisoned".to_string())?
        {
            return Err(error);
        }
    }

    #[cfg(not(target_os = "macos"))]
    std::thread::scope(|scope| {
        let decode_handles: Vec<_> = (0..decode_thread_count)
            .map(|_| {
                scope.spawn(|| {
                    decode_pipeline_worker(
                        data,
                        metadata,
                        &decoded,
                        decoded_indices_address,
                        flat_decoded_indices,
                        &palettes,
                        palettes_share_table,
                        output_address,
                        canvas_pixels,
                        parallel_direct_mapping,
                        &next_frame,
                        assist_decode,
                    )
                })
            })
            .collect();
        let compose_handles: Vec<_> = (0..compose_thread_count)
            .map(|thread_index| {
                let decoded = &decoded;
                let palettes = &palettes;
                let next_frame = &next_frame;
                scope.spawn(move || {
                    compose_pipeline_stripe(
                        data,
                        metadata,
                        decoded,
                        decoded_indices_address,
                        flat_decoded_indices,
                        parallel_direct_mapping,
                        palettes,
                        palettes_share_table,
                        canvas_width,
                        canvas_height,
                        canvas_pixels,
                        rows_per_thread,
                        thread_index,
                        output_address,
                        next_frame,
                        assist_decode,
                        direct_overlay_output,
                    )
                })
            })
            .collect();
        for handle in decode_handles {
            handle
                .join()
                .map_err(|_| "Parallel GIF decoder panicked".to_string())?;
        }
        for handle in compose_handles {
            handle
                .join()
                .map_err(|_| "Parallel GIF compositor panicked".to_string())??;
        }
        Ok::<_, String>(())
    })?;

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_small_independent_frames_into_native(
    data: &[u8],
    metadata: &GifMetadata,
    canvas_pixels: usize,
    output: &mut [std::mem::MaybeUninit<u32>],
) -> Result<(), String> {
    let output_pixels = metadata
        .frames
        .len()
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    if output.len() != output_pixels {
        return Err("Host output buffer has the wrong size".to_string());
    }
    let mut image_data = Vec::new();
    let mut fixed_image_data = [std::mem::MaybeUninit::<u8>::uninit(); 512];
    let mut lzw_scratch = LzwStackScratch::default();
    let mut indices = Vec::new();
    for (frame_index, frame) in metadata.frames.iter().enumerate() {
        let palette = build_palette_u32(data, frame, PixelFormat::Rgba)?;
        let destination =
            &mut output[frame_index * canvas_pixels..(frame_index + 1) * canvas_pixels];
        if frame_covers_canvas(frame, metadata) && !frame.interlaced {
            let image_bytes = if frame.data_length <= fixed_image_data.len() {
                collect_image_data_into_fixed(data, frame.data_offset, &mut fixed_image_data)?
            } else {
                if image_data.capacity() < frame.data_length {
                    image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
                }
                collect_image_data_into(data, frame.data_offset, &mut image_data)?;
                &image_data
            };
            let destination_bytes = unsafe {
                std::slice::from_raw_parts_mut(destination.as_mut_ptr().cast::<u8>(), canvas_pixels)
            };
            destination_bytes.fill(0);
            if should_decode_lzw_direct(frame, canvas_pixels) {
                lzw_decode_to_indices_direct_with_scratch(
                    frame.min_code_size,
                    image_bytes,
                    destination_bytes,
                    &mut lzw_scratch,
                )?;
            } else {
                lzw_decode_to_indices_stack_with_scratch(
                    frame.min_code_size,
                    image_bytes,
                    destination_bytes,
                    &mut lzw_scratch,
                )?;
            }
            for pixel_index in (0..canvas_pixels).rev() {
                let index = usize::from(destination_bytes[pixel_index]);
                let color = if frame.transparent_index == Some(index as u8) {
                    0
                } else {
                    *palette
                        .get(index)
                        .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?
                };
                destination[pixel_index].write(color);
            }
            continue;
        }
        if !frame.interlaced && frame.data_length <= fixed_image_data.len() {
            let image_bytes =
                collect_image_data_into_fixed(data, frame.data_offset, &mut fixed_image_data)?;
            let frame_size = usize::from(frame.width) * usize::from(frame.height);
            indices.resize(frame_size, 0);
            indices.fill(0);
            if should_decode_lzw_direct(frame, frame_size) {
                lzw_decode_to_indices_direct_with_scratch(
                    frame.min_code_size,
                    image_bytes,
                    &mut indices,
                    &mut lzw_scratch,
                )?;
            } else {
                lzw_decode_to_indices_stack_with_scratch(
                    frame.min_code_size,
                    image_bytes,
                    &mut indices,
                    &mut lzw_scratch,
                )?;
            }
        } else {
            decode_frame_indices_reusing_output(
                data,
                frame,
                &mut image_data,
                &mut lzw_scratch,
                &mut indices,
            )?;
        }
        if !frame_covers_canvas(frame, metadata) {
            destination.fill(std::mem::MaybeUninit::new(0));
            let destination = unsafe {
                std::slice::from_raw_parts_mut(
                    destination.as_mut_ptr().cast::<u32>(),
                    destination.len(),
                )
            };
            blit_indices_to_canvas_u32(&palette, metadata.width, frame, &indices, destination)?;
            continue;
        }
        match frame.transparent_index {
            None => {
                for (pixel, &index) in destination.iter_mut().zip(&indices) {
                    pixel.write(
                        *palette
                            .get(usize::from(index))
                            .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?,
                    );
                }
            }
            Some(transparent_index) => {
                for (pixel, &index) in destination.iter_mut().zip(&indices) {
                    pixel.write(if index == transparent_index {
                        0
                    } else {
                        *palette
                            .get(usize::from(index))
                            .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_independent_frames_native(
    data: &[u8],
    metadata: &GifMetadata,
) -> Result<Vec<u32>, String> {
    let canvas_pixels = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_pixels = metadata
        .frames
        .len()
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    if output_pixels < 50_000 {
        let mut output = Vec::<std::mem::MaybeUninit<u32>>::with_capacity(output_pixels);
        unsafe {
            output.set_len(output_pixels);
        }
        decode_small_independent_frames_into_native(data, metadata, canvas_pixels, &mut output)?;
        let pointer = output.as_mut_ptr().cast::<u32>();
        let length = output.len();
        let capacity = output.capacity();
        std::mem::forget(output);
        return Ok(unsafe { Vec::from_raw_parts(pointer, length, capacity) });
    }
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let thread_count = available_threads.min(metadata.frames.len());
    let mut output = Vec::<std::mem::MaybeUninit<u32>>::with_capacity(output_pixels);
    unsafe {
        output.set_len(output_pixels);
    }
    let output_address = output.as_mut_ptr() as usize;
    let next_frame = std::sync::atomic::AtomicUsize::new(0);

    let worker = || {
        let mut image_data = Vec::new();
        loop {
            let frame_index = next_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let Some(frame) = metadata.frames.get(frame_index) else {
                break;
            };
            let palette = build_palette_u32(data, frame, PixelFormat::Rgba)?;
            let destination = unsafe {
                (output_address as *mut std::mem::MaybeUninit<u32>).add(frame_index * canvas_pixels)
            };
            if frame.interlaced
                || canvas_pixels < 100_000
                || frame.transparent_index.is_some()
                || !frame_covers_canvas(frame, metadata)
            {
                let indices =
                    decode_frame_indices_with_image_scratch(data, frame, &mut image_data)?;
                let destination =
                    unsafe { std::slice::from_raw_parts_mut(destination, canvas_pixels) };
                if !frame_covers_canvas(frame, metadata) {
                    destination.fill(std::mem::MaybeUninit::new(0));
                    let destination = unsafe {
                        std::slice::from_raw_parts_mut(
                            destination.as_mut_ptr().cast::<u32>(),
                            destination.len(),
                        )
                    };
                    blit_indices_to_canvas_u32(
                        &palette,
                        metadata.width,
                        frame,
                        &indices,
                        destination,
                    )?;
                    continue;
                }
                match frame.transparent_index {
                    None => {
                        for (pixel, index) in destination.iter_mut().zip(indices) {
                            pixel.write(*palette.get(usize::from(index)).ok_or_else(|| {
                                format!("Palette index {index} exceeds palette size")
                            })?);
                        }
                    }
                    Some(transparent_index) => {
                        for (pixel, index) in destination.iter_mut().zip(indices) {
                            let color = if index == transparent_index {
                                0
                            } else {
                                *palette.get(usize::from(index)).ok_or_else(|| {
                                    format!("Palette index {index} exceeds palette size")
                                })?
                            };
                            pixel.write(color);
                        }
                    }
                }
                continue;
            }

            if image_data.capacity() < frame.data_length {
                image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
            }
            collect_image_data_into(data, frame.data_offset, &mut image_data)?;
            let destination_bytes =
                unsafe { std::slice::from_raw_parts_mut(destination.cast::<u8>(), canvas_pixels) };
            lzw_decode_to_indices_direct(frame.min_code_size, &image_data, destination_bytes)?;
            if palette.len() == 256 {
                let palette_address = palette.as_ptr();
                let indices_address = destination.cast::<u8>();
                let mut pixel_index = canvas_pixels;
                while pixel_index >= 8 {
                    pixel_index -= 8;
                    let packed_indices = unsafe {
                        std::ptr::read_unaligned(indices_address.add(pixel_index).cast::<u64>())
                    };
                    let colors = unsafe {
                        [
                            *palette_address.add((packed_indices & 0xff) as usize),
                            *palette_address.add(((packed_indices >> 8) & 0xff) as usize),
                            *palette_address.add(((packed_indices >> 16) & 0xff) as usize),
                            *palette_address.add(((packed_indices >> 24) & 0xff) as usize),
                            *palette_address.add(((packed_indices >> 32) & 0xff) as usize),
                            *palette_address.add(((packed_indices >> 40) & 0xff) as usize),
                            *palette_address.add(((packed_indices >> 48) & 0xff) as usize),
                            *palette_address.add(((packed_indices >> 56) & 0xff) as usize),
                        ]
                    };
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            colors.as_ptr(),
                            destination.add(pixel_index).cast::<u32>(),
                            colors.len(),
                        );
                    }
                }
                while pixel_index > 0 {
                    pixel_index -= 1;
                    let index = unsafe { usize::from(*indices_address.add(pixel_index)) };
                    let color = unsafe { *palette_address.add(index) };
                    unsafe {
                        destination
                            .add(pixel_index)
                            .write(std::mem::MaybeUninit::new(color));
                    }
                }
                continue;
            }

            for pixel_index in (0..canvas_pixels).rev() {
                let index = unsafe { *destination.cast::<u8>().add(pixel_index) };
                let color = *palette
                    .get(usize::from(index))
                    .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
                unsafe {
                    destination
                        .add(pixel_index)
                        .write(std::mem::MaybeUninit::new(color));
                }
            }
        }
        Ok::<_, String>(())
    };
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..thread_count).map(|_| scope.spawn(&worker)).collect();
        for handle in handles {
            handle
                .join()
                .map_err(|_| "Parallel GIF decoder panicked".to_string())??;
        }
        Ok::<_, String>(())
    })?;
    let pointer = output.as_mut_ptr().cast::<u32>();
    let length = output.len();
    let capacity = output.capacity();
    std::mem::forget(output);
    Ok(unsafe { Vec::from_raw_parts(pointer, length, capacity) })
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_indices_worker(
    data: &[u8],
    metadata: &GifMetadata,
    results_address: usize,
    next_frame: &std::sync::atomic::AtomicUsize,
) {
    loop {
        let frame_index = next_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(frame) = metadata.frames.get(frame_index) else {
            return;
        };
        unsafe {
            (results_address as *mut std::mem::MaybeUninit<Result<Vec<u8>, String>>)
                .add(frame_index)
                .write(std::mem::MaybeUninit::new(decode_frame_indices_inner(
                    data, frame,
                )));
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
struct DecodeIndicesDispatchContext<'a> {
    data: &'a [u8],
    metadata: &'a GifMetadata,
    results_address: usize,
    next_frame: std::sync::atomic::AtomicUsize,
    panicked: std::sync::atomic::AtomicBool,
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
unsafe extern "C" fn decode_indices_dispatch_worker(
    context: *mut std::ffi::c_void,
    _iteration: usize,
) {
    let context = &*(context.cast::<DecodeIndicesDispatchContext<'_>>());
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        decode_indices_worker(
            context.data,
            context.metadata,
            context.results_address,
            &context.next_frame,
        );
    }))
    .is_err()
    {
        context
            .panicked
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_frame_indices_parallel_native(
    data: &[u8],
    metadata: &GifMetadata,
) -> Result<Vec<Vec<u8>>, String> {
    let total_frame_pixels = metadata.frames.iter().try_fold(0usize, |total, frame| {
        usize::from(frame.width)
            .checked_mul(usize::from(frame.height))
            .and_then(|pixels| total.checked_add(pixels))
            .ok_or_else(|| "Decoded frame size overflow".to_string())
    })?;
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let thread_cap = if total_frame_pixels < 100_000 {
        6
    } else {
        usize::MAX
    };
    let thread_count = available_threads.min(metadata.frames.len()).min(thread_cap);
    if thread_count <= 1 || metadata.frames.len() < 8 || total_frame_pixels < 30_000 {
        let mut image_data = Vec::new();
        let mut decoded = Vec::with_capacity(metadata.frames.len());
        for frame in &metadata.frames {
            decoded.push(decode_frame_indices_with_image_scratch(
                data,
                frame,
                &mut image_data,
            )?);
        }
        return Ok(decoded);
    }

    let mut results =
        Vec::<std::mem::MaybeUninit<Result<Vec<u8>, String>>>::with_capacity(metadata.frames.len());
    unsafe {
        results.set_len(metadata.frames.len());
    }
    let results_address = results.as_mut_ptr() as usize;
    let next_frame = std::sync::atomic::AtomicUsize::new(0);
    #[cfg(target_os = "macos")]
    let joined = {
        let mut context = DecodeIndicesDispatchContext {
            data,
            metadata,
            results_address,
            next_frame,
            panicked: std::sync::atomic::AtomicBool::new(false),
        };
        unsafe {
            let queue = dispatch_get_global_queue(0, 0);
            if queue.is_null() {
                std::mem::forget(results);
                return Err("Could not acquire the system worker queue".to_string());
            }
            dispatch_apply_f(
                thread_count,
                queue,
                (&mut context as *mut DecodeIndicesDispatchContext<'_>).cast(),
                decode_indices_dispatch_worker,
            );
        }
        !context.panicked.load(std::sync::atomic::Ordering::Relaxed)
    };
    #[cfg(not(target_os = "macos"))]
    let joined = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..thread_count)
            .map(|_| {
                let next_frame = &next_frame;
                scope.spawn(move || {
                    decode_indices_worker(data, metadata, results_address, next_frame)
                })
            })
            .collect();
        handles.into_iter().all(|handle| handle.join().is_ok())
    });
    if !joined {
        std::mem::forget(results);
        return Err("Parallel GIF decoder panicked".to_string());
    }

    let pointer = results.as_mut_ptr().cast::<Result<Vec<u8>, String>>();
    let length = results.len();
    let capacity = results.capacity();
    std::mem::forget(results);
    unsafe { Vec::from_raw_parts(pointer, length, capacity) }
        .into_iter()
        .collect()
}

fn prepare_composited_delta_frames_inner(
    data: &[u8],
    metadata: &GifMetadata,
    requested_frames: &[u8],
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    if requested_frames.len() > metadata.frames.len() {
        return Err("Requested frame flags exceed frame count".to_string());
    }

    let canvas_width = usize::from(metadata.width);
    let canvas_height = usize::from(metadata.height);
    let canvas_pixels = canvas_width
        .checked_mul(canvas_height)
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let requested_count = requested_frames.iter().filter(|flag| **flag != 0).count();
    let table_len = COMPOSITED_DELTA_ENTRY_LEN
        .checked_mul(requested_count)
        .and_then(|len| len.checked_add(COMPOSITED_DELTA_HEADER_LEN))
        .ok_or_else(|| "Prepared frame table overflow".to_string())?;
    let output_pixels = requested_count
        .checked_mul(canvas_pixels)
        .and_then(|len| len.checked_add(table_len))
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    let mut output = vec![0u32; table_len];
    output[0] = COMPOSITED_DELTA_MAGIC;
    output[1] = COMPOSITED_DELTA_VERSION;
    output[2] = requested_count as u32;
    output[3] = canvas_pixels as u32;
    output.reserve(output_pixels.saturating_sub(table_len));

    let mut canvas = vec![0u32; canvas_pixels];
    let mut previous_requested_canvas: Option<Vec<u32>> = None;
    let mut output_frame_index = 0usize;

    for (frame_index, requested) in requested_frames.iter().enumerate() {
        let frame = metadata
            .frames
            .get(frame_index)
            .ok_or_else(|| "Frame index out of range".to_string())?;
        let restore = (frame.disposal == 3).then(|| canvas.clone());
        let palette = build_palette_u32(data, frame, format)?;
        let indices = decode_frame_indices_inner(data, frame)?;

        blit_indices_to_canvas_u32(&palette, metadata.width, frame, &indices, &mut canvas)?;

        if *requested != 0 {
            let entry =
                COMPOSITED_DELTA_HEADER_LEN + output_frame_index * COMPOSITED_DELTA_ENTRY_LEN;
            let full_start = output.len();
            output.extend_from_slice(&canvas);
            let changed_rect = previous_requested_canvas.as_deref().and_then(|previous| {
                find_changed_rect_u32(previous, &canvas, canvas_width, canvas_height)
            });
            let delta_start = output.len();
            let delta_len = if let Some(rect) = changed_rect {
                push_rect_pixels_u32(&canvas, canvas_width, rect, &mut output);
                rect.width * rect.height
            } else {
                0
            };

            output[entry] = frame_index as u32;
            output[entry + 1] = full_start as u32;
            output[entry + 2] = canvas_pixels as u32;
            if let Some(rect) = changed_rect {
                output[entry + 3] = rect.x as u32;
                output[entry + 4] = rect.y as u32;
                output[entry + 5] = rect.width as u32;
                output[entry + 6] = rect.height as u32;
            }
            output[entry + 7] = delta_start as u32;
            output[entry + 8] = delta_len as u32;

            match &mut previous_requested_canvas {
                Some(previous) => previous.copy_from_slice(&canvas),
                None => previous_requested_canvas = Some(canvas.clone()),
            }
            output_frame_index += 1;
        }

        apply_frame_disposal_u32(&mut canvas, metadata.width, metadata.height, frame, restore);
    }

    Ok(output)
}

#[derive(Clone, Copy)]
struct ChangedRectU32 {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

fn find_changed_rect_u32(
    previous: &[u32],
    current: &[u32],
    width: usize,
    height: usize,
) -> Option<ChangedRectU32> {
    let mut top = 0usize;
    let mut bottom = height.saturating_sub(1);

    while top < height {
        let row = top * width;
        if previous[row..row + width] != current[row..row + width] {
            break;
        }
        top += 1;
    }

    if top == height {
        return None;
    }

    while bottom > top {
        let row = bottom * width;
        if previous[row..row + width] != current[row..row + width] {
            break;
        }
        bottom -= 1;
    }

    let mut left = width - 1;
    let mut right = 0usize;
    for y in top..=bottom {
        let row = y * width;
        for x in 0..width {
            if previous[row + x] != current[row + x] {
                left = left.min(x);
                right = right.max(x);
            }
        }
    }

    Some(ChangedRectU32 {
        x: left,
        y: top,
        width: right - left + 1,
        height: bottom - top + 1,
    })
}

fn find_changed_rect_u8(
    previous: &[u8],
    current: &[u8],
    width: usize,
    height: usize,
) -> Option<ChangedRectU32> {
    let mut top = 0usize;
    let mut bottom = height.saturating_sub(1);

    while top < height {
        let row = top * width;
        if previous[row..row + width] != current[row..row + width] {
            break;
        }
        top += 1;
    }

    if top == height {
        return None;
    }

    while bottom > top {
        let row = bottom * width;
        if previous[row..row + width] != current[row..row + width] {
            break;
        }
        bottom -= 1;
    }

    let mut left = width - 1;
    let mut right = 0usize;
    for y in top..=bottom {
        let row = y * width;
        for x in 0..width {
            if previous[row + x] != current[row + x] {
                left = left.min(x);
                right = right.max(x);
            }
        }
    }

    Some(ChangedRectU32 {
        x: left,
        y: top,
        width: right - left + 1,
        height: bottom - top + 1,
    })
}

fn find_changed_rect_rgba_rgb(
    previous: &[u8],
    current: &[u8],
    width: usize,
    height: usize,
) -> Option<ChangedRectU32> {
    let mut top = 0usize;
    let mut bottom = height.saturating_sub(1);

    while top < height {
        if rgba_rgb_row_changed(previous, current, top, width) {
            break;
        }
        top += 1;
    }

    if top == height {
        return None;
    }

    while bottom > top {
        if rgba_rgb_row_changed(previous, current, bottom, width) {
            break;
        }
        bottom -= 1;
    }

    let mut left = width - 1;
    let mut right = 0usize;
    for y in top..=bottom {
        let row = y * width * 4;
        for x in 0..width {
            let offset = row + x * 4;
            if rgba_rgb_pixel_changed(previous, current, offset) {
                left = left.min(x);
                right = right.max(x);
            }
        }
    }

    Some(ChangedRectU32 {
        x: left,
        y: top,
        width: right - left + 1,
        height: bottom - top + 1,
    })
}

fn rgba_rgb_row_changed(previous: &[u8], current: &[u8], y: usize, width: usize) -> bool {
    let row = y * width * 4;
    for x in 0..width {
        let offset = row + x * 4;
        if rgba_rgb_pixel_changed(previous, current, offset) {
            return true;
        }
    }
    false
}

#[inline]
fn rgba_rgb_pixel_changed(previous: &[u8], current: &[u8], offset: usize) -> bool {
    // Wasm memory is little-endian, so RGBA bytes become 0xAABBGGRR.
    // Masking 0x00ff_ffff compares RGB and deliberately ignores alpha.
    let previous_pixel =
        unsafe { std::ptr::read_unaligned(previous.as_ptr().add(offset) as *const u32) };
    let current_pixel =
        unsafe { std::ptr::read_unaligned(current.as_ptr().add(offset) as *const u32) };
    ((previous_pixel ^ current_pixel) & 0x00ff_ffff) != 0
}

fn push_rect_pixels_u32(
    source: &[u32],
    source_width: usize,
    rect: ChangedRectU32,
    output: &mut Vec<u32>,
) {
    for y in 0..rect.height {
        let start = (rect.y + y) * source_width + rect.x;
        output.extend_from_slice(&source[start..start + rect.width]);
    }
}

#[derive(Default)]
struct ColorIndexTable {
    keys: Vec<i32>,
    values: Vec<u8>,
    mask: usize,
}

impl ColorIndexTable {
    fn empty() -> Self {
        Self {
            keys: Vec::new(),
            values: Vec::new(),
            mask: 0,
        }
    }

    fn new(capacity: usize) -> Self {
        debug_assert!(capacity.is_power_of_two());
        Self {
            keys: vec![-1; capacity],
            values: vec![0; capacity],
            mask: capacity - 1,
        }
    }

    fn reset(&mut self, capacity: usize) {
        debug_assert!(capacity.is_power_of_two());
        if self.keys.len() != capacity {
            self.keys.resize(capacity, -1);
            self.values.resize(capacity, 0);
        } else {
            self.keys.fill(-1);
            self.values.fill(0);
        }
        self.mask = capacity - 1;
    }

    #[inline(always)]
    fn get(&self, key: u32) -> Option<u8> {
        let key = key as i32;
        let mask = self.mask;
        let mut slot = (key as usize).wrapping_mul(2_654_435_761) & mask;
        loop {
            let stored = unsafe { *self.keys.get_unchecked(slot) };
            if stored == key {
                return Some(unsafe { *self.values.get_unchecked(slot) });
            }
            if stored == -1 {
                return None;
            }
            slot = (slot + 1) & mask;
        }
    }

    #[inline(always)]
    fn insert_if_absent(&mut self, key: u32, value: u8) {
        let key = key as i32;
        let mask = self.mask;
        let mut slot = (key as usize).wrapping_mul(2_654_435_761) & mask;
        loop {
            let stored = unsafe { *self.keys.get_unchecked(slot) };
            if stored == key {
                return;
            }
            if stored == -1 {
                unsafe {
                    *self.keys.get_unchecked_mut(slot) = key;
                    *self.values.get_unchecked_mut(slot) = value;
                }
                return;
            }
            slot = (slot + 1) & mask;
        }
    }
}

fn take_quality_color_index_table(capacity: usize) -> ColorIndexTable {
    REUSABLE_QUALITY_COLOR_INDEX.with(|scratch| {
        let mut table = std::mem::take(&mut *scratch.borrow_mut());
        table.reset(capacity);
        table
    })
}

fn recycle_quality_color_index_table(table: ColorIndexTable) {
    REUSABLE_QUALITY_COLOR_INDEX.with(|scratch| {
        *scratch.borrow_mut() = table;
    });
}

struct PaletteMapper<'a> {
    palette_rgb: &'a [u32],
    exact: ColorIndexTable,
}

impl<'a> PaletteMapper<'a> {
    fn new(palette_rgb: &'a [u32]) -> Self {
        let mut exact = ColorIndexTable::new(if palette_rgb.len() <= 4 {
            8
        } else if palette_rgb.len() <= 16 {
            32
        } else {
            COLOR_INDEX_CAP
        });
        for (index, color) in palette_rgb.iter().enumerate() {
            exact.insert_if_absent(color & 0x00ff_ffff, index as u8);
        }
        Self { palette_rgb, exact }
    }

    #[inline]
    fn exact_index(&self, r: u8, g: u8, b: u8) -> Option<u8> {
        self.exact.get(rgb_key(r, g, b))
    }

    #[inline]
    fn index_pixel(&self, r: u8, g: u8, b: u8) -> u8 {
        let rgb = rgb_key(r, g, b);
        self.exact
            .get(rgb)
            .unwrap_or_else(|| nearest_palette_index(r, g, b, self.palette_rgb))
    }
}

fn encode_rgba_gif_advanced_inner(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    deltas: bool,
    alpha_threshold: u8,
    literal: bool,
    quantization: RgbaQuantization,
    palette_mode: RgbaPaletteMode,
) -> Result<Vec<u8>, String> {
    encode_rgba_gif_advanced_inner_with_output(
        rgba_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        delays,
        loop_count,
        deltas,
        alpha_threshold,
        literal,
        quantization,
        palette_mode,
        Vec::new(),
    )
}

fn encode_rgba_gif_advanced_inner_with_output(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    deltas: bool,
    alpha_threshold: u8,
    literal: bool,
    quantization: RgbaQuantization,
    palette_mode: RgbaPaletteMode,
    output: Vec<u8>,
) -> Result<Vec<u8>, String> {
    validate_rgba_stream(rgba_stream, width, height, frame_count, delays, loop_count)?;

    if palette_mode == RgbaPaletteMode::Local {
        if !palette_rgb.is_empty() {
            return Err(
                "Local palette mode cannot use a caller-supplied global palette".to_string(),
            );
        }
        if deltas {
            return Err("Local palette mode does not support delta frames".to_string());
        }
        return encode_rgba_local_palette_gif_inner(
            rgba_stream,
            width,
            height,
            frame_count,
            delays,
            loop_count,
            alpha_threshold,
            literal,
            quantization,
        );
    }

    let (palette, indexed, transparent_index) = index_rgba_frames_with_quantization(
        rgba_stream,
        palette_rgb,
        alpha_threshold,
        quantization,
    )?;
    if literal && !deltas {
        let encoded = encode_indexed_literal_gif_inner_with_output(
            output,
            &indexed,
            width,
            height,
            frame_count,
            &palette,
            delays,
            loop_count,
            transparent_index,
        );
        recycle_quantized_indexed(indexed);
        return encoded;
    }
    let encoded = encode_indexed_gif_inner_with_rects(
        &indexed,
        width,
        height,
        frame_count,
        &palette,
        delays,
        loop_count,
        deltas,
        transparent_index,
        literal,
    );
    recycle_quantized_indexed(indexed);
    encoded
}

fn encode_rgba_quality_gif_inner_with_output(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    delays: &[u16],
    loop_count: i32,
    alpha_threshold: u8,
    output: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let delays = DelaySource::PerFrame(delays);
    validate_rgba_stream(rgba_stream, width, height, frame_count, delays, loop_count)?;
    match index_rgba_frames_quality_result(rgba_stream, alpha_threshold) {
        QualityIndexResult::Exact((palette, indexed, transparent_index)) => {
            let encoded = encode_indexed_literal_gif_inner_with_output(
                output,
                &indexed,
                width,
                height,
                frame_count,
                &palette,
                delays,
                loop_count,
                transparent_index,
            );
            recycle_quantized_indexed(indexed);
            encoded
        }
        QualityIndexResult::Quantized(plan) => encode_quality_index_plan_literal_gif(
            output,
            rgba_stream,
            width,
            height,
            frame_count,
            delays,
            loop_count,
            alpha_threshold,
            plan,
        ),
    }
}

fn encode_quality_index_plan_literal_gif(
    mut output: Vec<u8>,
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    delays: DelaySource<'_>,
    loop_count: i32,
    alpha_threshold: u8,
    plan: QualityIndexPlan,
) -> Result<Vec<u8>, String> {
    let QualityIndexPlan {
        palette,
        histogram_to_palette,
        transparent_index,
        mapping_bits,
    } = plan;
    let color_count = checked_palette_color_count(palette.len())?;
    if color_count != 256 {
        let (palette, indexed, transparent_index) = QualityIndexPlan {
            palette,
            histogram_to_palette,
            transparent_index,
            mapping_bits,
        }
        .into_indexed(rgba_stream, alpha_threshold);
        let encoded = encode_indexed_literal_gif_inner_with_output(
            output,
            &indexed,
            width,
            height,
            frame_count,
            &palette,
            delays,
            loop_count,
            transparent_index,
        );
        recycle_quantized_indexed(indexed);
        return encoded;
    }
    let frame_len = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| "Frame size overflow".to_string())?;
    let expected_len = frame_len
        .checked_mul(frame_count)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "RGBA frame stream overflow".to_string())?;
    if rgba_stream.len() != expected_len {
        return Err("RGBA frame stream length does not match dimensions".to_string());
    }
    let min_code_size = 8;
    let lzw_length = literal_lzw_block_size(frame_len, min_code_size)?;
    let frame_capacity = (0..frame_count).try_fold(0usize, |capacity, frame_index| {
        let graphic_control_length =
            usize::from(delays.get(frame_index) != 0 || transparent_index.is_some()) * 8;
        capacity
            .checked_add(10)
            .and_then(|length| length.checked_add(graphic_control_length))
            .and_then(|length| length.checked_add(lzw_length))
            .ok_or_else(|| "Encoded GIF size overflow".to_string())
    })?;
    let output_capacity = 13usize
        .checked_add(color_count * 3)
        .and_then(|length| length.checked_add(usize::from(loop_count >= 0) * 19))
        .and_then(|length| length.checked_add(frame_capacity))
        .and_then(|length| length.checked_add(1))
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?;
    output.clear();
    if output.capacity() < output_capacity {
        output.reserve(output_capacity - output.capacity());
    }
    write_indexed_gif_header(&mut output, width, height, &palette, color_count);
    write_loop_extension(&mut output, loop_count);
    for frame_index in 0..frame_count {
        let frame_start = frame_index * frame_len * 4;
        let frame = &rgba_stream[frame_start..frame_start + frame_len * 4];
        write_indexed_gif_frame_header(
            &mut output,
            0,
            0,
            width,
            height,
            delays.get(frame_index),
            transparent_index,
            if transparent_index.is_some() { 2 } else { 0 },
        );
        match (mapping_bits, transparent_index.is_some()) {
            (4, false) => encode_nine_bit_literal_lzw_mapped_to::<4, false>(
                &mut output,
                frame,
                alpha_threshold,
                0,
                &histogram_to_palette,
            )?,
            (4, true) => encode_nine_bit_literal_lzw_mapped_to::<4, true>(
                &mut output,
                frame,
                alpha_threshold,
                transparent_index.unwrap_or(0),
                &histogram_to_palette,
            )?,
            (5, false) => encode_nine_bit_literal_lzw_mapped_to::<5, false>(
                &mut output,
                frame,
                alpha_threshold,
                0,
                &histogram_to_palette,
            )?,
            (5, true) => encode_nine_bit_literal_lzw_mapped_to::<5, true>(
                &mut output,
                frame,
                alpha_threshold,
                transparent_index.unwrap_or(0),
                &histogram_to_palette,
            )?,
            _ => return Err("Unsupported quality histogram precision".to_string()),
        }
    }
    output.push(0x3b);
    recycle_quality_histogram_to_palette(histogram_to_palette);
    Ok(output)
}

fn validate_rgba_stream(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    delays: DelaySource<'_>,
    loop_count: i32,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("Width/Height invalid".to_string());
    }
    if frame_count == 0 {
        return Err("Frame count must be greater than zero".to_string());
    }
    if loop_count < -1 || loop_count > i32::from(u16::MAX) {
        return Err("Loop count invalid".to_string());
    }
    delays.validate(frame_count)?;

    let expected_len = usize::from(width)
        .checked_mul(usize::from(height))
        .and_then(|pixels| pixels.checked_mul(frame_count))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "RGBA frame stream overflow".to_string())?;
    if rgba_stream.len() != expected_len {
        return Err("RGBA frame stream length does not match dimensions".to_string());
    }
    Ok(())
}

fn encode_rgba_local_palette_gif_inner(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    delays: DelaySource<'_>,
    loop_count: i32,
    alpha_threshold: u8,
    literal: bool,
    quantization: RgbaQuantization,
) -> Result<Vec<u8>, String> {
    let frame_pixels = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| "Frame size overflow".to_string())?;
    let frame_bytes = frame_pixels
        .checked_mul(4)
        .ok_or_else(|| "RGBA frame size overflow".to_string())?;
    let mut output = Vec::with_capacity(
        13 + 20 + frame_count.saturating_mul(frame_pixels.saturating_mul(2) + 800) + 1,
    );
    output.extend_from_slice(b"GIF89a");
    push_u16_le(&mut output, width);
    push_u16_le(&mut output, height);
    output.extend_from_slice(&[0, 0, 0]);
    write_loop_extension(&mut output, loop_count);

    let mut lzw_tables = (!literal).then(LzwEncodeTables::new);
    for (frame_index, frame) in rgba_stream.chunks_exact(frame_bytes).enumerate() {
        let (palette, indexed, transparent_index) =
            index_rgba_frames_with_quantization(frame, &[], alpha_threshold, quantization)?;
        let color_count = checked_palette_color_count(palette.len())?;
        write_local_palette_frame_header(
            &mut output,
            width,
            height,
            delays.get(frame_index),
            transparent_index,
            &palette,
            color_count,
        );
        let min_code_size = (log2_pow2(color_count) as u8).max(2);
        if literal {
            let result = encode_indexed_literal_lzw_direct_to(
                &mut output,
                &indexed,
                min_code_size,
                color_count,
            );
            recycle_quantized_indexed(indexed);
            result?;
        } else {
            let result = encode_indexed_lzw_to_with_tables(
                &mut output,
                &indexed,
                min_code_size,
                color_count,
                lzw_tables.as_mut().unwrap(),
            );
            recycle_quantized_indexed(indexed);
            result?;
        }
    }
    output.push(0x3b);
    Ok(output)
}

fn write_local_palette_frame_header(
    output: &mut Vec<u8>,
    width: u16,
    height: u16,
    delay: u16,
    transparent_index: Option<u8>,
    palette: &[u32],
    color_count: usize,
) {
    output.extend_from_slice(&[
        0x21,
        0xf9,
        0x04,
        (if transparent_index.is_some() {
            0x01
        } else {
            0x00
        }) | (2 << 2),
    ]);
    push_u16_le(output, delay);
    output.push(transparent_index.unwrap_or(0));
    output.push(0);

    output.push(0x2c);
    output.extend_from_slice(&[0, 0, 0, 0]);
    push_u16_le(output, width);
    push_u16_le(output, height);
    output.push(0x80 | ((log2_pow2(color_count) as u8 - 1) & 7));
    for index in 0..color_count {
        let rgb = palette.get(index).copied().unwrap_or(0);
        output.push(((rgb >> 16) & 0xff) as u8);
        output.push(((rgb >> 8) & 0xff) as u8);
        output.push((rgb & 0xff) as u8);
    }
}

fn encode_rgba_gif_inner(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    deltas: bool,
    alpha_threshold: u8,
    literal: bool,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("Width/Height invalid".to_string());
    }
    if frame_count == 0 {
        return Err("Frame count must be greater than zero".to_string());
    }
    if loop_count < -1 || loop_count > i32::from(u16::MAX) {
        return Err("Loop count invalid".to_string());
    }
    delays.validate(frame_count)?;

    let frame_pixels = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| "Frame size overflow".to_string())?;
    let expected_len = frame_pixels
        .checked_mul(frame_count)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "RGBA frame stream overflow".to_string())?;
    if rgba_stream.len() != expected_len {
        return Err("RGBA frame stream length does not match dimensions".to_string());
    }

    if deltas
        && !palette_rgb.is_empty()
        && !rgba_stream_has_transparent_pixels(rgba_stream, alpha_threshold)
    {
        return encode_rgba_delta_gif_to_palette_inner(
            rgba_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delays,
            loop_count,
            literal,
        );
    }

    let (palette, indexed, transparent_index) =
        index_rgba_frames(rgba_stream, palette_rgb, alpha_threshold, literal)?;
    if literal && !deltas {
        return encode_indexed_literal_gif_inner(
            &indexed,
            width,
            height,
            frame_count,
            &palette,
            delays,
            loop_count,
            transparent_index,
        );
    }
    encode_indexed_gif_inner_with_rects(
        &indexed,
        width,
        height,
        frame_count,
        &palette,
        delays,
        loop_count,
        deltas,
        transparent_index,
        literal,
    )
}

fn encode_rgba_delta_gif_to_palette_inner(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    literal: bool,
) -> Result<Vec<u8>, String> {
    delays.validate(frame_count)?;
    let color_count = checked_palette_color_count(palette_rgb.len())?;
    let min_code_size = (log2_pow2(color_count) as u8).max(2);
    let canvas_width = usize::from(width);
    let canvas_height = usize::from(height);
    let frame_pixels = canvas_width
        .checked_mul(canvas_height)
        .ok_or_else(|| "Frame size overflow".to_string())?;
    let frame_bytes = frame_pixels
        .checked_mul(4)
        .ok_or_else(|| "RGBA frame size overflow".to_string())?;
    let palette_bytes = color_count
        .checked_mul(3)
        .ok_or_else(|| "Palette size overflow".to_string())?;
    let estimated_frame_bytes = frame_pixels / 8 + 48;
    let mut output = Vec::with_capacity(
        13 + palette_bytes + 20 + frame_count.saturating_mul(estimated_frame_bytes) + 1,
    );
    let mapper = PaletteMapper::new(palette_rgb);
    let mut lzw_tables = (!literal).then(LzwEncodeTables::new);
    let mut mapped = Vec::with_capacity(frame_pixels);

    write_indexed_gif_header(&mut output, width, height, palette_rgb, color_count);
    write_loop_extension(&mut output, loop_count);

    let mut previous_frame: Option<&[u8]> = None;
    for (frame_index, frame) in rgba_stream.chunks_exact(frame_bytes).enumerate() {
        let delay = delays.get(frame_index);
        if let Some(previous) = previous_frame {
            if let Some(rect) =
                find_changed_rect_rgba_rgb(previous, frame, canvas_width, canvas_height)
            {
                write_indexed_gif_frame_header(
                    &mut output,
                    rect.x as u16,
                    rect.y as u16,
                    rect.width as u16,
                    rect.height as u16,
                    delay,
                    None,
                    0,
                );
                map_rgba_rect_to_palette(frame, canvas_width, rect, &mapper, &mut mapped);
                if literal {
                    encode_indexed_literal_lzw_direct_to(
                        &mut output,
                        &mapped,
                        min_code_size,
                        color_count,
                    )?;
                } else {
                    encode_indexed_lzw_to_with_tables(
                        &mut output,
                        &mapped,
                        min_code_size,
                        color_count,
                        lzw_tables.as_mut().unwrap(),
                    )?;
                }
            } else {
                write_indexed_gif_frame_header(&mut output, 0, 0, 1, 1, delay, None, 0);
                let noop = [mapper.index_pixel(frame[0], frame[1], frame[2])];
                if literal {
                    encode_indexed_literal_lzw_direct_to(
                        &mut output,
                        &noop,
                        min_code_size,
                        color_count,
                    )?;
                } else {
                    encode_indexed_lzw_to_with_tables(
                        &mut output,
                        &noop,
                        min_code_size,
                        color_count,
                        lzw_tables.as_mut().unwrap(),
                    )?;
                }
            }
        } else {
            write_indexed_gif_frame_header(&mut output, 0, 0, width, height, delay, None, 0);
            map_rgba_frame_to_palette(frame, &mapper, &mut mapped);
            if literal {
                encode_indexed_literal_lzw_direct_to(
                    &mut output,
                    &mapped,
                    min_code_size,
                    color_count,
                )?;
            } else {
                encode_indexed_lzw_to_with_tables(
                    &mut output,
                    &mapped,
                    min_code_size,
                    color_count,
                    lzw_tables.as_mut().unwrap(),
                )?;
            }
        }
        previous_frame = Some(frame);
    }

    output.push(0x3b);
    Ok(output)
}

fn index_rgba_frames(
    rgba_stream: &[u8],
    palette_rgb: &[u32],
    alpha_threshold: u8,
    exact_only: bool,
) -> Result<(Vec<u32>, Vec<u8>, Option<u8>), String> {
    if !palette_rgb.is_empty() {
        checked_palette_color_count(palette_rgb.len())?;
        return index_rgba_frames_to_palette(rgba_stream, palette_rgb, alpha_threshold, exact_only);
    }

    if let Some(exact) = try_index_rgba_frames_exact(rgba_stream, alpha_threshold, exact_only)? {
        return Ok(exact);
    }
    if exact_only {
        return Err(
            "Pixel-perfect GIF encoding requires at most 256 exact palette entries".to_string(),
        );
    }

    Ok(index_rgba_frames_332(rgba_stream, alpha_threshold))
}

fn index_rgba_frames_with_quantization(
    rgba_stream: &[u8],
    palette_rgb: &[u32],
    alpha_threshold: u8,
    quantization: RgbaQuantization,
) -> Result<(Vec<u32>, Vec<u8>, Option<u8>), String> {
    if !palette_rgb.is_empty() {
        checked_palette_color_count(palette_rgb.len())?;
        return index_rgba_frames_to_palette(
            rgba_stream,
            palette_rgb,
            alpha_threshold,
            quantization == RgbaQuantization::Exact,
        );
    }

    match quantization {
        RgbaQuantization::Exact => {
            if let Some(exact) = try_index_rgba_frames_exact(rgba_stream, alpha_threshold, true)? {
                Ok(exact)
            } else {
                Err(
                    "Exact GIF quantization requires at most 256 colors and binary alpha"
                        .to_string(),
                )
            }
        }
        RgbaQuantization::Fast => {
            if let Some(exact) = try_index_rgba_frames_exact(rgba_stream, alpha_threshold, false)? {
                Ok(exact)
            } else {
                Ok(index_rgba_frames_332(rgba_stream, alpha_threshold))
            }
        }
        RgbaQuantization::Quality => Ok(index_rgba_frames_quality(rgba_stream, alpha_threshold)),
    }
}

const QUALITY_HISTOGRAM_BITS: usize = 5;
const QUALITY_HISTOGRAM_SIDE: usize = 1 << QUALITY_HISTOGRAM_BITS;
const QUALITY_HISTOGRAM_LEN: usize =
    QUALITY_HISTOGRAM_SIDE * QUALITY_HISTOGRAM_SIDE * QUALITY_HISTOGRAM_SIDE;
const QUALITY_DOMINANT_COLOR_LIMIT: usize = 2_048;
const QUALITY_U32_PIXEL_LIMIT: usize = u32::MAX as usize / 255;
const QUALITY_LOW_RES_PIXEL_LIMIT: usize = 1_000_000;

struct QualityIndexPlan {
    palette: Vec<u32>,
    histogram_to_palette: Vec<u8>,
    transparent_index: Option<u8>,
    mapping_bits: usize,
}

enum QualityIndexResult {
    Exact((Vec<u32>, Vec<u8>, Option<u8>)),
    Quantized(QualityIndexPlan),
}

impl QualityIndexPlan {
    fn into_indexed(
        self,
        rgba_stream: &[u8],
        alpha_threshold: u8,
    ) -> (Vec<u32>, Vec<u8>, Option<u8>) {
        let QualityIndexPlan {
            palette,
            histogram_to_palette,
            transparent_index,
            mapping_bits,
        } = self;
        let mut indexed = take_quantized_indexed(rgba_stream.len() / 4);
        match mapping_bits {
            4 => map_quality_pixels::<4>(
                rgba_stream,
                alpha_threshold,
                transparent_index.is_some(),
                transparent_index,
                &histogram_to_palette,
                &mut indexed,
            ),
            5 => map_quality_pixels::<5>(
                rgba_stream,
                alpha_threshold,
                transparent_index.is_some(),
                transparent_index,
                &histogram_to_palette,
                &mut indexed,
            ),
            _ => unreachable!("unsupported quality histogram precision"),
        }
        recycle_quality_histogram_to_palette(histogram_to_palette);
        (palette, indexed, transparent_index)
    }
}

#[derive(Clone, Copy, Default)]
struct RgbHistogramBin {
    count: u64,
    red: u64,
    green: u64,
    blue: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RgbHistogramBin32 {
    count: u32,
    red: u32,
    green: u32,
    blue: u32,
}

#[derive(Clone, Copy, Default)]
struct QuantizedColor {
    count: u64,
    // The quality histograms are at most 5 bits per channel (32,768 bins),
    // so a u16 is sufficient. Keeping the hot fields in a 16-byte record
    // halves the color-arena footprint versus usize + u64 + padding.
    histogram_index: u16,
    red: u8,
    green: u8,
    blue: u8,
}

#[cfg(test)]
struct QuantizedColorBox {
    colors: Vec<QuantizedColor>,
    weight: u64,
    score: u64,
    red_range: u8,
    green_range: u8,
    blue_range: u8,
}

#[cfg(test)]
impl QuantizedColorBox {
    fn new(colors: Vec<QuantizedColor>) -> Self {
        let (weight, red_range, green_range, blue_range) = Self::stats(&colors);
        Self::from_stats(colors, weight, red_range, green_range, blue_range)
    }

    fn from_stats(
        colors: Vec<QuantizedColor>,
        weight: u64,
        red_range: u8,
        green_range: u8,
        blue_range: u8,
    ) -> Self {
        let range = u64::from(red_range.max(green_range).max(blue_range));
        Self {
            colors,
            weight,
            score: weight * range * range,
            red_range,
            green_range,
            blue_range,
        }
    }

    fn stats(colors: &[QuantizedColor]) -> (u64, u8, u8, u8) {
        quantized_color_stats(colors)
    }

    fn split(mut self) -> Result<(Self, Self), Self> {
        if self.colors.len() < 2 {
            return Err(self);
        }
        let axis = if self.red_range >= self.green_range && self.red_range >= self.blue_range {
            0
        } else if self.green_range >= self.blue_range {
            1
        } else {
            2
        };
        let midpoint = (self.weight + 1) / 2;
        let split_index = weighted_axis_split_index(&mut self.colors, axis, midpoint);
        let (left_stats, right_stats) = quantized_color_split_stats(&self.colors, split_index);
        let right = self.colors.split_off(split_index);
        let left = Self::from_stats(
            self.colors,
            left_stats.0,
            left_stats.1,
            left_stats.2,
            left_stats.3,
        );
        let right = Self::from_stats(
            right,
            right_stats.0,
            right_stats.1,
            right_stats.2,
            right_stats.3,
        );
        Ok((left, right))
    }
}

#[inline(always)]
fn quantized_color_stats(colors: &[QuantizedColor]) -> (u64, u8, u8, u8) {
    let mut weight = 0u64;
    let mut min_red = u8::MAX;
    let mut min_green = u8::MAX;
    let mut min_blue = u8::MAX;
    let mut max_red = 0;
    let mut max_green = 0;
    let mut max_blue = 0;
    for color in colors {
        weight += color.count;
        min_red = min_red.min(color.red);
        min_green = min_green.min(color.green);
        min_blue = min_blue.min(color.blue);
        max_red = max_red.max(color.red);
        max_green = max_green.max(color.green);
        max_blue = max_blue.max(color.blue);
    }
    (
        weight,
        max_red - min_red,
        max_green - min_green,
        max_blue - min_blue,
    )
}

#[inline(always)]
#[cfg(test)]
fn quantized_color_split_stats(
    colors: &[QuantizedColor],
    split: usize,
) -> ((u64, u8, u8, u8), (u64, u8, u8, u8)) {
    debug_assert!(split > 0 && split < colors.len());
    let mut left_weight = 0u64;
    let mut left_min_red = u8::MAX;
    let mut left_min_green = u8::MAX;
    let mut left_min_blue = u8::MAX;
    let mut left_max_red = 0;
    let mut left_max_green = 0;
    let mut left_max_blue = 0;
    let mut right_weight = 0u64;
    let mut right_min_red = u8::MAX;
    let mut right_min_green = u8::MAX;
    let mut right_min_blue = u8::MAX;
    let mut right_max_red = 0;
    let mut right_max_green = 0;
    let mut right_max_blue = 0;
    for (index, color) in colors.iter().enumerate() {
        if index < split {
            left_weight += color.count;
            left_min_red = left_min_red.min(color.red);
            left_min_green = left_min_green.min(color.green);
            left_min_blue = left_min_blue.min(color.blue);
            left_max_red = left_max_red.max(color.red);
            left_max_green = left_max_green.max(color.green);
            left_max_blue = left_max_blue.max(color.blue);
        } else {
            right_weight += color.count;
            right_min_red = right_min_red.min(color.red);
            right_min_green = right_min_green.min(color.green);
            right_min_blue = right_min_blue.min(color.blue);
            right_max_red = right_max_red.max(color.red);
            right_max_green = right_max_green.max(color.green);
            right_max_blue = right_max_blue.max(color.blue);
        }
    }
    (
        (
            left_weight,
            left_max_red - left_min_red,
            left_max_green - left_min_green,
            left_max_blue - left_min_blue,
        ),
        (
            right_weight,
            right_max_red - right_min_red,
            right_max_green - right_min_green,
            right_max_blue - right_min_blue,
        ),
    )
}

struct QuantizedColorArenaBox {
    start: usize,
    end: usize,
    weight: u64,
    score: u64,
    representative: u32,
    red_range: u8,
    green_range: u8,
    blue_range: u8,
}

impl QuantizedColorArenaBox {
    fn new(start: usize, end: usize, colors: &[QuantizedColor]) -> Self {
        let (weight, red_range, green_range, blue_range) =
            quantized_color_stats(&colors[start..end]);
        Self::from_stats(start, end, weight, red_range, green_range, blue_range)
    }

    fn from_stats(
        start: usize,
        end: usize,
        weight: u64,
        red_range: u8,
        green_range: u8,
        blue_range: u8,
    ) -> Self {
        let range = u64::from(red_range.max(green_range).max(blue_range));
        Self {
            start,
            end,
            weight,
            score: weight * range * range,
            representative: 0,
            red_range,
            green_range,
            blue_range,
        }
    }

    #[inline(always)]
    fn len(&self) -> usize {
        self.end - self.start
    }

    fn split(&mut self, colors: &mut [QuantizedColor]) -> Option<Self> {
        if self.len() < 2 {
            return None;
        }
        let axis = if self.red_range >= self.green_range && self.red_range >= self.blue_range {
            0
        } else if self.green_range >= self.blue_range {
            1
        } else {
            2
        };
        let midpoint = (self.weight + 1) / 2;
        let (relative_split, left_stats, right_stats) =
            weighted_axis_split_index_with_stats(&mut colors[self.start..self.end], axis, midpoint);
        let split = self.start + relative_split;
        let right = Self::from_stats(
            split,
            self.end,
            right_stats.0,
            right_stats.1,
            right_stats.2,
            right_stats.3,
        );
        self.end = split;
        self.weight = left_stats.0;
        self.red_range = left_stats.1;
        self.green_range = left_stats.2;
        self.blue_range = left_stats.3;
        let range = u64::from(left_stats.1.max(left_stats.2).max(left_stats.3));
        self.score = left_stats.0 * range * range;
        Some(right)
    }

    fn calculate_representative(&mut self, colors: &[QuantizedColor]) {
        // A GIF input that fits in Wasm memory has fewer than 2^32 pixels, so
        // each channel's weighted sum (255 * pixel_count) fits comfortably in
        // u64. Keeping this hot representative pass in native-width integer
        // arithmetic avoids the software u128 multiply/divide sequence.
        let mut red = 0u64;
        let mut green = 0u64;
        let mut blue = 0u64;
        let mut count = 0u64;
        for color in &colors[self.start..self.end] {
            let weight = color.count;
            red += u64::from(color.red) * weight;
            green += u64::from(color.green) * weight;
            blue += u64::from(color.blue) * weight;
            count += weight;
        }
        if count == 0 {
            self.representative = 0;
            return;
        }
        let red = ((red + count / 2) / count) as u32;
        let green = ((green + count / 2) / count) as u32;
        let blue = ((blue + count / 2) / count) as u32;
        self.representative = (red << 16) | (green << 8) | blue;
    }
}

#[inline(always)]
#[cfg(test)]
fn quantized_color_axis(color: &QuantizedColor, axis: u8) -> u8 {
    match axis {
        0 => color.red,
        1 => color.green,
        _ => color.blue,
    }
}

#[inline(always)]
fn quantized_color_axis_const<const AXIS: usize>(color: &QuantizedColor) -> u8 {
    match AXIS {
        0 => color.red,
        1 => color.green,
        _ => color.blue,
    }
}

/// Partition a color box around its weighted median on one color axis.
///
/// Median-cut only needs every color on the left of the split to be no larger
/// on the chosen axis than every color on the right. The channel values are
/// bytes, so a 256-bin weight pass plus a three-way in-place partition avoids
/// repeated comparison-based selection work on dense histograms.
#[inline(always)]
#[cfg(test)]
fn weighted_axis_split_index(colors: &mut [QuantizedColor], axis: u8, midpoint: u64) -> usize {
    weighted_axis_split_index_with_stats(colors, axis, midpoint).0
}

#[inline(always)]
fn weighted_axis_split_index_with_stats(
    colors: &mut [QuantizedColor],
    axis: u8,
    midpoint: u64,
) -> (usize, (u64, u8, u8, u8), (u64, u8, u8, u8)) {
    match axis {
        0 => weighted_axis_split_index_with_stats_const::<0>(colors, midpoint),
        1 => weighted_axis_split_index_with_stats_const::<1>(colors, midpoint),
        _ => weighted_axis_split_index_with_stats_const::<2>(colors, midpoint),
    }
}

#[inline(always)]
fn weighted_axis_split_index_with_stats_const<const AXIS: usize>(
    colors: &mut [QuantizedColor],
    midpoint: u64,
) -> (usize, (u64, u8, u8, u8), (u64, u8, u8, u8)) {
    let total_len = colors.len();
    if total_len <= 1 {
        let stats = quantized_color_stats(colors);
        return (1.min(total_len), stats, (0, u8::MAX, u8::MAX, u8::MAX));
    }
    let mut weights = [0u64; 256];
    for color in colors.iter() {
        weights[usize::from(quantized_color_axis_const::<AXIS>(color))] += color.count;
    }
    let target = midpoint.max(1);
    let mut below_weight = 0u64;
    let mut split_axis = 0usize;
    for (value, &weight) in weights.iter().enumerate() {
        if weight == 0 {
            continue;
        }
        if target <= below_weight + weight {
            split_axis = value;
            break;
        }
        below_weight += weight;
    }

    // Three-way partition: [0, less_end) is below the split byte,
    // [less_end, equal_end) is equal, and [equal_end, len) is above it.
    // Unlike a comparison-based selection this touches each color once after
    // the weight pass and makes the equal-bin walk deterministic.
    let mut less_end = 0usize;
    let mut scan = 0usize;
    let mut greater_start = total_len;
    let mut less_weight = 0u64;
    let mut less_min_red = u8::MAX;
    let mut less_min_green = u8::MAX;
    let mut less_min_blue = u8::MAX;
    let mut less_max_red = 0u8;
    let mut less_max_green = 0u8;
    let mut less_max_blue = 0u8;
    let mut greater_weight = 0u64;
    let mut greater_min_red = u8::MAX;
    let mut greater_min_green = u8::MAX;
    let mut greater_min_blue = u8::MAX;
    let mut greater_max_red = 0u8;
    let mut greater_max_green = 0u8;
    let mut greater_max_blue = 0u8;
    while scan < greater_start {
        let color = colors[scan];
        let value = usize::from(quantized_color_axis_const::<AXIS>(&color));
        if value < split_axis {
            colors.swap(scan, less_end);
            less_end += 1;
            scan += 1;
            less_weight += color.count;
            less_min_red = less_min_red.min(color.red);
            less_min_green = less_min_green.min(color.green);
            less_min_blue = less_min_blue.min(color.blue);
            less_max_red = less_max_red.max(color.red);
            less_max_green = less_max_green.max(color.green);
            less_max_blue = less_max_blue.max(color.blue);
        } else if value > split_axis {
            greater_start -= 1;
            colors.swap(scan, greater_start);
            greater_weight += color.count;
            greater_min_red = greater_min_red.min(color.red);
            greater_min_green = greater_min_green.min(color.green);
            greater_min_blue = greater_min_blue.min(color.blue);
            greater_max_red = greater_max_red.max(color.red);
            greater_max_green = greater_max_green.max(color.green);
            greater_max_blue = greater_max_blue.max(color.blue);
        } else {
            scan += 1;
        }
    }
    let target_in_equal = target - below_weight;
    let mut split = less_end;
    let mut equal_weight = 0u64;
    while split < greater_start && equal_weight < target_in_equal {
        equal_weight += colors[split].count;
        split += 1;
    }
    let split = split.clamp(1, total_len - 1);
    let mut left_equal_weight = 0u64;
    let mut left_equal_min_red = u8::MAX;
    let mut left_equal_min_green = u8::MAX;
    let mut left_equal_min_blue = u8::MAX;
    let mut left_equal_max_red = 0u8;
    let mut left_equal_max_green = 0u8;
    let mut left_equal_max_blue = 0u8;
    let mut right_equal_weight = 0u64;
    let mut right_equal_min_red = u8::MAX;
    let mut right_equal_min_green = u8::MAX;
    let mut right_equal_min_blue = u8::MAX;
    let mut right_equal_max_red = 0u8;
    let mut right_equal_max_green = 0u8;
    let mut right_equal_max_blue = 0u8;
    for (index, color) in colors[less_end..greater_start].iter().enumerate() {
        let target = if less_end + index < split {
            (
                &mut left_equal_weight,
                &mut left_equal_min_red,
                &mut left_equal_min_green,
                &mut left_equal_min_blue,
                &mut left_equal_max_red,
                &mut left_equal_max_green,
                &mut left_equal_max_blue,
            )
        } else {
            (
                &mut right_equal_weight,
                &mut right_equal_min_red,
                &mut right_equal_min_green,
                &mut right_equal_min_blue,
                &mut right_equal_max_red,
                &mut right_equal_max_green,
                &mut right_equal_max_blue,
            )
        };
        *target.0 += color.count;
        *target.1 = (*target.1).min(color.red);
        *target.2 = (*target.2).min(color.green);
        *target.3 = (*target.3).min(color.blue);
        *target.4 = (*target.4).max(color.red);
        *target.5 = (*target.5).max(color.green);
        *target.6 = (*target.6).max(color.blue);
    }
    let left_min_red = less_min_red.min(left_equal_min_red);
    let left_min_green = less_min_green.min(left_equal_min_green);
    let left_min_blue = less_min_blue.min(left_equal_min_blue);
    let left_max_red = less_max_red.max(left_equal_max_red);
    let left_max_green = less_max_green.max(left_equal_max_green);
    let left_max_blue = less_max_blue.max(left_equal_max_blue);
    let right_min_red = right_equal_min_red.min(greater_min_red);
    let right_min_green = right_equal_min_green.min(greater_min_green);
    let right_min_blue = right_equal_min_blue.min(greater_min_blue);
    let right_max_red = right_equal_max_red.max(greater_max_red);
    let right_max_green = right_equal_max_green.max(greater_max_green);
    let right_max_blue = right_equal_max_blue.max(greater_max_blue);
    (
        split,
        (
            less_weight + left_equal_weight,
            left_max_red - left_min_red,
            left_max_green - left_min_green,
            left_max_blue - left_min_blue,
        ),
        (
            right_equal_weight + greater_weight,
            right_max_red - right_min_red,
            right_max_green - right_min_green,
            right_max_blue - right_min_blue,
        ),
    )
}

#[inline(always)]
fn quality_histogram_index_bits_const<const BITS: usize>(red: u8, green: u8, blue: u8) -> usize {
    (usize::from(red >> (8 - BITS)) << (BITS * 2))
        | (usize::from(green >> (8 - BITS)) << BITS)
        | usize::from(blue >> (8 - BITS))
}

#[inline(always)]
fn map_quality_pixels<const BITS: usize>(
    rgba_stream: &[u8],
    alpha_threshold: u8,
    has_transparent_pixels: bool,
    transparent_index: Option<u8>,
    histogram_to_palette: &[u8],
    indexed: &mut [u8],
) {
    let rgba_pointer = rgba_stream.as_ptr();
    let indexed_pointer = indexed.as_mut_ptr();
    if !has_transparent_pixels {
        let mut pixel_index = 0usize;
        let mut rgba_offset = 0usize;
        while pixel_index + 4 <= indexed.len() {
            let packed0 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(rgba_offset).cast())
            });
            let packed1 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(rgba_offset + 4).cast())
            });
            let packed2 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(rgba_offset + 8).cast())
            });
            let packed3 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(rgba_offset + 12).cast())
            });
            unsafe {
                indexed_pointer.add(pixel_index).write(
                    *histogram_to_palette
                        .get_unchecked(quality_histogram_index_packed::<BITS>(packed0)),
                );
                indexed_pointer.add(pixel_index + 1).write(
                    *histogram_to_palette
                        .get_unchecked(quality_histogram_index_packed::<BITS>(packed1)),
                );
                indexed_pointer.add(pixel_index + 2).write(
                    *histogram_to_palette
                        .get_unchecked(quality_histogram_index_packed::<BITS>(packed2)),
                );
                indexed_pointer.add(pixel_index + 3).write(
                    *histogram_to_palette
                        .get_unchecked(quality_histogram_index_packed::<BITS>(packed3)),
                );
            }
            pixel_index += 4;
            rgba_offset += 16;
        }
        while pixel_index < indexed.len() {
            let packed = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(rgba_offset).cast())
            });
            let histogram_index = quality_histogram_index_packed::<BITS>(packed);
            unsafe {
                indexed_pointer
                    .add(pixel_index)
                    .write(*histogram_to_palette.get_unchecked(histogram_index));
            }
            pixel_index += 1;
            rgba_offset += 4;
        }
    } else {
        let transparent_index = transparent_index.unwrap_or(0);
        let mut pixel_index = 0usize;
        let mut rgba_offset = 0usize;
        while pixel_index + 4 <= indexed.len() {
            let packed0 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(rgba_offset).cast())
            });
            let packed1 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(rgba_offset + 4).cast())
            });
            let packed2 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(rgba_offset + 8).cast())
            });
            let packed3 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(rgba_offset + 12).cast())
            });
            unsafe {
                indexed_pointer
                    .add(pixel_index)
                    .write(map_quality_pixel::<BITS>(
                        packed0,
                        alpha_threshold,
                        transparent_index,
                        histogram_to_palette,
                    ));
                indexed_pointer
                    .add(pixel_index + 1)
                    .write(map_quality_pixel::<BITS>(
                        packed1,
                        alpha_threshold,
                        transparent_index,
                        histogram_to_palette,
                    ));
                indexed_pointer
                    .add(pixel_index + 2)
                    .write(map_quality_pixel::<BITS>(
                        packed2,
                        alpha_threshold,
                        transparent_index,
                        histogram_to_palette,
                    ));
                indexed_pointer
                    .add(pixel_index + 3)
                    .write(map_quality_pixel::<BITS>(
                        packed3,
                        alpha_threshold,
                        transparent_index,
                        histogram_to_palette,
                    ));
            }
            pixel_index += 4;
            rgba_offset += 16;
        }
        while pixel_index < indexed.len() {
            let packed = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(rgba_offset).cast())
            });
            let index = map_quality_pixel::<BITS>(
                packed,
                alpha_threshold,
                transparent_index,
                histogram_to_palette,
            );
            unsafe {
                indexed_pointer.add(pixel_index).write(index);
            }
            pixel_index += 1;
            rgba_offset += 4;
        }
    }
}

#[inline(always)]
fn map_quality_pixel<const BITS: usize>(
    packed: u32,
    alpha_threshold: u8,
    transparent_index: u8,
    histogram_to_palette: &[u8],
) -> u8 {
    if ((packed >> 24) as u8) >= alpha_threshold {
        let histogram_index = quality_histogram_index_packed::<BITS>(packed);
        unsafe { *histogram_to_palette.get_unchecked(histogram_index) }
    } else {
        transparent_index
    }
}

fn quality_colors_from_histogram_u32<const SAFE_SUMS: bool>(
    histogram: &[RgbHistogramBin32],
) -> Vec<QuantizedColor> {
    let mut colors = take_quality_colors();
    for (histogram_index, bin) in histogram.iter().enumerate() {
        if bin.count == 0 {
            continue;
        }
        let red = if SAFE_SUMS {
            ((bin.red + bin.count / 2) / bin.count) as u8
        } else {
            rounded_histogram_average_u32(bin.red, bin.count)
        };
        let green = if SAFE_SUMS {
            ((bin.green + bin.count / 2) / bin.count) as u8
        } else {
            rounded_histogram_average_u32(bin.green, bin.count)
        };
        let blue = if SAFE_SUMS {
            ((bin.blue + bin.count / 2) / bin.count) as u8
        } else {
            rounded_histogram_average_u32(bin.blue, bin.count)
        };
        colors.push(QuantizedColor {
            count: u64::from(bin.count),
            histogram_index: histogram_index as u16,
            red,
            green,
            blue,
        });
    }
    colors
}

#[inline(always)]
fn rounded_histogram_average_u32(sum: u32, count: u32) -> u8 {
    let half = count / 2;
    match sum.checked_add(half) {
        Some(adjusted) => (adjusted / count) as u8,
        None => ((u64::from(sum) + u64::from(half)) / u64::from(count)) as u8,
    }
}

fn quality_colors_from_histogram_u64(histogram: &[RgbHistogramBin]) -> Vec<QuantizedColor> {
    let mut colors = take_quality_colors();
    for (histogram_index, bin) in histogram.iter().enumerate() {
        if bin.count == 0 {
            continue;
        }
        colors.push(QuantizedColor {
            count: bin.count,
            histogram_index: histogram_index as u16,
            red: ((bin.red + bin.count / 2) / bin.count) as u8,
            green: ((bin.green + bin.count / 2) / bin.count) as u8,
            blue: ((bin.blue + bin.count / 2) / bin.count) as u8,
        });
    }
    colors
}

fn take_quality_colors() -> Vec<QuantizedColor> {
    REUSABLE_QUALITY_COLORS.with(|scratch| std::mem::take(&mut *scratch.borrow_mut()))
}

fn recycle_quality_colors(mut colors: Vec<QuantizedColor>) {
    colors.clear();
    REUSABLE_QUALITY_COLORS.with(|scratch| {
        *scratch.borrow_mut() = colors;
    });
}

fn index_rgba_frames_quality(
    rgba_stream: &[u8],
    alpha_threshold: u8,
) -> (Vec<u32>, Vec<u8>, Option<u8>) {
    match index_rgba_frames_quality_result(rgba_stream, alpha_threshold) {
        QualityIndexResult::Exact(result) => result,
        QualityIndexResult::Quantized(plan) => plan.into_indexed(rgba_stream, alpha_threshold),
    }
}

fn index_rgba_frames_quality_result(
    rgba_stream: &[u8],
    alpha_threshold: u8,
) -> QualityIndexResult {
    let pixel_count = rgba_stream.len() / 4;
    if pixel_count <= QUALITY_LOW_RES_PIXEL_LIMIT {
        return index_rgba_frames_quality_low_res(rgba_stream, alpha_threshold);
    }
    index_rgba_frames_quality_high_res(rgba_stream, alpha_threshold)
}

fn index_rgba_frames_quality_low_res(
    rgba_stream: &[u8],
    alpha_threshold: u8,
) -> QualityIndexResult {
    const HISTOGRAM_BITS: usize = 4;
    const HISTOGRAM_LEN: usize = 1 << (HISTOGRAM_BITS * 3);
    let pixel_count = rgba_stream.len() / 4;
    let mut table = take_quality_color_index_table(COLOR_INDEX_CAP);
    let mut palette = Vec::with_capacity(256);
    // Keep the quality histogram in lockstep with the exact-color probe. If
    // the probe overflows (the normal photo/image case), this avoids replaying
    // the prefix through a second histogram pass.
    let mut histogram = take_quality_histogram_u32(HISTOGRAM_LEN);
    // Keep the exact-prefix indices so <=256-color inputs do not require a
    // second full RGBA scan after the palette decision is known.
    // The indexed stream is needed only if the exact-color probe succeeds.
    // Start with a small prefix buffer so the common quantized path does not
    // reserve the entire RGBA image just to discard the prefix after overflow.
    let mut indexed = Vec::with_capacity(pixel_count.min(4_096));
    let mut has_transparent_pixels = false;
    let rgba_pointer = rgba_stream.as_ptr();
    let mut overflow_offset = None;
    for pixel_index in 0..pixel_count {
        let packed = u32::from_le(unsafe {
            std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
        });
        let alpha = (packed >> 24) as u8;
        if alpha < alpha_threshold {
            has_transparent_pixels = true;
            indexed.push(u8::MAX);
            if palette.len() == 256 {
                overflow_offset = Some((pixel_index + 1) * 4);
                break;
            }
            continue;
        }
        add_quality_histogram_u32_bits_const::<HISTOGRAM_BITS>(&mut histogram, packed);
        let rgb = rgb_key(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8);
        if let Some(index) = table.get(rgb) {
            indexed.push(index);
            continue;
        }
        let color_limit = if has_transparent_pixels { 255 } else { 256 };
        if palette.len() == color_limit {
            overflow_offset = Some((pixel_index + 1) * 4);
            break;
        }
        let index = palette.len() as u8;
        table.insert_if_absent(rgb, index);
        palette.push(rgb);
        indexed.push(index);
    }
    let histogram = if let Some(start_offset) = overflow_offset {
        // The exact-prefix scan above already populated the histogram; finish
        // only the suffix after the palette limit was exceeded.
        has_transparent_pixels |= accumulate_quality_histogram_u32_bits_remaining::<HISTOGRAM_BITS>(
            &mut histogram,
            rgba_stream,
            start_offset,
            alpha_threshold,
        );
        histogram
    } else {
        recycle_quality_histogram_u32(histogram);
        return QualityIndexResult::Exact(finish_quality_exact_indexed(
            rgba_stream,
            alpha_threshold,
            has_transparent_pixels,
            palette,
            table,
            indexed,
        ));
    };
    recycle_quantized_indexed(indexed);
    let colors = quality_colors_from_histogram_u32::<true>(&histogram);
    recycle_quality_color_index_table(table);
    recycle_quality_histogram_u32(histogram);
    QualityIndexResult::Quantized(build_quality_index_plan_from_colors(
        rgba_stream,
        alpha_threshold,
        has_transparent_pixels,
        colors,
        HISTOGRAM_BITS,
    ))
}

fn finish_quality_exact_indexed(
    rgba_stream: &[u8],
    alpha_threshold: u8,
    has_transparent_pixels: bool,
    mut palette: Vec<u32>,
    table: ColorIndexTable,
    mut indexed: Vec<u8>,
) -> (Vec<u32>, Vec<u8>, Option<u8>) {
    let transparent_index = if has_transparent_pixels {
        let index = palette.len() as u8;
        palette.push(0);
        Some(index)
    } else {
        None
    };
    let pixel_count = rgba_stream.len() / 4;
    if indexed.len() == pixel_count {
        if let Some(transparent_index) = transparent_index {
            for index in &mut indexed {
                if *index == u8::MAX {
                    *index = transparent_index;
                }
            }
        }
        recycle_quality_color_index_table(table);
        return (palette, indexed, transparent_index);
    }
    recycle_quantized_indexed(indexed);
    let mut indexed = take_quantized_indexed(pixel_count);
    let rgba_pointer = rgba_stream.as_ptr();
    for pixel_index in 0..pixel_count {
        let packed = u32::from_le(unsafe {
            std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
        });
        indexed[pixel_index] = if ((packed >> 24) as u8) < alpha_threshold {
            transparent_index.unwrap_or(0)
        } else {
            table
                .get(rgb_key(
                    packed as u8,
                    (packed >> 8) as u8,
                    (packed >> 16) as u8,
                ))
                .unwrap_or(0)
        };
    }
    recycle_quality_color_index_table(table);
    (palette, indexed, transparent_index)
}

fn index_rgba_frames_quality_high_res(
    rgba_stream: &[u8],
    alpha_threshold: u8,
) -> QualityIndexResult {
    let pixel_count = rgba_stream.len() / 4;
    if pixel_count <= QUALITY_U32_PIXEL_LIMIT {
        if quality_prefers_high_precision_histogram(rgba_stream, alpha_threshold) {
            index_rgba_frames_quality_u32::<5>(rgba_stream, alpha_threshold)
        } else {
            index_rgba_frames_quality_u32::<4>(rgba_stream, alpha_threshold)
        }
    } else {
        index_rgba_frames_quality_u64(rgba_stream, alpha_threshold)
    }
}

/// Smooth gradients expose the one weakness of a coarse 4-bit histogram:
/// image-q can spend its palette on those gradual ramps, while the coarse
/// bins collapse nearby colors before median-cut sees them. Noisy/photo-like
/// input has much larger local color deltas and gets the faster 4-bit pass.
/// Sampling a few thousand adjacent pixels keeps this decision negligible
/// compared with the mandatory full histogram scan.
fn quality_prefers_high_precision_histogram(rgba_stream: &[u8], alpha_threshold: u8) -> bool {
    let pixel_count = rgba_stream.len() / 4;
    if pixel_count < 2 {
        return false;
    }
    let sample_step = (pixel_count / 8_192).max(1);
    let rgba_pointer = rgba_stream.as_ptr();
    let mut total_delta = 0u64;
    let mut samples = 0u64;
    let mut pixel_index = 0usize;
    while pixel_index + sample_step < pixel_count && samples < 8_192 {
        let first = unsafe { rgba_pointer.add(pixel_index * 4) };
        let second = unsafe { rgba_pointer.add((pixel_index + 1) * 4) };
        let first_alpha = unsafe { *first.add(3) };
        let second_alpha = unsafe { *second.add(3) };
        if first_alpha >= alpha_threshold && second_alpha >= alpha_threshold {
            for channel in 0..3 {
                let left = unsafe { *first.add(channel) };
                let right = unsafe { *second.add(channel) };
                total_delta += u64::from(left.abs_diff(right));
            }
            samples += 1;
        }
        pixel_index += sample_step;
    }
    // A mean adjacent RGB delta below sixteen is a smooth/illustrated ramp in
    // the normal-image corpus. Give those images the 5-bit quality path; the
    // 4-bit path remains the default for noisy photos and stress textures.
    samples > 0 && total_delta < samples * 3 * 16
}

fn take_quality_histogram_u32(length: usize) -> Vec<RgbHistogramBin32> {
    REUSABLE_QUALITY_HISTOGRAM_U32.with(|scratch| {
        let mut histogram = std::mem::take(&mut *scratch.borrow_mut());
        if histogram.len() != length {
            histogram.resize(length, RgbHistogramBin32::default());
        }
        histogram.fill(RgbHistogramBin32::default());
        histogram
    })
}

fn recycle_quality_histogram_u32(histogram: Vec<RgbHistogramBin32>) {
    REUSABLE_QUALITY_HISTOGRAM_U32.with(|scratch| {
        *scratch.borrow_mut() = histogram;
    });
}

fn take_quality_histogram_u64(length: usize) -> Vec<RgbHistogramBin> {
    REUSABLE_QUALITY_HISTOGRAM_U64.with(|scratch| {
        let mut histogram = std::mem::take(&mut *scratch.borrow_mut());
        if histogram.len() != length {
            histogram.resize(length, RgbHistogramBin::default());
        }
        histogram.fill(RgbHistogramBin::default());
        histogram
    })
}

fn recycle_quality_histogram_u64(histogram: Vec<RgbHistogramBin>) {
    REUSABLE_QUALITY_HISTOGRAM_U64.with(|scratch| {
        *scratch.borrow_mut() = histogram;
    });
}

fn take_quality_histogram_to_palette(length: usize) -> Vec<u8> {
    REUSABLE_QUALITY_HISTOGRAM_TO_PALETTE.with(|scratch| {
        let mut table = std::mem::take(&mut *scratch.borrow_mut());
        if table.len() != length {
            table.resize(length, 0);
        }
        // Every opaque input bin is rewritten before the table is read; stale
        // entries for bins absent from this image are therefore harmless.
        table
    })
}

fn recycle_quality_histogram_to_palette(table: Vec<u8>) {
    REUSABLE_QUALITY_HISTOGRAM_TO_PALETTE.with(|scratch| {
        *scratch.borrow_mut() = table;
    });
}

fn take_quantized_indexed(pixel_count: usize) -> Vec<u8> {
    REUSABLE_QUANTIZED_INDEXED.with(|scratch| {
        let mut indexed = std::mem::take(&mut *scratch.borrow_mut());
        indexed.resize(pixel_count, 0);
        indexed
    })
}

fn recycle_quantized_indexed(mut indexed: Vec<u8>) {
    indexed.clear();
    REUSABLE_QUANTIZED_INDEXED.with(|scratch| {
        *scratch.borrow_mut() = indexed;
    });
}

#[inline(always)]
fn quality_histogram_index_packed<const BITS: usize>(packed: u32) -> usize {
    // RGBA input is little-endian in Wasm memory. Keep the hot scan in one
    // packed load instead of extracting three channels before shifting them.
    if BITS == 5 {
        (((packed << 7) & 0x7c00) | ((packed >> 6) & 0x03e0) | ((packed >> 19) & 0x001f)) as usize
    } else if BITS == 4 {
        (((packed << 4) & 0x0f00) | ((packed >> 8) & 0x00f0) | ((packed >> 20) & 0x000f)) as usize
    } else {
        quality_histogram_index_bits_const::<BITS>(
            packed as u8,
            (packed >> 8) as u8,
            (packed >> 16) as u8,
        )
    }
}

#[inline(always)]
fn add_quality_histogram_u32_bits_const<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    packed: u32,
) {
    let red = packed as u8;
    let green = (packed >> 8) as u8;
    let blue = (packed >> 16) as u8;
    let histogram_index = quality_histogram_index_packed::<BITS>(packed);
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    unsafe {
        use core::arch::wasm32::{u32x4, u32x4_add, v128_load, v128_store};

        let bin = histogram.get_unchecked_mut(histogram_index) as *mut RgbHistogramBin32;
        let current = v128_load(bin.cast());
        let increment = u32x4(1, u32::from(red), u32::from(green), u32::from(blue));
        v128_store(bin.cast(), u32x4_add(current, increment));
    }
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    {
        let bin = unsafe { histogram.get_unchecked_mut(histogram_index) };
        bin.count += 1;
        bin.red += u32::from(red);
        bin.green += u32::from(green);
        bin.blue += u32::from(blue);
    }
}

#[inline(always)]
fn add_quality_histogram_u64(histogram: &mut [RgbHistogramBin], packed: u32) {
    let red = packed as u8;
    let green = (packed >> 8) as u8;
    let blue = (packed >> 16) as u8;
    let histogram_index = quality_histogram_index_packed::<QUALITY_HISTOGRAM_BITS>(packed);
    let bin = unsafe { histogram.get_unchecked_mut(histogram_index) };
    bin.count += 1;
    bin.red += u64::from(red);
    bin.green += u64::from(green);
    bin.blue += u64::from(blue);
}

fn accumulate_quality_histogram_u32_bits_remaining<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    rgba_stream: &[u8],
    start_offset: usize,
    alpha_threshold: u8,
) -> bool {
    if alpha_threshold == 0 {
        accumulate_quality_histogram_u32_bits_remaining_opaque::<BITS>(
            histogram,
            rgba_stream,
            start_offset,
        );
        return false;
    }
    let rgba_pointer = rgba_stream.as_ptr();
    let mut has_transparent_pixels = false;
    let mut offset = start_offset;
    // Keep eight packed pixels in flight. This is the fallback scan for the
    // normal-sized quality path (4-bit histogram), so avoiding a loop branch
    // and repeated offset arithmetic here matters more than the tiny prefix
    // scan that discovers the palette overflow.
    while offset + 32 <= rgba_stream.len() {
        let packed0 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        let packed1 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 4).cast()) });
        let packed2 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 8).cast()) });
        let packed3 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 12).cast()) });
        let packed4 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 16).cast()) });
        let packed5 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 20).cast()) });
        let packed6 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 24).cast()) });
        let packed7 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 28).cast()) });
        if ((packed0 >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u32_bits_const::<BITS>(histogram, packed0);
        }
        if ((packed1 >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u32_bits_const::<BITS>(histogram, packed1);
        }
        if ((packed2 >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u32_bits_const::<BITS>(histogram, packed2);
        }
        if ((packed3 >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u32_bits_const::<BITS>(histogram, packed3);
        }
        if ((packed4 >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u32_bits_const::<BITS>(histogram, packed4);
        }
        if ((packed5 >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u32_bits_const::<BITS>(histogram, packed5);
        }
        if ((packed6 >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u32_bits_const::<BITS>(histogram, packed6);
        }
        if ((packed7 >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u32_bits_const::<BITS>(histogram, packed7);
        }
        offset += 32;
    }
    while offset < rgba_stream.len() {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        if ((packed >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u32_bits_const::<BITS>(histogram, packed);
        }
        offset += 4;
    }
    has_transparent_pixels
}

#[inline(always)]
fn accumulate_quality_histogram_u32_bits_remaining_opaque<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    rgba_stream: &[u8],
    start_offset: usize,
) {
    let rgba_pointer = rgba_stream.as_ptr();
    let mut offset = start_offset;
    while offset + 32 <= rgba_stream.len() {
        let packed0 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        let packed1 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 4).cast()) });
        let packed2 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 8).cast()) });
        let packed3 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 12).cast()) });
        let packed4 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 16).cast()) });
        let packed5 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 20).cast()) });
        let packed6 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 24).cast()) });
        let packed7 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 28).cast()) });
        add_quality_histogram_u32_bits_const::<BITS>(histogram, packed0);
        add_quality_histogram_u32_bits_const::<BITS>(histogram, packed1);
        add_quality_histogram_u32_bits_const::<BITS>(histogram, packed2);
        add_quality_histogram_u32_bits_const::<BITS>(histogram, packed3);
        add_quality_histogram_u32_bits_const::<BITS>(histogram, packed4);
        add_quality_histogram_u32_bits_const::<BITS>(histogram, packed5);
        add_quality_histogram_u32_bits_const::<BITS>(histogram, packed6);
        add_quality_histogram_u32_bits_const::<BITS>(histogram, packed7);
        offset += 32;
    }
    while offset < rgba_stream.len() {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        add_quality_histogram_u32_bits_const::<BITS>(histogram, packed);
        offset += 4;
    }
}

fn accumulate_quality_histogram_u64_remaining(
    histogram: &mut [RgbHistogramBin],
    rgba_stream: &[u8],
    start_offset: usize,
    alpha_threshold: u8,
) -> bool {
    if alpha_threshold == 0 {
        accumulate_quality_histogram_u64_remaining_opaque(histogram, rgba_stream, start_offset);
        return false;
    }
    let rgba_pointer = rgba_stream.as_ptr();
    let mut has_transparent_pixels = false;
    let mut offset = start_offset;
    while offset + 16 <= rgba_stream.len() {
        let packed0 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        let packed1 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 4).cast()) });
        let packed2 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 8).cast()) });
        let packed3 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 12).cast()) });
        if ((packed0 >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u64(histogram, packed0);
        }
        if ((packed1 >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u64(histogram, packed1);
        }
        if ((packed2 >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u64(histogram, packed2);
        }
        if ((packed3 >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u64(histogram, packed3);
        }
        offset += 16;
    }
    while offset < rgba_stream.len() {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        if ((packed >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u64(histogram, packed);
        }
        offset += 4;
    }
    has_transparent_pixels
}

#[inline(always)]
fn accumulate_quality_histogram_u64_remaining_opaque(
    histogram: &mut [RgbHistogramBin],
    rgba_stream: &[u8],
    start_offset: usize,
) {
    let rgba_pointer = rgba_stream.as_ptr();
    let mut offset = start_offset;
    while offset + 16 <= rgba_stream.len() {
        let packed0 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        let packed1 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 4).cast()) });
        let packed2 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 8).cast()) });
        let packed3 =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 12).cast()) });
        add_quality_histogram_u64(histogram, packed0);
        add_quality_histogram_u64(histogram, packed1);
        add_quality_histogram_u64(histogram, packed2);
        add_quality_histogram_u64(histogram, packed3);
        offset += 16;
    }
    while offset < rgba_stream.len() {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        add_quality_histogram_u64(histogram, packed);
        offset += 4;
    }
}

fn index_rgba_frames_quality_u32<const BITS: usize>(
    rgba_stream: &[u8],
    alpha_threshold: u8,
) -> QualityIndexResult {
    let pixel_count = rgba_stream.len() / 4;
    let mut table = take_quality_color_index_table(COLOR_INDEX_CAP);
    let mut palette = Vec::with_capacity(256);
    // Quantized inputs discard the exact-prefix indices, so avoid reserving a
    // full-image buffer before the palette decision is known.
    let mut indexed = Vec::with_capacity(pixel_count.min(4_096));
    let mut has_transparent_pixels = false;
    let rgba_pointer = rgba_stream.as_ptr();
    let mut overflow_offset = None;
    for pixel_index in 0..pixel_count {
        let packed = u32::from_le(unsafe {
            std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
        });
        let alpha = (packed >> 24) as u8;
        if alpha < alpha_threshold {
            has_transparent_pixels = true;
            indexed.push(u8::MAX);
            if palette.len() == 256 {
                overflow_offset = Some((pixel_index + 1) * 4);
                break;
            }
            continue;
        }
        let red = packed as u8;
        let green = (packed >> 8) as u8;
        let blue = (packed >> 16) as u8;
        let rgb = rgb_key(red, green, blue);
        if let Some(index) = table.get(rgb) {
            indexed.push(index);
            continue;
        }
        let color_limit = if has_transparent_pixels { 255 } else { 256 };
        if palette.len() == color_limit {
            overflow_offset = Some((pixel_index + 1) * 4);
            break;
        }
        let index = palette.len() as u8;
        table.insert_if_absent(rgb, index);
        palette.push(rgb);
        indexed.push(index);
    }

    let histogram = if let Some(start_offset) = overflow_offset {
        // Keep the exact-prefix fast path allocation-free. Once overflow is
        // known, rebuild only that prefix and scan the remaining pixels once.
        let mut histogram = take_quality_histogram_u32(1 << (BITS * 3));
        let mut prefix_offset = 0usize;
        let rgba_pointer = rgba_stream.as_ptr();
        while prefix_offset < start_offset {
            let packed = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(prefix_offset).cast())
            });
            if ((packed >> 24) as u8) >= alpha_threshold {
                add_quality_histogram_u32_bits_const::<BITS>(&mut histogram, packed);
            }
            prefix_offset += 4;
        }
        has_transparent_pixels |= accumulate_quality_histogram_u32_bits_remaining::<BITS>(
            &mut histogram,
            rgba_stream,
            start_offset,
            alpha_threshold,
        );
        histogram
    } else {
        return QualityIndexResult::Exact(finish_quality_exact_indexed(
            rgba_stream,
            alpha_threshold,
            has_transparent_pixels,
            palette,
            table,
            indexed,
        ));
    };
    recycle_quantized_indexed(indexed);

    // This path is selected only while pixel_count <= u32::MAX / 255, so an
    // RGB channel sum (at most 255 * pixel_count) is guaranteed to fit u32.
    let colors = quality_colors_from_histogram_u32::<true>(&histogram);
    recycle_quality_color_index_table(table);
    recycle_quality_histogram_u32(histogram);
    QualityIndexResult::Quantized(build_quality_index_plan_from_colors(
        rgba_stream,
        alpha_threshold,
        has_transparent_pixels,
        colors,
        BITS,
    ))
}

fn index_rgba_frames_quality_u64(
    rgba_stream: &[u8],
    alpha_threshold: u8,
) -> QualityIndexResult {
    let pixel_count = rgba_stream.len() / 4;
    let mut histogram = take_quality_histogram_u64(QUALITY_HISTOGRAM_LEN);
    let mut table = take_quality_color_index_table(COLOR_INDEX_CAP);
    let mut palette = Vec::with_capacity(256);
    let mut indexed = Vec::with_capacity(pixel_count.min(4096));
    let mut has_transparent_pixels = false;
    let rgba_pointer = rgba_stream.as_ptr();
    let mut overflow_offset = None;
    for pixel_index in 0..pixel_count {
        let packed = u32::from_le(unsafe {
            std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
        });
        let alpha = (packed >> 24) as u8;
        if alpha < alpha_threshold {
            has_transparent_pixels = true;
            indexed.push(u8::MAX);
            if palette.len() == 256 {
                overflow_offset = Some((pixel_index + 1) * 4);
                break;
            }
            continue;
        }
        add_quality_histogram_u64(&mut histogram, packed);
        let red = packed as u8;
        let green = (packed >> 8) as u8;
        let blue = (packed >> 16) as u8;
        let rgb = rgb_key(red, green, blue);
        if let Some(index) = table.get(rgb) {
            indexed.push(index);
            continue;
        }
        let color_limit = if has_transparent_pixels { 255 } else { 256 };
        if palette.len() == color_limit {
            overflow_offset = Some((pixel_index + 1) * 4);
            break;
        }
        let index = palette.len() as u8;
        table.insert_if_absent(rgb, index);
        palette.push(rgb);
        indexed.push(index);
    }

    if let Some(start_offset) = overflow_offset {
        has_transparent_pixels |= accumulate_quality_histogram_u64_remaining(
            &mut histogram,
            rgba_stream,
            start_offset,
            alpha_threshold,
        );
    } else {
        recycle_quality_histogram_u64(histogram);
        return QualityIndexResult::Exact(finish_quality_exact_indexed(
            rgba_stream,
            alpha_threshold,
            has_transparent_pixels,
            palette,
            table,
            indexed,
        ));
    }

    let colors = quality_colors_from_histogram_u64(&histogram);
    recycle_quality_color_index_table(table);
    recycle_quality_histogram_u64(histogram);
    QualityIndexResult::Quantized(build_quality_index_plan_from_colors(
        rgba_stream,
        alpha_threshold,
        has_transparent_pixels,
        colors,
        QUALITY_HISTOGRAM_BITS,
    ))
}

fn build_quality_index_plan_from_colors(
    _rgba_stream: &[u8],
    _alpha_threshold: u8,
    has_transparent_pixels: bool,
    mut colors: Vec<QuantizedColor>,
    histogram_bits: usize,
) -> QualityIndexPlan {
    let mapping_bits = if histogram_bits == 5 && colors.len() > QUALITY_DOMINANT_COLOR_LIMIT {
        4
    } else {
        histogram_bits
    };
    let mapping_len = 1usize << (mapping_bits * 3);
    let opaque_color_limit = if has_transparent_pixels { 255 } else { 256 };

    let (mut palette, histogram_to_palette) = if colors.len() <= QUALITY_DOMINANT_COLOR_LIMIT {
        let palette_len = colors.len().min(opaque_color_limit);
        // Only the first palette_len colors can become palette entries. A
        // selection partition keeps the full color list for refinement while
        // avoiding a complete sort of the long tail of low-weight bins.
        let order = |left: &QuantizedColor, right: &QuantizedColor| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.histogram_index.cmp(&right.histogram_index))
        };
        if colors.len() > palette_len {
            let (_, _, _) = colors.select_nth_unstable_by(palette_len - 1, order);
            colors[..palette_len].sort_unstable_by(order);
        } else {
            colors.sort_unstable_by(order);
        }
        let mut palette = Vec::with_capacity(palette_len + usize::from(has_transparent_pixels));
        for color in colors.iter().take(palette_len) {
            palette.push(rgb_key(color.red, color.green, color.blue));
        }
        let initial_tree = PaletteKdTree::new(&palette);
        let initial_coarse_hints = initial_tree.coarse_hint_table(&palette);
        let mut histogram_to_palette = take_quality_histogram_to_palette(mapping_len);
        let mut palette_lookup = take_quality_color_index_table(COLOR_INDEX_CAP);
        for (index, &color) in palette.iter().enumerate() {
            palette_lookup.insert_if_absent(color, index as u8);
        }
        let mut counts = [0u64; 256];
        let mut red_sums = [0u64; 256];
        let mut green_sums = [0u64; 256];
        let mut blue_sums = [0u64; 256];
        for color in &colors {
            let rgb = rgb_key(color.red, color.green, color.blue);
            let index = if let Some(index) = palette_lookup.get(rgb) {
                usize::from(index)
            } else {
                let coarse_index = (usize::from(color.red >> 5) << 6)
                    | (usize::from(color.green >> 5) << 3)
                    | usize::from(color.blue >> 5);
                let hint_index = initial_coarse_hints[coarse_index];
                usize::from(initial_tree.nearest_with_hint(
                    color.red,
                    color.green,
                    color.blue,
                    Some((hint_index, palette[usize::from(hint_index)])),
                ))
            };
            histogram_to_palette[usize::from(color.histogram_index)] = index as u8;
            counts[index] += color.count;
            red_sums[index] += u64::from(color.red) * color.count;
            green_sums[index] += u64::from(color.green) * color.count;
            blue_sums[index] += u64::from(color.blue) * color.count;
        }
        let mut palette_changed = false;
        for index in 0..palette.len() {
            let count = counts[index];
            if count == 0 {
                continue;
            }
            let representative = rgb_key(
                ((red_sums[index] + count / 2) / count) as u8,
                ((green_sums[index] + count / 2) / count) as u8,
                ((blue_sums[index] + count / 2) / count) as u8,
            );
            palette_changed |= palette[index] != representative;
            palette[index] = representative;
        }
        if palette_changed {
            let palette_tree = PaletteKdTree::new(&palette);
            palette_lookup.reset(COLOR_INDEX_CAP);
            for (index, &color) in palette.iter().enumerate() {
                palette_lookup.insert_if_absent(color, index as u8);
            }
            for color in &colors {
                // The first-pass assignment is a much tighter exact seed
                // than a coarse cell-center lookup after representatives move
                // only one refinement step. The KD search still proves the
                // final nearest color, so this changes no output or tie rule.
                let hint_index = histogram_to_palette[usize::from(color.histogram_index)];
                let rgb = rgb_key(color.red, color.green, color.blue);
                histogram_to_palette[usize::from(color.histogram_index)] =
                    palette_lookup.get(rgb).unwrap_or_else(|| {
                        palette_tree.nearest_with_hint(
                            color.red,
                            color.green,
                            color.blue,
                            Some((hint_index, palette[usize::from(hint_index)])),
                        )
                    });
            }
        }
        recycle_quality_color_index_table(palette_lookup);
        recycle_quality_colors(colors);
        (palette, histogram_to_palette)
    } else {
        let mut colors = colors;
        let mut boxes = vec![QuantizedColorArenaBox::new(0, colors.len(), &colors)];
        while boxes.len() < opaque_color_limit {
            let Some((split_index, _)) = boxes
                .iter()
                .enumerate()
                .filter(|(_, color_box)| color_box.len() > 1)
                .max_by_key(|(_, color_box)| color_box.score)
            else {
                break;
            };
            let mut color_box = boxes.swap_remove(split_index);
            if let Some(right) = color_box.split(&mut colors) {
                boxes.push(color_box);
                boxes.push(right);
            } else {
                boxes.push(color_box);
                break;
            }
        }
        for color_box in &mut boxes {
            color_box.calculate_representative(&colors);
        }
        boxes.sort_unstable_by_key(|color_box| color_box.representative);

        let mut palette = Vec::with_capacity(boxes.len() + usize::from(has_transparent_pixels));
        for color_box in &boxes {
            palette.push(color_box.representative);
        }

        let mut histogram_to_palette = take_quality_histogram_to_palette(mapping_len);
        if histogram_bits == 4 {
            let palette_tree = PaletteKdTree::new(&palette);
            let coarse_hints = palette_tree.coarse_hint_table(&palette);
            let mut coarse_nearest = [0u8; 1 << 12];
            for red in 0..16u8 {
                for green in 0..16u8 {
                    for blue in 0..16u8 {
                        let index =
                            (usize::from(red) << 8) | (usize::from(green) << 4) | usize::from(blue);
                        let hint_index = coarse_hints[(usize::from(red >> 1) << 6)
                            | (usize::from(green >> 1) << 3)
                            | usize::from(blue >> 1)];
                        coarse_nearest[index] = palette_tree.nearest_with_hint_split(
                            (red << 4) | 8,
                            (green << 4) | 8,
                            (blue << 4) | 8,
                            Some((hint_index, palette[usize::from(hint_index)])),
                        );
                    }
                }
            }
            for color_box in &boxes {
                for color in &colors[color_box.start..color_box.end] {
                    let index = (usize::from(color.red >> 4) << 8)
                        | (usize::from(color.green >> 4) << 4)
                        | usize::from(color.blue >> 4);
                    histogram_to_palette[usize::from(color.histogram_index)] =
                        coarse_nearest[index];
                }
            }
        } else {
            let palette_tree = PaletteKdTree::new(&palette);
            let coarse_hints = palette_tree.coarse_hint_table(&palette);
            let mut coarse_nearest = [0u8; 1 << 12];
            for red in 0..16u8 {
                for green in 0..16u8 {
                    for blue in 0..16u8 {
                        let index =
                            (usize::from(red) << 8) | (usize::from(green) << 4) | usize::from(blue);
                        let hint_index = coarse_hints[(usize::from(red >> 1) << 6)
                            | (usize::from(green >> 1) << 3)
                            | usize::from(blue >> 1)];
                        coarse_nearest[index] = palette_tree.nearest_with_hint_split(
                            (red << 4) | 8,
                            (green << 4) | 8,
                            (blue << 4) | 8,
                            Some((hint_index, palette[usize::from(hint_index)])),
                        );
                    }
                }
            }
            for color_box in &boxes {
                for color in &colors[color_box.start..color_box.end] {
                    let coarse_index = (usize::from(color.red >> 4) << 8)
                        | (usize::from(color.green >> 4) << 4)
                        | usize::from(color.blue >> 4);
                    let mapping_index = if histogram_bits == 5 {
                        coarse_index
                    } else {
                        usize::from(color.histogram_index)
                    };
                    histogram_to_palette[mapping_index] = coarse_nearest[coarse_index];
                }
            }
        }
        recycle_quality_colors(colors);
        (palette, histogram_to_palette)
    };
    let transparent_index = if has_transparent_pixels {
        let index = palette.len() as u8;
        palette.push(0);
        Some(index)
    } else {
        None
    };
    if palette.is_empty() {
        palette.push(0);
    }

    QualityIndexPlan {
        palette,
        histogram_to_palette,
        transparent_index,
        mapping_bits,
    }
}

fn try_index_rgba_frames_exact(
    rgba_stream: &[u8],
    alpha_threshold: u8,
    exact_only: bool,
) -> Result<Option<(Vec<u32>, Vec<u8>, Option<u8>)>, String> {
    let mut table = ColorIndexTable::new(COLOR_INDEX_CAP);
    let mut palette = Vec::with_capacity(256);
    let mut has_transparent_pixels = false;
    let rgba_pointer = rgba_stream.as_ptr();

    let mut offset = 0usize;
    while offset < rgba_stream.len() {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        let alpha = (packed >> 24) as u8;
        if exact_only && alpha != 0 && alpha != 255 {
            return Err(
                "Pixel-perfect GIF encoding requires alpha values of exactly 0 or 255".to_string(),
            );
        }
        if alpha < alpha_threshold {
            has_transparent_pixels = true;
            offset += 4;
            continue;
        }
        let rgb = rgb_key(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8);
        if table.get(rgb).is_some() {
            offset += 4;
            continue;
        }
        if palette.len() == if has_transparent_pixels { 255 } else { 256 } {
            return Ok(None);
        }

        let index = palette.len() as u8;
        table.insert_if_absent(rgb, index);
        palette.push(rgb);
        offset += 4;
    }

    if has_transparent_pixels && palette.len() == 256 {
        return Ok(None);
    }

    let transparent_index = if has_transparent_pixels {
        let index = palette.len() as u8;
        palette.push(0);
        Some(index)
    } else {
        None
    };

    let mut indexed = Vec::with_capacity(rgba_stream.len() / 4);
    offset = 0;
    while offset < rgba_stream.len() {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        if ((packed >> 24) as u8) < alpha_threshold {
            indexed.push(transparent_index.unwrap());
            offset += 4;
            continue;
        }
        let rgb = rgb_key(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8);
        indexed.push(table.get(rgb).unwrap());
        offset += 4;
    }

    Ok(Some((palette, indexed, transparent_index)))
}

fn index_rgba_frames_to_palette(
    rgba_stream: &[u8],
    palette_rgb: &[u32],
    alpha_threshold: u8,
    exact_only: bool,
) -> Result<(Vec<u32>, Vec<u8>, Option<u8>), String> {
    let mapper = PaletteMapper::new(palette_rgb);
    let mut palette: Vec<u32> = palette_rgb
        .iter()
        .map(|color| color & 0x00ff_ffff)
        .collect();
    if !rgba_stream_has_transparent_pixels(rgba_stream, alpha_threshold) {
        let mut indexed = Vec::with_capacity(rgba_stream.len() / 4);
        if exact_only {
            for pixel in rgba_stream.chunks_exact(4) {
                if pixel[3] != 255 {
                    return Err(
                        "Pixel-perfect GIF encoding requires alpha values of exactly 0 or 255"
                            .to_string(),
                    );
                }
                indexed.push(
                    mapper
                        .exact_index(pixel[0], pixel[1], pixel[2])
                        .ok_or_else(|| {
                            "Pixel-perfect GIF encoding found an RGBA color outside the supplied palette"
                                .to_string()
                        })?,
                );
            }
        } else {
            map_rgba_frame_to_palette(rgba_stream, &mapper, &mut indexed);
        }
        return Ok((palette, indexed, None));
    }

    let mut used_indexes = vec![false; palette_rgb.len()];
    let mut indexed = Vec::with_capacity(rgba_stream.len() / 4);

    let mut offset = 0usize;
    while offset < rgba_stream.len() {
        if exact_only && rgba_stream[offset + 3] != 0 && rgba_stream[offset + 3] != 255 {
            return Err(
                "Pixel-perfect GIF encoding requires alpha values of exactly 0 or 255".to_string(),
            );
        }
        if rgba_stream[offset + 3] < alpha_threshold {
            indexed.push(0);
            offset += 4;
            continue;
        }
        let index = if exact_only {
            mapper
                .exact_index(
                    rgba_stream[offset],
                    rgba_stream[offset + 1],
                    rgba_stream[offset + 2],
                )
                .ok_or_else(|| {
                    "Pixel-perfect GIF encoding found an RGBA color outside the supplied palette"
                        .to_string()
                })?
        } else {
            mapper.index_pixel(
                rgba_stream[offset],
                rgba_stream[offset + 1],
                rgba_stream[offset + 2],
            )
        } as usize;
        used_indexes[index] = true;
        indexed.push(index as u8);
        offset += 4;
    }

    let transparent_index = if palette.len() < 256 {
        let index = palette.len() as u8;
        palette.push(0);
        Some(index)
    } else {
        Some(
            used_indexes
                .iter()
                .position(|used| !*used)
                .map(|index| index as u8)
                .ok_or_else(|| {
                    "RGBA frames contain transparent pixels, but the palette has no unused transparent slot."
                        .to_string()
                })?,
        )
    };

    if let Some(transparent_index) = transparent_index {
        for (pixel, index) in rgba_stream.chunks_exact(4).zip(indexed.iter_mut()) {
            if pixel[3] < alpha_threshold {
                *index = transparent_index;
            }
        }
    }

    Ok((palette, indexed, transparent_index))
}

fn map_rgba_frame_to_palette(
    rgba_stream: &[u8],
    mapper: &PaletteMapper<'_>,
    indexed: &mut Vec<u8>,
) {
    indexed.clear();
    let mut offset = 0usize;
    while offset < rgba_stream.len() {
        indexed.push(mapper.index_pixel(
            rgba_stream[offset],
            rgba_stream[offset + 1],
            rgba_stream[offset + 2],
        ));
        offset += 4;
    }
}

fn map_rgba_rect_to_palette(
    rgba_stream: &[u8],
    canvas_width: usize,
    rect: ChangedRectU32,
    mapper: &PaletteMapper<'_>,
    indexed: &mut Vec<u8>,
) {
    indexed.clear();
    let row_skip = (canvas_width - rect.width) * 4;
    let mut offset = (rect.y * canvas_width + rect.x) * 4;
    for _ in 0..rect.height {
        for _ in 0..rect.width {
            indexed.push(mapper.index_pixel(
                rgba_stream[offset],
                rgba_stream[offset + 1],
                rgba_stream[offset + 2],
            ));
            offset += 4;
        }
        offset += row_skip;
    }
}

fn rgba_stream_has_transparent_pixels(rgba_stream: &[u8], alpha_threshold: u8) -> bool {
    let mut offset = 3usize;
    while offset < rgba_stream.len() {
        if rgba_stream[offset] < alpha_threshold {
            return true;
        }
        offset += 4;
    }
    false
}

fn index_rgba_frames_332(
    rgba_stream: &[u8],
    alpha_threshold: u8,
) -> (Vec<u32>, Vec<u8>, Option<u8>) {
    let mut indexed = Vec::with_capacity(rgba_stream.len() / 4);
    let mut offset = 0usize;
    let mut has_transparent_pixels = false;
    while offset < rgba_stream.len() {
        if rgba_stream[offset + 3] < alpha_threshold {
            has_transparent_pixels = true;
            indexed.push(255);
        } else {
            indexed.push(rgb332_index(
                rgba_stream[offset],
                rgba_stream[offset + 1],
                rgba_stream[offset + 2],
            ));
        }
        offset += 4;
    }
    if has_transparent_pixels {
        let transparent_index = 255u8;
        for (pixel, index) in rgba_stream.chunks_exact(4).zip(indexed.iter_mut()) {
            if pixel[3] < alpha_threshold {
                *index = transparent_index;
            } else if *index == transparent_index {
                *index = transparent_index - 1;
            }
        }
        let mut palette = fixed_332_palette();
        palette[usize::from(transparent_index)] = 0;
        return (palette, indexed, Some(transparent_index));
    }
    (fixed_332_palette(), indexed, None)
}

#[inline]
fn rgb_key(r: u8, g: u8, b: u8) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

#[inline]
fn rgb332_index(r: u8, g: u8, b: u8) -> u8 {
    (r & 0xe0) | ((g >> 3) & 0x1c) | (b >> 6)
}

fn fixed_332_palette() -> Vec<u32> {
    let mut palette = Vec::with_capacity(256);
    for index in 0..256u32 {
        let r = (((index >> 5) & 0x07) * 255 + 3) / 7;
        let g = (((index >> 2) & 0x07) * 255 + 3) / 7;
        let b = ((index & 0x03) * 255 + 1) / 3;
        palette.push((r << 16) | (g << 8) | b);
    }
    palette
}

fn nearest_palette_index(r: u8, g: u8, b: u8, palette_rgb: &[u32]) -> u8 {
    let r = i32::from(r);
    let g = i32::from(g);
    let b = i32::from(b);
    let mut best_index = 0u8;
    let mut best_distance = i32::MAX;

    for (index, color) in palette_rgb.iter().enumerate() {
        let pr = ((*color >> 16) & 0xff) as i32;
        let pg = ((*color >> 8) & 0xff) as i32;
        let pb = (*color & 0xff) as i32;
        let dr = r - pr;
        let dg = g - pg;
        let db = b - pb;
        let distance = dr * dr + dg * dg + db * db;
        if distance < best_distance {
            best_distance = distance;
            best_index = index as u8;
            if distance == 0 {
                break;
            }
        }
    }

    best_index
}

#[derive(Clone, Copy)]
struct PaletteKdNode {
    red: u8,
    green: u8,
    blue: u8,
    palette_index: u8,
    axis: u8,
    split: u8,
    min_rgb: u32,
    max_rgb: u32,
    left: usize,
    right: usize,
}

const PALETTE_KD_EMPTY: usize = usize::MAX;

struct PaletteKdTree {
    nodes: Vec<PaletteKdNode>,
    root: usize,
}

impl PaletteKdTree {
    fn new(palette_rgb: &[u32]) -> Self {
        fn component(color: u32, axis: u8) -> u8 {
            match axis {
                0 => (color >> 16) as u8,
                1 => (color >> 8) as u8,
                _ => color as u8,
            }
        }

        fn build(
            indices: &mut [usize],
            palette_rgb: &[u32],
            nodes: &mut Vec<PaletteKdNode>,
        ) -> usize {
            if indices.is_empty() {
                return PALETTE_KD_EMPTY;
            }
            let mut min_red = u8::MAX;
            let mut min_green = u8::MAX;
            let mut min_blue = u8::MAX;
            let mut max_red = 0u8;
            let mut max_green = 0u8;
            let mut max_blue = 0u8;
            for &index in indices.iter() {
                let color = palette_rgb[index];
                let red = (color >> 16) as u8;
                let green = (color >> 8) as u8;
                let blue = color as u8;
                min_red = min_red.min(red);
                min_green = min_green.min(green);
                min_blue = min_blue.min(blue);
                max_red = max_red.max(red);
                max_green = max_green.max(green);
                max_blue = max_blue.max(blue);
            }
            let red_range = max_red - min_red;
            let green_range = max_green - min_green;
            let blue_range = max_blue - min_blue;
            let axis = if red_range >= green_range && red_range >= blue_range {
                0
            } else if green_range >= blue_range {
                1
            } else {
                2
            };
            let midpoint = indices.len() / 2;
            let (_, pivot, _) = indices.select_nth_unstable_by(midpoint, |left, right| {
                component(palette_rgb[*left], axis).cmp(&component(palette_rgb[*right], axis))
            });
            let palette_index = *pivot;
            let left = build(&mut indices[..midpoint], palette_rgb, nodes);
            let right = build(&mut indices[midpoint + 1..], palette_rgb, nodes);
            let node_index = nodes.len();
            let color = palette_rgb[palette_index];
            nodes.push(PaletteKdNode {
                red: (color >> 16) as u8,
                green: (color >> 8) as u8,
                blue: color as u8,
                palette_index: palette_index as u8,
                axis,
                split: component(color, axis),
                min_rgb: rgb_key(min_red, min_green, min_blue),
                max_rgb: rgb_key(max_red, max_green, max_blue),
                left,
                right,
            });
            node_index
        }

        debug_assert!(palette_rgb.len() <= 256);
        let mut indices = [0usize; 256];
        for (index, value) in indices.iter_mut().take(palette_rgb.len()).enumerate() {
            *value = index;
        }
        let mut nodes = Vec::with_capacity(palette_rgb.len());
        let root = build(&mut indices[..palette_rgb.len()], palette_rgb, &mut nodes);
        Self { nodes, root }
    }

    /// Build a coarse 3-bit/channel nearest-color table used only as an exact
    /// search seed. The final KD-tree lookup still proves the true nearest
    /// palette entry, but starting from the cell's candidate sharply tightens
    /// its branch-and-bound distance for dense 5-bit histograms.
    fn coarse_hint_table(&self, palette_rgb: &[u32]) -> [u8; 512] {
        let mut table = [0u8; 512];
        let mut occupied = [false; 512];
        let mut best_distance = [u32::MAX; 512];
        for (palette_index, &color) in palette_rgb.iter().enumerate() {
            let red = (color >> 16) as u8;
            let green = (color >> 8) as u8;
            let blue = color as u8;
            let cell = (usize::from(red >> 5) << 6)
                | (usize::from(green >> 5) << 3)
                | usize::from(blue >> 5);
            let dr = i32::from(red) - i32::from((red >> 5) * 32 + 16);
            let dg = i32::from(green) - i32::from((green >> 5) * 32 + 16);
            let db = i32::from(blue) - i32::from((blue >> 5) * 32 + 16);
            let distance = (dr * dr + dg * dg + db * db) as u32;
            if distance < best_distance[cell] {
                best_distance[cell] = distance;
                table[cell] = palette_index as u8;
                occupied[cell] = true;
            }
        }
        for red in 0..8u8 {
            for green in 0..8u8 {
                for blue in 0..8u8 {
                    let index = usize::from(red) << 6 | usize::from(green) << 3 | usize::from(blue);
                    if !occupied[index] {
                        table[index] = self.nearest_with_hint(
                            (red << 5) | 16,
                            (green << 5) | 16,
                            (blue << 5) | 16,
                            None,
                        );
                    }
                }
            }
        }
        table
    }

    #[cfg(test)]
    #[inline]
    fn nearest(&self, r: u8, g: u8, b: u8) -> u8 {
        self.nearest_with_hint(r, g, b, None)
    }

    #[inline]
    fn nearest_with_hint(&self, r: u8, g: u8, b: u8, hint: Option<(u8, u32)>) -> u8 {
        self.nearest_with_hint_impl::<true>(r, g, b, hint)
    }

    #[inline]
    fn nearest_with_hint_split(&self, r: u8, g: u8, b: u8, hint: Option<(u8, u32)>) -> u8 {
        self.nearest_with_hint_impl::<false>(r, g, b, hint)
    }

    #[inline(always)]
    fn nearest_with_hint_impl<const USE_BOUNDS: bool>(
        &self,
        r: u8,
        g: u8,
        b: u8,
        hint: Option<(u8, u32)>,
    ) -> u8 {
        #[inline(always)]
        fn distance(color: u32, r: i32, g: i32, b: i32) -> u32 {
            let dr = r - ((color >> 16) & 0xff) as i32;
            let dg = g - ((color >> 8) & 0xff) as i32;
            let db = b - (color & 0xff) as i32;
            (dr * dr + dg * dg + db * db) as u32
        }

        if self.root == PALETTE_KD_EMPTY {
            return 0;
        }
        let r = i32::from(r);
        let g = i32::from(g);
        let b = i32::from(b);
        let (mut best_index, mut best_distance) = hint
            .map(|(index, color)| (index, distance(color, r, g, b)))
            .unwrap_or((0, u32::MAX));
        if best_distance == 0 {
            return best_index;
        }
        // A balanced 256-entry tree has depth at most eight; one slot per
        // level is enough for pending far branches without zeroing a larger
        // stack for every histogram color lookup.
        // The stack entries are written before they are read.  Avoiding a
        // per-lookup zeroing pass matters here because the median-cut mapper
        // performs one nearest-color search for every occupied histogram bin.
        let mut stack = [std::mem::MaybeUninit::<(usize, u32)>::uninit(); 8];
        let mut stack_len = 0usize;
        let mut next = self.root;
        while next != PALETTE_KD_EMPTY {
            let node_index = next;
            let node = self.nodes[node_index];
            let dr = r - i32::from(node.red);
            let dg = g - i32::from(node.green);
            let db = b - i32::from(node.blue);
            let node_distance = (dr * dr + dg * dg + db * db) as u32;
            if node_distance < best_distance
                || (node_distance == best_distance && node.palette_index < best_index)
            {
                best_distance = node_distance;
                best_index = node.palette_index;
            }
            let value = match node.axis {
                0 => r,
                1 => g,
                _ => b,
            };
            let split = i32::from(node.split);
            let (near, far) = if value < split {
                (node.left, node.right)
            } else {
                (node.right, node.left)
            };
            if far != PALETTE_KD_EMPTY {
                let far_distance = if USE_BOUNDS {
                    // Each node carries the exact RGB bounds of its subtree.
                    // The box distance is a stronger lower bound than the
                    // split plane when a far subtree is separated on another
                    // channel.
                    let far_node = self.nodes[far];
                    let min_red = ((far_node.min_rgb >> 16) & 0xff) as i32;
                    let min_green = ((far_node.min_rgb >> 8) & 0xff) as i32;
                    let min_blue = (far_node.min_rgb & 0xff) as i32;
                    let max_red = ((far_node.max_rgb >> 16) & 0xff) as i32;
                    let max_green = ((far_node.max_rgb >> 8) & 0xff) as i32;
                    let max_blue = (far_node.max_rgb & 0xff) as i32;
                    let red_distance = if r < min_red {
                        min_red - r
                    } else if r > max_red {
                        r - max_red
                    } else {
                        0
                    };
                    let green_distance = if g < min_green {
                        min_green - g
                    } else if g > max_green {
                        g - max_green
                    } else {
                        0
                    };
                    let blue_distance = if b < min_blue {
                        min_blue - b
                    } else if b > max_blue {
                        b - max_blue
                    } else {
                        0
                    };
                    (red_distance * red_distance
                        + green_distance * green_distance
                        + blue_distance * blue_distance)
                        as u32
                } else {
                    let delta = (value - split).unsigned_abs();
                    (delta * delta) as u32
                };
                if far_distance <= best_distance {
                    debug_assert!(stack_len < stack.len());
                    stack[stack_len].write((far, far_distance));
                    stack_len += 1;
                }
            }
            next = near;
            if next == PALETTE_KD_EMPTY {
                while stack_len > 0 {
                    stack_len -= 1;
                    let (far_index, far_distance) = unsafe { stack[stack_len].assume_init() };
                    if far_distance <= best_distance {
                        next = far_index;
                        break;
                    }
                }
            }
        }
        best_index
    }
}

fn encode_indexed_gif_inner(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    transparent_index: Option<u8>,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("Width/Height invalid".to_string());
    }
    if frame_count == 0 {
        return Err("Frame count must be greater than zero".to_string());
    }
    if loop_count < -1 || loop_count > i32::from(u16::MAX) {
        return Err("Loop count invalid".to_string());
    }
    delays.validate(frame_count)?;

    let color_count = checked_palette_color_count(palette_rgb.len())?;
    let color_table_size_bits = (log2_pow2(color_count) as u8 - 1) & 7;
    let min_code_size = (log2_pow2(color_count) as u8).max(2);
    let frame_len = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| "Frame size overflow".to_string())?;
    let expected_len = frame_len
        .checked_mul(frame_count)
        .ok_or_else(|| "Frame stream overflow".to_string())?;
    if index_stream.len() != expected_len {
        return Err("Indexed frame stream length does not match dimensions".to_string());
    }

    let palette_bytes = color_count
        .checked_mul(3)
        .ok_or_else(|| "Palette size overflow".to_string())?;
    let estimated_frame_bytes = frame_len / 2 + 32;
    let mut output = Vec::with_capacity(
        13 + palette_bytes + 20 + frame_count.saturating_mul(estimated_frame_bytes) + 1,
    );
    output.extend_from_slice(b"GIF89a");
    push_u16_le(&mut output, width);
    push_u16_le(&mut output, height);
    output.push(0x80 | color_table_size_bits);
    output.push(0);
    output.push(0);

    for index in 0..color_count {
        let rgb = palette_rgb.get(index).copied().unwrap_or(0);
        output.push(((rgb >> 16) & 0xff) as u8);
        output.push(((rgb >> 8) & 0xff) as u8);
        output.push((rgb & 0xff) as u8);
    }

    if loop_count >= 0 {
        output.extend_from_slice(&[
            0x21, 0xff, 0x0b, b'N', b'E', b'T', b'S', b'C', b'A', b'P', b'E', b'2', b'.', b'0',
            0x03, 0x01,
        ]);
        push_u16_le(&mut output, loop_count as u16);
        output.push(0);
    }

    REUSABLE_LZW_TABLES.with(|tables| {
        let mut tables = tables.borrow_mut();
        for (frame_index, frame) in index_stream.chunks_exact(frame_len).enumerate() {
            write_indexed_gif_frame_header(
                &mut output,
                0,
                0,
                width,
                height,
                delays.get(frame_index),
                transparent_index,
                if transparent_index.is_some() { 2 } else { 0 },
            );
            encode_indexed_lzw_to_with_tables(
                &mut output,
                frame,
                min_code_size,
                color_count,
                &mut tables,
            )?;
        }
        Ok::<(), String>(())
    })?;

    output.push(0x3b);
    Ok(output)
}

fn encode_indexed_literal_gif_inner(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    transparent_index: Option<u8>,
) -> Result<Vec<u8>, String> {
    encode_indexed_literal_gif_inner_with_output(
        Vec::new(),
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        delays,
        loop_count,
        transparent_index,
    )
}

fn encode_indexed_literal_gif_inner_with_output(
    mut output: Vec<u8>,
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    transparent_index: Option<u8>,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("Width/Height invalid".to_string());
    }
    if frame_count == 0 {
        return Err("Frame count must be greater than zero".to_string());
    }
    if loop_count < -1 || loop_count > i32::from(u16::MAX) {
        return Err("Loop count invalid".to_string());
    }
    delays.validate(frame_count)?;

    let color_count = checked_palette_color_count(palette_rgb.len())?;
    let min_code_size = (log2_pow2(color_count) as u8).max(2);
    let frame_len = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| "Frame size overflow".to_string())?;
    let expected_len = frame_len
        .checked_mul(frame_count)
        .ok_or_else(|| "Frame stream overflow".to_string())?;
    if index_stream.len() != expected_len {
        return Err("Indexed frame stream length does not match dimensions".to_string());
    }

    #[cfg(not(target_arch = "wasm32"))]
    if min_code_size > 2 && frame_count >= 8 && expected_len >= 50_000 {
        return encode_indexed_literal_gif_parallel_native(
            index_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delays,
            loop_count,
            transparent_index,
            color_count,
            min_code_size,
            frame_len,
        );
    }

    let palette_bytes = color_count
        .checked_mul(3)
        .ok_or_else(|| "Palette size overflow".to_string())?;
    let lzw_length = literal_lzw_block_size(frame_len, min_code_size)?;
    let frame_capacity = (0..frame_count).try_fold(0usize, |capacity, frame_index| {
        let graphic_control_length =
            usize::from(delays.get(frame_index) != 0 || transparent_index.is_some()) * 8;
        capacity
            .checked_add(10)
            .and_then(|length| length.checked_add(graphic_control_length))
            .and_then(|length| length.checked_add(lzw_length))
            .ok_or_else(|| "Encoded GIF size overflow".to_string())
    })?;
    let output_capacity =
        13 + palette_bytes + usize::from(loop_count >= 0) * 19 + frame_capacity + 1;
    output.clear();
    if output.capacity() < output_capacity {
        output.reserve(output_capacity - output.capacity());
    }
    write_indexed_gif_header(&mut output, width, height, palette_rgb, color_count);
    write_loop_extension(&mut output, loop_count);
    for (frame_index, frame) in index_stream.chunks_exact(frame_len).enumerate() {
        write_indexed_gif_frame_header(
            &mut output,
            0,
            0,
            width,
            height,
            delays.get(frame_index),
            transparent_index,
            if transparent_index.is_some() { 2 } else { 0 },
        );
        encode_indexed_literal_lzw_direct_to(&mut output, frame, min_code_size, color_count)?;
    }
    output.push(0x3b);
    Ok(output)
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn encode_indexed_literal_gif_parallel_native(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    transparent_index: Option<u8>,
    color_count: usize,
    min_code_size: u8,
    frame_len: usize,
) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    write_indexed_gif_header(&mut output, width, height, palette_rgb, color_count);
    write_loop_extension(&mut output, loop_count);
    let lzw_length = literal_lzw_block_size(frame_len, min_code_size)?;
    let mut output_length = output.len();
    let mut frame_layout = Vec::with_capacity(frame_count);
    for frame_index in 0..frame_count {
        let gce_length =
            usize::from(delays.get(frame_index) != 0 || transparent_index.is_some()) * 8;
        let frame_length = 10usize
            .checked_add(gce_length)
            .and_then(|length| length.checked_add(lzw_length))
            .ok_or_else(|| "Encoded GIF size overflow".to_string())?;
        frame_layout.push((output_length, frame_length));
        output_length = output_length
            .checked_add(frame_length)
            .ok_or_else(|| "Encoded GIF size overflow".to_string())?;
    }
    output_length = output_length
        .checked_add(1)
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?;
    output.reserve_exact(output_length - output.len());
    unsafe {
        output.set_len(output_length);
    }
    let output_address = output.as_mut_ptr() as usize;
    let frame_results: Vec<std::sync::OnceLock<Result<(), String>>> = (0..frame_count)
        .map(|_| std::sync::OnceLock::new())
        .collect();
    let next_frame = std::sync::atomic::AtomicUsize::new(0);
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let thread_count = available_threads.min(frame_count);

    let joined = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..thread_count)
            .map(|_| {
                let next_frame = &next_frame;
                let frame_results = &frame_results;
                let frame_layout = &frame_layout;
                scope.spawn(move || loop {
                    let frame_index = next_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if frame_index >= frame_count {
                        break;
                    }
                    let result = (|| {
                        let frame =
                            &index_stream[frame_index * frame_len..(frame_index + 1) * frame_len];
                        let (destination, expected_length) = frame_layout[frame_index];
                        let mut chunk = Vec::with_capacity(expected_length);
                        write_indexed_gif_frame_header(
                            &mut chunk,
                            0,
                            0,
                            width,
                            height,
                            delays.get(frame_index),
                            transparent_index,
                            if transparent_index.is_some() { 2 } else { 0 },
                        );
                        let mut compressed_scratch = Vec::new();
                        encode_indexed_literal_lzw_to(
                            &mut chunk,
                            frame,
                            min_code_size,
                            color_count,
                            &mut compressed_scratch,
                        )?;
                        if chunk.len() != expected_length {
                            return Err("Predicted encoded frame size differs".to_string());
                        }
                        unsafe {
                            std::ptr::copy_nonoverlapping(
                                chunk.as_ptr(),
                                (output_address as *mut u8).add(destination),
                                expected_length,
                            );
                        }
                        Ok(())
                    })();
                    let _ = frame_results[frame_index].set(result);
                })
            })
            .collect();
        handles.into_iter().all(|handle| handle.join().is_ok())
    });
    if !joined {
        return Err("Parallel GIF encoder panicked".to_string());
    }
    for frame_result in frame_results {
        frame_result
            .into_inner()
            .ok_or_else(|| "Parallel GIF encoder skipped a frame".to_string())??;
    }
    output[output_length - 1] = 0x3b;
    Ok(output)
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn try_reencode_parallel_gif_into_host(
    data: &[u8],
    metadata: &GifMetadata,
    loop_count: i32,
    allocate: NativeRgbaAllocator,
    allocate_context: *mut std::ffi::c_void,
) -> Result<Option<(*mut u8, usize)>, String> {
    let total_frame_pixels = metadata.frames.iter().try_fold(0usize, |total, frame| {
        usize::from(frame.width)
            .checked_mul(usize::from(frame.height))
            .and_then(|pixels| total.checked_add(pixels))
            .ok_or_else(|| "Decoded frame size overflow".to_string())
    })?;
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let thread_cap = if total_frame_pixels < 100_000 {
        6
    } else if metadata.frames.len() <= 12 {
        6
    } else {
        usize::MAX
    };
    let thread_count = available_threads.min(metadata.frames.len()).min(thread_cap);
    if thread_count <= 1 || metadata.frames.len() < 8 || total_frame_pixels < 30_000 {
        return Ok(None);
    }

    let global_palette_length = metadata
        .global_palette_size
        .checked_mul(3)
        .ok_or_else(|| "Global palette size overflow".to_string())?;
    let header_length = 13usize
        .checked_add(global_palette_length)
        .and_then(|length| length.checked_add(usize::from(loop_count >= 0) * 19))
        .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    let mut output_length = header_length;
    let mut frame_layout = Vec::with_capacity(metadata.frames.len());
    for frame in &metadata.frames {
        let frame_length = reencoded_frame_literal_size(metadata, frame)?;
        frame_layout.push((output_length, frame_length));
        output_length = output_length
            .checked_add(frame_length)
            .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    }
    output_length = output_length
        .checked_add(1)
        .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;

    let output_pointer = allocate(allocate_context, output_length);
    if output_pointer.is_null() {
        return Err("Could not allocate host GIF output".to_string());
    }
    let mut output =
        std::mem::ManuallyDrop::new(Vec::from_raw_parts(output_pointer, 0, output_length));
    write_reencoded_gif_header_to(&mut output, data, metadata, loop_count)?;
    if output.len() != header_length {
        return Err("Predicted host GIF header size differs".to_string());
    }
    output.set_len(output_length);
    fill_reencoded_gif_literal_parallel_native(
        data,
        metadata,
        &mut output,
        &frame_layout,
        output_length,
        thread_count,
    )?;
    if output.as_mut_ptr() != output_pointer {
        return Err("Host GIF output unexpectedly reallocated".to_string());
    }
    Ok(Some((output_pointer, output_length)))
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn try_reencode_small_gif_into_host(
    data: &[u8],
    allocate: NativeRgbaAllocator,
    allocate_context: *mut std::ffi::c_void,
) -> Result<Option<(*mut u8, usize)>, String> {
    if data.len() < 13 || data.len() > 32_768 {
        return Ok(None);
    }
    let version = match &data[0..6] {
        b"GIF87a" => "GIF87a",
        b"GIF89a" => "GIF89a",
        _ => return Err("Invalid GIF signature".to_string()),
    };
    let width = read_u16(data, 6, "logical screen width")?;
    let height = read_u16(data, 8, "logical screen height")?;
    if !matches!(
        usize::from(width).checked_mul(usize::from(height)),
        Some(pixels) if pixels <= 4_096
    ) {
        return Ok(None);
    }
    let packed = data[10];
    let has_global_palette = (packed & 0x80) != 0;
    let global_palette_size = if has_global_palette {
        2usize << usize::from(packed & 0x07)
    } else {
        0
    };
    let global_palette_offset = has_global_palette.then_some(13);
    let mut offset = 13usize;
    if has_global_palette {
        offset = checked_add(
            offset,
            global_palette_size * 3,
            data.len(),
            "global color table",
        )?;
    }

    let mut frame_storage =
        std::array::from_fn::<_, 16, _>(|_| std::mem::MaybeUninit::<FrameMetadata>::uninit());
    let mut frames = std::mem::ManuallyDrop::new(Vec::from_raw_parts(
        frame_storage.as_mut_ptr().cast::<FrameMetadata>(),
        0,
        frame_storage.len(),
    ));
    let mut graphic_control = GraphicControl::default();
    let mut loop_count = None;
    let mut total_frame_pixels = 0usize;
    while offset < data.len() {
        let byte = data[offset];
        offset += 1;
        match byte {
            0x2c => {
                if frames.len() == frames.capacity() {
                    return Ok(None);
                }
                let (frame, next_offset) = parse_image_descriptor(
                    data,
                    offset,
                    global_palette_offset,
                    global_palette_size,
                    graphic_control,
                )?;
                if frame.interlaced || frame.data_length > 8_192 {
                    return Ok(None);
                }
                let frame_pixels = usize::from(frame.width)
                    .checked_mul(usize::from(frame.height))
                    .ok_or_else(|| "Frame size overflow".to_string())?;
                if frame_pixels > 4_096 {
                    return Ok(None);
                }
                total_frame_pixels = total_frame_pixels
                    .checked_add(frame_pixels)
                    .ok_or_else(|| "Decoded frame size overflow".to_string())?;
                if total_frame_pixels > 30_000 {
                    return Ok(None);
                }
                frames.push(frame);
                offset = next_offset;
                graphic_control = GraphicControl::default();
            }
            0x21 => {
                if offset >= data.len() {
                    return Err("Truncated extension block".to_string());
                }
                let label = data[offset];
                offset += 1;
                if label == 0xf9 {
                    let (gce, next_offset) = parse_graphic_control(data, offset)?;
                    graphic_control = gce;
                    offset = next_offset;
                } else {
                    if label == 0xff {
                        loop_count = read_loop_count_extension(data, offset).or(loop_count);
                    }
                    offset = skip_sub_blocks(data, offset, "extension data")?;
                }
            }
            0x3b => break,
            _ => {
                return Err(format!(
                    "Unexpected GIF block byte 0x{byte:02x} at offset {}",
                    offset - 1
                ));
            }
        }
    }
    if frames.is_empty() {
        return Ok(None);
    }

    let metadata = std::mem::ManuallyDrop::new(GifMetadata {
        version,
        width,
        height,
        global_palette_offset,
        global_palette_size,
        loop_count,
        frames: std::mem::ManuallyDrop::take(&mut frames),
    });
    let loop_count = metadata.loop_count.map(i32::from).unwrap_or(-1);
    let global_palette_length = metadata
        .global_palette_size
        .checked_mul(3)
        .ok_or_else(|| "Global palette size overflow".to_string())?;
    let mut output_length = 13usize
        .checked_add(global_palette_length)
        .and_then(|length| length.checked_add(usize::from(loop_count >= 0) * 19))
        .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    for frame in &metadata.frames {
        output_length = output_length
            .checked_add(reencoded_frame_literal_size(&metadata, frame)?)
            .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    }
    output_length = output_length
        .checked_add(1)
        .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    if output_length > 64 * 1_024 {
        return Ok(None);
    }

    let output_pointer = allocate(allocate_context, output_length);
    if output_pointer.is_null() {
        return Err("Could not allocate host GIF output".to_string());
    }
    let mut output =
        std::mem::ManuallyDrop::new(Vec::from_raw_parts(output_pointer, 0, output_length));
    write_reencoded_gif_header_to(&mut output, data, &metadata, loop_count)?;

    let mut image_data_storage = [std::mem::MaybeUninit::<u8>::uninit(); 8_192];
    let mut indices_storage = [std::mem::MaybeUninit::<u8>::uninit(); 4_096];
    let mut compressed_storage = [std::mem::MaybeUninit::<u8>::uninit(); 8_192];
    let mut image_data = std::mem::ManuallyDrop::new(Vec::from_raw_parts(
        image_data_storage.as_mut_ptr().cast::<u8>(),
        0,
        image_data_storage.len(),
    ));
    let mut indices = std::mem::ManuallyDrop::new(Vec::from_raw_parts(
        indices_storage.as_mut_ptr().cast::<u8>(),
        0,
        indices_storage.len(),
    ));
    let mut compressed = std::mem::ManuallyDrop::new(Vec::from_raw_parts(
        compressed_storage.as_mut_ptr().cast::<u8>(),
        0,
        compressed_storage.len(),
    ));
    let mut lzw_scratch = LzwStackScratch::default();
    for frame in &metadata.frames {
        decode_frame_indices_reusing_output(
            data,
            frame,
            &mut image_data,
            &mut lzw_scratch,
            &mut indices,
        )?;
        write_reencoded_frame_literal_to(
            &mut output,
            data,
            &metadata,
            frame,
            &indices,
            &mut compressed,
        )?;
    }
    output.push(0x3b);
    if output.len() != output_length || output.as_mut_ptr() != output_pointer {
        return Err("Predicted host GIF output size differs".to_string());
    }
    Ok(Some((output_pointer, output_length)))
}

fn reencode_gif_literal_sequential(
    data: &[u8],
    metadata: &GifMetadata,
    loop_count: i32,
    decoded_size: usize,
) -> Result<Vec<u8>, String> {
    if loop_count < -1 || loop_count > i32::from(u16::MAX) {
        return Err("Loop count invalid".to_string());
    }

    let mut output = write_reencoded_gif_header(
        data,
        metadata,
        loop_count,
        data.len()
            .saturating_add(decoded_size)
            .saturating_add(metadata.frames.len() * 32),
    )?;
    let mut image_data = Vec::new();
    let mut lzw_scratch = LzwStackScratch::default();
    let mut indices = Vec::new();
    let mut compressed_scratch = Vec::new();

    for frame in &metadata.frames {
        decode_frame_indices_reusing_output(
            data,
            frame,
            &mut image_data,
            &mut lzw_scratch,
            &mut indices,
        )?;
        write_reencoded_frame_literal_to(
            &mut output,
            data,
            metadata,
            frame,
            &indices,
            &mut compressed_scratch,
        )?;
    }

    output.push(0x3b);
    Ok(output)
}

#[cfg(not(target_arch = "wasm32"))]
fn reencode_gif_literal_parallel_native(
    data: &[u8],
    metadata: &GifMetadata,
    loop_count: i32,
) -> Result<Vec<u8>, String> {
    let total_frame_pixels = metadata.frames.iter().try_fold(0usize, |total, frame| {
        usize::from(frame.width)
            .checked_mul(usize::from(frame.height))
            .and_then(|pixels| total.checked_add(pixels))
            .ok_or_else(|| "Decoded frame size overflow".to_string())
    })?;
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let thread_cap = if total_frame_pixels < 100_000 {
        6
    } else if metadata.frames.len() <= 12 {
        6
    } else {
        usize::MAX
    };
    let thread_count = available_threads.min(metadata.frames.len()).min(thread_cap);
    if thread_count <= 1 || metadata.frames.len() < 8 || total_frame_pixels < 30_000 {
        return reencode_gif_literal_sequential(data, metadata, loop_count, total_frame_pixels);
    }

    let mut output = write_reencoded_gif_header(data, metadata, loop_count, 0)?;
    let mut output_length = output.len();
    let mut frame_layout = Vec::with_capacity(metadata.frames.len());
    for frame in &metadata.frames {
        let frame_length = reencoded_frame_literal_size(metadata, frame)?;
        frame_layout.push((output_length, frame_length));
        output_length = output_length
            .checked_add(frame_length)
            .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    }
    output_length = output_length
        .checked_add(1)
        .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    output.reserve_exact(output_length - output.len());
    unsafe {
        output.set_len(output_length);
    }
    fill_reencoded_gif_literal_parallel_native(
        data,
        metadata,
        &mut output,
        &frame_layout,
        output_length,
        thread_count,
    )?;
    Ok(output)
}

#[cfg(not(target_arch = "wasm32"))]
fn reencode_gif_literal_worker(
    data: &[u8],
    metadata: &GifMetadata,
    frame_layout: &[(usize, usize)],
    output_address: usize,
    next_frame: &std::sync::atomic::AtomicUsize,
) -> Result<(), String> {
    let mut image_data = Vec::new();
    let mut lzw_scratch = LzwStackScratch::default();
    let mut indices = Vec::new();
    let mut compressed_scratch = Vec::new();
    reencode_gif_literal_worker_with_scratch(
        data,
        metadata,
        frame_layout,
        output_address,
        next_frame,
        &mut image_data,
        &mut lzw_scratch,
        &mut indices,
        &mut compressed_scratch,
    )
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn reencode_gif_literal_worker_with_scratch(
    data: &[u8],
    metadata: &GifMetadata,
    frame_layout: &[(usize, usize)],
    output_address: usize,
    next_frame: &std::sync::atomic::AtomicUsize,
    image_data: &mut Vec<u8>,
    lzw_scratch: &mut LzwStackScratch,
    indices: &mut Vec<u8>,
    compressed_scratch: &mut Vec<u8>,
) -> Result<(), String> {
    loop {
        let frame_index = next_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(frame) = metadata.frames.get(frame_index) else {
            return Ok(());
        };
        let (destination, expected_length) = frame_layout[frame_index];
        let destination_pointer = unsafe { (output_address as *mut u8).add(destination) };
        let mut chunk = std::mem::ManuallyDrop::new(unsafe {
            Vec::from_raw_parts(destination_pointer, 0, expected_length)
        });
        decode_frame_indices_reusing_output(data, frame, image_data, lzw_scratch, indices)?;
        write_reencoded_frame_literal_to(
            &mut chunk,
            data,
            metadata,
            frame,
            indices,
            compressed_scratch,
        )?;
        if chunk.len() != expected_length {
            return Err("Predicted reencoded frame size differs".to_string());
        }
        if chunk.as_ptr() != destination_pointer {
            return Err("Reencoded frame unexpectedly reallocated".to_string());
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn reencode_gif_literal_worker_fixed<
    const INDICES_CAPACITY: usize,
    const COMPRESSED_CAPACITY: usize,
>(
    data: &[u8],
    metadata: &GifMetadata,
    frame_layout: &[(usize, usize)],
    output_address: usize,
    next_frame: &std::sync::atomic::AtomicUsize,
) -> Result<(), String> {
    let mut image_data_storage = [std::mem::MaybeUninit::<u8>::uninit(); 8_192];
    let mut indices_storage = [std::mem::MaybeUninit::<u8>::uninit(); INDICES_CAPACITY];
    let mut compressed_storage = [std::mem::MaybeUninit::<u8>::uninit(); COMPRESSED_CAPACITY];
    let mut image_data = std::mem::ManuallyDrop::new(unsafe {
        Vec::from_raw_parts(
            image_data_storage.as_mut_ptr().cast::<u8>(),
            0,
            image_data_storage.len(),
        )
    });
    let mut indices = std::mem::ManuallyDrop::new(unsafe {
        Vec::from_raw_parts(
            indices_storage.as_mut_ptr().cast::<u8>(),
            0,
            indices_storage.len(),
        )
    });
    let mut compressed_scratch = std::mem::ManuallyDrop::new(unsafe {
        Vec::from_raw_parts(
            compressed_storage.as_mut_ptr().cast::<u8>(),
            0,
            compressed_storage.len(),
        )
    });
    let mut lzw_scratch = LzwStackScratch::default();
    reencode_gif_literal_worker_with_scratch(
        data,
        metadata,
        frame_layout,
        output_address,
        next_frame,
        &mut image_data,
        &mut lzw_scratch,
        &mut indices,
        &mut compressed_scratch,
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn reencode_frames_fit_fixed_worker_scratch(
    metadata: &GifMetadata,
    frame_layout: &[(usize, usize)],
) -> u8 {
    let fits_small = metadata
        .frames
        .iter()
        .zip(frame_layout)
        .all(|(frame, &(_, frame_length))| {
            let pixels = usize::from(frame.width) * usize::from(frame.height);
            frame.data_length <= 8_192 && pixels <= 4_096 && frame_length <= 8_192
        });
    if fits_small {
        return 1;
    }
    let fits_medium =
        metadata
            .frames
            .iter()
            .zip(frame_layout)
            .all(|(frame, &(_, frame_length))| {
                let pixels = usize::from(frame.width) * usize::from(frame.height);
                frame.data_length <= 8_192 && pixels <= 16_384 && frame_length <= 16_384
            });
    u8::from(fits_medium) * 2
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
struct ReencodeDispatchContext<'a> {
    data: &'a [u8],
    metadata: &'a GifMetadata,
    frame_layout: &'a [(usize, usize)],
    output_address: usize,
    next_frame: std::sync::atomic::AtomicUsize,
    error: std::sync::Mutex<Option<String>>,
    fixed_worker_scratch: u8,
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
unsafe extern "C" fn reencode_dispatch_worker(context: *mut std::ffi::c_void, _iteration: usize) {
    let context = &*(context.cast::<ReencodeDispatchContext<'_>>());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if context.fixed_worker_scratch == 1 {
            reencode_gif_literal_worker_fixed::<4_096, 8_192>(
                context.data,
                context.metadata,
                context.frame_layout,
                context.output_address,
                &context.next_frame,
            )
        } else if context.fixed_worker_scratch == 2 {
            reencode_gif_literal_worker_fixed::<16_384, 16_384>(
                context.data,
                context.metadata,
                context.frame_layout,
                context.output_address,
                &context.next_frame,
            )
        } else {
            reencode_gif_literal_worker(
                context.data,
                context.metadata,
                context.frame_layout,
                context.output_address,
                &context.next_frame,
            )
        }
    }));
    let error = match result {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error,
        Err(_) => "Parallel GIF transcoder panicked".to_string(),
    };
    if let Ok(mut stored_error) = context.error.lock() {
        if stored_error.is_none() {
            *stored_error = Some(error);
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
fn fill_reencoded_gif_literal_dispatch(
    data: &[u8],
    metadata: &GifMetadata,
    output_address: usize,
    frame_layout: &[(usize, usize)],
    thread_count: usize,
) -> Result<(), String> {
    let mut context = ReencodeDispatchContext {
        data,
        metadata,
        frame_layout,
        output_address,
        next_frame: std::sync::atomic::AtomicUsize::new(0),
        error: std::sync::Mutex::new(None),
        fixed_worker_scratch: reencode_frames_fit_fixed_worker_scratch(metadata, frame_layout),
    };
    unsafe {
        let queue = dispatch_get_global_queue(0, 0);
        if queue.is_null() {
            return Err("Could not acquire the system worker queue".to_string());
        }
        dispatch_apply_f(
            thread_count,
            queue,
            (&mut context as *mut ReencodeDispatchContext<'_>).cast(),
            reencode_dispatch_worker,
        );
    }
    context
        .error
        .into_inner()
        .map_err(|_| "Parallel GIF transcoder error lock was poisoned".to_string())?
        .map_or(Ok(()), Err)
}

#[cfg(not(target_arch = "wasm32"))]
fn fill_reencoded_gif_literal_parallel_native(
    data: &[u8],
    metadata: &GifMetadata,
    output: &mut Vec<u8>,
    frame_layout: &[(usize, usize)],
    output_length: usize,
    thread_count: usize,
) -> Result<(), String> {
    let output_address = output.as_mut_ptr() as usize;
    #[cfg(target_os = "macos")]
    {
        fill_reencoded_gif_literal_dispatch(
            data,
            metadata,
            output_address,
            frame_layout,
            thread_count,
        )?;
        output[output_length - 1] = 0x3b;
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let next_frame = std::sync::atomic::AtomicUsize::new(0);
        let fixed_worker_scratch = reencode_frames_fit_fixed_worker_scratch(metadata, frame_layout);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..thread_count)
                .map(|_| {
                    let next_frame = &next_frame;
                    let frame_layout = &frame_layout;
                    scope.spawn(move || -> Result<(), String> {
                        if fixed_worker_scratch == 1 {
                            reencode_gif_literal_worker_fixed::<4_096, 8_192>(
                                data,
                                metadata,
                                frame_layout,
                                output_address,
                                next_frame,
                            )
                        } else if fixed_worker_scratch == 2 {
                            reencode_gif_literal_worker_fixed::<16_384, 16_384>(
                                data,
                                metadata,
                                frame_layout,
                                output_address,
                                next_frame,
                            )
                        } else {
                            reencode_gif_literal_worker(
                                data,
                                metadata,
                                frame_layout,
                                output_address,
                                next_frame,
                            )
                        }
                    })
                })
                .collect();
            for handle in handles {
                handle
                    .join()
                    .map_err(|_| "Parallel GIF transcoder panicked".to_string())??;
            }
            Ok::<_, String>(())
        })?;
        output[output_length - 1] = 0x3b;
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn reencoded_frame_literal_size(
    metadata: &GifMetadata,
    frame: &FrameMetadata,
) -> Result<usize, String> {
    let color_count = checked_palette_color_count(frame.palette_size)?;
    let pixel_count = usize::from(frame.width)
        .checked_mul(usize::from(frame.height))
        .ok_or_else(|| "Frame size overflow".to_string())?;
    let min_code_size = (log2_pow2(color_count) as u8).max(2);
    let lzw_length = literal_lzw_block_size(pixel_count, min_code_size)?;
    let gce_length =
        usize::from(frame.delay != 0 || frame.disposal != 0 || frame.transparent_index.is_some())
            * 8;
    let uses_global_palette = metadata.global_palette_offset == Some(frame.palette_offset)
        && metadata.global_palette_size == frame.palette_size;
    let palette_length = if uses_global_palette {
        0
    } else {
        color_count
            .checked_mul(3)
            .ok_or_else(|| "Palette size overflow".to_string())?
    };
    gce_length
        .checked_add(10)
        .and_then(|length| length.checked_add(palette_length))
        .and_then(|length| length.checked_add(lzw_length))
        .ok_or_else(|| "Reencoded GIF size overflow".to_string())
}

fn literal_lzw_block_size(pixel_count: usize, min_code_size: u8) -> Result<usize, String> {
    let code_size = usize::from(min_code_size) + 1;
    let literals_per_clear = (1usize << min_code_size) - 2;
    let clear_count = pixel_count.div_ceil(literals_per_clear);
    let code_count = pixel_count
        .checked_add(clear_count)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    let compressed_length = code_count
        .checked_mul(code_size)
        .and_then(|bits| bits.checked_add(7))
        .map(|bits| bits / 8)
        .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    let lzw_length = compressed_length
        .checked_add(compressed_length.div_ceil(255))
        .and_then(|length| length.checked_add(2))
        .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    Ok(lzw_length)
}

fn write_reencoded_gif_header(
    data: &[u8],
    metadata: &GifMetadata,
    loop_count: i32,
    capacity: usize,
) -> Result<Vec<u8>, String> {
    if loop_count < -1 || loop_count > i32::from(u16::MAX) {
        return Err("Loop count invalid".to_string());
    }
    let mut output = Vec::with_capacity(capacity);
    write_reencoded_gif_header_to(&mut output, data, metadata, loop_count)?;
    Ok(output)
}

fn write_reencoded_gif_header_to(
    output: &mut Vec<u8>,
    data: &[u8],
    metadata: &GifMetadata,
    loop_count: i32,
) -> Result<(), String> {
    if loop_count < -1 || loop_count > i32::from(u16::MAX) {
        return Err("Loop count invalid".to_string());
    }
    output.extend_from_slice(b"GIF89a");
    push_u16_le(output, metadata.width);
    push_u16_le(output, metadata.height);
    output.extend_from_slice(&data[10..13]);
    if let Some(global_palette_offset) = metadata.global_palette_offset {
        let global_palette_length = metadata
            .global_palette_size
            .checked_mul(3)
            .ok_or_else(|| "Global palette size overflow".to_string())?;
        let global_palette_end = checked_add(
            global_palette_offset,
            global_palette_length,
            data.len(),
            "global color table",
        )?;
        output.extend_from_slice(&data[global_palette_offset..global_palette_end]);
    }
    write_loop_extension(output, loop_count);
    Ok(())
}

fn write_reencoded_frame_literal_to(
    output: &mut Vec<u8>,
    data: &[u8],
    metadata: &GifMetadata,
    frame: &FrameMetadata,
    indices: &[u8],
    compressed_scratch: &mut Vec<u8>,
) -> Result<(), String> {
    if frame.palette_size == 0 {
        return Err("GIF frame has no color table".to_string());
    }
    let color_count = checked_palette_color_count(frame.palette_size)?;
    let palette_length = color_count
        .checked_mul(3)
        .ok_or_else(|| "Palette size overflow".to_string())?;
    let palette_end = checked_add(
        frame.palette_offset,
        palette_length,
        data.len(),
        "frame color table",
    )?;
    let expected_indices = usize::from(frame.width)
        .checked_mul(usize::from(frame.height))
        .ok_or_else(|| "Frame size overflow".to_string())?;
    if indices.len() != expected_indices {
        return Err("Decoded frame length does not match dimensions".to_string());
    }

    let uses_global_palette = metadata.global_palette_offset == Some(frame.palette_offset)
        && metadata.global_palette_size == frame.palette_size;
    write_reencoded_frame_header(output, frame, color_count, !uses_global_palette);
    if !uses_global_palette {
        output.extend_from_slice(&data[frame.palette_offset..palette_end]);
    }
    let min_code_size = (log2_pow2(color_count) as u8).max(2);
    let direct_output = indices.len() >= 100_000;
    if direct_output {
        encode_indexed_literal_lzw_direct_to(output, indices, min_code_size, color_count)
    } else {
        encode_indexed_literal_lzw_to(
            output,
            indices,
            min_code_size,
            color_count,
            compressed_scratch,
        )
    }
}

fn remux_gif_pixel_perfect_inner(
    data: &[u8],
    metadata: &GifMetadata,
    loop_count: i32,
) -> Result<Vec<u8>, String> {
    let mut output = write_reencoded_gif_header(data, metadata, loop_count, data.len())?;
    for frame in &metadata.frames {
        if frame.palette_size == 0 {
            return Err("GIF frame has no color table".to_string());
        }
        let color_count = checked_palette_color_count(frame.palette_size)?;
        let uses_global_palette = metadata.global_palette_offset == Some(frame.palette_offset)
            && metadata.global_palette_size == frame.palette_size;
        write_reencoded_frame_header(&mut output, frame, color_count, !uses_global_palette);
        if frame.interlaced {
            let packed_offset = output
                .len()
                .checked_sub(1)
                .ok_or_else(|| "Missing image descriptor".to_string())?;
            output[packed_offset] |= 0x40;
        }
        if !uses_global_palette {
            let palette_length = color_count
                .checked_mul(3)
                .ok_or_else(|| "Palette size overflow".to_string())?;
            let palette_end = checked_add(
                frame.palette_offset,
                palette_length,
                data.len(),
                "frame color table",
            )?;
            output.extend_from_slice(&data[frame.palette_offset..palette_end]);
        }
        let data_end = checked_add(
            frame.data_offset,
            frame.data_length,
            data.len(),
            "frame image data",
        )?;
        output.extend_from_slice(&data[frame.data_offset..data_end]);
    }
    output.push(0x3b);
    Ok(output)
}

fn write_reencoded_frame_header(
    output: &mut Vec<u8>,
    frame: &FrameMetadata,
    color_count: usize,
    write_local_palette: bool,
) {
    if frame.delay != 0 || frame.disposal != 0 || frame.transparent_index.is_some() {
        output.extend_from_slice(&[
            0x21,
            0xf9,
            0x04,
            ((frame.disposal & 0x07) << 2)
                | if frame.transparent_index.is_some() {
                    0x01
                } else {
                    0x00
                },
        ]);
        push_u16_le(output, frame.delay);
        output.push(frame.transparent_index.unwrap_or(0));
        output.push(0);
    }

    output.push(0x2c);
    push_u16_le(output, frame.x);
    push_u16_le(output, frame.y);
    push_u16_le(output, frame.width);
    push_u16_le(output, frame.height);
    let color_table_size_bits = (log2_pow2(color_count) as u8 - 1) & 7;
    output.push(if write_local_palette {
        0x80 | color_table_size_bits
    } else {
        0
    });
}

fn encode_indexed_delta_gif_inner(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
) -> Result<Vec<u8>, String> {
    encode_indexed_gif_inner_with_rects(
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        delays,
        loop_count,
        true,
        None,
        false,
    )
}

fn encode_indexed_literal_delta_gif_inner(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
) -> Result<Vec<u8>, String> {
    encode_indexed_gif_inner_with_rects(
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        delays,
        loop_count,
        true,
        None,
        true,
    )
}

fn encode_indexed_gif_inner_with_rects(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    deltas: bool,
    transparent_index: Option<u8>,
    literal: bool,
) -> Result<Vec<u8>, String> {
    if !deltas {
        return if literal {
            encode_indexed_literal_gif_inner(
                index_stream,
                width,
                height,
                frame_count,
                palette_rgb,
                delays,
                loop_count,
                transparent_index,
            )
        } else {
            encode_indexed_gif_inner(
                index_stream,
                width,
                height,
                frame_count,
                palette_rgb,
                delays,
                loop_count,
                transparent_index,
            )
        };
    }

    if width == 0 || height == 0 {
        return Err("Width/Height invalid".to_string());
    }
    if frame_count == 0 {
        return Err("Frame count must be greater than zero".to_string());
    }
    if loop_count < -1 || loop_count > i32::from(u16::MAX) {
        return Err("Loop count invalid".to_string());
    }
    delays.validate(frame_count)?;

    let color_count = checked_palette_color_count(palette_rgb.len())?;
    let min_code_size = (log2_pow2(color_count) as u8).max(2);
    let canvas_width = usize::from(width);
    let canvas_height = usize::from(height);
    let frame_len = canvas_width
        .checked_mul(canvas_height)
        .ok_or_else(|| "Frame size overflow".to_string())?;
    let expected_len = frame_len
        .checked_mul(frame_count)
        .ok_or_else(|| "Frame stream overflow".to_string())?;
    if index_stream.len() != expected_len {
        return Err("Indexed frame stream length does not match dimensions".to_string());
    }

    let palette_bytes = color_count
        .checked_mul(3)
        .ok_or_else(|| "Palette size overflow".to_string())?;
    let estimated_frame_bytes = frame_len / 8 + 48;
    let mut output = Vec::with_capacity(
        13 + palette_bytes + 20 + frame_count.saturating_mul(estimated_frame_bytes) + 1,
    );
    let mut lzw_tables = (!literal).then(LzwEncodeTables::new);
    let mut rect_scratch = Vec::new();
    let mut compressed_scratch = Vec::new();

    write_indexed_gif_header(&mut output, width, height, palette_rgb, color_count);
    write_loop_extension(&mut output, loop_count);

    let mut previous_frame: Option<&[u8]> = None;
    for (frame_index, frame) in index_stream.chunks_exact(frame_len).enumerate() {
        let delay = delays.get(frame_index);
        if let Some(previous) = previous_frame {
            if let Some(rect) = find_changed_rect_u8(previous, frame, canvas_width, canvas_height) {
                write_indexed_gif_frame_header(
                    &mut output,
                    rect.x as u16,
                    rect.y as u16,
                    rect.width as u16,
                    rect.height as u16,
                    delay,
                    transparent_index,
                    0,
                );
                if literal {
                    rect_scratch.clear();
                    let rect_length = rect.width * rect.height;
                    if rect_scratch.capacity() < rect_length {
                        rect_scratch.reserve(rect_length - rect_scratch.capacity());
                    }
                    for row in 0..rect.height {
                        let start = (rect.y + row) * canvas_width + rect.x;
                        rect_scratch.extend_from_slice(&frame[start..start + rect.width]);
                    }
                    encode_indexed_literal_lzw_to(
                        &mut output,
                        &rect_scratch,
                        min_code_size,
                        color_count,
                        &mut compressed_scratch,
                    )?;
                } else {
                    encode_indexed_lzw_rect_to(
                        &mut output,
                        frame,
                        rect.y * canvas_width + rect.x,
                        rect.width,
                        rect.height,
                        canvas_width,
                        min_code_size,
                        color_count,
                        lzw_tables.as_mut().unwrap(),
                    )?;
                }
            } else {
                write_indexed_gif_frame_header(
                    &mut output,
                    0,
                    0,
                    1,
                    1,
                    delay,
                    transparent_index,
                    0,
                );
                if literal {
                    encode_indexed_literal_lzw_to(
                        &mut output,
                        &frame[..1],
                        min_code_size,
                        color_count,
                        &mut compressed_scratch,
                    )?;
                } else {
                    encode_indexed_lzw_to_with_tables(
                        &mut output,
                        &frame[..1],
                        min_code_size,
                        color_count,
                        lzw_tables.as_mut().unwrap(),
                    )?;
                }
            }
        } else {
            write_indexed_gif_frame_header(
                &mut output,
                0,
                0,
                width,
                height,
                delay,
                transparent_index,
                0,
            );
            if literal {
                encode_indexed_literal_lzw_to(
                    &mut output,
                    frame,
                    min_code_size,
                    color_count,
                    &mut compressed_scratch,
                )?;
            } else {
                encode_indexed_lzw_to_with_tables(
                    &mut output,
                    frame,
                    min_code_size,
                    color_count,
                    lzw_tables.as_mut().unwrap(),
                )?;
            }
        }
        previous_frame = Some(frame);
    }

    output.push(0x3b);
    Ok(output)
}

fn write_indexed_gif_header(
    output: &mut Vec<u8>,
    width: u16,
    height: u16,
    palette_rgb: &[u32],
    color_count: usize,
) {
    let color_table_size_bits = (log2_pow2(color_count) as u8 - 1) & 7;

    output.extend_from_slice(b"GIF89a");
    push_u16_le(output, width);
    push_u16_le(output, height);
    output.push(0x80 | color_table_size_bits);
    output.push(0);
    output.push(0);

    for index in 0..color_count {
        let rgb = palette_rgb.get(index).copied().unwrap_or(0);
        output.push(((rgb >> 16) & 0xff) as u8);
        output.push(((rgb >> 8) & 0xff) as u8);
        output.push((rgb & 0xff) as u8);
    }
}

fn write_loop_extension(output: &mut Vec<u8>, loop_count: i32) {
    if loop_count >= 0 {
        output.extend_from_slice(&[
            0x21, 0xff, 0x0b, b'N', b'E', b'T', b'S', b'C', b'A', b'P', b'E', b'2', b'.', b'0',
            0x03, 0x01,
        ]);
        push_u16_le(output, loop_count as u16);
        output.push(0);
    }
}

fn write_indexed_gif_frame_header(
    output: &mut Vec<u8>,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    delay: u16,
    transparent_index: Option<u8>,
    disposal: u8,
) {
    if delay != 0 || transparent_index.is_some() || disposal != 0 {
        output.extend_from_slice(&[
            0x21,
            0xf9,
            0x04,
            (disposal << 2)
                | if transparent_index.is_some() {
                    0x01
                } else {
                    0x00
                },
        ]);
        push_u16_le(output, delay);
        output.push(transparent_index.unwrap_or(0));
        output.push(0x00);
    }

    output.push(0x2c);
    push_u16_le(output, x);
    push_u16_le(output, y);
    push_u16_le(output, width);
    push_u16_le(output, height);
    output.push(0);
}

fn checked_palette_color_count(palette_len: usize) -> Result<usize, String> {
    if palette_len == 0 || palette_len > 256 {
        return Err("Invalid palette size (must be 1..256)".to_string());
    }
    let mut color_count = palette_len.next_power_of_two();
    if color_count < 2 {
        color_count = 2;
    }
    if color_count > 256 {
        return Err("Invalid palette size (must be 1..256)".to_string());
    }
    Ok(color_count)
}

fn log2_pow2(value: usize) -> usize {
    usize::BITS as usize - 1 - value.leading_zeros() as usize
}

#[inline]
fn push_u16_le(output: &mut Vec<u8>, value: u16) {
    output.push((value & 0xff) as u8);
    output.push((value >> 8) as u8);
}

struct LzwEncodeTables {
    entries: Vec<u32>,
    epoch: u32,
}

impl LzwEncodeTables {
    fn new() -> Self {
        Self {
            entries: vec![0; LZW_DIRECT_ENTRY_COUNT],
            epoch: 0,
        }
    }

    fn reset(&mut self) -> u32 {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 || self.epoch > LZW_ENTRY_EPOCH_MAX {
            self.entries.fill(0);
            self.epoch = 1;
        }
        self.epoch
    }
}

// Callers still expose this allocation as raw bytes, but the base address is
// stable at a four-byte boundary for packed RGBA loads.
#[repr(align(4))]
#[allow(dead_code)]
struct AlignedByte(u8);

struct LzwEncodeScratch {
    input: Vec<AlignedByte>,
    output: Vec<u8>,
    // Literal LZW, including the default arbitrary-RGBA path, never needs
    // the 4 MiB dictionary. Allocate it only when a caller explicitly asks
    // for dictionary compression.
    tables: Option<LzwEncodeTables>,
}

std::thread_local! {
    static REUSABLE_LZW_SCRATCH: std::cell::RefCell<LzwEncodeScratch> =
        std::cell::RefCell::new(LzwEncodeScratch {
            input: Vec::new(),
            output: Vec::new(),
            tables: None,
        });
    static REUSABLE_LZW_TABLES: std::cell::RefCell<LzwEncodeTables> =
        std::cell::RefCell::new(LzwEncodeTables::new());
    static REUSABLE_GIF_OUTPUT: std::cell::RefCell<Vec<u8>> =
        std::cell::RefCell::new(Vec::new());
    static REUSABLE_QUALITY_HISTOGRAM_U32: std::cell::RefCell<Vec<RgbHistogramBin32>> =
        std::cell::RefCell::new(Vec::new());
    static REUSABLE_QUALITY_HISTOGRAM_U64: std::cell::RefCell<Vec<RgbHistogramBin>> =
        std::cell::RefCell::new(Vec::new());
    static REUSABLE_QUALITY_HISTOGRAM_TO_PALETTE: std::cell::RefCell<Vec<u8>> =
        std::cell::RefCell::new(Vec::new());
    static REUSABLE_QUALITY_COLORS: std::cell::RefCell<Vec<QuantizedColor>> =
        std::cell::RefCell::new(Vec::new());
    static REUSABLE_QUALITY_COLOR_INDEX: std::cell::RefCell<ColorIndexTable> =
        std::cell::RefCell::new(ColorIndexTable::empty());
    static REUSABLE_QUANTIZED_INDEXED: std::cell::RefCell<Vec<u8>> =
        std::cell::RefCell::new(Vec::new());
}

fn encode_indexed_lzw_inner(
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<Vec<u8>, String> {
    encode_indexed_lzw_scratch_inner(index_stream, min_code_size, color_count, false)?;
    Ok(REUSABLE_LZW_SCRATCH.with(|scratch| scratch.borrow().output.clone()))
}

fn encode_indexed_lzw_scratch_inner(
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
    literal: bool,
) -> Result<usize, String> {
    REUSABLE_LZW_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let LzwEncodeScratch { output, tables, .. } = &mut *scratch;
        output.clear();
        output.reserve(index_stream.len() / 2 + 16);
        if literal {
            encode_indexed_literal_lzw_direct_to(output, index_stream, min_code_size, color_count)?;
        } else {
            let tables = tables.get_or_insert_with(LzwEncodeTables::new);
            encode_indexed_lzw_to_with_tables(
                output,
                index_stream,
                min_code_size,
                color_count,
                tables,
            )?;
        }
        Ok(output.len())
    })
}

fn encode_indexed_lzw_scratch_from_input_inner(
    length: usize,
    min_code_size: u8,
    color_count: usize,
    literal: bool,
) -> Result<usize, String> {
    REUSABLE_LZW_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let LzwEncodeScratch {
            input,
            output,
            tables,
        } = &mut *scratch;
        if length > input.len() * std::mem::size_of::<AlignedByte>() {
            return Err("Indexed input scratch length exceeds capacity".to_string());
        }
        let index_stream = unsafe {
            std::slice::from_raw_parts(input.as_ptr().cast::<u8>(), length)
        };
        output.clear();
        output.reserve(index_stream.len() / 2 + 16);
        if literal {
            encode_indexed_literal_lzw_direct_to(output, index_stream, min_code_size, color_count)?;
        } else {
            let tables = tables.get_or_insert_with(LzwEncodeTables::new);
            encode_indexed_lzw_to_with_tables(
                output,
                index_stream,
                min_code_size,
                color_count,
                tables,
            )?;
        }
        Ok(output.len())
    })
}

fn encode_indexed_literal_lzw_to(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
    compressed: &mut Vec<u8>,
) -> Result<(), String> {
    if min_code_size < 2 || min_code_size > 8 {
        return Err(format!("Invalid LZW minimum code size {min_code_size}"));
    }
    if color_count == 0 || color_count > 256 {
        return Err("Invalid color count".to_string());
    }
    if index_stream.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    let code_size = usize::from(min_code_size) + 1;
    let literals_per_clear = (1usize << min_code_size) - 2;
    let estimated_bytes = index_stream
        .len()
        .saturating_mul(code_size)
        .saturating_mul(literals_per_clear + 1)
        / literals_per_clear
        / 8
        + 8;
    compressed.clear();
    if compressed.capacity() < estimated_bytes {
        compressed.reserve(estimated_bytes - compressed.capacity());
    }
    encode_indexed_literal_codes_raw(compressed, index_stream, min_code_size, color_count)?;
    output.push(min_code_size);
    for block in compressed.chunks(255) {
        output.push(block.len() as u8);
        output.extend_from_slice(block);
    }
    output.push(0);
    Ok(())
}

fn encode_indexed_literal_lzw_direct_to(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<(), String> {
    if min_code_size == 7 && color_count <= 128 {
        return encode_eight_bit_literal_lzw_direct_to(output, index_stream, color_count);
    }
    if min_code_size == 8 {
        return encode_nine_bit_literal_lzw_direct_to(output, index_stream, color_count);
    }
    output.push(min_code_size);
    let compressed_start = output.len();
    encode_indexed_literal_codes_raw(output, index_stream, min_code_size, color_count)?;
    let compressed_length = output.len() - compressed_start;
    let block_count = compressed_length.div_ceil(255);
    let final_length = output
        .len()
        .checked_add(block_count + 1)
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?;
    output.resize(final_length, 0);
    for block in (0..block_count).rev() {
        let source_start = compressed_start + block * 255;
        let length = (compressed_length - block * 255).min(255);
        let destination_start = compressed_start + block * 256;
        output.copy_within(source_start..source_start + length, destination_start + 1);
        output[destination_start] = length as u8;
    }
    Ok(())
}

struct GifSubblockAppender<'a> {
    output: &'a mut [u8],
    position: usize,
    raw_remaining: usize,
    block_remaining: usize,
}

impl GifSubblockAppender<'_> {
    #[inline]
    fn start_block(&mut self) {
        if self.block_remaining == 0 && self.raw_remaining > 0 {
            self.block_remaining = self.raw_remaining.min(255);
            self.output[self.position] = self.block_remaining as u8;
            self.position += 1;
        }
    }

    #[inline]
    fn write_byte(&mut self, value: u8) {
        self.start_block();
        self.output[self.position] = value;
        self.position += 1;
        self.block_remaining -= 1;
        self.raw_remaining -= 1;
    }

    #[inline]
    fn write_slice(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            self.start_block();
            let length = bytes.len().min(self.block_remaining);
            self.output[self.position..self.position + length].copy_from_slice(&bytes[..length]);
            self.position += length;
            self.block_remaining -= length;
            self.raw_remaining -= length;
            bytes = &bytes[length..];
        }
    }
}

fn encode_eight_bit_literal_lzw_direct_to(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    color_count: usize,
) -> Result<(), String> {
    if color_count == 0 || color_count > 128 {
        return Err("Invalid color count".to_string());
    }
    if index_stream.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    if !indices_fit_color_count(index_stream, color_count) {
        return Err("Pixel index out of range".to_string());
    }

    let raw_length = index_stream.len() + index_stream.len().div_ceil(126) + 1;
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    output.resize(output_start + 1 + block_count + raw_length + 1, 0);
    output[output_start] = 7;
    let mut appender = GifSubblockAppender {
        output: &mut output[output_start + 1..],
        position: 0,
        raw_remaining: raw_length,
        block_remaining: 0,
    };
    for literals in index_stream.chunks(126) {
        appender.write_byte(128);
        appender.write_slice(literals);
    }
    appender.write_byte(129);
    debug_assert_eq!(appender.raw_remaining, 0);
    debug_assert_eq!(appender.position, block_count + raw_length);
    Ok(())
}

struct DirectGifSubblockWriter {
    output: *mut u8,
    position: usize,
    block_remaining: usize,
    raw_position: usize,
}

impl DirectGifSubblockWriter {
    #[inline(always)]
    fn write_byte(&mut self, value: u8) {
        if self.block_remaining == 0 {
            self.position += 1;
            self.block_remaining = 255;
        }
        unsafe { self.output.add(self.position).write(value) };
        self.position += 1;
        self.block_remaining -= 1;
        self.raw_position += 1;
    }

    #[inline(always)]
    fn write_fixed(&mut self, bytes: &[u8; 8], length: usize) {
        if length > self.block_remaining {
            for &byte in &bytes[..length] {
                self.write_byte(byte);
            }
            return;
        }
        unsafe {
            if length == 8 || (length == 7 && self.block_remaining >= 8) {
                self.output
                    .add(self.position)
                    .cast::<u64>()
                    .write_unaligned(u64::from_le_bytes(*bytes));
            } else {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    self.output.add(self.position),
                    length,
                );
            }
        }
        self.position += length;
        self.block_remaining -= length;
        self.raw_position += length;
    }
}

fn encode_nine_bit_literal_lzw_direct_to(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    color_count: usize,
) -> Result<(), String> {
    if color_count == 0 || color_count > 256 {
        return Err("Invalid color count".to_string());
    }
    if index_stream.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    if !indices_fit_color_count(index_stream, color_count) {
        return Err("Pixel index out of range".to_string());
    }

    let clear_count = index_stream.len().div_ceil(254);
    let raw_length = index_stream
        .len()
        .checked_add(clear_count)
        .and_then(|codes| codes.checked_add(1))
        .and_then(|codes| codes.checked_mul(9))
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?
        .div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    // Reserve a small write-ahead pad for the final unaligned u64 store, but
    // do not extend the Vec length until every actual output byte is written.
    // This keeps the spare-capacity fast path free of a multi-megabyte zero fill.
    output.reserve(2 + block_count + raw_length + 8);
    unsafe {
        output.as_mut_ptr().add(output_start).write(8);
    }
    let mut writer = DirectGifSubblockWriter {
        output: unsafe { output.as_mut_ptr().add(output_start) },
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;

    for literals in index_stream.chunks(254) {
        append_nine_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, 256);

        let mut groups = literals.chunks_exact(7);
        for group in &mut groups {
            let packed = u64::from(group[0])
                | (u64::from(group[1]) << 9)
                | (u64::from(group[2]) << 18)
                | (u64::from(group[3]) << 27)
                | (u64::from(group[4]) << 36)
                | (u64::from(group[5]) << 45)
                | (u64::from(group[6]) << 54);
            let combined = bits | (packed << bit_count);
            let total_bits = bit_count + 63;
            let bytes = combined.to_le_bytes();
            if total_bits >= 64 {
                writer.write_fixed(&bytes, 8);
                bits = if bit_count == 0 {
                    0
                } else {
                    packed >> (64 - bit_count)
                };
                bit_count = total_bits - 64;
            } else {
                debug_assert_eq!(bit_count, 0);
                writer.write_fixed(&bytes, 7);
                bits = combined >> 56;
                bit_count = total_bits - 56;
            }
        }
        for &pixel in groups.remainder() {
            append_nine_bit_literal_code_to_direct(
                &mut writer,
                &mut bits,
                &mut bit_count,
                u16::from(pixel),
            );
        }
    }
    append_nine_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, 257);
    while bit_count > 0 {
        writer.write_byte(bits as u8);
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(writer.raw_position, raw_length);

    let mut raw_offset = 0usize;
    for block in 0..block_count {
        let length = (raw_length - raw_offset).min(255);
        unsafe {
            output
                .as_mut_ptr()
                .add(output_start + 1 + block * 256)
                .write(length as u8);
        }
        raw_offset += length;
    }
    unsafe {
        output
            .as_mut_ptr()
            .add(output_start + 1 + block_count + raw_length)
            .write(0);
        output.set_len(output_start + 2 + block_count + raw_length);
    }
    Ok(())
}

/// Encode the 256-color literal stream directly from RGBA input. The quality
/// palette has already been built, so retaining a full indexed scratch buffer
fn encode_nine_bit_literal_lzw_mapped_to<const BITS: usize, const HAS_TRANSPARENT: bool>(
    output: &mut Vec<u8>,
    rgba_stream: &[u8],
    alpha_threshold: u8,
    transparent_index: u8,
    histogram_to_palette: &[u8],
) -> Result<(), String> {
    if rgba_stream.is_empty() || rgba_stream.len() % 4 != 0 {
        return Err("RGBA frame stream is empty or misaligned".to_string());
    }
    let pixel_count = rgba_stream.len() / 4;
    let clear_count = pixel_count.div_ceil(254);
    let raw_length = pixel_count
        .checked_add(clear_count)
        .and_then(|codes| codes.checked_add(1))
        .and_then(|codes| codes.checked_mul(9))
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?
        .div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    output.reserve(2 + block_count + raw_length + 8);
    unsafe {
        output.as_mut_ptr().add(output_start).write(8);
    }
    let mut writer = DirectGifSubblockWriter {
        output: unsafe { output.as_mut_ptr().add(output_start) },
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let rgba_pointer = rgba_stream.as_ptr();

    for chunk_start in (0..pixel_count).step_by(254) {
        append_nine_bit_literal_code_to_direct(
            &mut writer,
            &mut bits,
            &mut bit_count,
            256,
        );
        let chunk_end = (chunk_start + 254).min(pixel_count);
        let mut pixel_index = chunk_start;
        while pixel_index + 7 <= chunk_end {
            let packed0 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
            });
            let packed1 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add((pixel_index + 1) * 4).cast())
            });
            let packed2 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add((pixel_index + 2) * 4).cast())
            });
            let packed3 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add((pixel_index + 3) * 4).cast())
            });
            let packed4 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add((pixel_index + 4) * 4).cast())
            });
            let packed5 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add((pixel_index + 5) * 4).cast())
            });
            let packed6 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add((pixel_index + 6) * 4).cast())
            });
            let code0 = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
                packed0,
                alpha_threshold,
                transparent_index,
                histogram_to_palette,
            );
            let code1 = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
                packed1,
                alpha_threshold,
                transparent_index,
                histogram_to_palette,
            );
            let code2 = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
                packed2,
                alpha_threshold,
                transparent_index,
                histogram_to_palette,
            );
            let code3 = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
                packed3,
                alpha_threshold,
                transparent_index,
                histogram_to_palette,
            );
            let code4 = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
                packed4,
                alpha_threshold,
                transparent_index,
                histogram_to_palette,
            );
            let code5 = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
                packed5,
                alpha_threshold,
                transparent_index,
                histogram_to_palette,
            );
            let code6 = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
                packed6,
                alpha_threshold,
                transparent_index,
                histogram_to_palette,
            );
            let packed_codes = u64::from(code0)
                | (u64::from(code1) << 9)
                | (u64::from(code2) << 18)
                | (u64::from(code3) << 27)
                | (u64::from(code4) << 36)
                | (u64::from(code5) << 45)
                | (u64::from(code6) << 54);
            let combined = bits | (packed_codes << bit_count);
            let total_bits = bit_count + 63;
            if total_bits >= 64 {
                writer.write_fixed(&combined.to_le_bytes(), 8);
                bits = if bit_count == 0 {
                    0
                } else {
                    packed_codes >> (64 - bit_count)
                };
                bit_count = total_bits - 64;
            } else {
                debug_assert_eq!(bit_count, 0);
                writer.write_fixed(&combined.to_le_bytes(), 7);
                bits = combined >> 56;
                bit_count = total_bits - 56;
            }
            pixel_index += 7;
        }
        while pixel_index < chunk_end {
            let packed = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
            });
            let code = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
                packed,
                alpha_threshold,
                transparent_index,
                histogram_to_palette,
            );
            append_nine_bit_literal_code_to_direct(
                &mut writer,
                &mut bits,
                &mut bit_count,
                u16::from(code),
            );
            pixel_index += 1;
        }
    }
    append_nine_bit_literal_code_to_direct(
        &mut writer,
        &mut bits,
        &mut bit_count,
        257,
    );
    while bit_count > 0 {
        writer.write_byte(bits as u8);
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(writer.raw_position, raw_length);
    let mut raw_offset = 0usize;
    for block in 0..block_count {
        let length = (raw_length - raw_offset).min(255);
        unsafe {
            output
                .as_mut_ptr()
                .add(output_start + 1 + block * 256)
                .write(length as u8);
        }
        raw_offset += length;
    }
    unsafe {
        output
            .as_mut_ptr()
            .add(output_start + 1 + block_count + raw_length)
            .write(0);
        output.set_len(output_start + 2 + block_count + raw_length);
    }
    Ok(())
}

#[inline(always)]
fn mapped_quality_pixel<const BITS: usize, const HAS_TRANSPARENT: bool>(
    packed: u32,
    alpha_threshold: u8,
    transparent_index: u8,
    histogram_to_palette: &[u8],
) -> u8 {
    if HAS_TRANSPARENT && ((packed >> 24) as u8) < alpha_threshold {
        transparent_index
    } else {
        let index = quality_histogram_index_packed::<BITS>(packed);
        unsafe { *histogram_to_palette.get_unchecked(index) }
    }
}

#[inline(always)]
fn append_nine_bit_literal_code_to_direct(
    writer: &mut DirectGifSubblockWriter,
    bits: &mut u64,
    bit_count: &mut usize,
    code: u16,
) {
    *bits |= u64::from(code) << *bit_count;
    *bit_count += 9;
    while *bit_count >= 8 {
        writer.write_byte(*bits as u8);
        *bits >>= 8;
        *bit_count -= 8;
    }
}

fn encode_indexed_literal_codes_raw(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<(), String> {
    if min_code_size < 2 || min_code_size > 8 {
        return Err(format!("Invalid LZW minimum code size {min_code_size}"));
    }
    if color_count == 0 || color_count > 256 {
        return Err("Invalid color count".to_string());
    }
    if index_stream.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    if !indices_fit_color_count(index_stream, color_count) {
        return Err("Pixel index out of range".to_string());
    }
    let clear = 1usize << min_code_size;
    let eoi = clear + 1;
    let code_size = usize::from(min_code_size) + 1;
    // Reset before a literal-only decoder dictionary would require wider codes.
    let literals_per_clear = clear - 2;
    let mut bits = 0u64;
    let mut bit_count = 0usize;

    if min_code_size == 2 {
        return encode_two_bit_literal_codes(output, index_stream, color_count);
    }
    if min_code_size == 7 {
        return encode_eight_bit_literal_codes(output, index_stream);
    }
    if min_code_size == 8 {
        return encode_nine_bit_literal_codes(output, index_stream);
    }
    if min_code_size == 4 && color_count == 16 {
        return encode_five_bit_literal_codes(output, index_stream);
    }
    if min_code_size == 6 && color_count == 64 {
        return encode_seven_bit_literal_codes(output, index_stream);
    }
    for literals in index_stream.chunks(literals_per_clear) {
        emit_raw_lzw_code(output, &mut bits, &mut bit_count, code_size, clear);
        for &pixel in literals {
            emit_raw_lzw_code(
                output,
                &mut bits,
                &mut bit_count,
                code_size,
                usize::from(pixel),
            );
        }
    }
    emit_raw_lzw_code(output, &mut bits, &mut bit_count, code_size, eoi);
    while bit_count > 0 {
        output.push((bits & 0xff) as u8);
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    Ok(())
}

#[inline]
fn indices_fit_color_count(index_stream: &[u8], color_count: usize) -> bool {
    if color_count >= 256 {
        return true;
    }
    if color_count.is_power_of_two() {
        let invalid_bits = !(color_count as u8 - 1);
        let invalid_mask = u64::from(invalid_bits) * 0x0101_0101_0101_0101;
        let mut chunks = index_stream.chunks_exact(8);
        for chunk in &mut chunks {
            let packed = u64::from_le_bytes(chunk.try_into().unwrap());
            if packed & invalid_mask != 0 {
                return false;
            }
        }
        return chunks
            .remainder()
            .iter()
            .all(|&pixel| usize::from(pixel) < color_count);
    }
    index_stream
        .iter()
        .all(|&pixel| usize::from(pixel) < color_count)
}

fn encode_eight_bit_literal_codes(output: &mut Vec<u8>, index_stream: &[u8]) -> Result<(), String> {
    for literals in index_stream.chunks(126) {
        output.push(128);
        output.extend_from_slice(literals);
    }
    output.push(129);
    Ok(())
}

const fn build_five_bit_pair_table() -> [u16; 4096] {
    let mut table = [0u16; 4096];
    let mut index = 0usize;
    while index < table.len() {
        table[index] = ((index & 0x0f) | (((index >> 8) & 0x0f) << 5)) as u16;
        index += 1;
    }
    table
}

static FIVE_BIT_PAIR_TABLE: [u16; 4096] = build_five_bit_pair_table();

#[inline(always)]
unsafe fn five_bit_pair(input: *const u8) -> u64 {
    let index = u16::from_le(unsafe { input.cast::<u16>().read_unaligned() }) as usize;
    u64::from(*unsafe { FIVE_BIT_PAIR_TABLE.get_unchecked(index) })
}

fn encode_five_bit_literal_codes(output: &mut Vec<u8>, index_stream: &[u8]) -> Result<(), String> {
    let clear_count = index_stream.len().div_ceil(14);
    let raw_length = ((index_stream.len() + clear_count + 1) * 5).div_ceil(8);
    let output_start = output.len();
    output.resize(output_start + raw_length, 0);
    let output_pointer = unsafe { output.as_mut_ptr().add(output_start) };
    let input_pointer = index_stream.as_ptr();
    let mut output_position = 0usize;
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let full_group_count = index_stream.len() / 14;
    for group in 0..full_group_count {
        let literals = unsafe { input_pointer.add(group * 14) };
        bits |= 16u64 << bit_count;
        bit_count += 5;
        while bit_count >= 8 {
            unsafe { output_pointer.add(output_position).write(bits as u8) };
            output_position += 1;
            bits >>= 8;
            bit_count -= 8;
        }

        let first = unsafe {
            five_bit_pair(literals)
                | (five_bit_pair(literals.add(2)) << 10)
                | (five_bit_pair(literals.add(4)) << 20)
                | (five_bit_pair(literals.add(6)) << 30)
        };
        let combined = bits | (first << bit_count);
        let first_bytes = combined.to_le_bytes();
        unsafe {
            std::ptr::copy_nonoverlapping(
                first_bytes.as_ptr(),
                output_pointer.add(output_position),
                5,
            )
        };
        output_position += 5;
        bits = combined >> 40;

        let second = unsafe {
            five_bit_pair(literals.add(8))
                | (five_bit_pair(literals.add(10)) << 10)
                | (five_bit_pair(literals.add(12)) << 20)
        };
        bits |= second << bit_count;
        bit_count += 30;
        while bit_count >= 8 {
            unsafe { output_pointer.add(output_position).write(bits as u8) };
            output_position += 1;
            bits >>= 8;
            bit_count -= 8;
        }
    }

    let remainder = &index_stream[full_group_count * 14..];
    if !remainder.is_empty() {
        bits |= 16u64 << bit_count;
        bit_count += 5;
        while bit_count >= 8 {
            unsafe { output_pointer.add(output_position).write(bits as u8) };
            output_position += 1;
            bits >>= 8;
            bit_count -= 8;
        }
        for &pixel in remainder {
            bits |= u64::from(pixel) << bit_count;
            bit_count += 5;
            while bit_count >= 8 {
                unsafe { output_pointer.add(output_position).write(bits as u8) };
                output_position += 1;
                bits >>= 8;
                bit_count -= 8;
            }
        }
    }

    bits |= 17u64 << bit_count;
    bit_count += 5;
    while bit_count > 0 {
        unsafe { output_pointer.add(output_position).write(bits as u8) };
        output_position += 1;
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(output_position, raw_length);
    Ok(())
}

fn encode_seven_bit_literal_codes(output: &mut Vec<u8>, index_stream: &[u8]) -> Result<(), String> {
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    for literals in index_stream.chunks(62) {
        bits |= 64u64 << bit_count;
        bit_count += 7;
        while bit_count >= 8 {
            output.push(bits as u8);
            bits >>= 8;
            bit_count -= 8;
        }
        let mut groups = literals.chunks_exact(8);
        for group in &mut groups {
            let packed = u64::from(group[0])
                | (u64::from(group[1]) << 7)
                | (u64::from(group[2]) << 14)
                | (u64::from(group[3]) << 21)
                | (u64::from(group[4]) << 28)
                | (u64::from(group[5]) << 35)
                | (u64::from(group[6]) << 42)
                | (u64::from(group[7]) << 49);
            bits |= packed << bit_count;
            bit_count += 56;
            output.extend_from_slice(&(bits as u64).to_le_bytes()[..7]);
            bits >>= 56;
            bit_count -= 56;
        }
        for &pixel in groups.remainder() {
            bits |= u64::from(pixel) << bit_count;
            bit_count += 7;
            while bit_count >= 8 {
                output.push(bits as u8);
                bits >>= 8;
                bit_count -= 8;
            }
        }
    }
    bits |= 65u64 << bit_count;
    bit_count += 7;
    while bit_count > 0 {
        output.push(bits as u8);
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    Ok(())
}

fn encode_nine_bit_literal_codes(output: &mut Vec<u8>, index_stream: &[u8]) -> Result<(), String> {
    let clear_count = index_stream.len().div_ceil(254);
    let raw_length = index_stream
        .len()
        .checked_add(clear_count)
        .and_then(|codes| codes.checked_add(1))
        .and_then(|codes| codes.checked_mul(9))
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?
        .div_ceil(8);
    let output_start = output.len();
    let writable_length = raw_length
        .checked_add(8)
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?;
    output.resize(output_start + writable_length, 0);
    let output_pointer = unsafe { output.as_mut_ptr().add(output_start) };
    let mut output_position = 0usize;
    let mut bits = 0u64;
    let mut bit_count = 0usize;

    for literals in index_stream.chunks(254) {
        append_nine_bit_literal_code(
            output_pointer,
            &mut output_position,
            &mut bits,
            &mut bit_count,
            256,
        );

        let mut groups = literals.chunks_exact(7);
        for group in &mut groups {
            let packed = u64::from(group[0])
                | (u64::from(group[1]) << 9)
                | (u64::from(group[2]) << 18)
                | (u64::from(group[3]) << 27)
                | (u64::from(group[4]) << 36)
                | (u64::from(group[5]) << 45)
                | (u64::from(group[6]) << 54);
            let combined = bits | (packed << bit_count);
            let total_bits = bit_count + 63;
            if total_bits >= 64 {
                unsafe {
                    output_pointer
                        .add(output_position)
                        .cast::<u64>()
                        .write_unaligned(combined.to_le());
                };
                output_position += 8;
                bits = if bit_count == 0 {
                    0
                } else {
                    packed >> (64 - bit_count)
                };
                bit_count = total_bits - 64;
            } else {
                debug_assert_eq!(bit_count, 0);
                unsafe {
                    output_pointer
                        .add(output_position)
                        .cast::<u64>()
                        .write_unaligned(combined.to_le());
                };
                output_position += 7;
                bits = combined >> 56;
                bit_count = total_bits - 56;
            }
        }
        for &pixel in groups.remainder() {
            append_nine_bit_literal_code(
                output_pointer,
                &mut output_position,
                &mut bits,
                &mut bit_count,
                u16::from(pixel),
            );
        }
    }
    append_nine_bit_literal_code(
        output_pointer,
        &mut output_position,
        &mut bits,
        &mut bit_count,
        257,
    );
    while bit_count > 0 {
        unsafe { output_pointer.add(output_position).write(bits as u8) };
        output_position += 1;
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(output_position, raw_length);
    output.truncate(output_start + raw_length);
    Ok(())
}

#[inline(always)]
fn append_nine_bit_literal_code(
    output: *mut u8,
    output_position: &mut usize,
    bits: &mut u64,
    bit_count: &mut usize,
    code: u16,
) {
    *bits |= u64::from(code) << *bit_count;
    *bit_count += 9;
    while *bit_count >= 8 {
        unsafe { output.add(*output_position).write(*bits as u8) };
        *output_position += 1;
        *bits >>= 8;
        *bit_count -= 8;
    }
}

fn encode_two_bit_literal_codes(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    _color_count: usize,
) -> Result<(), String> {
    let pair_count = index_stream.len() / 2;
    let raw_bit_length = pair_count * 9 + usize::from(index_stream.len() % 2 != 0) * 6 + 3;
    let raw_length = raw_bit_length.div_ceil(8);
    let output_start = output.len();
    output.resize(output_start + raw_length, 0);
    let output_pointer = unsafe { output.as_mut_ptr().add(output_start) };
    let mut output_position = 0usize;
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let mut groups = index_stream.chunks_exact(14);
    for group in &mut groups {
        let mut packed = 0u64;
        for pair in 0..7 {
            let first = usize::from(group[pair * 2]);
            let second = usize::from(group[pair * 2 + 1]);
            let reset_group = 4 | (first << 3) | (second << 6);
            packed |= (reset_group as u64) << (pair * 9);
        }

        let combined = bits | (packed << bit_count);
        let total_bits = bit_count + 63;
        if total_bits >= 64 {
            let bytes = combined.to_le_bytes();
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    output_pointer.add(output_position),
                    8,
                )
            };
            output_position += 8;
            bits = packed >> (64 - bit_count);
            bit_count = total_bits - 64;
        } else {
            let bytes = combined.to_le_bytes();
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    output_pointer.add(output_position),
                    7,
                )
            };
            output_position += 7;
            bits = combined >> 56;
            bit_count = total_bits - 56;
        }
    }

    let remainder = groups.remainder();
    let mut index = 0usize;
    while index + 1 < remainder.len() {
        let first = usize::from(remainder[index]);
        let second = usize::from(remainder[index + 1]);
        // Each reset group is CLEAR, literal, literal: three 3-bit codes.
        bits |= ((4 | (first << 3) | (second << 6)) as u64) << bit_count;
        bit_count += 9;
        if bit_count >= 32 {
            let bytes = (bits as u32).to_le_bytes();
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    output_pointer.add(output_position),
                    4,
                )
            };
            output_position += 4;
            bits >>= 32;
            bit_count -= 32;
        }
        index += 2;
    }
    if index < remainder.len() {
        let pixel = usize::from(remainder[index]);
        bits |= ((4 | (pixel << 3)) as u64) << bit_count;
        bit_count += 6;
    }
    // EOI is code 5 at the unchanged 3-bit width.
    bits |= 5u64 << bit_count;
    bit_count += 3;
    while bit_count > 0 {
        unsafe { output_pointer.add(output_position).write(bits as u8) };
        output_position += 1;
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(output_position, raw_length);
    Ok(())
}

#[inline]
fn emit_raw_lzw_code(
    output: &mut Vec<u8>,
    bits: &mut u64,
    bit_count: &mut usize,
    code_size: usize,
    code: usize,
) {
    *bits |= (code as u64) << *bit_count;
    *bit_count += code_size;
    if *bit_count >= 32 {
        output.extend_from_slice(&(*bits as u32).to_le_bytes());
        *bits >>= 32;
        *bit_count -= 32;
    }
}

fn encode_indexed_lzw_to_with_tables(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
    tables: &mut LzwEncodeTables,
) -> Result<(), String> {
    encode_indexed_lzw_slice_to(output, index_stream, min_code_size, color_count, tables)
}

#[inline(always)]
fn encode_indexed_lzw_slice_to(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
    tables: &mut LzwEncodeTables,
) -> Result<(), String> {
    if min_code_size == 0 || min_code_size > 8 {
        return Err(format!("Invalid LZW minimum code size {min_code_size}"));
    }
    if color_count == 0 || color_count > 256 {
        return Err("Invalid color count".to_string());
    }
    if index_stream.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    if min_code_size == 8 && color_count == 256 {
        return encode_indexed_lzw_256_to(output, index_stream, tables);
    }

    output.push(min_code_size);
    let mut sub_len_pos = output.len();
    output.push(0);
    let mut sub_len = 0u8;

    let clear = 1usize << min_code_size;
    let eoi = clear + 1;
    let mut next_code = eoi + 1;
    let mut code_size = usize::from(min_code_size) + 1;
    let mut code_mask = (1usize << code_size) - 1;
    let mut bits = 0u32;
    let mut bit_count = 0usize;
    let mut epoch = tables.reset();
    let mut epoch_base = epoch << LZW_ENTRY_EPOCH_SHIFT;

    emit_lzw_code(
        output,
        &mut sub_len_pos,
        &mut sub_len,
        &mut bits,
        &mut bit_count,
        code_size,
        clear,
    );

    let mut ib = usize::from(index_stream[0]);
    if ib >= color_count {
        return Err("Pixel index out of range".to_string());
    }

    for &pixel in &index_stream[1..] {
        let k = usize::from(pixel);
        if k >= color_count {
            return Err("Pixel index out of range".to_string());
        }
        let slot = ib * color_count + k;
        let entry = tables.entries[slot];
        if entry >= epoch_base {
            ib = (entry & LZW_ENTRY_CODE_MASK) as usize;
            continue;
        }

        emit_lzw_code(
            output,
            &mut sub_len_pos,
            &mut sub_len,
            &mut bits,
            &mut bit_count,
            code_size,
            ib,
        );
        if next_code == 4096 {
            emit_lzw_code(
                output,
                &mut sub_len_pos,
                &mut sub_len,
                &mut bits,
                &mut bit_count,
                code_size,
                clear,
            );
            next_code = eoi + 1;
            code_size = usize::from(min_code_size) + 1;
            code_mask = (1usize << code_size) - 1;
            epoch = tables.reset();
            epoch_base = epoch << LZW_ENTRY_EPOCH_SHIFT;
        } else {
            if next_code >= code_mask + 1 && code_size < 12 {
                code_size += 1;
                code_mask = (1usize << code_size) - 1;
            }
            tables.entries[slot] = epoch_base | next_code as u32;
            next_code += 1;
        }
        ib = k;
    }

    emit_lzw_code(
        output,
        &mut sub_len_pos,
        &mut sub_len,
        &mut bits,
        &mut bit_count,
        code_size,
        ib,
    );
    emit_lzw_code(
        output,
        &mut sub_len_pos,
        &mut sub_len,
        &mut bits,
        &mut bit_count,
        code_size,
        eoi,
    );
    if bit_count > 0 {
        push_lzw_byte(output, &mut sub_len_pos, &mut sub_len, (bits & 0xff) as u8);
    }
    output[sub_len_pos] = sub_len;
    if sub_len > 0 {
        output.push(0);
    }
    Ok(())
}

fn encode_indexed_lzw_256_to(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    tables: &mut LzwEncodeTables,
) -> Result<(), String> {
    output.push(8);
    let mut sub_len_pos = output.len();
    output.push(0);
    let mut sub_len = 0u8;
    let clear = 256usize;
    let eoi = 257usize;
    let mut next_code = 258usize;
    let mut code_size = 9usize;
    let mut bits = 0u32;
    let mut bit_count = 0usize;
    let mut epoch = tables.reset();
    let mut epoch_base = epoch << LZW_ENTRY_EPOCH_SHIFT;

    emit_lzw_code(
        output,
        &mut sub_len_pos,
        &mut sub_len,
        &mut bits,
        &mut bit_count,
        code_size,
        clear,
    );

    let mut ib = usize::from(index_stream[0]);
    for &pixel in &index_stream[1..] {
        let k = usize::from(pixel);
        let slot = (ib << 8) | k;
        let entry = unsafe { *tables.entries.get_unchecked(slot) };
        if entry >= epoch_base {
            ib = (entry & LZW_ENTRY_CODE_MASK) as usize;
            continue;
        }

        emit_lzw_code(
            output,
            &mut sub_len_pos,
            &mut sub_len,
            &mut bits,
            &mut bit_count,
            code_size,
            ib,
        );
        if next_code == 4096 {
            emit_lzw_code(
                output,
                &mut sub_len_pos,
                &mut sub_len,
                &mut bits,
                &mut bit_count,
                code_size,
                clear,
            );
            next_code = 258;
            code_size = 9;
            epoch = tables.reset();
            epoch_base = epoch << LZW_ENTRY_EPOCH_SHIFT;
        } else {
            if next_code == 512 || next_code == 1024 || next_code == 2048 {
                code_size += 1;
            }
            unsafe {
                *tables.entries.get_unchecked_mut(slot) = epoch_base | next_code as u32;
            }
            next_code += 1;
        }
        ib = k;
    }

    emit_lzw_code(
        output,
        &mut sub_len_pos,
        &mut sub_len,
        &mut bits,
        &mut bit_count,
        code_size,
        ib,
    );
    emit_lzw_code(
        output,
        &mut sub_len_pos,
        &mut sub_len,
        &mut bits,
        &mut bit_count,
        code_size,
        eoi,
    );
    if bit_count > 0 {
        push_lzw_byte(output, &mut sub_len_pos, &mut sub_len, (bits & 0xff) as u8);
    }
    output[sub_len_pos] = sub_len;
    if sub_len > 0 {
        output.push(0);
    }
    Ok(())
}

fn encode_indexed_lzw_rect_to(
    output: &mut Vec<u8>,
    source: &[u8],
    offset: usize,
    width: usize,
    height: usize,
    stride: usize,
    min_code_size: u8,
    color_count: usize,
    tables: &mut LzwEncodeTables,
) -> Result<(), String> {
    if width == 0 || height == 0 || stride < width {
        return Err("Indexed pixel stream is empty".to_string());
    }
    let last_pixel_end = offset
        .checked_add(
            height
                .saturating_sub(1)
                .checked_mul(stride)
                .and_then(|start| start.checked_add(width))
                .ok_or_else(|| "Frame size overflow".to_string())?,
        )
        .ok_or_else(|| "Frame size overflow".to_string())?;
    if last_pixel_end > source.len() {
        return Err("Indexed pixel stream is empty".to_string());
    }

    let mut remaining = width * height;
    let mut source_index = offset;
    let mut row_remaining = width;
    let row_skip = stride - width;
    encode_indexed_lzw_source_to(
        output,
        remaining,
        min_code_size,
        color_count,
        tables,
        || {
            if remaining == 0 {
                return None;
            }
            let pixel = source.get(source_index).copied();
            source_index += 1;
            row_remaining -= 1;
            remaining -= 1;
            if row_remaining == 0 {
                source_index += row_skip;
                row_remaining = width;
            }
            pixel
        },
    )
}

fn encode_indexed_lzw_source_to<F>(
    output: &mut Vec<u8>,
    index_count: usize,
    min_code_size: u8,
    color_count: usize,
    tables: &mut LzwEncodeTables,
    mut next_pixel: F,
) -> Result<(), String>
where
    F: FnMut() -> Option<u8>,
{
    if min_code_size == 0 || min_code_size > 8 {
        return Err(format!("Invalid LZW minimum code size {min_code_size}"));
    }
    if color_count == 0 || color_count > 256 {
        return Err("Invalid color count".to_string());
    }
    if index_count == 0 {
        return Err("Indexed pixel stream is empty".to_string());
    }

    output.push(min_code_size);
    let mut sub_len_pos = output.len();
    output.push(0);
    let mut sub_len = 0u8;

    let clear = 1usize << min_code_size;
    let eoi = clear + 1;
    let mut next_code = eoi + 1;
    let mut code_size = usize::from(min_code_size) + 1;
    let mut code_mask = (1usize << code_size) - 1;
    let mut bits = 0u32;
    let mut bit_count = 0usize;

    let mut epoch = tables.reset();

    emit_lzw_code(
        output,
        &mut sub_len_pos,
        &mut sub_len,
        &mut bits,
        &mut bit_count,
        code_size,
        clear,
    );

    let mut ib =
        usize::from(next_pixel().ok_or_else(|| "Indexed pixel stream is empty".to_string())?);
    if ib >= color_count {
        return Err("Pixel index out of range".to_string());
    }

    for _ in 1..index_count {
        let pixel = next_pixel().ok_or_else(|| "Indexed pixel stream is empty".to_string())?;
        let k = usize::from(pixel);
        if k >= color_count {
            return Err("Pixel index out of range".to_string());
        }

        let slot = ib * color_count + k;
        let entry = tables.entries[slot];
        let found = (entry >> LZW_ENTRY_EPOCH_SHIFT == epoch)
            .then_some((entry & LZW_ENTRY_CODE_MASK) as usize);

        if let Some(code) = found {
            if code < next_code {
                ib = code;
                continue;
            }
        }

        emit_lzw_code(
            output,
            &mut sub_len_pos,
            &mut sub_len,
            &mut bits,
            &mut bit_count,
            code_size,
            ib,
        );

        if next_code == 4096 {
            emit_lzw_code(
                output,
                &mut sub_len_pos,
                &mut sub_len,
                &mut bits,
                &mut bit_count,
                code_size,
                clear,
            );
            next_code = eoi + 1;
            code_size = usize::from(min_code_size) + 1;
            code_mask = (1usize << code_size) - 1;
            epoch = tables.reset();
        } else if min_code_size == 1 {
            tables.entries[slot] = (epoch << LZW_ENTRY_EPOCH_SHIFT) | next_code as u32;
            next_code += 1;
            if next_code > code_mask && code_size < 12 {
                code_size += 1;
                code_mask = (1usize << code_size) - 1;
            }
        } else {
            if next_code >= code_mask + 1 && code_size < 12 {
                code_size += 1;
                code_mask = (1usize << code_size) - 1;
            }
            tables.entries[slot] = (epoch << LZW_ENTRY_EPOCH_SHIFT) | next_code as u32;
            next_code += 1;
        }

        ib = k;
    }

    emit_lzw_code(
        output,
        &mut sub_len_pos,
        &mut sub_len,
        &mut bits,
        &mut bit_count,
        code_size,
        ib,
    );
    emit_lzw_code(
        output,
        &mut sub_len_pos,
        &mut sub_len,
        &mut bits,
        &mut bit_count,
        code_size,
        eoi,
    );

    if bit_count > 0 {
        push_lzw_byte(output, &mut sub_len_pos, &mut sub_len, (bits & 0xff) as u8);
    }

    output[sub_len_pos] = sub_len;
    if sub_len > 0 {
        output.push(0);
    }

    Ok(())
}

#[inline(always)]
fn emit_lzw_code(
    output: &mut Vec<u8>,
    sub_len_pos: &mut usize,
    sub_len: &mut u8,
    bits: &mut u32,
    bit_count: &mut usize,
    code_size: usize,
    code: usize,
) {
    *bits |= ((code & 0xffff) as u32) << *bit_count;
    *bit_count += code_size;
    while *bit_count >= 8 {
        push_lzw_byte(output, sub_len_pos, sub_len, (*bits & 0xff) as u8);
        *bits >>= 8;
        *bit_count -= 8;
    }
}

#[inline(always)]
fn push_lzw_byte(output: &mut Vec<u8>, sub_len_pos: &mut usize, sub_len: &mut u8, byte: u8) {
    output.push(byte);
    *sub_len = sub_len.wrapping_add(1);
    if *sub_len == 255 {
        output[*sub_len_pos] = 255;
        *sub_len_pos = output.len();
        output.push(0);
        *sub_len = 0;
    }
}

fn build_palette_pixels(
    data: &[u8],
    frame: &FrameMetadata,
    format: PixelFormat,
) -> Result<Vec<u8>, String> {
    let byte_len = frame
        .palette_size
        .checked_mul(4)
        .ok_or_else(|| "Palette size overflow".to_string())?;
    let mut palette = vec![0; byte_len];

    for index in 0..frame.palette_size {
        let palette_offset = frame
            .palette_offset
            .checked_add(index * 3)
            .ok_or_else(|| "Palette offset overflow".to_string())?;
        let end = checked_add(palette_offset, 3, data.len(), "palette color")?;
        let dst = index * 4;
        let r = data[palette_offset];
        let g = data[palette_offset + 1];
        let b = data[end - 1];
        match format {
            PixelFormat::Rgba => {
                palette[dst] = r;
                palette[dst + 1] = g;
                palette[dst + 2] = b;
            }
            PixelFormat::Bgra => {
                palette[dst] = b;
                palette[dst + 1] = g;
                palette[dst + 2] = r;
            }
        }
        palette[dst + 3] = 255;
    }

    Ok(palette)
}

fn build_palette_u32(
    data: &[u8],
    frame: &FrameMetadata,
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    let palette_bytes = frame
        .palette_size
        .checked_mul(3)
        .ok_or_else(|| "Palette size overflow".to_string())?;
    let palette_end = checked_add(
        frame.palette_offset,
        palette_bytes,
        data.len(),
        "palette color table",
    )?;
    let mut palette = Vec::with_capacity(frame.palette_size);
    for color in data[frame.palette_offset..palette_end].chunks_exact(3) {
        let r = u32::from(color[0]);
        let g = u32::from(color[1]);
        let b = u32::from(color[2]);
        palette.push(match format {
            PixelFormat::Rgba => r | (g << 8) | (b << 16) | (255 << 24),
            PixelFormat::Bgra => b | (g << 8) | (r << 16) | (255 << 24),
        });
    }

    Ok(palette)
}

fn blit_indices_to_pixels(
    palette: &[u8],
    canvas_width: u16,
    frame: &FrameMetadata,
    indices: &[u8],
    pixels: &mut [u8],
) -> Result<(), String> {
    let canvas_width = usize::from(canvas_width);
    let frame_width = usize::from(frame.width);
    let frame_height = usize::from(frame.height);
    let frame_x = usize::from(frame.x);
    let frame_y = usize::from(frame.y);
    let transparent_index = frame.transparent_index;
    let mut src = 0usize;

    for y in 0..frame_height {
        let mut dst = ((frame_y + y) * canvas_width + frame_x)
            .checked_mul(4)
            .ok_or_else(|| "Frame destination offset overflow".to_string())?;

        for _ in 0..frame_width {
            let index = *indices
                .get(src)
                .ok_or_else(|| "Decoded index buffer is too short".to_string())?;
            src += 1;

            if Some(index) == transparent_index {
                dst += 4;
                continue;
            }

            write_palette_pixel(palette, index, pixels, dst)?;
            dst += 4;
        }
    }

    Ok(())
}

fn blit_indices_to_canvas_u32(
    palette: &[u32],
    canvas_width: u16,
    frame: &FrameMetadata,
    indices: &[u8],
    canvas: &mut [u32],
) -> Result<(), String> {
    let canvas_width = usize::from(canvas_width);
    let frame_width = usize::from(frame.width);
    let frame_height = usize::from(frame.height);
    let frame_x = usize::from(frame.x);
    let frame_y = usize::from(frame.y);
    let frame_pixels = frame_width
        .checked_mul(frame_height)
        .ok_or_else(|| "Decoded frame size overflow".to_string())?;
    if indices.len() < frame_pixels {
        return Err("Decoded index buffer is too short".to_string());
    }
    let frame_right = frame_x
        .checked_add(frame_width)
        .ok_or_else(|| "Frame destination offset overflow".to_string())?;
    let frame_bottom = frame_y
        .checked_add(frame_height)
        .ok_or_else(|| "Frame destination offset overflow".to_string())?;
    if frame_right > canvas_width || canvas_width == 0 || frame_bottom > canvas.len() / canvas_width
    {
        return Err("Frame destination exceeds canvas bounds".to_string());
    }
    let full_byte_palette = palette.len() == 256;
    if full_byte_palette
        && frame.transparent_index.is_none()
        && frame_x == 0
        && frame_y == 0
        && frame_width == canvas_width
        && frame_height == canvas.len() / canvas_width
    {
        // The common GIF case is an opaque full-canvas frame with a complete
        // 256-entry palette. Decode eight index bytes per iteration so the
        // palette lookups and stores stay in a tight pointer loop.
        let indices_pointer = indices.as_ptr();
        let palette_pointer = palette.as_ptr();
        let canvas_pointer = canvas.as_mut_ptr();
        let mut pixel_index = 0usize;
        while pixel_index + 8 <= frame_pixels {
            let packed =
                unsafe { std::ptr::read_unaligned(indices_pointer.add(pixel_index).cast::<u64>()) };
            let colors = unsafe {
                [
                    *palette_pointer.add((packed & 0xff) as usize),
                    *palette_pointer.add(((packed >> 8) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 16) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 24) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 32) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 40) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 48) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 56) & 0xff) as usize),
                ]
            };
            unsafe {
                std::ptr::copy_nonoverlapping(
                    colors.as_ptr(),
                    canvas_pointer.add(pixel_index),
                    colors.len(),
                );
            }
            pixel_index += 8;
        }
        while pixel_index < frame_pixels {
            let index = unsafe { *indices_pointer.add(pixel_index) };
            unsafe {
                canvas_pointer
                    .add(pixel_index)
                    .write(*palette_pointer.add(usize::from(index)));
            }
            pixel_index += 1;
        }
        return Ok(());
    }
    match frame.transparent_index {
        None => {
            for y in 0..frame_height {
                let source = y * frame_width;
                let source_row = &indices[source..source + frame_width];
                let destination = (frame_y + y) * canvas_width + frame_x;
                let destination_row = &mut canvas[destination..destination + frame_width];
                if full_byte_palette {
                    let source_pointer = source_row.as_ptr();
                    let palette_pointer = palette.as_ptr();
                    let destination_pointer = destination_row.as_mut_ptr();
                    let mut pixel_index = 0usize;
                    while pixel_index + 8 <= frame_width {
                        let packed = unsafe {
                            std::ptr::read_unaligned(source_pointer.add(pixel_index).cast::<u64>())
                        };
                        let colors = unsafe {
                            [
                                *palette_pointer.add((packed & 0xff) as usize),
                                *palette_pointer.add(((packed >> 8) & 0xff) as usize),
                                *palette_pointer.add(((packed >> 16) & 0xff) as usize),
                                *palette_pointer.add(((packed >> 24) & 0xff) as usize),
                                *palette_pointer.add(((packed >> 32) & 0xff) as usize),
                                *palette_pointer.add(((packed >> 40) & 0xff) as usize),
                                *palette_pointer.add(((packed >> 48) & 0xff) as usize),
                                *palette_pointer.add(((packed >> 56) & 0xff) as usize),
                            ]
                        };
                        unsafe {
                            std::ptr::copy_nonoverlapping(
                                colors.as_ptr(),
                                destination_pointer.add(pixel_index),
                                colors.len(),
                            );
                        }
                        pixel_index += 8;
                    }
                    while pixel_index < frame_width {
                        let index = unsafe { *source_pointer.add(pixel_index) };
                        unsafe {
                            destination_pointer
                                .add(pixel_index)
                                .write(*palette_pointer.add(usize::from(index)));
                        }
                        pixel_index += 1;
                    }
                    continue;
                }
                for (pixel, &index) in destination_row.iter_mut().zip(source_row) {
                    *pixel = *palette
                        .get(usize::from(index))
                        .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
                }
            }
        }
        Some(transparent_index) => {
            for y in 0..frame_height {
                let source = y * frame_width;
                let source_row = &indices[source..source + frame_width];
                let destination = (frame_y + y) * canvas_width + frame_x;
                let destination_row = &mut canvas[destination..destination + frame_width];
                if full_byte_palette {
                    let source_pointer = source_row.as_ptr();
                    let palette_pointer = palette.as_ptr();
                    let destination_pointer = destination_row.as_mut_ptr();
                    let transparent_bytes =
                        u64::from(transparent_index).wrapping_mul(0x0101_0101_0101_0101);
                    let mut pixel_index = 0usize;
                    while pixel_index + 8 <= frame_width {
                        let packed_indices = unsafe {
                            std::ptr::read_unaligned(source_pointer.add(pixel_index).cast::<u64>())
                        };
                        if packed_indices == transparent_bytes {
                            pixel_index += 8;
                            continue;
                        }
                        let compared = packed_indices ^ transparent_bytes;
                        let transparent_lanes = compared.wrapping_sub(0x0101_0101_0101_0101)
                            & !compared
                            & 0x8080_8080_8080_8080;
                        if transparent_lanes == 0 {
                            let colors = unsafe {
                                [
                                    *palette_pointer.add((packed_indices & 0xff) as usize),
                                    *palette_pointer.add(((packed_indices >> 8) & 0xff) as usize),
                                    *palette_pointer.add(((packed_indices >> 16) & 0xff) as usize),
                                    *palette_pointer.add(((packed_indices >> 24) & 0xff) as usize),
                                    *palette_pointer.add(((packed_indices >> 32) & 0xff) as usize),
                                    *palette_pointer.add(((packed_indices >> 40) & 0xff) as usize),
                                    *palette_pointer.add(((packed_indices >> 48) & 0xff) as usize),
                                    *palette_pointer.add(((packed_indices >> 56) & 0xff) as usize),
                                ]
                            };
                            unsafe {
                                std::ptr::copy_nonoverlapping(
                                    colors.as_ptr(),
                                    destination_pointer.add(pixel_index),
                                    colors.len(),
                                );
                            }
                            pixel_index += 8;
                            continue;
                        }
                        for lane in 0..8 {
                            let index = unsafe { *source_pointer.add(pixel_index + lane) };
                            if index != transparent_index {
                                unsafe {
                                    destination_pointer
                                        .add(pixel_index + lane)
                                        .write(*palette_pointer.add(usize::from(index)));
                                }
                            }
                        }
                        pixel_index += 8;
                    }
                    while pixel_index < frame_width {
                        let index = unsafe { *source_pointer.add(pixel_index) };
                        if index != transparent_index {
                            unsafe {
                                destination_pointer
                                    .add(pixel_index)
                                    .write(*palette_pointer.add(usize::from(index)));
                            }
                        }
                        pixel_index += 1;
                    }
                    continue;
                }
                for (pixel, &index) in destination_row.iter_mut().zip(source_row) {
                    if index != transparent_index {
                        *pixel = *palette
                            .get(usize::from(index))
                            .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn apply_frame_disposal_u32(
    canvas: &mut [u32],
    canvas_width: u16,
    canvas_height: u16,
    frame: &FrameMetadata,
    restore: Option<Vec<u32>>,
) {
    if frame.disposal == 2 {
        clear_frame_rect_u32(canvas, canvas_width, canvas_height, frame);
    } else if frame.disposal == 3 {
        if let Some(restore) = restore {
            canvas.copy_from_slice(&restore);
        }
    }
}

fn clear_frame_rect_u32(
    canvas: &mut [u32],
    canvas_width: u16,
    canvas_height: u16,
    frame: &FrameMetadata,
) {
    let canvas_width = usize::from(canvas_width);
    let canvas_height = usize::from(canvas_height);
    let x = usize::from(frame.x).min(canvas_width);
    let y = usize::from(frame.y).min(canvas_height);
    let right = usize::from(frame.x)
        .saturating_add(usize::from(frame.width))
        .min(canvas_width);
    let bottom = usize::from(frame.y)
        .saturating_add(usize::from(frame.height))
        .min(canvas_height);

    if right <= x {
        return;
    }

    for row in y..bottom {
        let start = row * canvas_width + x;
        canvas[start..start + (right - x)].fill(0);
    }
}

#[inline]
fn write_palette_pixel(
    palette: &[u8],
    index: u8,
    pixels: &mut [u8],
    dst: usize,
) -> Result<(), String> {
    let color = usize::from(index)
        .checked_mul(4)
        .ok_or_else(|| "Palette index overflow".to_string())?;
    if color + 3 >= palette.len() {
        return Err(format!("Palette index {index} exceeds palette size"));
    }
    if dst + 3 >= pixels.len() {
        return Err("Frame destination exceeds canvas bounds".to_string());
    }

    pixels[dst] = palette[color];
    pixels[dst + 1] = palette[color + 1];
    pixels[dst + 2] = palette[color + 2];
    pixels[dst + 3] = palette[color + 3];

    Ok(())
}

fn collect_image_data_into(
    data: &[u8],
    data_offset: usize,
    image_data: &mut Vec<u8>,
) -> Result<(), String> {
    if data_offset >= data.len() {
        return Err("Image data is missing an LZW minimum code size".to_string());
    }

    let mut offset = data_offset + 1;
    image_data.clear();
    loop {
        if offset >= data.len() {
            return Err("Truncated image data".to_string());
        }
        let length = usize::from(data[offset]);
        offset += 1;
        if length == 0 {
            return Ok(());
        }
        let end = checked_add(offset, length, data.len(), "image data")?;
        image_data.extend_from_slice(&data[offset..end]);
        offset = end;
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn collect_image_data_into_fixed<'a>(
    data: &[u8],
    data_offset: usize,
    image_data: &'a mut [std::mem::MaybeUninit<u8>],
) -> Result<&'a [u8], String> {
    if data_offset >= data.len() {
        return Err("Image data is missing an LZW minimum code size".to_string());
    }

    let mut offset = data_offset + 1;
    let mut output_len = 0usize;
    loop {
        if offset >= data.len() {
            return Err("Truncated image data".to_string());
        }
        let length = usize::from(data[offset]);
        offset += 1;
        if length == 0 {
            return Ok(unsafe {
                std::slice::from_raw_parts(image_data.as_ptr().cast::<u8>(), output_len)
            });
        }
        let end = checked_add(offset, length, data.len(), "image data")?;
        let output_end = output_len
            .checked_add(length)
            .ok_or_else(|| "Image data size overflow".to_string())?;
        if output_end > image_data.len() {
            return Err("Fixed image data buffer is too short".to_string());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr().add(offset),
                image_data.as_mut_ptr().add(output_len).cast::<u8>(),
                length,
            );
        }
        output_len = output_end;
        offset = end;
    }
}

fn lzw_decode_to_indices_direct(
    min_code_size: u8,
    image_data: &[u8],
    output: &mut [u8],
) -> Result<(), String> {
    let mut scratch = LzwStackScratch::default();
    lzw_decode_to_indices_direct_with_scratch(min_code_size, image_data, output, &mut scratch)
}

fn lzw_decode_to_indices_direct_with_scratch(
    min_code_size: u8,
    image_data: &[u8],
    output: &mut [u8],
    scratch: &mut LzwStackScratch,
) -> Result<(), String> {
    if min_code_size > 11 {
        return Err(format!("Invalid LZW minimum code size {min_code_size}"));
    }
    let clear = 1usize << min_code_size;
    let eoi = clear + 1;
    let mut next_code = eoi + 1;
    let mut code_size = usize::from(min_code_size) + 1;
    let mut code_mask = (1usize << code_size) - 1;

    let prefix = &mut scratch.prefix;
    let suffix = &mut scratch.suffix;
    let string_length = &mut scratch.string_length;

    let mut output_index = 0usize;

    let mut q = 0usize;
    let mut bits = 0usize;
    let mut bit_count = 0usize;
    let mut prev_code: Option<usize> = None;
    let mut prev_first = 0u8;
    let mut prev_length = 0u16;

    loop {
        #[cfg(target_pointer_width = "32")]
        if bit_count < code_size && image_data.len().saturating_sub(q) >= 2 {
            let word =
                unsafe { std::ptr::read_unaligned(image_data.as_ptr().add(q).cast::<u16>()) };
            bits |= usize::from(u16::from_le(word)) << bit_count;
            bit_count += 16;
            q += 2;
        }
        #[cfg(target_pointer_width = "64")]
        if bit_count < code_size && image_data.len().saturating_sub(q) >= 4 {
            let word =
                unsafe { std::ptr::read_unaligned(image_data.as_ptr().add(q).cast::<u32>()) };
            bits |= usize::try_from(u32::from_le(word)).unwrap() << bit_count;
            bit_count += 32;
            q += 4;
        }
        while bit_count < code_size && q < image_data.len() {
            bits |= usize::from(image_data[q]) << bit_count;
            bit_count += 8;
            q += 1;
        }

        if bit_count < code_size {
            break;
        }

        let code = bits & code_mask;
        bits >>= code_size;
        bit_count -= code_size;

        if code == clear {
            next_code = eoi + 1;
            code_size = usize::from(min_code_size) + 1;
            code_mask = (1usize << code_size) - 1;
            prev_code = None;
            prev_length = 0;
            continue;
        }

        if code == eoi {
            break;
        }

        let mut cur = code;
        let append_first = cur >= next_code;
        if append_first {
            if cur != next_code {
                return Err("Invalid LZW dictionary code".to_string());
            }
            let Some(prev) = prev_code else {
                return Err("Invalid first LZW dictionary code".to_string());
            };
            cur = prev;
        }
        let base_length = if append_first {
            usize::from(prev_length)
        } else if cur < clear {
            1
        } else {
            usize::from(unsafe { string_length.get_unchecked(cur).assume_init() })
        };
        let decoded_length = base_length + usize::from(append_first);
        let decoded_end = output_index + decoded_length;
        if decoded_end > output.len() {
            return Err("LZW decoded output exceeds frame dimensions".to_string());
        }

        let output_address = output.as_mut_ptr();
        let mut write_position = decoded_end;
        if append_first {
            write_position -= 1;
            unsafe {
                output_address.add(write_position).write(prev_first);
            }
        }
        while cur >= clear {
            write_position -= 1;
            unsafe {
                output_address
                    .add(write_position)
                    .write(suffix.get_unchecked(cur).assume_init());
            }
            cur = usize::from(unsafe { prefix.get_unchecked(cur).assume_init() });
        }
        let out_first = cur as u8;
        write_position -= 1;
        unsafe {
            output_address.add(write_position).write(out_first);
        }
        debug_assert_eq!(write_position, output_index);
        output_index = decoded_end;

        if let Some(prev) = prev_code {
            if next_code < 4096 {
                unsafe {
                    prefix.get_unchecked_mut(next_code).write(prev as u16);
                    suffix.get_unchecked_mut(next_code).write(out_first);
                }
                unsafe {
                    string_length
                        .get_unchecked_mut(next_code)
                        .write(prev_length + 1);
                }
                next_code += 1;

                if next_code >= code_mask + 1 && code_size < 12 {
                    code_size += 1;
                    code_mask = (1usize << code_size) - 1;
                }
            }
        }

        prev_code = Some(code);
        prev_first = out_first;
        prev_length = decoded_length as u16;
    }

    Ok(())
}

#[inline(always)]
unsafe fn copy_lzw_dictionary_string(
    source: *const u8,
    destination: *mut u8,
    length: usize,
    inline_limit: usize,
) {
    if inline_limit > 16 {
        if length <= inline_limit {
            for offset in 0..length {
                destination.add(offset).write(*source.add(offset));
            }
        } else {
            std::ptr::copy_nonoverlapping(source, destination, length);
        }
        return;
    }
    match length {
        0 => {}
        1 => destination.write(*source),
        2 => destination
            .cast::<u16>()
            .write_unaligned(source.cast::<u16>().read_unaligned()),
        3 => {
            destination
                .cast::<u16>()
                .write_unaligned(source.cast::<u16>().read_unaligned());
            destination.add(2).write(*source.add(2));
        }
        4 => destination
            .cast::<u32>()
            .write_unaligned(source.cast::<u32>().read_unaligned()),
        5 => {
            destination
                .cast::<u32>()
                .write_unaligned(source.cast::<u32>().read_unaligned());
            destination.add(4).write(*source.add(4));
        }
        6 => {
            destination
                .cast::<u32>()
                .write_unaligned(source.cast::<u32>().read_unaligned());
            destination
                .add(4)
                .cast::<u16>()
                .write_unaligned(source.add(4).cast::<u16>().read_unaligned());
        }
        7 => {
            destination
                .cast::<u32>()
                .write_unaligned(source.cast::<u32>().read_unaligned());
            destination
                .add(4)
                .cast::<u16>()
                .write_unaligned(source.add(4).cast::<u16>().read_unaligned());
            destination.add(6).write(*source.add(6));
        }
        8 => destination
            .cast::<u64>()
            .write_unaligned(source.cast::<u64>().read_unaligned()),
        _ if length <= inline_limit => {
            for offset in 0..length {
                destination.add(offset).write(*source.add(offset));
            }
        }
        _ => std::ptr::copy_nonoverlapping(source, destination, length),
    }
}

fn lzw_decode_to_indices_copy_with_scratch(
    min_code_size: u8,
    image_data: &[u8],
    output: &mut [u8],
    scratch: &mut LzwStackScratch,
) -> Result<(), String> {
    if min_code_size > 11 {
        return Err(format!("Invalid LZW minimum code size {min_code_size}"));
    }
    let clear = 1usize << min_code_size;
    let eoi = clear + 1;
    let mut next_code = eoi + 1;
    let mut code_size = usize::from(min_code_size) + 1;
    let mut code_mask = (1usize << code_size) - 1;
    let string_length = &mut scratch.string_length;
    let string_start = &mut scratch.string_start;
    let inline_copy_limit = if output.len() <= 20_000 { 32 } else { 8 };

    let mut output_index = 0usize;
    let mut q = 0usize;
    let mut bits = 0usize;
    let mut bit_count = 0usize;
    let mut have_previous = false;
    let mut previous_start = 0usize;
    let mut previous_length = 0usize;
    let mut previous_first = 0u8;

    loop {
        #[cfg(target_pointer_width = "32")]
        if bit_count < code_size && image_data.len().saturating_sub(q) >= 2 {
            let word =
                unsafe { std::ptr::read_unaligned(image_data.as_ptr().add(q).cast::<u16>()) };
            bits |= usize::from(u16::from_le(word)) << bit_count;
            bit_count += 16;
            q += 2;
        }
        #[cfg(target_pointer_width = "64")]
        if bit_count < code_size && image_data.len().saturating_sub(q) >= 4 {
            let word =
                unsafe { std::ptr::read_unaligned(image_data.as_ptr().add(q).cast::<u32>()) };
            bits |= usize::try_from(u32::from_le(word)).unwrap() << bit_count;
            bit_count += 32;
            q += 4;
        }
        while bit_count < code_size && q < image_data.len() {
            bits |= usize::from(image_data[q]) << bit_count;
            bit_count += 8;
            q += 1;
        }
        if bit_count < code_size {
            break;
        }

        let code = bits & code_mask;
        bits >>= code_size;
        bit_count -= code_size;
        if code == clear {
            next_code = eoi + 1;
            code_size = usize::from(min_code_size) + 1;
            code_mask = (1usize << code_size) - 1;
            have_previous = false;
            continue;
        }
        if code == eoi {
            break;
        }

        let current_start = output_index;
        let (decoded_length, out_first) = if code < clear {
            if output_index >= output.len() {
                return Err("LZW decoded output exceeds frame dimensions".to_string());
            }
            let literal = code as u8;
            output[output_index] = literal;
            output_index += 1;
            (1usize, literal)
        } else if code < next_code {
            let decoded_length =
                usize::from(unsafe { string_length.get_unchecked(code).assume_init() });
            let source_start =
                usize::try_from(unsafe { string_start.get_unchecked(code).assume_init() }).unwrap();
            let source_end = source_start + decoded_length;
            let decoded_end = output_index + decoded_length;
            if source_end > current_start || decoded_end > output.len() {
                return Err("LZW dictionary string exceeds decoded output".to_string());
            }
            let out_first = unsafe { *output.get_unchecked(source_start) };
            unsafe {
                copy_lzw_dictionary_string(
                    output.as_ptr().add(source_start),
                    output.as_mut_ptr().add(output_index),
                    decoded_length,
                    inline_copy_limit,
                );
            }
            output_index = decoded_end;
            (decoded_length, out_first)
        } else if code == next_code && have_previous {
            let decoded_length = previous_length + 1;
            let decoded_end = output_index + decoded_length;
            let previous_end = previous_start + previous_length;
            if previous_end > current_start || decoded_end > output.len() {
                return Err("LZW dictionary string exceeds decoded output".to_string());
            }
            unsafe {
                copy_lzw_dictionary_string(
                    output.as_ptr().add(previous_start),
                    output.as_mut_ptr().add(output_index),
                    previous_length,
                    inline_copy_limit,
                );
                output
                    .as_mut_ptr()
                    .add(output_index + previous_length)
                    .write(previous_first);
            }
            output_index = decoded_end;
            (decoded_length, previous_first)
        } else {
            return Err("Invalid LZW dictionary code".to_string());
        };

        if have_previous && next_code < 4096 {
            let dictionary_length = previous_length + 1;
            unsafe {
                string_start
                    .get_unchecked_mut(next_code)
                    .write(previous_start as u32);
                string_length
                    .get_unchecked_mut(next_code)
                    .write(dictionary_length as u16);
            }
            next_code += 1;
            if next_code >= code_mask + 1 && code_size < 12 {
                code_size += 1;
                code_mask = (1usize << code_size) - 1;
            }
        }
        previous_start = current_start;
        previous_length = decoded_length;
        previous_first = out_first;
        have_previous = true;
    }
    Ok(())
}

struct LzwStackScratch {
    prefix: [std::mem::MaybeUninit<u16>; 4096],
    suffix: [std::mem::MaybeUninit<u8>; 4096],
    stack: [std::mem::MaybeUninit<u8>; 4096],
    string_length: [std::mem::MaybeUninit<u16>; 4096],
    string_start: [std::mem::MaybeUninit<u32>; 4096],
}

impl Default for LzwStackScratch {
    fn default() -> Self {
        Self {
            prefix: [std::mem::MaybeUninit::uninit(); 4096],
            suffix: [std::mem::MaybeUninit::uninit(); 4096],
            stack: [std::mem::MaybeUninit::uninit(); 4096],
            string_length: [std::mem::MaybeUninit::uninit(); 4096],
            string_start: [std::mem::MaybeUninit::uninit(); 4096],
        }
    }
}

fn lzw_decode_to_indices_stack_with_scratch(
    min_code_size: u8,
    image_data: &[u8],
    output: &mut [u8],
    scratch: &mut LzwStackScratch,
) -> Result<(), String> {
    if min_code_size > 11 {
        return Err(format!("Invalid LZW minimum code size {min_code_size}"));
    }
    let clear = 1usize << min_code_size;
    let eoi = clear + 1;
    let mut next_code = eoi + 1;
    let mut code_size = usize::from(min_code_size) + 1;
    let mut code_mask = (1usize << code_size) - 1;

    let prefix = &mut scratch.prefix;
    let suffix = &mut scratch.suffix;
    let stack = &mut scratch.stack;

    let mut output_index = 0usize;
    let mut q = 0usize;
    let mut bits = 0usize;
    let mut bit_count = 0usize;
    let mut prev_code: Option<usize> = None;
    let mut prev_first = 0u8;

    loop {
        #[cfg(target_pointer_width = "32")]
        if bit_count < code_size && image_data.len().saturating_sub(q) >= 2 {
            let word =
                unsafe { std::ptr::read_unaligned(image_data.as_ptr().add(q).cast::<u16>()) };
            bits |= usize::from(u16::from_le(word)) << bit_count;
            bit_count += 16;
            q += 2;
        }
        #[cfg(target_pointer_width = "64")]
        if bit_count < code_size && bit_count <= 32 && image_data.len().saturating_sub(q) >= 4 {
            let word =
                unsafe { std::ptr::read_unaligned(image_data.as_ptr().add(q).cast::<u32>()) };
            bits |= usize::try_from(u32::from_le(word)).unwrap() << bit_count;
            bit_count += 32;
            q += 4;
        }
        while bit_count < code_size && q < image_data.len() {
            bits |= usize::from(image_data[q]) << bit_count;
            bit_count += 8;
            q += 1;
        }

        if bit_count < code_size {
            break;
        }

        let code = bits & code_mask;
        bits >>= code_size;
        bit_count -= code_size;

        if code == clear {
            next_code = eoi + 1;
            code_size = usize::from(min_code_size) + 1;
            code_mask = (1usize << code_size) - 1;
            prev_code = None;
            continue;
        }

        if code == eoi {
            break;
        }

        let out_first;
        let mut cur = code;

        if cur < clear {
            out_first = cur as u8;
            if output_index >= output.len() {
                return Err("LZW decoded output exceeds frame dimensions".to_string());
            }
            unsafe {
                output.as_mut_ptr().add(output_index).write(out_first);
            }
            output_index += 1;
        } else {
            let mut sp = 0usize;
            if cur > next_code {
                return Err("Invalid LZW dictionary code".to_string());
            }
            if cur == next_code {
                let Some(prev) = prev_code else {
                    return Err("Invalid first LZW dictionary code".to_string());
                };
                stack[sp].write(prev_first);
                sp += 1;
                cur = prev;
            }

            while cur >= clear {
                if sp >= stack.len() {
                    return Err("LZW decode stack overflow".to_string());
                }
                stack[sp].write(unsafe { suffix.get_unchecked(cur).assume_init() });
                sp += 1;
                cur = usize::from(unsafe { prefix.get_unchecked(cur).assume_init() });
            }

            out_first = cur as u8;
            if output.len().saturating_sub(output_index) < sp + 1 {
                return Err("LZW decoded output exceeds frame dimensions".to_string());
            }
            unsafe {
                output.as_mut_ptr().add(output_index).write(out_first);
            }
            output_index += 1;
            while sp > 0 {
                sp -= 1;
                unsafe {
                    output
                        .as_mut_ptr()
                        .add(output_index)
                        .write(stack.get_unchecked(sp).assume_init());
                }
                output_index += 1;
            }
        }

        if let Some(prev) = prev_code {
            if next_code < 4096 {
                prefix[next_code].write(prev as u16);
                suffix[next_code].write(out_first);
                next_code += 1;

                if next_code >= code_mask + 1 && code_size < 12 {
                    code_size += 1;
                    code_mask = (1usize << code_size) - 1;
                }
            }
        }

        prev_code = Some(code);
        prev_first = out_first;
    }

    Ok(())
}

fn skip_sub_blocks(data: &[u8], mut offset: usize, context: &str) -> Result<usize, String> {
    loop {
        if offset >= data.len() {
            return Err(format!("Truncated {context}"));
        }
        let length = usize::from(data[offset]);
        offset += 1;
        if length == 0 {
            return Ok(offset);
        }
        offset = checked_add(offset, length, data.len(), context)?;
    }
}

fn read_u16(data: &[u8], offset: usize, context: &str) -> Result<u16, String> {
    let end = checked_add(offset, 2, data.len(), context)?;
    Ok(u16::from_le_bytes([data[offset], data[end - 1]]))
}

fn checked_add(
    offset: usize,
    length: usize,
    data_len: usize,
    context: &str,
) -> Result<usize, String> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| format!("Offset overflow while reading {context}"))?;
    if end > data_len {
        return Err(format!("Truncated {context}"));
    }
    Ok(end)
}

impl GifMetadata {
    fn to_json(&self) -> String {
        let mut json = String::new();
        json.push('{');
        push_json_field_str(&mut json, "version", self.version);
        json.push(',');
        push_json_field_u16(&mut json, "width", self.width);
        json.push(',');
        push_json_field_u16(&mut json, "height", self.height);
        json.push(',');
        push_json_field_usize_option(
            &mut json,
            "global_palette_offset",
            self.global_palette_offset,
        );
        json.push(',');
        push_json_field_usize(&mut json, "global_palette_size", self.global_palette_size);
        json.push_str(",\"frame_count\":");
        json.push_str(&self.frames.len().to_string());
        json.push_str(",\"frames\":[");
        for (index, frame) in self.frames.iter().enumerate() {
            if index > 0 {
                json.push(',');
            }
            frame.push_json(&mut json);
        }
        json.push_str("]}");
        json
    }
}

impl FrameMetadata {
    fn push_json(&self, json: &mut String) {
        json.push('{');
        push_json_field_u16(json, "x", self.x);
        json.push(',');
        push_json_field_u16(json, "y", self.y);
        json.push(',');
        push_json_field_u16(json, "width", self.width);
        json.push(',');
        push_json_field_u16(json, "height", self.height);
        json.push(',');
        push_json_field_bool(json, "has_local_palette", self.has_local_palette);
        json.push(',');
        push_json_field_usize(json, "palette_offset", self.palette_offset);
        json.push(',');
        push_json_field_usize(json, "palette_size", self.palette_size);
        json.push(',');
        push_json_field_usize(json, "data_offset", self.data_offset);
        json.push(',');
        push_json_field_usize(json, "data_length", self.data_length);
        json.push(',');
        push_json_field_u8_option(json, "transparent_index", self.transparent_index);
        json.push(',');
        push_json_field_bool(json, "interlaced", self.interlaced);
        json.push(',');
        push_json_field_u16(json, "delay", self.delay);
        json.push(',');
        push_json_field_u8(json, "disposal", self.disposal);
        json.push(',');
        push_json_field_u8(json, "min_code_size", self.min_code_size);
        json.push('}');
    }
}

fn push_json_field_str(json: &mut String, key: &str, value: &str) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":\"");
    json.push_str(value);
    json.push('"');
}

fn push_json_field_bool(json: &mut String, key: &str, value: bool) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":");
    json.push_str(if value { "true" } else { "false" });
}

fn push_json_field_u8(json: &mut String, key: &str, value: u8) {
    push_json_field_number(json, key, usize::from(value));
}

fn push_json_field_u16(json: &mut String, key: &str, value: u16) {
    push_json_field_number(json, key, usize::from(value));
}

fn push_json_field_usize(json: &mut String, key: &str, value: usize) {
    push_json_field_number(json, key, value);
}

fn push_json_field_u8_option(json: &mut String, key: &str, value: Option<u8>) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":");
    match value {
        Some(value) => json.push_str(&value.to_string()),
        None => json.push_str("null"),
    }
}

fn push_json_field_usize_option(json: &mut String, key: &str, value: Option<usize>) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":");
    match value {
        Some(value) => json.push_str(&value.to_string()),
        None => json.push_str("null"),
    }
}

fn push_json_field_number(json: &mut String, key: &str, value: usize) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":");
    json.push_str(&value.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_PIXEL_TRANSPARENT_GIF: &[u8] = &[
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, // GIF89a
        0x01, 0x00, 0x01, 0x00, // 1x1 logical screen
        0x80, 0x00, 0x00, // global table: 2 colors
        0x00, 0x00, 0x00, // black
        0xff, 0xff, 0xff, // white
        0x21, 0xf9, 0x04, 0x09, 0x05, 0x00, 0x01, 0x00, // GCE
        0x2c, 0x00, 0x00, 0x00, 0x00, // image separator + x/y
        0x01, 0x00, 0x01, 0x00, 0x00, // 1x1, no local table
        0x02, 0x02, 0x4c, 0x01, 0x00, // image data
        0x3b, // trailer
    ];

    #[test]
    fn parses_screen_and_frame_metadata() {
        let metadata = parse_metadata(ONE_PIXEL_TRANSPARENT_GIF).unwrap();

        assert_eq!(metadata.version, "GIF89a");
        assert_eq!(metadata.width, 1);
        assert_eq!(metadata.height, 1);
        assert_eq!(metadata.global_palette_offset, Some(13));
        assert_eq!(metadata.global_palette_size, 2);
        assert_eq!(metadata.frames.len(), 1);

        let frame = &metadata.frames[0];
        assert_eq!(frame.x, 0);
        assert_eq!(frame.y, 0);
        assert_eq!(frame.width, 1);
        assert_eq!(frame.height, 1);
        assert!(!frame.has_local_palette);
        assert_eq!(frame.palette_offset, 13);
        assert_eq!(frame.palette_size, 2);
        assert_eq!(frame.data_offset, 37);
        assert_eq!(frame.data_length, 5);
        assert_eq!(frame.transparent_index, Some(1));
        assert_eq!(frame.delay, 5);
        assert_eq!(frame.disposal, 2);
        assert_eq!(frame.min_code_size, 2);
    }

    #[test]
    fn rejects_invalid_signatures() {
        let error = parse_metadata(b"not a gif.....").unwrap_err();
        assert!(error.contains("Invalid GIF signature"));
    }

    #[test]
    fn validates_small_gifs_without_allocating_metadata() {
        validate_gif_structure_no_alloc(ONE_PIXEL_TRANSPARENT_GIF).unwrap();
        let error = validate_gif_structure_no_alloc(b"not a gif.....").unwrap_err();
        assert!(error.contains("Invalid GIF signature"));
    }

    #[test]
    fn lossless_remux_can_return_a_valid_small_gif_unchanged() {
        let remuxed = remux_gif_pixel_perfect(ONE_PIXEL_TRANSPARENT_GIF).unwrap();
        assert_eq!(remuxed, ONE_PIXEL_TRANSPARENT_GIF);
    }

    #[test]
    fn serializes_metadata_json() {
        let json = parse_metadata(ONE_PIXEL_TRANSPARENT_GIF).unwrap().to_json();

        assert!(json.contains("\"width\":1"));
        assert!(json.contains("\"frame_count\":1"));
        assert!(json.contains("\"transparent_index\":1"));
    }

    #[test]
    fn decodes_frame_indices() {
        let metadata = parse_metadata(ONE_PIXEL_TRANSPARENT_GIF).unwrap();
        let indices =
            decode_frame_indices_inner(ONE_PIXEL_TRANSPARENT_GIF, &metadata.frames[0]).unwrap();

        assert_eq!(indices, vec![1]);
    }

    #[test]
    fn decodes_transparent_frame_to_zero_rgba() {
        let metadata = parse_metadata(ONE_PIXEL_TRANSPARENT_GIF).unwrap();
        let pixels =
            decode_frame_pixels_inner(ONE_PIXEL_TRANSPARENT_GIF, &metadata, 0, PixelFormat::Rgba)
                .unwrap();

        assert_eq!(pixels, vec![0, 0, 0, 0]);
    }

    #[test]
    fn decodes_opaque_frame_to_rgba_and_bgra() {
        let mut gif = ONE_PIXEL_TRANSPARENT_GIF.to_vec();
        gif[16] = 0xff;
        gif[17] = 0x00;
        gif[18] = 0x00;
        gif[22] = 0x08;
        let metadata = parse_metadata(&gif).unwrap();

        let rgba = decode_frame_pixels_inner(&gif, &metadata, 0, PixelFormat::Rgba).unwrap();
        let bgra = decode_frame_pixels_inner(&gif, &metadata, 0, PixelFormat::Bgra).unwrap();

        assert_eq!(rgba, vec![255, 0, 0, 255]);
        assert_eq!(bgra, vec![0, 0, 255, 255]);
    }

    #[test]
    fn prepares_composited_rgba_and_bgra_frames() {
        let mut gif = ONE_PIXEL_TRANSPARENT_GIF.to_vec();
        gif[16] = 0xff;
        gif[17] = 0x00;
        gif[18] = 0x00;
        gif[22] = 0x08;
        let metadata = parse_metadata(&gif).unwrap();

        let rgba =
            prepare_composited_frames_inner(&gif, &metadata, &[1], PixelFormat::Rgba).unwrap();
        let bgra =
            prepare_composited_frames_inner(&gif, &metadata, &[1], PixelFormat::Bgra).unwrap();

        assert_eq!(rgba, vec![0xff0000ff]);
        assert_eq!(bgra, vec![0xffff0000]);
    }

    #[test]
    fn prepares_composited_delta_stream() {
        let mut gif = ONE_PIXEL_TRANSPARENT_GIF.to_vec();
        gif[16] = 0xff;
        gif[17] = 0x00;
        gif[18] = 0x00;
        gif[22] = 0x08;
        let metadata = parse_metadata(&gif).unwrap();

        let stream =
            prepare_composited_delta_frames_inner(&gif, &metadata, &[1], PixelFormat::Rgba)
                .unwrap();

        assert_eq!(stream[0], COMPOSITED_DELTA_MAGIC);
        assert_eq!(stream[1], COMPOSITED_DELTA_VERSION);
        assert_eq!(stream[2], 1);
        assert_eq!(stream[3], 1);
        assert_eq!(stream[4], 0);
        assert_eq!(stream[5], 13);
        assert_eq!(stream[6], 1);
        assert_eq!(stream[11], 14);
        assert_eq!(stream[12], 0);
        assert_eq!(stream[13], 0xff0000ff);
    }

    #[test]
    fn encodes_indexed_lzw_code_stream() {
        let encoded = encode_indexed_lzw_inner(&[1], 1, 2).unwrap();
        assert_eq!(encoded, vec![1, 1, 0x36, 0]);

        assert!(encode_indexed_lzw_inner(&[2], 1, 2)
            .unwrap_err()
            .contains("Pixel index out of range"));
    }

    #[test]
    fn encodes_indexed_gif_frames() {
        let palette = [0x000000, 0xff0000, 0x00ff00];
        let encoded = encode_indexed_gif_inner(
            &[1, 1, 1, 1, 2, 0, 0, 2],
            2,
            2,
            2,
            &palette,
            DelaySource::Constant(5),
            0,
            None,
        )
        .unwrap();
        let metadata = parse_metadata(&encoded).unwrap();

        assert_eq!(metadata.width, 2);
        assert_eq!(metadata.height, 2);
        assert_eq!(metadata.global_palette_size, 4);
        assert_eq!(metadata.frames.len(), 2);
        assert_eq!(metadata.frames[0].delay, 5);
        assert_eq!(metadata.frames[1].delay, 5);
        assert_eq!(
            decode_frame_indices_inner(&encoded, &metadata.frames[0]).unwrap(),
            vec![1, 1, 1, 1]
        );
        assert_eq!(
            decode_frame_indices_inner(&encoded, &metadata.frames[1]).unwrap(),
            vec![2, 0, 0, 2]
        );
    }

    #[test]
    fn encodes_full_256_color_lzw_stream() {
        let palette: Vec<u32> = (0..256u32)
            .map(|value| (value << 16) | (value << 8) | value)
            .collect();
        let indices: Vec<u8> = (0..4096u32).map(|value| (value & 0xff) as u8).collect();
        let encoded = encode_indexed_gif_inner(
            &indices,
            64,
            64,
            1,
            &palette,
            DelaySource::Constant(0),
            -1,
            None,
        )
        .unwrap();
        let metadata = parse_metadata(&encoded).unwrap();
        assert_eq!(
            decode_frame_indices_inner(&encoded, &metadata.frames[0]).unwrap(),
            indices
        );
    }

    #[test]
    fn nine_bit_literal_packer_matches_reference_bitstream() {
        for length in [1usize, 2, 6, 7, 8, 14, 253, 254, 255, 508, 1024, 4096] {
            let indices: Vec<u8> = (0..length)
                .map(|index| ((index * 73 + index / 11) & 0xff) as u8)
                .collect();
            let mut optimized = Vec::new();
            encode_nine_bit_literal_codes(&mut optimized, &indices).unwrap();

            let mut reference = Vec::new();
            let mut bits = 0u64;
            let mut bit_count = 0usize;
            for literals in indices.chunks(254) {
                emit_raw_lzw_code(&mut reference, &mut bits, &mut bit_count, 9, 256);
                for &pixel in literals {
                    emit_raw_lzw_code(
                        &mut reference,
                        &mut bits,
                        &mut bit_count,
                        9,
                        usize::from(pixel),
                    );
                }
            }
            emit_raw_lzw_code(&mut reference, &mut bits, &mut bit_count, 9, 257);
            while bit_count > 0 {
                reference.push(bits as u8);
                bits >>= 8;
                bit_count = bit_count.saturating_sub(8);
            }

            assert_eq!(optimized, reference, "length {length}");
        }
    }

    #[test]
    fn nine_bit_literal_direct_subblocks_match_buffered_writer() {
        for length in [1usize, 254, 255, 508, 4096] {
            let indices: Vec<u8> = (0..length)
                .map(|index| ((index * 73 + index / 11) & 0xff) as u8)
                .collect();
            let mut direct = Vec::new();
            encode_indexed_literal_lzw_direct_to(&mut direct, &indices, 8, 256).unwrap();

            let mut buffered = Vec::new();
            let mut compressed = Vec::new();
            encode_indexed_literal_lzw_to(&mut buffered, &indices, 8, 256, &mut compressed)
                .unwrap();

            assert_eq!(direct, buffered, "length {length}");
        }
    }

    #[test]
    fn encodes_indexed_delta_gif_frames() {
        let palette = [0x000000, 0xff0000, 0x00ff00];
        let encoded = encode_indexed_delta_gif_inner(
            &[
                1, 1, 1, 1, //
                1, 2, 1, 1, //
                1, 2, 1, 1,
            ],
            2,
            2,
            3,
            &palette,
            DelaySource::Constant(4),
            0,
        )
        .unwrap();
        let metadata = parse_metadata(&encoded).unwrap();

        assert_eq!(metadata.frames.len(), 3);
        assert_eq!(metadata.frames[0].width, 2);
        assert_eq!(metadata.frames[0].height, 2);
        assert_eq!(metadata.frames[1].x, 1);
        assert_eq!(metadata.frames[1].y, 0);
        assert_eq!(metadata.frames[1].width, 1);
        assert_eq!(metadata.frames[1].height, 1);
        assert_eq!(metadata.frames[2].x, 0);
        assert_eq!(metadata.frames[2].y, 0);
        assert_eq!(metadata.frames[2].width, 1);
        assert_eq!(metadata.frames[2].height, 1);
        assert_eq!(
            decode_frame_indices_inner(&encoded, &metadata.frames[1]).unwrap(),
            vec![2]
        );
    }

    #[test]
    fn encodes_rgba_gif_frames_with_generated_exact_palette() {
        let encoded = encode_rgba_gif_inner(
            &[
                255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 255, 0, 255, 0,
                0, 255, 255, 255, 0, 0, 255, 255, 0, 0, 255,
            ],
            2,
            2,
            2,
            &[],
            DelaySource::Constant(6),
            0,
            false,
            TRANSPARENT_ALPHA_THRESHOLD,
            false,
        )
        .unwrap();
        let metadata = parse_metadata(&encoded).unwrap();

        assert_eq!(metadata.global_palette_size, 4);
        assert_eq!(metadata.frames.len(), 2);
        assert_eq!(metadata.frames[0].delay, 6);
        assert_eq!(
            decode_frame_indices_inner(&encoded, &metadata.frames[0]).unwrap(),
            vec![0, 0, 1, 2]
        );
        assert_eq!(
            decode_frame_indices_inner(&encoded, &metadata.frames[1]).unwrap(),
            vec![1, 2, 0, 0]
        );
    }

    #[test]
    fn transparent_rgba_frames_restore_the_canvas() {
        let encoded = encode_rgba_gif_advanced_inner(
            &[
                255, 0, 0, 255, 0, 0, 0, 0, // red, transparent
                0, 0, 0, 0, 0, 0, 255, 255, // transparent, blue
            ],
            2,
            1,
            2,
            &[],
            DelaySource::Constant(6),
            0,
            false,
            TRANSPARENT_ALPHA_THRESHOLD,
            true,
            RgbaQuantization::Fast,
            RgbaPaletteMode::Global,
        )
        .unwrap();
        let metadata = parse_metadata(&encoded).unwrap();

        assert!(metadata.frames.iter().all(|frame| frame.disposal == 2));
    }

    #[test]
    fn encodes_rgba_delta_gif_frames_with_provided_palette() {
        let palette = [0x000000, 0xff0000, 0x00ff00];
        let encoded = encode_rgba_gif_inner(
            &[
                255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 0,
                255, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
            ],
            2,
            2,
            2,
            &palette,
            DelaySource::Constant(4),
            0,
            true,
            TRANSPARENT_ALPHA_THRESHOLD,
            false,
        )
        .unwrap();
        let metadata = parse_metadata(&encoded).unwrap();

        assert_eq!(metadata.frames.len(), 2);
        assert_eq!(metadata.frames[1].x, 1);
        assert_eq!(metadata.frames[1].y, 0);
        assert_eq!(metadata.frames[1].width, 1);
        assert_eq!(metadata.frames[1].height, 1);
        assert_eq!(
            decode_frame_indices_inner(&encoded, &metadata.frames[1]).unwrap(),
            vec![2]
        );
    }

    #[test]
    fn quantizes_rgba_gif_frames_when_exact_palette_overflows() {
        let mut rgba = Vec::new();
        for value in 0..257u16 {
            rgba.push((value & 0xff) as u8);
            rgba.push(((value * 3) & 0xff) as u8);
            rgba.push(((value * 7) & 0xff) as u8);
            rgba.push(255);
        }

        let encoded = encode_rgba_gif_inner(
            &rgba,
            257,
            1,
            1,
            &[],
            DelaySource::Constant(0),
            -1,
            false,
            TRANSPARENT_ALPHA_THRESHOLD,
            false,
        )
        .unwrap();
        let metadata = parse_metadata(&encoded).unwrap();

        assert_eq!(metadata.global_palette_size, 256);
        assert_eq!(metadata.frames.len(), 1);
        assert_eq!(
            decode_frame_indices_inner(&encoded, &metadata.frames[0])
                .unwrap()
                .len(),
            257
        );
    }

    #[test]
    fn advanced_fast_compression_quantizes_arbitrary_rgba() {
        let mut rgba = Vec::new();
        for value in 0..1024u16 {
            rgba.extend_from_slice(&[
                (value & 0xff) as u8,
                ((value * 5) & 0xff) as u8,
                ((value * 11) & 0xff) as u8,
                255,
            ]);
        }

        let encoded = encode_rgba_gif_advanced_inner(
            &rgba,
            32,
            32,
            1,
            &[],
            DelaySource::Constant(3),
            0,
            false,
            TRANSPARENT_ALPHA_THRESHOLD,
            true,
            RgbaQuantization::Fast,
            RgbaPaletteMode::Global,
        )
        .unwrap();
        let metadata = parse_metadata(&encoded).unwrap();

        assert_eq!(metadata.global_palette_size, 256);
        assert_eq!(metadata.frames.len(), 1);
        assert_eq!(metadata.frames[0].delay, 3);
        assert_eq!(
            decode_frame_indices_inner(&encoded, &metadata.frames[0])
                .unwrap()
                .len(),
            1024
        );
    }

    #[test]
    fn median_cut_split_carries_exact_child_statistics() {
        let colors = vec![
            QuantizedColor {
                histogram_index: 0,
                count: 1,
                red: 0,
                green: 10,
                blue: 20,
            },
            QuantizedColor {
                histogram_index: 1,
                count: 1,
                red: 10,
                green: 20,
                blue: 30,
            },
            QuantizedColor {
                histogram_index: 2,
                count: 100,
                red: 20,
                green: 30,
                blue: 40,
            },
        ];
        let color_box = QuantizedColorBox::new(colors);
        let Some((left, right)) = color_box.split().ok() else {
            panic!("expected a splittable color box");
        };
        let expected_left = QuantizedColorBox::new(left.colors.clone());
        let expected_right = QuantizedColorBox::new(right.colors.clone());

        assert_eq!(left.weight, expected_left.weight);
        assert_eq!(left.score, expected_left.score);
        assert_eq!(left.red_range, expected_left.red_range);
        assert_eq!(left.green_range, expected_left.green_range);
        assert_eq!(left.blue_range, expected_left.blue_range);
        assert_eq!(right.weight, expected_right.weight);
        assert_eq!(right.score, expected_right.score);
        assert_eq!(right.red_range, expected_right.red_range);
        assert_eq!(right.green_range, expected_right.green_range);
        assert_eq!(right.blue_range, expected_right.blue_range);
    }

    #[test]
    fn weighted_median_partition_keeps_axis_order() {
        let colors = vec![
            QuantizedColor {
                histogram_index: 0,
                count: 1,
                red: 240,
                green: 8,
                blue: 200,
            },
            QuantizedColor {
                histogram_index: 1,
                count: 7,
                red: 12,
                green: 220,
                blue: 30,
            },
            QuantizedColor {
                histogram_index: 2,
                count: 3,
                red: 140,
                green: 70,
                blue: 180,
            },
            QuantizedColor {
                histogram_index: 3,
                count: 11,
                red: 60,
                green: 180,
                blue: 90,
            },
            QuantizedColor {
                histogram_index: 4,
                count: 5,
                red: 190,
                green: 45,
                blue: 15,
            },
            QuantizedColor {
                histogram_index: 5,
                count: 2,
                red: 35,
                green: 130,
                blue: 240,
            },
        ];
        let midpoint = (colors.iter().map(|color| color.count).sum::<u64>() + 1) / 2;
        for axis in 0..3 {
            let mut partitioned = colors.clone();
            let split = weighted_axis_split_index(&mut partitioned, axis, midpoint);
            assert!(split > 0 && split < partitioned.len());
            let left_max = partitioned[..split]
                .iter()
                .map(|color| quantized_color_axis(color, axis))
                .max()
                .unwrap();
            let right_min = partitioned[split..]
                .iter()
                .map(|color| quantized_color_axis(color, axis))
                .min()
                .unwrap();
            assert!(left_max <= right_min);
        }
    }

    #[test]
    fn arena_median_cut_preserves_vector_partition_membership() {
        let colors = (0..5_000usize)
            .map(|histogram_index| QuantizedColor {
                histogram_index: histogram_index as u16,
                count: ((histogram_index * 17) % 11 + 1) as u64,
                red: ((histogram_index * 37) & 255) as u8,
                green: ((histogram_index * 61 + 13) & 255) as u8,
                blue: ((histogram_index * 97 + 29) & 255) as u8,
            })
            .collect::<Vec<_>>();

        let mut arena_colors = colors.clone();
        let mut arena_boxes = vec![QuantizedColorArenaBox::new(
            0,
            arena_colors.len(),
            &arena_colors,
        )];
        while arena_boxes.len() < 256 {
            let Some((split_index, _)) = arena_boxes
                .iter()
                .enumerate()
                .filter(|(_, color_box)| color_box.len() > 1)
                .max_by_key(|(_, color_box)| color_box.score)
            else {
                break;
            };
            let mut color_box = arena_boxes.swap_remove(split_index);
            let Some(right) = color_box.split(&mut arena_colors) else {
                arena_boxes.push(color_box);
                break;
            };
            arena_boxes.push(color_box);
            arena_boxes.push(right);
        }
        let mut arena_groups = arena_boxes
            .iter()
            .map(|color_box| {
                let mut group = arena_colors[color_box.start..color_box.end]
                    .iter()
                    .map(|color| color.histogram_index)
                    .collect::<Vec<_>>();
                group.sort_unstable();
                group
            })
            .collect::<Vec<_>>();
        arena_groups.sort_unstable();

        let mut vector_boxes = vec![QuantizedColorBox::new(colors)];
        while vector_boxes.len() < 256 {
            let Some((split_index, _)) = vector_boxes
                .iter()
                .enumerate()
                .filter(|(_, color_box)| color_box.colors.len() > 1)
                .max_by_key(|(_, color_box)| color_box.score)
            else {
                break;
            };
            let color_box = vector_boxes.swap_remove(split_index);
            match color_box.split() {
                Ok((left, right)) => {
                    vector_boxes.push(left);
                    vector_boxes.push(right);
                }
                Err(unsplit) => {
                    vector_boxes.push(unsplit);
                    break;
                }
            }
        }
        let mut vector_groups = vector_boxes
            .iter()
            .map(|color_box| {
                let mut group = color_box
                    .colors
                    .iter()
                    .map(|color| color.histogram_index)
                    .collect::<Vec<_>>();
                group.sort_unstable();
                group
            })
            .collect::<Vec<_>>();
        vector_groups.sort_unstable();

        assert_eq!(arena_groups, vector_groups);
    }

    #[test]
    fn quality_palette_lookup_matches_linear_nearest_color() {
        let palette = [
            0x000000, 0xff0000, 0x00ff00, 0x0000ff, 0xffff00, 0xff00ff, 0x00ffff, 0xffffff,
            0x402010, 0x804020, 0x204080, 0xc08040,
        ];
        let tree = PaletteKdTree::new(&palette);
        for red in (0..=255).step_by(17) {
            for green in (0..=255).step_by(19) {
                for blue in (0..=255).step_by(23) {
                    assert_eq!(
                        tree.nearest(red, green, blue),
                        nearest_palette_index(red, green, blue, &palette),
                    );
                }
            }
        }
    }

    #[test]
    fn hinted_palette_lookup_matches_unhinted_exact_search() {
        let palette = (0..256u32)
            .map(|index| {
                let red = (index * 73 + 19) & 255;
                let green = (index * 151 + 7) & 255;
                let blue = (index * 199 + 43) & 255;
                (red << 16) | (green << 8) | blue
            })
            .collect::<Vec<_>>();
        let tree = PaletteKdTree::new(&palette);
        for red in (0..=255).step_by(11) {
            for green in (0..=255).step_by(13) {
                for blue in (0..=255).step_by(17) {
                    let expected = tree.nearest(red, green, blue);
                    for hint in (0..palette.len()).step_by(19) {
                        assert_eq!(
                            tree.nearest_with_hint(
                                red,
                                green,
                                blue,
                                Some((hint as u8, palette[hint])),
                            ),
                            expected,
                            "color {red},{green},{blue} hint {hint}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn quality_quantization_improves_rgb_error_over_fast_quantization() {
        let mut rgba = Vec::new();
        for y in 0..64u16 {
            for x in 0..64u16 {
                rgba.extend_from_slice(&[
                    (x * 255 / 63) as u8,
                    (y * 255 / 63) as u8,
                    ((x * 3 + y * 5) & 0xff) as u8,
                    255,
                ]);
            }
        }

        let (fast_palette, fast_indices, _) = index_rgba_frames_with_quantization(
            &rgba,
            &[],
            TRANSPARENT_ALPHA_THRESHOLD,
            RgbaQuantization::Fast,
        )
        .unwrap();
        let (quality_palette, quality_indices, _) = index_rgba_frames_with_quantization(
            &rgba,
            &[],
            TRANSPARENT_ALPHA_THRESHOLD,
            RgbaQuantization::Quality,
        )
        .unwrap();
        let error = |palette: &[u32], indices: &[u8]| -> u64 {
            rgba.chunks_exact(4)
                .zip(indices)
                .map(|(pixel, index)| {
                    let color = palette[usize::from(*index)];
                    let red = ((color >> 16) & 0xff) as i32;
                    let green = ((color >> 8) & 0xff) as i32;
                    let blue = (color & 0xff) as i32;
                    let dr = i32::from(pixel[0]) - red;
                    let dg = i32::from(pixel[1]) - green;
                    let db = i32::from(pixel[2]) - blue;
                    (dr * dr + dg * dg + db * db) as u64
                })
                .sum()
        };

        assert!(error(&quality_palette, &quality_indices) < error(&fast_palette, &fast_indices));
    }

    #[test]
    fn quality_precision_guard_keeps_smooth_ramps_on_the_fine_histogram() {
        let mut gradient = Vec::new();
        let mut noisy = Vec::new();
        for y in 0..64u16 {
            for x in 0..64u16 {
                gradient.extend_from_slice(&[
                    (x * 255 / 63) as u8,
                    (y * 255 / 63) as u8,
                    ((x + y) * 127 / 126) as u8,
                    255,
                ]);
                noisy.extend_from_slice(&[
                    ((x * 73 + y * 151) & 255) as u8,
                    ((x * 193 + y * 47) & 255) as u8,
                    ((x * 11 + y * 223) & 255) as u8,
                    255,
                ]);
            }
        }

        assert!(quality_prefers_high_precision_histogram(
            &gradient,
            TRANSPARENT_ALPHA_THRESHOLD,
        ));
        assert!(!quality_prefers_high_precision_histogram(
            &noisy,
            TRANSPARENT_ALPHA_THRESHOLD,
        ));
    }

    #[test]
    fn fused_quality_literal_encoding_matches_indexed_encoding() {
        let width = 64u16;
        let height = 64u16;
        let mut rgba = Vec::with_capacity(usize::from(width) * usize::from(height) * 4);
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[
                    ((x * 73 + y * 151) & 255) as u8,
                    ((x * 193 + y * 47) & 255) as u8,
                    ((x * 11 + y * 223) & 255) as u8,
                    if (x + y) % 5 == 0 { 0 } else { 255 },
                ]);
            }
        }
        let delays = [10u16];
        let (palette, indexed, transparent_index) =
            index_rgba_frames_quality(&rgba, TRANSPARENT_ALPHA_THRESHOLD);
        let indexed_output = encode_indexed_literal_gif_inner_with_output(
            Vec::new(),
            &indexed,
            width,
            height,
            1,
            &palette,
            DelaySource::PerFrame(&delays),
            0,
            transparent_index,
        )
        .unwrap();
        recycle_quantized_indexed(indexed);

        let plan = match index_rgba_frames_quality_result(&rgba, TRANSPARENT_ALPHA_THRESHOLD) {
            QualityIndexResult::Quantized(plan) => plan,
            QualityIndexResult::Exact(_) => panic!("fixture should overflow the exact palette"),
        };
        let fused_output = encode_quality_index_plan_literal_gif(
            Vec::new(),
            &rgba,
            width,
            height,
            1,
            DelaySource::PerFrame(&delays),
            0,
            TRANSPARENT_ALPHA_THRESHOLD,
            plan,
        )
        .unwrap();
        assert_eq!(fused_output, indexed_output);
    }

    #[test]
    fn opaque_quality_scan_is_identical_for_zero_alpha_threshold() {
        let mut rgba = Vec::new();
        for y in 0..48u16 {
            for x in 0..48u16 {
                rgba.extend_from_slice(&[
                    ((x * 13 + y * 7) & 255) as u8,
                    ((x * 5 + y * 17) & 255) as u8,
                    ((x * 19 + y * 3) & 255) as u8,
                    255,
                ]);
            }
        }

        let thresholded = index_rgba_frames_quality(&rgba, TRANSPARENT_ALPHA_THRESHOLD);
        let opaque = index_rgba_frames_quality(&rgba, 0);
        assert_eq!(thresholded, opaque);
    }

    #[test]
    fn local_palette_mode_preserves_independent_exact_frame_colors() {
        let mut rgba = Vec::new();
        for frame in 0..2u16 {
            for value in 0..256u16 {
                rgba.extend_from_slice(&[
                    value as u8,
                    ((value * 3 + frame * 17) & 0xff) as u8,
                    ((value * 7 + frame * 29) & 0xff) as u8,
                    255,
                ]);
            }
        }

        let encoded = encode_rgba_gif_advanced_inner(
            &rgba,
            16,
            16,
            2,
            &[],
            DelaySource::PerFrame(&[4, 9]),
            0,
            false,
            TRANSPARENT_ALPHA_THRESHOLD,
            true,
            RgbaQuantization::Exact,
            RgbaPaletteMode::Local,
        )
        .unwrap();
        let metadata = parse_metadata(&encoded).unwrap();

        assert_eq!(metadata.global_palette_size, 0);
        assert_eq!(metadata.frames.len(), 2);
        assert!(metadata.frames.iter().all(|frame| frame.has_local_palette));
        assert_eq!(metadata.frames[0].delay, 4);
        assert_eq!(metadata.frames[1].delay, 9);
        assert!(metadata.frames.iter().all(|frame| frame.disposal == 2));
        for frame_index in 0..metadata.frames.len() {
            let decoded =
                decode_frame_pixels_inner(&encoded, &metadata, frame_index, PixelFormat::Rgba)
                    .unwrap();
            let expected_start = frame_index * 16 * 16 * 4;
            assert_eq!(decoded, rgba[expected_start..expected_start + 16 * 16 * 4]);
        }
    }
    #[test]
    fn packed_quality_histogram_indices_match_channel_indices() {
        for red in (0..=255u16).step_by(17) {
            for green in (0..=255u16).step_by(17) {
                for blue in (0..=255u16).step_by(17) {
                    let packed = u32::from(red as u8)
                        | (u32::from(green as u8) << 8)
                        | (u32::from(blue as u8) << 16)
                        | 0xaa00_0000;
                    assert_eq!(
                        quality_histogram_index_packed::<4>(packed),
                        quality_histogram_index_bits_const::<4>(red as u8, green as u8, blue as u8,)
                    );
                    assert_eq!(
                        quality_histogram_index_packed::<5>(packed),
                        quality_histogram_index_bits_const::<5>(red as u8, green as u8, blue as u8,)
                    );
                }
            }
        }
    }

}
