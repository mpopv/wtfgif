#![cfg_attr(feature = "quality-only", allow(dead_code))]

use wasm_bindgen::prelude::*;

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
// The dictionary key is `(previous_code * color_count) + next_pixel`. The
// previous code can grow to 4095, so the full 256-color table needs 2^20
// slots; smaller palettes can use a proportionally smaller direct table.
const LZW_DIRECT_ENTRY_COUNT: usize = 1 << 20;
const LZW_ENTRY_CODE_MASK: u32 = 0x0fff;
const LZW_ENTRY_EPOCH_SHIFT: u32 = 12;
const LZW_ENTRY_EPOCH_MAX: u32 = (1 << (32 - LZW_ENTRY_EPOCH_SHIFT)) - 1;
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

mod wasm_api;
pub use wasm_api::*;

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

    /// Decode a common opaque full-canvas frame into reusable Wasm-owned
    /// storage. JavaScript can copy the returned range out without passing its
    /// caller-owned canvas into Wasm first, avoiding an otherwise unavoidable
    /// input copy for frames that overwrite every pixel.
    pub fn decode_frame_rgba_scratch(&self, frame_index: usize) -> Result<usize, JsValue> {
        decode_frame_to_scratch(
            &self.data,
            &self.metadata,
            frame_index,
            PixelFormat::Rgba,
            &mut self.decode_scratch.borrow_mut(),
        )
        .map_err(|message| JsValue::from_str(&message))
    }

    pub fn decode_frame_bgra_scratch(&self, frame_index: usize) -> Result<usize, JsValue> {
        decode_frame_to_scratch(
            &self.data,
            &self.metadata,
            frame_index,
            PixelFormat::Bgra,
            &mut self.decode_scratch.borrow_mut(),
        )
        .map_err(|message| JsValue::from_str(&message))
    }

    /// Decode one frame's rectangle into reusable RGBA/BGRA scratch storage.
    /// Transparent pixels are returned as zero and must be left untouched by
    /// the caller when overlaying the rectangle onto an existing canvas.
    pub fn decode_frame_rect_rgba_scratch(&self, frame_index: usize) -> Result<usize, JsValue> {
        decode_frame_rect_to_scratch(
            &self.data,
            &self.metadata,
            frame_index,
            PixelFormat::Rgba,
            &mut self.decode_scratch.borrow_mut(),
        )
        .map_err(|message| JsValue::from_str(&message))
    }

    pub fn decode_frame_rect_bgra_scratch(&self, frame_index: usize) -> Result<usize, JsValue> {
        decode_frame_rect_to_scratch(
            &self.data,
            &self.metadata,
            frame_index,
            PixelFormat::Bgra,
            &mut self.decode_scratch.borrow_mut(),
        )
        .map_err(|message| JsValue::from_str(&message))
    }

    pub fn decode_scratch_ptr(&self) -> usize {
        self.decode_scratch.borrow().output.as_ptr() as usize
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

    /// Prepare composited frames in Wasm-owned scratch storage. The returned
    /// length refers to `composited_scratch_ptr()` and avoids copying the
    /// complete animation into JavaScript when a caller will consume frames
    /// one at a time.
    pub fn prepare_composited_rgba_scratch(
        &self,
        requested_frames: &[u8],
    ) -> Result<usize, JsValue> {
        let prepared = prepare_composited_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Rgba,
        )
        .map_err(|message| JsValue::from_str(&message))?;
        let length = prepared.len();
        self.decode_scratch.borrow_mut().composited_output = prepared;
        Ok(length)
    }

    pub fn prepare_composited_bgra_scratch(
        &self,
        requested_frames: &[u8],
    ) -> Result<usize, JsValue> {
        let prepared = prepare_composited_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Bgra,
        )
        .map_err(|message| JsValue::from_str(&message))?;
        let length = prepared.len();
        self.decode_scratch.borrow_mut().composited_output = prepared;
        Ok(length)
    }

    pub fn composited_scratch_ptr(&self) -> usize {
        self.decode_scratch.borrow().composited_output.as_ptr() as usize
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

    /// Prepare composited delta frames in Wasm-owned scratch storage. The
    /// returned length refers to `composited_scratch_ptr()` and avoids copying
    /// the delta stream through a wasm-bindgen return value.
    pub fn prepare_composited_delta_rgba_scratch(
        &self,
        requested_frames: &[u8],
    ) -> Result<usize, JsValue> {
        let prepared = prepare_composited_delta_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Rgba,
        )
        .map_err(|message| JsValue::from_str(&message))?;
        let length = prepared.len();
        self.decode_scratch.borrow_mut().composited_output = prepared;
        Ok(length)
    }

    pub fn prepare_composited_delta_bgra_scratch(
        &self,
        requested_frames: &[u8],
    ) -> Result<usize, JsValue> {
        let prepared = prepare_composited_delta_frames_inner(
            &self.data,
            &self.metadata,
            requested_frames,
            PixelFormat::Bgra,
        )
        .map_err(|message| JsValue::from_str(&message))?;
        let length = prepared.len();
        self.decode_scratch.borrow_mut().composited_output = prepared;
        Ok(length)
    }
}

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[cfg(not(feature = "encode-only"))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn decode_frame_indices_inner(data: &[u8], frame: &FrameMetadata) -> Result<Vec<u8>, String> {
    let mut image_data = Vec::new();
    decode_frame_indices_with_image_scratch(data, frame, &mut image_data)
}

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn decode_frame_indices_with_image_scratch(
    data: &[u8],
    frame: &FrameMetadata,
    image_data: &mut Vec<u8>,
) -> Result<Vec<u8>, String> {
    let frame_size = usize::from(frame.width) * usize::from(frame.height);
    if should_decode_lzw_direct(frame, frame_size) {
        let image_data_slice;
        if let Some(range) = single_image_data_range(data, frame.data_offset) {
            image_data_slice = &data[range];
        } else {
            if image_data.capacity() < frame.data_length {
                image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
            }
            collect_image_data_into(data, frame.data_offset, image_data)?;
            image_data_slice = image_data.as_slice();
        }
        let mut output = vec![0; frame_size];
        lzw_decode_to_indices_direct(frame.min_code_size, image_data_slice, &mut output)?;
        return deinterlace_frame_indices(output, frame);
    }
    let mut lzw_scratch = LzwStackScratch::default();
    decode_frame_indices_with_scratches(data, frame, image_data, &mut lzw_scratch)
}

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn decode_frame_indices_with_scratches(
    data: &[u8],
    frame: &FrameMetadata,
    image_data: &mut Vec<u8>,
    lzw_scratch: &mut LzwStackScratch,
) -> Result<Vec<u8>, String> {
    let image_data_slice;
    if let Some(range) = single_image_data_range(data, frame.data_offset) {
        image_data_slice = &data[range];
    } else {
        if image_data.capacity() < frame.data_length {
            image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
        }
        collect_image_data_into(data, frame.data_offset, image_data)?;
        image_data_slice = image_data.as_slice();
    }
    let frame_size = usize::from(frame.width) * usize::from(frame.height);
    let linear = if should_decode_lzw_direct(frame, frame_size) {
        let mut output = vec![0; frame_size];
        if should_decode_lzw_copy(frame, frame_size) {
            lzw_decode_to_indices_copy_with_scratch(
                frame.min_code_size,
                image_data_slice,
                &mut output,
                lzw_scratch,
            )?;
        } else {
            lzw_decode_to_indices_direct_with_scratch(
                frame.min_code_size,
                image_data_slice,
                &mut output,
                lzw_scratch,
            )?;
        }
        output
    } else {
        let mut output = vec![0; frame_size];
        lzw_decode_to_indices_stack_with_scratch(
            frame.min_code_size,
            image_data_slice,
            &mut output,
            lzw_scratch,
        )?;
        output
    };
    deinterlace_frame_indices(linear, frame)
}

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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
    let image_data_slice;
    if let Some(range) = single_image_data_range(data, frame.data_offset) {
        image_data_slice = &data[range];
    } else {
        if image_data.capacity() < frame.data_length {
            image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
        }
        collect_image_data_into(data, frame.data_offset, image_data)?;
        image_data_slice = image_data.as_slice();
    }
    let frame_size = usize::from(frame.width) * usize::from(frame.height);
    output.resize(frame_size, 0);
    if should_decode_lzw_direct(frame, frame_size) {
        if should_decode_lzw_copy(frame, frame_size) {
            lzw_decode_to_indices_copy_with_scratch(
                frame.min_code_size,
                image_data_slice,
                output,
                lzw_scratch,
            )
        } else {
            lzw_decode_to_indices_direct_with_scratch(
                frame.min_code_size,
                image_data_slice,
                output,
                lzw_scratch,
            )
        }
    } else {
        lzw_decode_to_indices_stack_with_scratch(
            frame.min_code_size,
            image_data_slice,
            output,
            lzw_scratch,
        )
    }
}

#[inline]
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn single_image_data_range(data: &[u8], data_offset: usize) -> Option<std::ops::Range<usize>> {
    let length_offset = data_offset.checked_add(1)?;
    let payload_start = length_offset.checked_add(1)?;
    let length = usize::from(*data.get(length_offset)?);
    if length == 0 {
        return Some(payload_start..payload_start);
    }
    let payload_end = payload_start.checked_add(length)?;
    (data.get(payload_end) == Some(&0)).then_some(payload_start..payload_end)
}

#[inline]
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn should_decode_lzw_direct(frame: &FrameMetadata, frame_size: usize) -> bool {
    (frame.min_code_size == 8 && frame_size >= 256)
        || frame_size >= 40_000
        || (frame_size >= 1_000
            && frame.data_length.saturating_mul(20) < frame_size.saturating_mul(13))
        || (frame_size >= 256 && frame.data_length.saturating_mul(5) < frame_size.saturating_mul(2))
}

#[inline]
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn should_decode_lzw_copy(frame: &FrameMetadata, frame_size: usize) -> bool {
    frame.min_code_size <= 6
        || frame_size >= 10_000
        || frame.data_length.saturating_mul(5) < frame_size.saturating_mul(2)
}

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[cfg(not(feature = "encode-only"))]
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

#[cfg(not(feature = "encode-only"))]
fn decode_frame_to_scratch(
    data: &[u8],
    metadata: &GifMetadata,
    frame_index: usize,
    format: PixelFormat,
    scratch: &mut FrameDecodeScratch,
) -> Result<usize, String> {
    let frame = metadata
        .frames
        .get(frame_index)
        .ok_or_else(|| "Frame index out of range".to_string())?;
    if frame.interlaced
        || frame.transparent_index.is_some()
        || frame.x != 0
        || frame.y != 0
        || frame.width != metadata.width
        || frame.height != metadata.height
    {
        return Err("Scratch decode requires an opaque full-canvas frame".to_string());
    }

    let canvas_pixels = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_len = canvas_pixels
        .checked_mul(4)
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    if scratch.output.len() != output_len {
        scratch.output.resize(output_len, 0);
    }

    let (prefix, canvas, suffix) = unsafe { scratch.output.align_to_mut::<u32>() };
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
        let image_data_slice;
        if let Some(range) = single_image_data_range(data, frame.data_offset) {
            image_data_slice = &data[range];
        } else {
            if scratch.image_data.capacity() < frame.data_length {
                scratch
                    .image_data
                    .reserve(frame.data_length.saturating_sub(scratch.image_data.len()));
            }
            collect_image_data_into(data, frame.data_offset, &mut scratch.image_data)?;
            image_data_slice = scratch.image_data.as_slice();
        }
        if lzw_decode_to_pixels_copy_with_scratch(
            frame.min_code_size,
            image_data_slice,
            &scratch.palette,
            canvas,
            &mut scratch.lzw,
        )
        .is_ok()
        {
            return Ok(output_len);
        }
    }

    decode_frame_indices_reusing_output(
        data,
        frame,
        &mut scratch.image_data,
        &mut scratch.lzw,
        &mut scratch.indices,
    )?;
    if scratch.palette_offset != frame.palette_offset
        || scratch.palette_size != frame.palette_size
        || scratch.palette_format != Some(format)
    {
        scratch.palette = build_palette_u32(data, frame, format)?;
        scratch.palette_offset = frame.palette_offset;
        scratch.palette_size = frame.palette_size;
        scratch.palette_format = Some(format);
    }

    let (prefix, canvas, suffix) = unsafe { scratch.output.align_to_mut::<u32>() };
    if prefix.is_empty() && suffix.is_empty() {
        blit_indices_to_canvas_u32(
            &scratch.palette,
            metadata.width,
            frame,
            &scratch.indices,
            canvas,
        )?;
    } else {
        let palette = build_palette_pixels(data, frame, format)?;
        blit_indices_to_pixels(
            &palette,
            metadata.width,
            frame,
            &scratch.indices,
            &mut scratch.output,
        )?;
    }
    Ok(output_len)
}

#[cfg(not(feature = "encode-only"))]
fn decode_frame_rect_to_scratch(
    data: &[u8],
    metadata: &GifMetadata,
    frame_index: usize,
    format: PixelFormat,
    scratch: &mut FrameDecodeScratch,
) -> Result<usize, String> {
    let frame = metadata
        .frames
        .get(frame_index)
        .ok_or_else(|| "Frame index out of range".to_string())?;
    let frame_pixels = usize::from(frame.width)
        .checked_mul(usize::from(frame.height))
        .ok_or_else(|| "Decoded frame size overflow".to_string())?;

    scratch.composited_output.resize(frame_pixels, 0);
    if !frame.interlaced {
        if scratch.palette_offset != frame.palette_offset
            || scratch.palette_size != frame.palette_size
            || scratch.palette_format != Some(format)
        {
            scratch.palette = build_palette_u32(data, frame, format)?;
            scratch.palette_offset = frame.palette_offset;
            scratch.palette_size = frame.palette_size;
            scratch.palette_format = Some(format);
        }
        let image_data_slice;
        if let Some(range) = single_image_data_range(data, frame.data_offset) {
            image_data_slice = &data[range];
        } else {
            if scratch.image_data.capacity() < frame.data_length {
                scratch
                    .image_data
                    .reserve(frame.data_length.saturating_sub(scratch.image_data.len()));
            }
            collect_image_data_into(data, frame.data_offset, &mut scratch.image_data)?;
            image_data_slice = scratch.image_data.as_slice();
        }
        if frame.transparent_index.is_none()
            && lzw_decode_to_pixels_copy_with_scratch(
                frame.min_code_size,
                image_data_slice,
                &scratch.palette,
                &mut scratch.composited_output,
                &mut scratch.lzw,
            )
            .is_ok()
        {
            return Ok(frame_pixels);
        }
    }

    decode_frame_indices_reusing_output(
        data,
        frame,
        &mut scratch.image_data,
        &mut scratch.lzw,
        &mut scratch.indices,
    )?;
    if scratch.palette_offset != frame.palette_offset
        || scratch.palette_size != frame.palette_size
        || scratch.palette_format != Some(format)
    {
        scratch.palette = build_palette_u32(data, frame, format)?;
        scratch.palette_offset = frame.palette_offset;
        scratch.palette_size = frame.palette_size;
        scratch.palette_format = Some(format);
    }

    let transparent_index = frame.transparent_index;
    for (destination, &index) in scratch
        .composited_output
        .iter_mut()
        .zip(scratch.indices.iter().take(frame_pixels))
    {
        *destination = if transparent_index == Some(index) {
            0
        } else {
            *scratch
                .palette
                .get(usize::from(index))
                .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?
        };
    }
    Ok(frame_pixels)
}

#[cfg(not(feature = "encode-only"))]
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
    let (prefix, canvas, suffix) = unsafe { target.align_to_mut::<u32>() };
    if prefix.is_empty()
        && suffix.is_empty()
        && !frame.interlaced
        && frame.transparent_index.is_none()
        && frame.x == 0
        && frame.y == 0
        && frame.width == metadata.width
        && frame.height == metadata.height
    {
        if scratch.palette_offset != frame.palette_offset
            || scratch.palette_size != frame.palette_size
            || scratch.palette_format != Some(format)
        {
            scratch.palette = build_palette_u32(data, frame, format)?;
            scratch.palette_offset = frame.palette_offset;
            scratch.palette_size = frame.palette_size;
            scratch.palette_format = Some(format);
        }
        let image_data_slice;
        if let Some(range) = single_image_data_range(data, frame.data_offset) {
            image_data_slice = &data[range];
        } else {
            if scratch.image_data.capacity() < frame.data_length {
                scratch
                    .image_data
                    .reserve(frame.data_length.saturating_sub(scratch.image_data.len()));
            }
            collect_image_data_into(data, frame.data_offset, &mut scratch.image_data)?;
            image_data_slice = scratch.image_data.as_slice();
        }
        if lzw_decode_to_pixels_copy_with_scratch(
            frame.min_code_size,
            image_data_slice,
            &scratch.palette,
            canvas,
            &mut scratch.lzw,
        )
        .is_ok()
        {
            return Ok(());
        }
    }

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

#[cfg(not(feature = "encode-only"))]
fn prepare_composited_frames_inner(
    data: &[u8],
    metadata: &GifMetadata,
    requested_frames: &[u8],
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    prepare_composited_frames_selected(data, metadata, Some(requested_frames), format)
}

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn prepare_all_composited_frames_inner(
    data: &[u8],
    metadata: &GifMetadata,
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    prepare_composited_frames_selected(data, metadata, None, format)
}

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn prepare_composited_frames_selected(
    data: &[u8],
    metadata: &GifMetadata,
    requested_frames: Option<&[u8]>,
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    let all_requested = requested_frames.is_none()
        || requested_frames.is_some_and(|frames| {
            frames.len() == metadata.frames.len() && frames.iter().all(|flag| *flag != 0)
        });
    if all_requested && all_frames_full_opaque(metadata) {
        return decode_full_opaque_frames_direct(data, metadata, format);
    }
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
        decode_frame_indices_reusing_output(
            data,
            frame,
            &mut image_data,
            &mut lzw_scratch,
            &mut indices_scratch,
        )?;
        blit_indices_to_canvas_u32(
            palette,
            metadata.width,
            frame,
            &indices_scratch,
            &mut canvas,
        )?;

        if requested != 0 {
            output.extend_from_slice(&canvas);
        }

        apply_frame_disposal_u32(&mut canvas, metadata.width, metadata.height, frame, restore);
    }

    Ok(output)
}

#[inline]
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn all_frames_full_opaque(metadata: &GifMetadata) -> bool {
    !metadata.frames.is_empty()
        && metadata.frames.iter().all(|frame| {
            frame.x == 0
                && frame.y == 0
                && frame.width == metadata.width
                && frame.height == metadata.height
                && frame.transparent_index.is_none()
        })
}

/// Decode animations whose frames overwrite the entire canvas without
/// transparency directly into the returned frame stream. The general
/// compositor first maps each frame into a reusable canvas and then copies
/// that canvas into the output; this common independent-frame shape only
/// needs the palette mapping once per pixel.
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn decode_full_opaque_frames_direct(
    data: &[u8],
    metadata: &GifMetadata,
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    let canvas_pixels = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_pixels = canvas_pixels
        .checked_mul(metadata.frames.len())
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
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
    let mut output = uninitialized(output_pixels);
    let mut image_data = Vec::new();
    let mut lzw_scratch = LzwStackScratch::default();
    let mut indices_scratch = Vec::new();
    for (frame_index, frame) in metadata.frames.iter().enumerate() {
        let palette_storage;
        let palette = if let Some(palette) = shared_palette.as_ref() {
            palette
        } else {
            palette_storage = build_palette_u32(data, frame, format)?;
            &palette_storage
        };
        let destination =
            &mut output[frame_index * canvas_pixels..(frame_index + 1) * canvas_pixels];
        let destination_u32 = unsafe {
            std::slice::from_raw_parts_mut(destination.as_mut_ptr().cast::<u32>(), canvas_pixels)
        };
        let image_data_slice;
        if let Some(range) = single_image_data_range(data, frame.data_offset) {
            image_data_slice = &data[range];
        } else {
            if image_data.capacity() < frame.data_length {
                image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
            }
            collect_image_data_into(data, frame.data_offset, &mut image_data)?;
            image_data_slice = image_data.as_slice();
        }
        if lzw_decode_to_pixels_copy_with_scratch(
            frame.min_code_size,
            image_data_slice,
            palette,
            destination_u32,
            &mut lzw_scratch,
        )
        .is_ok()
        {
            continue;
        }
        decode_frame_indices_reusing_output(
            data,
            frame,
            &mut image_data,
            &mut lzw_scratch,
            &mut indices_scratch,
        )?;
        blit_indices_to_uninit_full_canvas_u32(palette, &indices_scratch, destination)?;
    }
    Ok(unsafe { assume_initialized(output) })
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
    let mut output = uninitialized(output_pixels);
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

    Ok(unsafe { assume_initialized(output) })
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
#[allow(clippy::too_many_arguments)]
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
    let mut output = uninitialized(output_pixels);
    decode_segmented_frames_into_native(data, metadata, segments, &mut output)?;
    Ok(unsafe { assume_initialized(output) })
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
    let mut output = uninitialized(output_pixels);
    decode_and_compose_pipeline_into_native(data, metadata, &mut output)?;
    Ok(unsafe { assume_initialized(output) })
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
        uninitialized::<u8>(decoded_indices_length)
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
        let mut output = uninitialized(output_pixels);
        decode_small_independent_frames_into_native(data, metadata, canvas_pixels, &mut output)?;
        return Ok(unsafe { assume_initialized(output) });
    }
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let thread_count = available_threads.min(metadata.frames.len());
    let mut output = uninitialized(output_pixels);
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
        let handles: Vec<_> = (0..thread_count).map(|_| scope.spawn(worker)).collect();
        for handle in handles {
            handle
                .join()
                .map_err(|_| "Parallel GIF decoder panicked".to_string())??;
        }
        Ok::<_, String>(())
    })?;
    Ok(unsafe { assume_initialized(output) })
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
            (results_address as *const std::sync::OnceLock<Result<Vec<u8>, String>>)
                .add(frame_index)
                .as_ref()
                .expect("parallel result slot exists")
                .set(decode_frame_indices_inner(data, frame))
                .expect("parallel frame is decoded once");
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
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        decode_indices_worker(
            context.data,
            context.metadata,
            context.results_address,
            &context.next_frame,
        );
    }))
    .is_err();
    if panicked {
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

    let results: Vec<std::sync::OnceLock<Result<Vec<u8>, String>>> = (0..metadata.frames.len())
        .map(|_| std::sync::OnceLock::new())
        .collect();
    let results_address = results.as_ptr() as usize;
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
        return Err("Parallel GIF decoder panicked".to_string());
    }
    results
        .into_iter()
        .map(|result| {
            result
                .into_inner()
                .ok_or_else(|| "Parallel GIF decoder skipped a frame".to_string())?
        })
        .collect()
}

#[cfg(not(feature = "encode-only"))]
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

#[cfg(not(feature = "encode-only"))]
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
    let previous_pointer = previous.as_ptr();
    let current_pointer = current.as_ptr();
    for y in top..=bottom {
        let row = y * width;
        let mut x = 0usize;
        while x + 2 <= width {
            let previous_word =
                unsafe { std::ptr::read_unaligned(previous_pointer.add(row + x).cast::<u64>()) };
            let current_word =
                unsafe { std::ptr::read_unaligned(current_pointer.add(row + x).cast::<u64>()) };
            if previous_word != current_word {
                for offset in 0..2 {
                    if previous[row + x + offset] != current[row + x + offset] {
                        let changed = x + offset;
                        left = left.min(changed);
                        right = right.max(changed);
                    }
                }
            }
            x += 2;
        }
        while x < width {
            if previous[row + x] != current[row + x] {
                left = left.min(x);
                right = right.max(x);
            }
            x += 1;
        }
    }

    Some(ChangedRectU32 {
        x: left,
        y: top,
        width: right - left + 1,
        height: bottom - top + 1,
    })
}

#[inline(always)]
fn find_changed_rect_u8(
    previous: &[u8],
    current: &[u8],
    width: usize,
    height: usize,
) -> Option<ChangedRectU32> {
    let mut top = height;
    let mut bottom = 0usize;
    let mut left = width;
    let mut right = 0usize;
    let previous_pointer = previous.as_ptr();
    let current_pointer = current.as_ptr();
    for y in 0..height {
        let row = y * width;
        let previous_row = &previous[row..row + width];
        let current_row = &current[row..row + width];
        if previous_row == current_row {
            continue;
        }
        let mut row_left = width;
        let mut row_right = 0usize;
        let mut x = 0usize;
        while x + 8 <= width {
            let previous_word =
                unsafe { std::ptr::read_unaligned(previous_pointer.add(row + x).cast::<u64>()) };
            let current_word =
                unsafe { std::ptr::read_unaligned(current_pointer.add(row + x).cast::<u64>()) };
            if previous_word != current_word {
                for offset in 0..8 {
                    if previous[row + x + offset] != current[row + x + offset] {
                        let changed = x + offset;
                        row_left = row_left.min(changed);
                        row_right = row_right.max(changed);
                    }
                }
            }
            x += 8;
        }
        while x < width {
            if previous[row + x] != current[row + x] {
                row_left = row_left.min(x);
                row_right = row_right.max(x);
            }
            x += 1;
        }
        if row_left < width {
            if top == height {
                top = y;
            }
            bottom = y;
            left = left.min(row_left);
            right = right.max(row_right);
        }
    }

    if top == height {
        return None;
    }

    Some(ChangedRectU32 {
        x: left,
        y: top,
        width: right - left + 1,
        height: bottom - top + 1,
    })
}

#[inline(always)]
fn find_changed_rect_rgba_bytes(
    previous: &[u8],
    current: &[u8],
    width: usize,
    height: usize,
) -> Option<ChangedRectU32> {
    let row_bytes = width * 4;
    let mut top = height;
    let mut bottom = 0usize;
    let mut left = width;
    let mut right = 0usize;
    for y in 0..height {
        let row = y * row_bytes;
        let previous_row = &previous[row..row + row_bytes];
        let current_row = &current[row..row + row_bytes];
        if previous_row == current_row {
            continue;
        }

        let mut row_left = width;
        let mut row_right = 0usize;
        let previous_pointer = previous_row.as_ptr();
        let current_pointer = current_row.as_ptr();
        let pair_end = width & !1;
        let mut x = 0usize;
        while x < pair_end {
            let offset = x * 4;
            let difference = unsafe {
                u64::from_le(std::ptr::read_unaligned(
                    previous_pointer.add(offset) as *const u64
                )) ^ u64::from_le(std::ptr::read_unaligned(
                    current_pointer.add(offset) as *const u64
                ))
            };
            if difference != 0 {
                if difference & 0xffff_ffff != 0 {
                    row_left = row_left.min(x);
                    row_right = row_right.max(x);
                }
                if difference >> 32 != 0 {
                    row_left = row_left.min(x + 1);
                    row_right = row_right.max(x + 1);
                }
            }
            x += 2;
        }
        if x < width {
            let offset = x * 4;
            let difference = unsafe {
                u32::from_le(std::ptr::read_unaligned(
                    previous_pointer.add(offset) as *const u32
                )) ^ u32::from_le(std::ptr::read_unaligned(
                    current_pointer.add(offset) as *const u32
                ))
            };
            if difference != 0 {
                row_left = row_left.min(x);
                row_right = row_right.max(x);
            }
        }
        if row_left < width {
            if top == height {
                top = y;
            }
            bottom = y;
            left = left.min(row_left);
            right = right.max(row_right);
        }
    }

    if top == height {
        return None;
    }

    Some(ChangedRectU32 {
        x: left,
        y: top,
        width: right - left + 1,
        height: bottom - top + 1,
    })
}

#[cfg(not(feature = "encode-only"))]
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
    keys: Vec<u32>,
    values: Vec<u8>,
    mask: usize,
}

const COLOR_INDEX_EMPTY: u32 = u32::MAX;

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
            keys: vec![COLOR_INDEX_EMPTY; capacity],
            values: vec![0; capacity],
            mask: capacity - 1,
        }
    }

    fn reset(&mut self, capacity: usize) {
        debug_assert!(capacity.is_power_of_two());
        if self.keys.len() != capacity {
            self.keys.resize(capacity, COLOR_INDEX_EMPTY);
            self.values.resize(capacity, 0);
        } else {
            self.keys.fill(COLOR_INDEX_EMPTY);
        }
        self.mask = capacity - 1;
    }

    #[inline(always)]
    fn get(&self, key: u32) -> Option<u8> {
        let mask = self.mask;
        let mut slot = (key as usize).wrapping_mul(2_654_435_761) & mask;
        loop {
            let stored = unsafe { *self.keys.get_unchecked(slot) };
            if stored == key {
                return Some(unsafe { *self.values.get_unchecked(slot) });
            }
            if stored == COLOR_INDEX_EMPTY {
                return None;
            }
            slot = (slot + 1) & mask;
        }
    }

    #[inline(always)]
    fn insert_if_absent(&mut self, key: u32, value: u8) {
        let mask = self.mask;
        let mut slot = (key as usize).wrapping_mul(2_654_435_761) & mask;
        loop {
            let stored = unsafe { *self.keys.get_unchecked(slot) };
            if stored == key {
                return;
            }
            if stored == COLOR_INDEX_EMPTY {
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

// These hot codec primitives keep the GIF geometry and format switches
// explicit. Grouping scalar arguments into transient option objects would
// make the call graph less direct without reducing state or validation.
#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
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

    // With a caller-supplied global palette, quantization is only a per-pixel
    // palette lookup. For delta output we can find the changed rectangle in
    // the source RGB stream first and map only the pixels that will actually
    // be written. This avoids materializing and scanning a full indexed copy
    // of every frame while preserving the nearest-palette and exact-palette
    // semantics below.
    if deltas && !palette_rgb.is_empty() && quantization != RgbaQuantization::Exact {
        match encode_rgba_delta_gif_to_palette_inner(
            rgba_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delays,
            loop_count,
            literal,
            alpha_threshold,
            quantization == RgbaQuantization::Exact,
            false,
        ) {
            Ok(encoded) => return Ok(encoded),
            Err(message) if message == RGBA_DELTA_FALLBACK => {}
            Err(message) => return Err(message),
        }
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

#[allow(clippy::too_many_arguments)]
fn encode_rgba_quality_gif_inner_with_output(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    delays: DelaySource<'_>,
    loop_count: i32,
    alpha_threshold: u8,
    output: Vec<u8>,
) -> Result<Vec<u8>, String> {
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
            recycle_quality_palette(palette);
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

#[allow(clippy::too_many_arguments)]
#[inline(never)]
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
        histogram_indices,
        transparent_index,
        histogram_bits,
        mapping_bits,
    } = plan;
    let color_count = checked_palette_color_count(palette.len())?;
    if color_count != 256 {
        let (palette, indexed, transparent_index) = QualityIndexPlan {
            palette,
            histogram_to_palette,
            histogram_indices,
            transparent_index,
            histogram_bits,
            mapping_bits,
        }
        .into_indexed(rgba_stream, alpha_threshold);
        let encoded = encode_indexed_literal_gif_inner_with_output_unchecked(
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
        recycle_quality_palette(palette);
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
    if let Some(histogram_indices) = histogram_indices {
        if histogram_bits == mapping_bits {
            return encode_quality_index_plan_literal_gif_from_histogram_indices(
                output,
                rgba_stream,
                width,
                height,
                frame_count,
                delays,
                loop_count,
                palette,
                histogram_to_palette,
                histogram_indices,
                transparent_index,
            );
        }
        let indexed = materialize_quality_indices(
            histogram_indices,
            transparent_index,
            &histogram_to_palette,
        );
        recycle_quality_histogram_to_palette(histogram_to_palette);
        let encoded = encode_indexed_literal_gif_inner_with_output_unchecked(
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
        recycle_quality_palette(palette);
        recycle_quantized_indexed(indexed);
        return encoded;
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
    recycle_quality_palette(palette);
    Ok(output)
}

/// Emit a 256-color quality plan directly from the retained histogram cells.
/// The high-resolution histogram path already records one u16 cell per pixel
/// while building the palette; materializing those cells into a second u8
/// buffer before literal LZW is a redundant full-image pass.
#[allow(clippy::too_many_arguments)]
fn encode_quality_index_plan_literal_gif_from_histogram_indices(
    mut output: Vec<u8>,
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    delays: DelaySource<'_>,
    loop_count: i32,
    palette: Vec<u32>,
    histogram_to_palette: Vec<u8>,
    histogram_indices: Vec<u16>,
    transparent_index: Option<u8>,
) -> Result<Vec<u8>, String> {
    let color_count = checked_palette_color_count(palette.len())?;
    debug_assert_eq!(color_count, 256);
    let frame_len = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| "Frame size overflow".to_string())?;
    let expected_pixels = frame_len
        .checked_mul(frame_count)
        .ok_or_else(|| "RGBA frame stream overflow".to_string())?;
    if rgba_stream.len() != expected_pixels.saturating_mul(4)
        || histogram_indices.len() != expected_pixels
    {
        return Err("Quality histogram stream length does not match dimensions".to_string());
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
        let frame_start = frame_index * frame_len;
        encode_nine_bit_literal_lzw_histogram_indices(
            &mut output,
            &histogram_indices[frame_start..frame_start + frame_len],
            transparent_index.unwrap_or(0),
            transparent_index.is_some(),
            &histogram_to_palette,
        )?;
    }
    output.push(0x3b);
    recycle_quality_histogram_to_palette(histogram_to_palette);
    recycle_quality_palette(palette);
    recycle_quality_histogram_indices(histogram_indices);
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

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
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

    // A supplied 256-color palette with opaque input is the common known-
    // palette path. Map directly into the literal LZW stream so we do not
    // materialize an intermediate indexed frame and then scan it again.
    // This preserves the exact-palette contract of the existing literal path.
    if literal && !deltas && palette_rgb.len() == 256 && rgba_stream_is_opaque(rgba_stream) {
        return encode_rgba_literal_gif_to_palette_inner(
            rgba_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delays,
            loop_count,
        );
    }

    if literal && !deltas && (9..=16).contains(&palette_rgb.len()) {
        match encode_rgba_sixteen_color_literal_gif_inner(
            rgba_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delays,
            loop_count,
            alpha_threshold,
        ) {
            Ok(encoded) => return Ok(encoded),
            Err(message) if message == RGBA_DELTA_FALLBACK => {}
            Err(message) => return Err(message),
        }
    }

    if deltas && !palette_rgb.is_empty() {
        match encode_rgba_delta_gif_to_palette_inner(
            rgba_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delays,
            loop_count,
            literal,
            alpha_threshold,
            true,
            true,
        ) {
            Ok(encoded) => return Ok(encoded),
            Err(message) if message == RGBA_DELTA_FALLBACK => {}
            Err(message) => return Err(message),
        }
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

fn encode_rgba_literal_gif_to_palette_inner(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
) -> Result<Vec<u8>, String> {
    let frame_pixels = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| "Frame size overflow".to_string())?;
    let frame_bytes = frame_pixels
        .checked_mul(4)
        .ok_or_else(|| "RGBA frame size overflow".to_string())?;
    let color_count = checked_palette_color_count(palette_rgb.len())?;
    let min_code_size = (log2_pow2(color_count) as u8).max(2);
    let lzw_length = literal_lzw_block_size(frame_pixels, min_code_size)?;
    let frame_capacity = (0..frame_count).try_fold(0usize, |capacity, frame_index| {
        capacity
            .checked_add(10)
            .and_then(|length| length.checked_add(usize::from(delays.get(frame_index) != 0) * 8))
            .and_then(|length| length.checked_add(lzw_length))
            .ok_or_else(|| "Encoded GIF size overflow".to_string())
    })?;
    let output_capacity = 13usize
        .checked_add(color_count * 3)
        .and_then(|length| length.checked_add(usize::from(loop_count >= 0) * 19))
        .and_then(|length| length.checked_add(frame_capacity))
        .and_then(|length| length.checked_add(1))
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?;
    let mut output = Vec::with_capacity(output_capacity);
    let mapper = PaletteMapper::new(palette_rgb);
    write_indexed_gif_header(&mut output, width, height, palette_rgb, color_count);
    write_loop_extension(&mut output, loop_count);

    for (frame_index, frame) in rgba_stream.chunks_exact(frame_bytes).enumerate() {
        write_indexed_gif_frame_header(
            &mut output,
            0,
            0,
            width,
            height,
            delays.get(frame_index),
            None,
            0,
        );
        debug_assert_eq!(min_code_size, 8);
        encode_nine_bit_literal_lzw_palette_mapped_to(&mut output, frame, &mapper)?;
    }
    output.push(0x3b);
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn encode_rgba_sixteen_color_literal_gif_inner(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    alpha_threshold: u8,
) -> Result<Vec<u8>, String> {
    delays.validate(frame_count)?;
    let frame_pixels = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| "Frame size overflow".to_string())?;
    let frame_bytes = frame_pixels
        .checked_mul(4)
        .ok_or_else(|| "RGBA frame size overflow".to_string())?;
    let color_count = checked_palette_color_count(palette_rgb.len())?;
    if color_count != 16 {
        return Err(RGBA_DELTA_FALLBACK.to_string());
    }
    let lzw_length = literal_lzw_block_size(frame_pixels, 4)?;
    let frame_capacity = (0..frame_count).try_fold(0usize, |capacity, frame_index| {
        capacity
            .checked_add(10)
            .and_then(|length| length.checked_add(usize::from(delays.get(frame_index) != 0) * 8))
            .and_then(|length| length.checked_add(lzw_length))
            .ok_or_else(|| "Encoded GIF size overflow".to_string())
    })?;
    let output_capacity = 13usize
        .checked_add(color_count * 3)
        .and_then(|length| length.checked_add(usize::from(loop_count >= 0) * 19))
        .and_then(|length| length.checked_add(frame_capacity))
        .and_then(|length| length.checked_add(1))
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?;
    let mut output = Vec::with_capacity(output_capacity);
    let mapper = PaletteMapper::new(palette_rgb);
    write_indexed_gif_header(&mut output, width, height, palette_rgb, color_count);
    write_loop_extension(&mut output, loop_count);
    for (frame_index, frame) in rgba_stream.chunks_exact(frame_bytes).enumerate() {
        write_indexed_gif_frame_header(
            &mut output,
            0,
            0,
            width,
            height,
            delays.get(frame_index),
            None,
            0,
        );
        encode_rgba_five_bit_literal_frame_to(
            &mut output,
            frame,
            &mapper,
            alpha_threshold,
            true,
            true,
        )?;
    }
    output.push(0x3b);
    Ok(output)
}

fn rgba_alpha_invalid(rgba: &[u8], alpha_threshold: u8, exact_alpha: bool) -> bool {
    if alpha_threshold == 0 && !exact_alpha {
        return false;
    }
    let mut offset = 3usize;
    while offset < rgba.len() {
        let alpha = rgba[offset];
        if alpha < alpha_threshold || (exact_alpha && alpha != 255) {
            return true;
        }
        offset += 4;
    }
    false
}

fn rgba_rect_alpha_invalid(
    rgba_stream: &[u8],
    canvas_width: usize,
    rect: ChangedRectU32,
    alpha_threshold: u8,
    exact_alpha: bool,
) -> bool {
    if alpha_threshold == 0 && !exact_alpha {
        return false;
    }
    for row in 0..rect.height {
        let start = ((rect.y + row) * canvas_width + rect.x) * 4 + 3;
        let end = start + rect.width * 4;
        let mut offset = start;
        while offset < end {
            let alpha = rgba_stream[offset];
            if alpha < alpha_threshold || (exact_alpha && alpha != 255) {
                return true;
            }
            offset += 4;
        }
    }
    false
}

const RGBA_DELTA_FALLBACK: &str = "RGBA delta palette path requires the general encoder";

#[allow(clippy::too_many_arguments)]
fn encode_rgba_delta_gif_to_palette_inner(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    literal: bool,
    alpha_threshold: u8,
    exact_alpha: bool,
    exact_palette: bool,
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
    let literal_small_palette = !literal && palette_rgb.len() <= 16;
    let mut lzw_tables = (!literal && !literal_small_palette).then(LzwEncodeTables::new);
    let mut mapped = (literal || literal_small_palette)
        .then(|| Vec::with_capacity(frame_pixels))
        .unwrap_or_default();

    write_indexed_gif_header(&mut output, width, height, palette_rgb, color_count);
    write_loop_extension(&mut output, loop_count);

    let mut previous_frame: Option<&[u8]> = None;
    for (frame_index, frame) in rgba_stream.chunks_exact(frame_bytes).enumerate() {
        let delay = delays.get(frame_index);
        if let Some(previous) = previous_frame {
            if let Some(rect) =
                find_changed_rect_rgba_bytes(previous, frame, canvas_width, canvas_height)
            {
                let direct_two_bit = literal_small_palette && color_count <= 4;
                if !direct_two_bit
                    && rgba_rect_alpha_invalid(
                        frame,
                        canvas_width,
                        rect,
                        alpha_threshold,
                        exact_alpha,
                    )
                {
                    return Err(RGBA_DELTA_FALLBACK.to_string());
                }
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
                if literal || literal_small_palette {
                    if direct_two_bit {
                        encode_rgba_two_bit_literal_rect_to(
                            &mut output,
                            frame,
                            canvas_width,
                            rect,
                            &mapper,
                            alpha_threshold,
                            exact_alpha,
                            exact_palette,
                        )?;
                    } else {
                        map_rgba_rect_to_palette(frame, canvas_width, rect, &mapper, &mut mapped);
                        encode_indexed_literal_lzw_direct_to(
                            &mut output,
                            &mapped,
                            min_code_size,
                            color_count,
                        )?;
                    }
                } else {
                    let rect_pixels = rect.width * rect.height;
                    if rect_pixels <= 1024 {
                        map_rgba_rect_to_palette(frame, canvas_width, rect, &mapper, &mut mapped);
                        encode_indexed_literal_lzw_direct_to(
                            &mut output,
                            &mapped,
                            min_code_size,
                            color_count,
                        )?;
                    } else {
                        encode_rgba_lzw_rect_to_palette(
                            &mut output,
                            frame,
                            canvas_width,
                            rect,
                            &mapper,
                            min_code_size,
                            color_count,
                            lzw_tables.as_mut().unwrap(),
                        )?;
                    }
                }
            } else {
                write_indexed_gif_frame_header(&mut output, 0, 0, 1, 1, delay, None, 0);
                let noop = [mapper.index_pixel(frame[0], frame[1], frame[2])];
                encode_indexed_literal_lzw_direct_to(
                    &mut output,
                    &noop,
                    min_code_size,
                    color_count,
                )?;
            }
        } else {
            let direct_two_bit = literal_small_palette && color_count <= 4;
            if !direct_two_bit && rgba_alpha_invalid(frame, alpha_threshold, exact_alpha) {
                return Err(RGBA_DELTA_FALLBACK.to_string());
            }
            write_indexed_gif_frame_header(&mut output, 0, 0, width, height, delay, None, 0);
            if literal || literal_small_palette {
                if direct_two_bit {
                    encode_rgba_two_bit_literal_frame_to(
                        &mut output,
                        frame,
                        &mapper,
                        alpha_threshold,
                        exact_alpha,
                        exact_palette,
                    )?;
                } else {
                    map_rgba_frame_to_palette(frame, &mapper, &mut mapped);
                    encode_indexed_literal_lzw_direct_to(
                        &mut output,
                        &mapped,
                        min_code_size,
                        color_count,
                    )?;
                }
            } else {
                map_rgba_frame_to_palette(frame, &mapper, &mut mapped);
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
) -> Result<IndexedPalette, String> {
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
) -> Result<IndexedPalette, String> {
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
const QUANTIZED_SORT_COUNT_LIMIT: u64 = 1 << 48;

struct QualityIndexPlan {
    palette: Vec<u32>,
    histogram_to_palette: Vec<u8>,
    histogram_indices: Option<Vec<u16>>,
    transparent_index: Option<u8>,
    histogram_bits: usize,
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
            histogram_indices,
            transparent_index,
            histogram_bits: _,
            mapping_bits,
        } = self;
        let indexed = if let Some(histogram_indices) = histogram_indices {
            // The retained histogram cells and final palette indices have
            // the same element count. Rewrite the low byte of that existing
            // allocation in place, then hand it to the indexed-output
            // scratch pool. This avoids a second full-sized allocation and
            // copy between palette planning and literal GIF emission.
            materialize_quality_indices(histogram_indices, transparent_index, &histogram_to_palette)
        } else {
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
            indexed
        };
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

#[repr(C, align(8))]
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

type QuantizedColorStats = (u64, u8, u8, u8);
type QuantizedColorSplit = (usize, QuantizedColorStats, QuantizedColorStats);

#[cfg(all(test, not(feature = "encode-only")))]
struct QuantizedColorBox {
    colors: Vec<QuantizedColor>,
    weight: u64,
    score: u64,
    red_range: u8,
    green_range: u8,
    blue_range: u8,
}

#[cfg(all(test, not(feature = "encode-only")))]
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
fn quantized_color_stats(colors: &[QuantizedColor]) -> QuantizedColorStats {
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
#[cfg(all(test, not(feature = "encode-only")))]
fn quantized_color_split_stats(
    colors: &[QuantizedColor],
    split: usize,
) -> (QuantizedColorStats, QuantizedColorStats) {
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

thread_local! {
    static REUSABLE_QUANTIZED_COLOR_BOXES:
        std::cell::RefCell<Vec<QuantizedColorArenaBox>> =
        const { std::cell::RefCell::new(Vec::new()) };
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
#[cfg(all(test, not(feature = "encode-only")))]
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
#[cfg(all(test, not(feature = "encode-only")))]
fn weighted_axis_split_index(colors: &mut [QuantizedColor], axis: u8, midpoint: u64) -> usize {
    weighted_axis_split_index_with_stats(colors, axis, midpoint).0
}

#[inline(always)]
fn weighted_axis_split_index_with_stats(
    colors: &mut [QuantizedColor],
    axis: u8,
    midpoint: u64,
) -> QuantizedColorSplit {
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
) -> QuantizedColorSplit {
    let total_len = colors.len();
    if total_len <= 1 {
        let stats = quantized_color_stats(colors);
        return (1.min(total_len), stats, (0, u8::MAX, u8::MAX, u8::MAX));
    }
    // Median-cut splits repeatedly rebuild this byte-axis histogram. Keep the
    // sparse weight slots uninitialized and track which byte values were seen;
    // zeroing 256 u64s for every split is otherwise pure overhead.
    let mut weights = [std::mem::MaybeUninit::<u64>::uninit(); 256];
    let mut seen = [0u64; 4];
    for color in colors.iter() {
        let value = usize::from(quantized_color_axis_const::<AXIS>(color));
        let word = value >> 6;
        let bit = 1u64 << (value & 63);
        if seen[word] & bit == 0 {
            seen[word] |= bit;
            weights[value].write(color.count);
        } else {
            unsafe {
                *weights[value].assume_init_mut() += color.count;
            }
        }
    }
    let target = midpoint.max(1);
    let mut below_weight = 0u64;
    let mut split_axis = 0usize;
    for (value, weight_slot) in weights.iter().enumerate() {
        let word = value >> 6;
        let bit = 1u64 << (value & 63);
        if seen[word] & bit == 0 {
            continue;
        }
        let weight = unsafe { weight_slot.assume_init() };
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
        match value.cmp(&split_axis) {
            std::cmp::Ordering::Less => {
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
            }
            std::cmp::Ordering::Greater => {
                greater_start -= 1;
                colors.swap(scan, greater_start);
                greater_weight += color.count;
                greater_min_red = greater_min_red.min(color.red);
                greater_min_green = greater_min_green.min(color.green);
                greater_min_blue = greater_min_blue.min(color.blue);
                greater_max_red = greater_max_red.max(color.red);
                greater_max_green = greater_max_green.max(color.green);
                greater_max_blue = greater_max_blue.max(color.blue);
            }
            std::cmp::Ordering::Equal => {
                scan += 1;
            }
        }
    }
    let target_in_equal = target - below_weight;
    let mut split = less_end;
    let mut equal_weight = 0u64;
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
    // Accumulate the equal-axis statistics while finding the weighted split.
    // The previous two-pass form walked this potentially large middle region
    // once to find the boundary and again to rebuild both child bounds.
    let split_limit = greater_start.min(total_len - 1);
    while split < split_limit && equal_weight < target_in_equal {
        let color = colors[split];
        equal_weight += color.count;
        left_equal_weight += color.count;
        left_equal_min_red = left_equal_min_red.min(color.red);
        left_equal_min_green = left_equal_min_green.min(color.green);
        left_equal_min_blue = left_equal_min_blue.min(color.blue);
        left_equal_max_red = left_equal_max_red.max(color.red);
        left_equal_max_green = left_equal_max_green.max(color.green);
        left_equal_max_blue = left_equal_max_blue.max(color.blue);
        split += 1;
    }
    for color in &colors[split..greater_start] {
        right_equal_weight += color.count;
        right_equal_min_red = right_equal_min_red.min(color.red);
        right_equal_min_green = right_equal_min_green.min(color.green);
        right_equal_min_blue = right_equal_min_blue.min(color.blue);
        right_equal_max_red = right_equal_max_red.max(color.red);
        right_equal_max_green = right_equal_max_green.max(color.green);
        right_equal_max_blue = right_equal_max_blue.max(color.blue);
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
fn materialize_quality_indices(
    histogram_indices: Vec<u16>,
    transparent_index: Option<u8>,
    histogram_to_palette: &[u8],
) -> Vec<u8> {
    let mut indexed = take_quantized_indexed(histogram_indices.len());
    match transparent_index {
        Some(transparent_index) => {
            for (destination, histogram_index) in
                indexed.iter_mut().zip(histogram_indices.iter().copied())
            {
                *destination = if histogram_index == u16::MAX {
                    transparent_index
                } else {
                    histogram_to_palette[usize::from(histogram_index)]
                };
            }
        }
        None => {
            for (destination, histogram_index) in
                indexed.iter_mut().zip(histogram_indices.iter().copied())
            {
                *destination = histogram_to_palette[usize::from(histogram_index)];
            }
        }
    }
    recycle_quality_histogram_indices(histogram_indices);
    indexed
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
    if colors.capacity() < histogram.len() {
        colors.reserve(histogram.len() - colors.capacity());
    }
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

#[inline(always)]
fn rounded_weighted_average_u32(sum: u32, count: u32) -> u8 {
    let half = count / 2;
    match sum.checked_add(half) {
        Some(adjusted) => (adjusted / count) as u8,
        None => ((u64::from(sum) + u64::from(half)) / u64::from(count)) as u8,
    }
}

fn quality_colors_from_histogram_u64(histogram: &[RgbHistogramBin]) -> Vec<QuantizedColor> {
    let mut colors = take_quality_colors();
    if colors.capacity() < histogram.len() {
        colors.reserve(histogram.len() - colors.capacity());
    }
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

fn take_quality_palette(capacity: usize) -> Vec<u32> {
    REUSABLE_QUALITY_PALETTE.with(|scratch| {
        let mut palette = std::mem::take(&mut *scratch.borrow_mut());
        palette.clear();
        if palette.capacity() < capacity {
            palette.reserve(capacity - palette.capacity());
        }
        palette
    })
}

fn recycle_quality_palette(mut palette: Vec<u32>) {
    palette.clear();
    REUSABLE_QUALITY_PALETTE.with(|scratch| {
        *scratch.borrow_mut() = palette;
    });
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

fn index_rgba_frames_quality_result(rgba_stream: &[u8], alpha_threshold: u8) -> QualityIndexResult {
    let pixel_count = rgba_stream.len() / 4;
    if pixel_count <= QUALITY_LOW_RES_PIXEL_LIMIT {
        let all_opaque = alpha_threshold == 0
            || (rgba_stream_samples_opaque(rgba_stream, alpha_threshold)
                && !rgba_stream_has_transparent_pixels(rgba_stream, alpha_threshold));
        return if all_opaque {
            index_rgba_frames_quality_low_res::<false>(rgba_stream, alpha_threshold)
        } else {
            index_rgba_frames_quality_low_res::<true>(rgba_stream, alpha_threshold)
        };
    }
    // Inputs that fit the u32 histogram use an adaptive opaque probe inside
    // the histogram pass, avoiding a separate full alpha scan for the common
    // all-255 case. The wide u64 path retains the cheap SIMD preflight because
    // it already processes the stream in separate bounded chunks.
    let all_opaque = alpha_threshold == 0
        || (pixel_count > QUALITY_U32_PIXEL_LIMIT
            && !rgba_stream_has_transparent_pixels(rgba_stream, alpha_threshold));
    index_rgba_frames_quality_high_res(rgba_stream, alpha_threshold, all_opaque)
}

fn index_rgba_frames_quality_low_res<const HAS_TRANSPARENT: bool>(
    rgba_stream: &[u8],
    alpha_threshold: u8,
) -> QualityIndexResult {
    const HISTOGRAM_BITS: usize = 4;
    const HISTOGRAM_LEN: usize = 1 << (HISTOGRAM_BITS * 3);
    let pixel_count = rgba_stream.len() / 4;
    let mut table = take_quality_color_index_table(COLOR_INDEX_CAP);
    let mut palette = take_quality_palette(256);
    // Delay histogram updates until the exact-color probe overflows. Exact
    // inputs are common for flat/illustrated frames, and their histogram is
    // discarded immediately; overflowing inputs replay only the tiny prefix
    // that was already scanned before the 257th color was discovered.
    // Keep the exact-prefix indices so <=256-color inputs do not require a
    // second full RGBA scan after the palette decision is known.
    // The indexed stream is needed only if the exact-color probe succeeds.
    // Start with a small prefix buffer so the common quantized path does not
    // reserve the entire RGBA image just to discard the prefix after overflow.
    let mut indexed = take_quality_probe_indices(pixel_count.min(4_096));
    let mut has_transparent_pixels = false;
    let rgba_pointer = rgba_stream.as_ptr();
    let mut overflow_offset = None;
    for pixel_index in 0..pixel_count {
        if pixel_index == 4_096 {
            indexed.reserve(pixel_count.saturating_sub(indexed.len()));
        }
        let packed = u32::from_le(unsafe {
            std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
        });
        let alpha = (packed >> 24) as u8;
        if HAS_TRANSPARENT && alpha < alpha_threshold {
            has_transparent_pixels = true;
            indexed.push(u8::MAX);
            if palette.len() == 256 {
                overflow_offset = Some((pixel_index + 1) * 4);
                break;
            }
            continue;
        }
        let rgb = rgb_key(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8);
        if let Some(index) = table.get(rgb) {
            indexed.push(index);
            continue;
        }
        let color_limit = if HAS_TRANSPARENT && has_transparent_pixels {
            255
        } else {
            256
        };
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
        recycle_quantized_indexed(indexed);
        // Rebuild the histogram cells for the exact prefix and finish only
        // the suffix after the palette limit was exceeded.
        let mut histogram = take_quality_histogram_u32(HISTOGRAM_LEN);
        let mut prefix_offset = 0usize;
        while prefix_offset < start_offset {
            let packed = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(prefix_offset).cast())
            });
            let transparent = HAS_TRANSPARENT && ((packed >> 24) as u8) < alpha_threshold;
            if !transparent {
                let index = quality_histogram_index_packed::<HISTOGRAM_BITS>(packed);
                add_quality_histogram_u32_bits_indexed(&mut histogram, packed, index);
            }
            prefix_offset += 4;
        }
        if HAS_TRANSPARENT {
            has_transparent_pixels |=
                accumulate_quality_histogram_u32_bits_remaining::<HISTOGRAM_BITS, false>(
                    &mut histogram,
                    rgba_stream,
                    start_offset,
                    alpha_threshold,
                    &mut [],
                );
        } else {
            accumulate_quality_histogram_u32_bits_remaining_opaque::<HISTOGRAM_BITS, false, false>(
                &mut histogram,
                rgba_stream,
                start_offset,
                &mut [],
            );
        }
        let colors = quality_colors_from_histogram_u32::<true>(&histogram);
        recycle_quality_color_index_table(table);
        recycle_quality_histogram_u32(histogram);
        let plan = build_quality_index_plan_from_colors(
            rgba_stream,
            alpha_threshold,
            has_transparent_pixels,
            colors,
            HISTOGRAM_BITS,
            None,
        );
        QualityIndexResult::Quantized(plan)
    } else {
        QualityIndexResult::Exact(finish_quality_exact_indexed(
            rgba_stream,
            alpha_threshold,
            has_transparent_pixels,
            palette,
            table,
            indexed,
        ))
    }
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
    for (pixel_index, index) in indexed.iter_mut().enumerate() {
        let packed = u32::from_le(unsafe {
            std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
        });
        *index = if ((packed >> 24) as u8) < alpha_threshold {
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
    all_opaque: bool,
) -> QualityIndexResult {
    let pixel_count = rgba_stream.len() / 4;
    if pixel_count <= QUALITY_U32_PIXEL_LIMIT {
        if quality_prefers_high_precision_histogram(rgba_stream, alpha_threshold) {
            index_rgba_frames_quality_u32::<5>(rgba_stream, alpha_threshold, all_opaque)
        } else {
            index_rgba_frames_quality_u32::<4>(rgba_stream, alpha_threshold, all_opaque)
        }
    } else {
        index_rgba_frames_quality_u64(rgba_stream, alpha_threshold, all_opaque)
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

fn take_quality_histogram_indices(pixel_count: usize) -> Vec<u16> {
    REUSABLE_QUANTIZED_INDEXED.with(|scratch| {
        let mut indices = std::mem::take(&mut *scratch.borrow_mut());
        if indices.capacity() >= pixel_count {
            // Both histogram accumulation variants write every cell before
            // the indexed stream is read, so the common-size scratch range
            // does not need a redundant zeroing pass.
            unsafe { indices.set_len(pixel_count) };
        } else {
            indices.resize(pixel_count, 0);
        }
        indices
    })
}

fn recycle_quality_histogram_indices(indices: Vec<u16>) {
    REUSABLE_QUANTIZED_INDEXED.with(|scratch| {
        *scratch.borrow_mut() = indices;
    });
}

fn take_quantized_indexed(pixel_count: usize) -> Vec<u8> {
    REUSABLE_QUANTIZED_BYTES.with(|scratch| {
        let mut indexed = std::mem::take(&mut *scratch.borrow_mut());
        if indexed.capacity() >= pixel_count {
            // Every caller fills each indexed byte before reading it. The
            // Wasm start hook reserves this common-size range, so only the
            // logical length needs updating on the first real encode.
            unsafe { indexed.set_len(pixel_count) };
        } else {
            indexed.resize(pixel_count, 0);
        }
        indexed
    })
}

fn recycle_quantized_indexed(indexed: Vec<u8>) {
    REUSABLE_QUANTIZED_BYTES.with(|scratch| {
        *scratch.borrow_mut() = indexed;
    });
}

fn take_quality_probe_indices(capacity: usize) -> Vec<u8> {
    REUSABLE_QUANTIZED_BYTES.with(|scratch| {
        let mut indexed = std::mem::take(&mut *scratch.borrow_mut());
        indexed.clear();
        if indexed.capacity() < capacity {
            indexed.reserve(capacity - indexed.capacity());
        }
        indexed
    })
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
fn quality_histogram_index_pair_packed<const BITS: usize>(packed: u64) -> (usize, usize) {
    if BITS == 5 {
        let indices = ((packed << 7) & 0x0000_7c00_0000_7c00)
            | ((packed >> 6) & 0x0000_03e0_0000_03e0)
            | ((packed >> 19) & 0x0000_001f_0000_001f);
        (indices as u32 as usize, (indices >> 32) as u32 as usize)
    } else if BITS == 4 {
        let indices = ((packed << 4) & 0x0000_0f00_0000_0f00)
            | ((packed >> 8) & 0x0000_00f0_0000_00f0)
            | ((packed >> 20) & 0x0000_000f_0000_000f);
        (indices as u32 as usize, (indices >> 32) as u32 as usize)
    } else {
        let packed0 = packed as u32;
        let packed1 = (packed >> 32) as u32;
        (
            quality_histogram_index_packed::<BITS>(packed0),
            quality_histogram_index_packed::<BITS>(packed1),
        )
    }
}

#[inline(always)]
unsafe fn read_rgba_pair(pointer: *const u8, offset: usize) -> (u32, u32) {
    let packed = u64::from_le(std::ptr::read_unaligned(pointer.add(offset).cast::<u64>()));
    (packed as u32, (packed >> 32) as u32)
}

// Histogram updates hit a pseudo-random bin for each pixel. Keep each update
// as two adjacent packed-field operations; a SIMD load/modify/store for a
// scattered bin is still slower on the ARM64 Wasm runtime for the common
// split-bin case.
#[inline(always)]
unsafe fn add_quality_histogram_bin32(
    bin: *mut RgbHistogramBin32,
    count: u32,
    red: u32,
    green: u32,
    blue: u32,
) {
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    {
        use core::arch::wasm32::{u32x4, u32x4_add, v128_load, v128_store};

        let current = v128_load(bin.cast());
        let increment = u32x4(count, red, green, blue);
        v128_store(bin.cast(), u32x4_add(current, increment));
        return;
    }
    #[cfg(all(
        not(all(target_arch = "wasm32", target_feature = "simd128")),
        target_endian = "little"
    ))]
    {
        let packed = bin.cast::<u64>();
        // The u32 pixel limit keeps every field below 2^32, so adding the
        // packed halves cannot carry from one field into the next.
        let count_red =
            std::ptr::read(packed).wrapping_add(u64::from(count) | (u64::from(red) << 32));
        let green_blue =
            std::ptr::read(packed.add(1)).wrapping_add(u64::from(green) | (u64::from(blue) << 32));
        std::ptr::write(packed, count_red);
        std::ptr::write(packed.add(1), green_blue);
    }
    #[cfg(all(
        not(all(target_arch = "wasm32", target_feature = "simd128")),
        not(target_endian = "little")
    ))]
    {
        let bin = &mut *bin;
        bin.count = bin.count.wrapping_add(count);
        bin.red = bin.red.wrapping_add(red);
        bin.green = bin.green.wrapping_add(green);
        bin.blue = bin.blue.wrapping_add(blue);
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
    unsafe {
        add_quality_histogram_bin32(
            histogram.as_mut_ptr().add(histogram_index),
            1,
            u32::from(red),
            u32::from(green),
            u32::from(blue),
        );
    }
}

#[inline(always)]
fn add_quality_histogram_u32_bits_indexed(
    histogram: &mut [RgbHistogramBin32],
    packed: u32,
    histogram_index: usize,
) {
    let red = u32::from(packed as u8);
    let green = u32::from((packed >> 8) as u8);
    let blue = u32::from((packed >> 16) as u8);
    unsafe {
        add_quality_histogram_bin32(
            histogram.as_mut_ptr().add(histogram_index),
            1,
            red,
            green,
            blue,
        );
    }
}

#[inline(always)]
fn add_quality_histogram_u32_pair_preindexed(
    histogram: &mut [RgbHistogramBin32],
    packed0: u32,
    packed1: u32,
    index0: usize,
    index1: usize,
) {
    if index0 != index1 {
        add_quality_histogram_u32_bits_indexed(histogram, packed0, index0);
        add_quality_histogram_u32_bits_indexed(histogram, packed1, index1);
        return;
    }
    let red = u32::from(packed0 as u8) + u32::from(packed1 as u8);
    let green = u32::from((packed0 >> 8) as u8) + u32::from((packed1 >> 8) as u8);
    let blue = u32::from((packed0 >> 16) as u8) + u32::from((packed1 >> 16) as u8);
    unsafe {
        add_quality_histogram_bin32(histogram.as_mut_ptr().add(index0), 2, red, green, blue);
    }
}

#[inline(always)]
fn add_quality_histogram_u32_pair<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    packed0: u32,
    packed1: u32,
) {
    let index0 = quality_histogram_index_packed::<BITS>(packed0);
    let index1 = quality_histogram_index_packed::<BITS>(packed1);
    add_quality_histogram_u32_pair_indexed::<BITS>(histogram, packed0, packed1, index0, index1);
}

#[inline(always)]
fn add_quality_histogram_u32_pair_packed<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    packed: u64,
) {
    let packed0 = packed as u32;
    let packed1 = (packed >> 32) as u32;
    let (index0, index1) = quality_histogram_index_pair_packed::<BITS>(packed);
    add_quality_histogram_u32_pair_indexed::<BITS>(histogram, packed0, packed1, index0, index1);
}

#[inline(always)]
fn add_quality_histogram_u32_pair_indexed<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    packed0: u32,
    packed1: u32,
    index0: usize,
    index1: usize,
) {
    if index0 != index1 {
        // The caller already decoded both histogram cells. Reuse those
        // indices instead of recomputing the packed shifts on the split-bin
        // path, which is the common case for noisy/photographic input.
        add_quality_histogram_u32_bits_indexed(histogram, packed0, index0);
        add_quality_histogram_u32_bits_indexed(histogram, packed1, index1);
        return;
    }
    let red = u32::from(packed0 as u8) + u32::from(packed1 as u8);
    let green = u32::from((packed0 >> 8) as u8) + u32::from((packed1 >> 8) as u8);
    let blue = u32::from((packed0 >> 16) as u8) + u32::from((packed1 >> 16) as u8);
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    unsafe {
        use core::arch::wasm32::{u32x4, u32x4_add, v128_load, v128_store};

        let bin = histogram.get_unchecked_mut(index0) as *mut RgbHistogramBin32;
        let current = v128_load(bin.cast());
        let increment = u32x4(2, red, green, blue);
        v128_store(bin.cast(), u32x4_add(current, increment));
    }
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    {
        unsafe {
            add_quality_histogram_bin32(histogram.as_mut_ptr().add(index0), 2, red, green, blue);
        }
    }
}

#[inline(always)]
fn add_quality_histogram_u32_pair_with_alpha<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    packed0: u32,
    packed1: u32,
    alpha_threshold: u8,
) -> bool {
    let first_transparent = ((packed0 >> 24) as u8) < alpha_threshold;
    let second_transparent = ((packed1 >> 24) as u8) < alpha_threshold;
    if first_transparent {
        if !second_transparent {
            add_quality_histogram_u32_bits_const::<BITS>(histogram, packed1);
        }
        true
    } else if second_transparent {
        add_quality_histogram_u32_bits_const::<BITS>(histogram, packed0);
        true
    } else {
        add_quality_histogram_u32_pair::<BITS>(histogram, packed0, packed1);
        false
    }
}

#[inline(always)]
fn add_quality_histogram_u32_pair_with_alpha_record<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    histogram_indices: *mut u16,
    pixel_index: usize,
    packed0: u32,
    packed1: u32,
    alpha_threshold: u8,
) -> bool {
    let first_transparent = ((packed0 >> 24) as u8) < alpha_threshold;
    let second_transparent = ((packed1 >> 24) as u8) < alpha_threshold;
    let index0 = if first_transparent {
        u16::MAX
    } else {
        quality_histogram_index_packed::<BITS>(packed0) as u16
    };
    let index1 = if second_transparent {
        u16::MAX
    } else {
        quality_histogram_index_packed::<BITS>(packed1) as u16
    };
    let packed_indices = u32::from_ne_bytes([
        index0 as u8,
        (index0 >> 8) as u8,
        index1 as u8,
        (index1 >> 8) as u8,
    ]);
    unsafe {
        std::ptr::write_unaligned(histogram_indices.add(pixel_index).cast(), packed_indices);
    }
    if !first_transparent && !second_transparent {
        add_quality_histogram_u32_pair_preindexed(
            histogram,
            packed0,
            packed1,
            usize::from(index0),
            usize::from(index1),
        );
    } else {
        if !first_transparent {
            add_quality_histogram_u32_bits_indexed(histogram, packed0, usize::from(index0));
        }
        if !second_transparent {
            add_quality_histogram_u32_bits_indexed(histogram, packed1, usize::from(index1));
        }
    }
    first_transparent || second_transparent
}

#[inline(always)]
fn add_quality_histogram_u32_pair_record_opaque<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    histogram_indices: *mut u16,
    pixel_index: usize,
    packed: u64,
) {
    let packed0 = packed as u32;
    let packed1 = (packed >> 32) as u32;
    let (index0, index1) = quality_histogram_index_pair_packed::<BITS>(packed);
    let index0 = index0 as u16;
    let index1 = index1 as u16;
    let packed_indices = u32::from_ne_bytes([
        index0 as u8,
        (index0 >> 8) as u8,
        index1 as u8,
        (index1 >> 8) as u8,
    ]);
    unsafe {
        std::ptr::write_unaligned(histogram_indices.add(pixel_index).cast(), packed_indices);
    }
    add_quality_histogram_u32_pair_preindexed(
        histogram,
        packed0,
        packed1,
        usize::from(index0),
        usize::from(index1),
    );
}

#[inline(never)]
fn accumulate_quality_histogram_u32_bits_remaining_mixed<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    rgba_stream: &[u8],
    start_offset: usize,
    alpha_threshold: u8,
) -> bool {
    let rgba_pointer = rgba_stream.as_ptr();
    let mut has_transparent_pixels = false;
    let mut offset = start_offset;
    while offset + 32 <= rgba_stream.len() {
        let (packed0, packed1) = unsafe { read_rgba_pair(rgba_pointer, offset) };
        let (packed2, packed3) = unsafe { read_rgba_pair(rgba_pointer, offset + 8) };
        let (packed4, packed5) = unsafe { read_rgba_pair(rgba_pointer, offset + 16) };
        let (packed6, packed7) = unsafe { read_rgba_pair(rgba_pointer, offset + 24) };
        has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha::<BITS>(
            histogram,
            packed0,
            packed1,
            alpha_threshold,
        );
        has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha::<BITS>(
            histogram,
            packed2,
            packed3,
            alpha_threshold,
        );
        has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha::<BITS>(
            histogram,
            packed4,
            packed5,
            alpha_threshold,
        );
        has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha::<BITS>(
            histogram,
            packed6,
            packed7,
            alpha_threshold,
        );
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

fn accumulate_quality_histogram_u32_bits_remaining<
    const BITS: usize,
    const RECORD_INDICES: bool,
>(
    histogram: &mut [RgbHistogramBin32],
    rgba_stream: &[u8],
    start_offset: usize,
    alpha_threshold: u8,
    histogram_indices: &mut [u16],
) -> bool {
    if alpha_threshold == 0 {
        accumulate_quality_histogram_u32_bits_remaining_opaque::<BITS, RECORD_INDICES, false>(
            histogram,
            rgba_stream,
            start_offset,
            histogram_indices,
        );
        return false;
    }
    if BITS == 5 && !RECORD_INDICES {
        return accumulate_quality_histogram_u32_bits_remaining_mixed::<BITS>(
            histogram,
            rgba_stream,
            start_offset,
            alpha_threshold,
        );
    }
    let rgba_pointer = rgba_stream.as_ptr();
    let histogram_indices_pointer = histogram_indices.as_mut_ptr();
    let mut has_transparent_pixels = false;
    let mut offset = start_offset;
    // Keep sixteen packed pixels in flight. This is the fallback scan for the
    // normal-sized quality path (4-bit histogram), so avoiding a loop branch
    // and repeated offset arithmetic here matters more than the tiny prefix
    // scan that discovers the palette overflow.
    while offset + 64 <= rgba_stream.len() {
        let (packed0, packed1) = unsafe { read_rgba_pair(rgba_pointer, offset) };
        let (packed2, packed3) = unsafe { read_rgba_pair(rgba_pointer, offset + 8) };
        let (packed4, packed5) = unsafe { read_rgba_pair(rgba_pointer, offset + 16) };
        let (packed6, packed7) = unsafe { read_rgba_pair(rgba_pointer, offset + 24) };
        if RECORD_INDICES {
            let pixel_index = offset / 4;
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha_record::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index,
                packed0,
                packed1,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha_record::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index + 2,
                packed2,
                packed3,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha_record::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index + 4,
                packed4,
                packed5,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha_record::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index + 6,
                packed6,
                packed7,
                alpha_threshold,
            );
        } else {
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha::<BITS>(
                histogram,
                packed0,
                packed1,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha::<BITS>(
                histogram,
                packed2,
                packed3,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha::<BITS>(
                histogram,
                packed4,
                packed5,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha::<BITS>(
                histogram,
                packed6,
                packed7,
                alpha_threshold,
            );
        }
        let (packed8, packed9) = unsafe { read_rgba_pair(rgba_pointer, offset + 32) };
        let (packed10, packed11) = unsafe { read_rgba_pair(rgba_pointer, offset + 40) };
        let (packed12, packed13) = unsafe { read_rgba_pair(rgba_pointer, offset + 48) };
        let (packed14, packed15) = unsafe { read_rgba_pair(rgba_pointer, offset + 56) };
        if RECORD_INDICES {
            let pixel_index = offset / 4 + 8;
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha_record::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index,
                packed8,
                packed9,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha_record::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index + 2,
                packed10,
                packed11,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha_record::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index + 4,
                packed12,
                packed13,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha_record::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index + 6,
                packed14,
                packed15,
                alpha_threshold,
            );
        } else {
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha::<BITS>(
                histogram,
                packed8,
                packed9,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha::<BITS>(
                histogram,
                packed10,
                packed11,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha::<BITS>(
                histogram,
                packed12,
                packed13,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_with_alpha::<BITS>(
                histogram,
                packed14,
                packed15,
                alpha_threshold,
            );
        }
        offset += 64;
    }
    while offset < rgba_stream.len() {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        if RECORD_INDICES {
            let pixel_index = offset / 4;
            let transparent = ((packed >> 24) as u8) < alpha_threshold;
            let index = if transparent {
                u16::MAX
            } else {
                let index = quality_histogram_index_packed::<BITS>(packed) as u16;
                add_quality_histogram_u32_bits_indexed(histogram, packed, usize::from(index));
                index
            };
            unsafe { histogram_indices_pointer.add(pixel_index).write(index) };
            has_transparent_pixels |= transparent;
        } else if ((packed >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u32_bits_const::<BITS>(histogram, packed);
        }
        offset += 4;
    }
    has_transparent_pixels
}

#[inline(always)]
fn accumulate_quality_histogram_u32_bits_remaining_opaque<
    const BITS: usize,
    const RECORD_INDICES: bool,
    const PROBE_ALPHA: bool,
>(
    histogram: &mut [RgbHistogramBin32],
    rgba_stream: &[u8],
    start_offset: usize,
    histogram_indices: &mut [u16],
) -> bool {
    let rgba_pointer = rgba_stream.as_ptr();
    let histogram_indices_pointer = histogram_indices.as_mut_ptr();
    const ALPHA_MASK: u64 = 0xff00_0000_ff00_0000;
    let mut all_alpha_255 = true;
    let mut offset = start_offset;
    while offset + 64 <= rgba_stream.len() {
        let packed01 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        let packed23 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 8).cast()) });
        let packed45 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 16).cast()) });
        let packed67 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 24).cast()) });
        if RECORD_INDICES {
            let pixel_index = offset / 4;
            add_quality_histogram_u32_pair_record_opaque::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index,
                packed01,
            );
            add_quality_histogram_u32_pair_record_opaque::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index + 2,
                packed23,
            );
            add_quality_histogram_u32_pair_record_opaque::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index + 4,
                packed45,
            );
            add_quality_histogram_u32_pair_record_opaque::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index + 6,
                packed67,
            );
        } else {
            add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed01);
            add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed23);
            add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed45);
            add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed67);
        }
        let packed89 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 32).cast()) });
        let packed1011 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 40).cast()) });
        let packed1213 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 48).cast()) });
        let packed1415 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 56).cast()) });
        if PROBE_ALPHA {
            all_alpha_255 &= packed01 & ALPHA_MASK == ALPHA_MASK;
            all_alpha_255 &= packed23 & ALPHA_MASK == ALPHA_MASK;
            all_alpha_255 &= packed45 & ALPHA_MASK == ALPHA_MASK;
            all_alpha_255 &= packed67 & ALPHA_MASK == ALPHA_MASK;
            all_alpha_255 &= packed89 & ALPHA_MASK == ALPHA_MASK;
            all_alpha_255 &= packed1011 & ALPHA_MASK == ALPHA_MASK;
            all_alpha_255 &= packed1213 & ALPHA_MASK == ALPHA_MASK;
            all_alpha_255 &= packed1415 & ALPHA_MASK == ALPHA_MASK;
        }
        if RECORD_INDICES {
            let pixel_index = offset / 4 + 8;
            add_quality_histogram_u32_pair_record_opaque::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index,
                packed89,
            );
            add_quality_histogram_u32_pair_record_opaque::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index + 2,
                packed1011,
            );
            add_quality_histogram_u32_pair_record_opaque::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index + 4,
                packed1213,
            );
            add_quality_histogram_u32_pair_record_opaque::<BITS>(
                histogram,
                histogram_indices_pointer,
                pixel_index + 6,
                packed1415,
            );
        } else {
            add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed89);
            add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed1011);
            add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed1213);
            add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed1415);
        }
        offset += 64;
    }
    while offset < rgba_stream.len() {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        if PROBE_ALPHA && (packed >> 24) as u8 != u8::MAX {
            all_alpha_255 = false;
        }
        if RECORD_INDICES {
            let index = quality_histogram_index_packed::<BITS>(packed) as u16;
            unsafe { histogram_indices_pointer.add(offset / 4).write(index) };
            add_quality_histogram_u32_bits_indexed(histogram, packed, usize::from(index));
        } else {
            add_quality_histogram_u32_bits_const::<BITS>(histogram, packed);
        }
        offset += 4;
    }
    all_alpha_255
}

fn index_rgba_frames_quality_u32<const BITS: usize>(
    rgba_stream: &[u8],
    alpha_threshold: u8,
    all_opaque: bool,
) -> QualityIndexResult {
    let pixel_count = rgba_stream.len() / 4;
    let mut table = take_quality_color_index_table(COLOR_INDEX_CAP);
    let mut palette = take_quality_palette(256);
    // Quantized inputs discard the exact-prefix indices, so avoid reserving a
    // full-image buffer before the palette decision is known.
    let mut indexed = take_quality_probe_indices(pixel_count.min(4_096));
    let mut has_transparent_pixels = false;
    let rgba_pointer = rgba_stream.as_ptr();
    let mut overflow_offset = None;
    for pixel_index in 0..pixel_count {
        if pixel_index == 4_096 {
            indexed.reserve(pixel_count.saturating_sub(indexed.len()));
        }
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

    if let Some(start_offset) = overflow_offset {
        // Keep the exact-prefix fast path allocation-free. Once overflow is
        // known, rebuild only that prefix and scan the remaining pixels once.
        let mut histogram = take_quality_histogram_u32(1 << (BITS * 3));
        // Opaque 4-bit inputs can reuse the retained cells during literal
        // emission without checking alpha or recomputing the cell index.
        // Mixed-alpha inputs pay less by mapping directly from RGBA while
        // the histogram is already being built, so retain cells only for the
        // branch that can consume them most cheaply.
        let probe_opaque =
            !all_opaque && alpha_threshold != 0 && rgba_stream_samples_alpha_255(rgba_stream);
        let mut record_histogram_indices = all_opaque && BITS == 4;
        let mut histogram_indices = if record_histogram_indices {
            take_quality_histogram_indices(pixel_count)
        } else {
            Vec::new()
        };
        if probe_opaque {
            let all_alpha_255 = if BITS == 4 {
                histogram_indices = take_quality_histogram_indices(pixel_count);
                let all_alpha_255 = accumulate_quality_histogram_u32_bits_remaining_opaque::<
                    BITS,
                    true,
                    true,
                >(
                    &mut histogram, rgba_stream, 0, &mut histogram_indices
                );
                if all_alpha_255 {
                    record_histogram_indices = true;
                }
                all_alpha_255
            } else {
                accumulate_quality_histogram_u32_bits_remaining_opaque::<BITS, false, true>(
                    &mut histogram,
                    rgba_stream,
                    0,
                    &mut [],
                )
            };
            if !all_alpha_255 {
                if !histogram_indices.is_empty() {
                    recycle_quality_histogram_indices(histogram_indices);
                    histogram_indices = Vec::new();
                }
                histogram.fill(RgbHistogramBin32::default());
                has_transparent_pixels = accumulate_quality_histogram_u32_bits_remaining::<
                    BITS,
                    false,
                >(
                    &mut histogram, rgba_stream, 0, alpha_threshold, &mut []
                );
            }
        } else {
            let mut prefix_offset = 0usize;
            let rgba_pointer = rgba_stream.as_ptr();
            while prefix_offset < start_offset {
                let packed = u32::from_le(unsafe {
                    std::ptr::read_unaligned(rgba_pointer.add(prefix_offset).cast())
                });
                if ((packed >> 24) as u8) >= alpha_threshold {
                    add_quality_histogram_u32_bits_const::<BITS>(&mut histogram, packed);
                }
                if record_histogram_indices {
                    histogram_indices[prefix_offset / 4] =
                        quality_histogram_index_packed::<BITS>(packed) as u16;
                }
                prefix_offset += 4;
            }
            if all_opaque {
                if record_histogram_indices {
                    accumulate_quality_histogram_u32_bits_remaining_opaque::<BITS, true, false>(
                        &mut histogram,
                        rgba_stream,
                        start_offset,
                        &mut histogram_indices,
                    );
                } else {
                    accumulate_quality_histogram_u32_bits_remaining_opaque::<BITS, false, false>(
                        &mut histogram,
                        rgba_stream,
                        start_offset,
                        &mut [],
                    );
                }
            } else {
                has_transparent_pixels |=
                    accumulate_quality_histogram_u32_bits_remaining::<BITS, false>(
                        &mut histogram,
                        rgba_stream,
                        start_offset,
                        alpha_threshold,
                        &mut [],
                    );
            }
        }
        recycle_quantized_indexed(indexed);

        let colors = quality_colors_from_histogram_u32::<true>(&histogram);
        recycle_quality_color_index_table(table);
        recycle_quality_histogram_u32(histogram);
        let plan = build_quality_index_plan_from_colors(
            rgba_stream,
            alpha_threshold,
            has_transparent_pixels,
            colors,
            BITS,
            record_histogram_indices.then_some(histogram_indices),
        );
        QualityIndexResult::Quantized(plan)
    } else {
        QualityIndexResult::Exact(finish_quality_exact_indexed(
            rgba_stream,
            alpha_threshold,
            has_transparent_pixels,
            palette,
            table,
            indexed,
        ))
    }
}

fn index_rgba_frames_quality_u64(
    rgba_stream: &[u8],
    alpha_threshold: u8,
    all_opaque: bool,
) -> QualityIndexResult {
    let pixel_count = rgba_stream.len() / 4;
    let mut table = take_quality_color_index_table(COLOR_INDEX_CAP);
    let mut palette = take_quality_palette(256);
    let mut indexed = take_quality_probe_indices(pixel_count.min(4096));
    // Keep the exact-color probe independent from the large histogram. Exact
    // inputs return their indexed stream directly, so building a histogram
    // that will be discarded would turn a linear scan into needless work.
    // Quantized inputs scan the full stream once for histogram accumulation
    // after the short exact probe has found an overflow.
    let mut has_transparent_pixels = false;
    let rgba_pointer = rgba_stream.as_ptr();
    let mut overflow_offset = None;
    for pixel_index in 0..pixel_count {
        if pixel_index == 4_096 {
            indexed.reserve(pixel_count.saturating_sub(indexed.len()));
        }
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

    if overflow_offset.is_none() {
        return QualityIndexResult::Exact(finish_quality_exact_indexed(
            rgba_stream,
            alpha_threshold,
            has_transparent_pixels,
            palette,
            table,
            indexed,
        ));
    }

    let mut histogram = take_quality_histogram_u64(QUALITY_HISTOGRAM_LEN);
    has_transparent_pixels |= accumulate_quality_histogram_u64_via_u32(
        &mut histogram,
        rgba_stream,
        alpha_threshold,
        all_opaque,
    );

    let colors = quality_colors_from_histogram_u64(&histogram);
    recycle_quality_color_index_table(table);
    recycle_quality_histogram_u64(histogram);
    let plan = build_quality_index_plan_from_colors(
        rgba_stream,
        alpha_threshold,
        has_transparent_pixels,
        colors,
        QUALITY_HISTOGRAM_BITS,
        None,
    );
    QualityIndexResult::Quantized(plan)
}

/// Accumulate a wide histogram in chunks whose channel sums are guaranteed to
/// fit in `u32`. The final bins stay `u64` so arbitrarily large RGBA streams
/// retain the same exact counts and weighted sums as the direct wide scan.
fn accumulate_quality_histogram_u64_via_u32(
    histogram: &mut [RgbHistogramBin],
    rgba_stream: &[u8],
    alpha_threshold: u8,
    all_opaque: bool,
) -> bool {
    let pixel_count = rgba_stream.len() / 4;
    let mut has_transparent_pixels = false;
    let mut pixel_offset = 0usize;
    while pixel_offset < pixel_count {
        let chunk_pixels = (pixel_count - pixel_offset).min(QUALITY_U32_PIXEL_LIMIT);
        let byte_start = pixel_offset * 4;
        let byte_end = byte_start + chunk_pixels * 4;
        let chunk = &rgba_stream[byte_start..byte_end];
        let mut chunk_histogram = take_quality_histogram_u32(QUALITY_HISTOGRAM_LEN);
        if all_opaque {
            accumulate_quality_histogram_u32_bits_remaining_opaque::<5, false, false>(
                &mut chunk_histogram,
                chunk,
                0,
                &mut [],
            );
        } else {
            has_transparent_pixels |= accumulate_quality_histogram_u32_bits_remaining::<5, false>(
                &mut chunk_histogram,
                chunk,
                0,
                alpha_threshold,
                &mut [],
            );
        }
        for (wide, narrow) in histogram.iter_mut().zip(chunk_histogram.iter()) {
            wide.count += u64::from(narrow.count);
            wide.red += u64::from(narrow.red);
            wide.green += u64::from(narrow.green);
            wide.blue += u64::from(narrow.blue);
        }
        recycle_quality_histogram_u32(chunk_histogram);
        pixel_offset += chunk_pixels;
    }
    has_transparent_pixels
}

#[inline(never)]
fn build_quality_index_plan_from_colors(
    rgba_stream: &[u8],
    _alpha_threshold: u8,
    has_transparent_pixels: bool,
    mut colors: Vec<QuantizedColor>,
    histogram_bits: usize,
    histogram_indices: Option<Vec<u16>>,
) -> QualityIndexPlan {
    let mapping_bits = if histogram_bits == 5 && colors.len() > QUALITY_DOMINANT_COLOR_LIMIT {
        4
    } else {
        histogram_bits
    };
    // A dense 5-bit histogram may be mapped through a compact 4-bit parent
    // table. Its retained per-pixel 5-bit cells cannot be indexed into that
    // compact table, so discard them and remap the RGBA stream from the
    // parent-cell plan instead of risking an out-of-bounds lookup.
    let histogram_indices = (mapping_bits == histogram_bits)
        .then_some(histogram_indices)
        .flatten();
    let mapping_len = 1usize << (mapping_bits * 3);
    let opaque_color_limit = if has_transparent_pixels { 255 } else { 256 };

    let (mut palette, histogram_to_palette) = if colors.len() <= QUALITY_DOMINANT_COLOR_LIMIT {
        let palette_len = colors.len().min(opaque_color_limit);
        // The normal arbitrary-image path has only a few thousand occupied
        // histogram bins. Select only the palette prefix, then sort that
        // prefix by the exact count/index key used for palette selection.
        if colors
            .iter()
            .all(|color| color.count < QUANTIZED_SORT_COUNT_LIMIT)
        {
            let order = |color: &QuantizedColor| {
                ((QUANTIZED_SORT_COUNT_LIMIT - 1 - color.count) << 16)
                    | u64::from(color.histogram_index)
            };
            if colors.len() > palette_len {
                colors.select_nth_unstable_by_key(palette_len - 1, order);
                colors[..palette_len].sort_unstable_by_key(order);
            } else {
                colors.sort_unstable_by_key(order);
            }
        } else {
            colors.sort_unstable_by_key(|color| {
                (std::cmp::Reverse(color.count), color.histogram_index)
            });
        }
        let mut palette = Vec::with_capacity(palette_len + usize::from(has_transparent_pixels));
        for color in colors.iter().take(palette_len) {
            palette.push(rgb_key(color.red, color.green, color.blue));
        }
        let mut initial_tree = PaletteKdTree::new(&palette);
        let mut histogram_to_palette = take_quality_histogram_to_palette(mapping_len);
        // Build the exact-color lookup while checking for duplicate palette
        // representatives. The hash table avoids the quadratic prefix scan;
        // keep it alive as reusable scratch even when the compact direct-cell
        // table wins for this palette.
        let mut palette_lookup = take_quality_color_index_table(COLOR_INDEX_CAP);
        let mut duplicate_palette = false;
        for (index, &color) in palette.iter().enumerate() {
            if palette_lookup.get(color).is_some() {
                duplicate_palette = true;
            } else {
                palette_lookup.insert_if_absent(color, index as u8);
            }
        }
        // A histogram cell has exactly one representative color, so a cell
        // occupied by a selected palette entry is an exact lookup. Keep the
        // compact direct table for the transparent/255-color case; a full
        // 256-color table would collide with the `u8::MAX` miss sentinel and
        // costs more to fill than the hash table it replaces.
        let use_direct_palette_cells = mapping_bits == 4 && palette_len < 256 && !duplicate_palette;
        let palette_lookup = if use_direct_palette_cells {
            recycle_quality_color_index_table(palette_lookup);
            ColorIndexTable::empty()
        } else {
            palette_lookup
        };
        if use_direct_palette_cells {
            // Only occupied cells are read below; clear those cells instead
            // of touching the entire 4,096-entry mapping on every encode.
            for color in &colors {
                histogram_to_palette[usize::from(color.histogram_index)] = u8::MAX;
            }
            for (index, color) in colors.iter().take(palette_len).enumerate() {
                histogram_to_palette[usize::from(color.histogram_index)] = index as u8;
            }
        }
        // Map histogram cells in spatial order so the previous cell's exact
        // winner is a useful seed for the next KD search. Palette membership
        // was recorded above, before this reorder, so palette order is fixed.
        if histogram_bits == 4 {
            reorder_quality_colors_by_histogram_index_4(&mut colors);
        } else {
            colors.sort_unstable_by_key(|color| color.histogram_index);
        }
        let mut palette_changed = false;
        let mut previous_hint = None;
        // Every caller that reaches the u32 histogram has already bounded the
        // source pixel count by QUALITY_U32_PIXEL_LIMIT. The u64 histogram is
        // the only path that can exceed it, so use the input length directly
        // instead of summing every occupied color a second time.
        if rgba_stream.len() / 4 <= QUALITY_U32_PIXEL_LIMIT {
            let mut counts = [0u32; 256];
            let mut red_sums = [0u32; 256];
            let mut green_sums = [0u32; 256];
            let mut blue_sums = [0u32; 256];
            for color in &colors {
                let index = if use_direct_palette_cells {
                    let cell = usize::from(color.histogram_index);
                    let candidate = histogram_to_palette[cell];
                    if candidate != u8::MAX {
                        usize::from(candidate)
                    } else {
                        usize::from(match previous_hint {
                            Some((hint_index, hint_color)) => initial_tree
                                .nearest_with_seed_bounds(
                                    color.red,
                                    color.green,
                                    color.blue,
                                    hint_index,
                                    palette_color_distance(
                                        hint_color,
                                        color.red,
                                        color.green,
                                        color.blue,
                                    ),
                                ),
                            None => initial_tree.nearest_with_hint_split(
                                color.red,
                                color.green,
                                color.blue,
                                None,
                            ),
                        })
                    }
                } else {
                    let rgb = rgb_key(color.red, color.green, color.blue);
                    if let Some(index) = palette_lookup.get(rgb) {
                        usize::from(index)
                    } else {
                        usize::from(match previous_hint {
                            Some((hint_index, hint_color)) => initial_tree
                                .nearest_with_seed_bounds(
                                    color.red,
                                    color.green,
                                    color.blue,
                                    hint_index,
                                    palette_color_distance(
                                        hint_color,
                                        color.red,
                                        color.green,
                                        color.blue,
                                    ),
                                ),
                            None => initial_tree.nearest_with_hint_split(
                                color.red,
                                color.green,
                                color.blue,
                                None,
                            ),
                        })
                    }
                };
                previous_hint = Some((index as u8, palette[index]));
                let count = color.count as u32;
                histogram_to_palette[usize::from(color.histogram_index)] = index as u8;
                counts[index] += count;
                red_sums[index] += u32::from(color.red) * count;
                green_sums[index] += u32::from(color.green) * count;
                blue_sums[index] += u32::from(color.blue) * count;
            }
            for index in 0..palette.len() {
                let count = counts[index];
                if count == 0 {
                    continue;
                }
                let representative = rgb_key(
                    rounded_weighted_average_u32(red_sums[index], count),
                    rounded_weighted_average_u32(green_sums[index], count),
                    rounded_weighted_average_u32(blue_sums[index], count),
                );
                let changed = palette[index] != representative;
                palette_changed |= changed;
                palette[index] = representative;
            }
        } else {
            let mut counts = [0u64; 256];
            let mut red_sums = [0u64; 256];
            let mut green_sums = [0u64; 256];
            let mut blue_sums = [0u64; 256];
            for color in &colors {
                let index = if use_direct_palette_cells {
                    let cell = usize::from(color.histogram_index);
                    let candidate = histogram_to_palette[cell];
                    if candidate != u8::MAX {
                        usize::from(candidate)
                    } else {
                        usize::from(match previous_hint {
                            Some((hint_index, hint_color)) => initial_tree
                                .nearest_with_seed_bounds(
                                    color.red,
                                    color.green,
                                    color.blue,
                                    hint_index,
                                    palette_color_distance(
                                        hint_color,
                                        color.red,
                                        color.green,
                                        color.blue,
                                    ),
                                ),
                            None => initial_tree.nearest_with_hint_split(
                                color.red,
                                color.green,
                                color.blue,
                                None,
                            ),
                        })
                    }
                } else {
                    let rgb = rgb_key(color.red, color.green, color.blue);
                    if let Some(index) = palette_lookup.get(rgb) {
                        usize::from(index)
                    } else {
                        usize::from(match previous_hint {
                            Some((hint_index, hint_color)) => initial_tree
                                .nearest_with_seed_bounds(
                                    color.red,
                                    color.green,
                                    color.blue,
                                    hint_index,
                                    palette_color_distance(
                                        hint_color,
                                        color.red,
                                        color.green,
                                        color.blue,
                                    ),
                                ),
                            None => initial_tree.nearest_with_hint_split(
                                color.red,
                                color.green,
                                color.blue,
                                None,
                            ),
                        })
                    }
                };
                previous_hint = Some((index as u8, palette[index]));
                histogram_to_palette[usize::from(color.histogram_index)] = index as u8;
                counts[index] += color.count;
                red_sums[index] += u64::from(color.red) * color.count;
                green_sums[index] += u64::from(color.green) * color.count;
                blue_sums[index] += u64::from(color.blue) * color.count;
            }
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
                let changed = palette[index] != representative;
                palette_changed |= changed;
                palette[index] = representative;
            }
        }
        if palette_changed {
            // Representative updates do not change the palette indices or
            // the tree's membership. Recolor the existing topology instead
            // of selecting every median a second time. Recompute exact
            // subtree bounds so refinement can prune on all three channels.
            initial_tree.recolor(&palette);
            for color in &colors {
                // The first-pass assignment is a much tighter exact seed
                // than a coarse cell-center lookup after representatives move
                // only one refinement step. The KD search still proves the
                // final nearest color, so this changes no output or tie rule.
                let hint_index = histogram_to_palette[usize::from(color.histogram_index)];
                let color_key = rgb_key(color.red, color.green, color.blue);
                let hint_color = palette[usize::from(hint_index)];
                let index = if hint_color == color_key {
                    hint_index
                } else {
                    initial_tree.nearest_with_seed_bounds(
                        color.red,
                        color.green,
                        color.blue,
                        hint_index,
                        palette_color_distance(hint_color, color.red, color.green, color.blue),
                    )
                };
                histogram_to_palette[usize::from(color.histogram_index)] = index;
            }
        }
        if !use_direct_palette_cells {
            recycle_quality_color_index_table(palette_lookup);
        }
        recycle_quality_colors(colors);
        (palette, histogram_to_palette)
    } else {
        let mut colors = colors;
        let mut boxes = REUSABLE_QUANTIZED_COLOR_BOXES.with(|scratch| {
            let mut boxes = std::mem::take(&mut *scratch.borrow_mut());
            boxes.clear();
            boxes.push(QuantizedColorArenaBox::new(0, colors.len(), &colors));
            boxes
        });
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
        let mut requested_cells = [false; 1 << 12];
        for color in &colors {
            let coarse_index = (usize::from(color.red >> 4) << 8)
                | (usize::from(color.green >> 4) << 4)
                | usize::from(color.blue >> 4);
            requested_cells[coarse_index] = true;
        }
        let palette_tree = PaletteKdTree::new(&palette);
        let coarse_nearest =
            palette_tree.coarse_nearest_table_for_cells(&palette, &requested_cells);
        if histogram_bits == 4 {
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
        REUSABLE_QUANTIZED_COLOR_BOXES.with(|scratch| {
            *scratch.borrow_mut() = boxes;
        });
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
        histogram_indices,
        transparent_index,
        histogram_bits,
        mapping_bits,
    }
}

/// Reorder the unique occupied cells of a 4-bit histogram without doing a
/// comparison sort. There is one `QuantizedColor` per occupied cell, and the
/// cell index is bounded to 12 bits, so a small position permutation can place
/// every color in exact histogram-index order in linear time.
#[inline(never)]
fn reorder_quality_colors_by_histogram_index_4(colors: &mut [QuantizedColor]) {
    if colors.is_empty() {
        return;
    }
    let mut positions = [std::mem::MaybeUninit::<u16>::uninit(); 1 << 12];
    let mut seen = [0u64; 1 << 6];
    let mut minimum = usize::MAX;
    let mut maximum = 0usize;
    for (position, color) in colors.iter().enumerate() {
        let histogram_index = usize::from(color.histogram_index);
        positions[histogram_index].write(position as u16);
        seen[histogram_index >> 6] |= 1u64 << (histogram_index & 63);
        minimum = minimum.min(histogram_index);
        maximum = maximum.max(histogram_index);
    }
    let mut target = 0usize;
    for histogram_index in minimum..=maximum {
        if seen[histogram_index >> 6] & (1u64 << (histogram_index & 63)) == 0 {
            continue;
        }
        let position = usize::from(unsafe { positions[histogram_index].assume_init() });
        if position != target {
            let moved_index = usize::from(colors[target].histogram_index);
            colors.swap(target, position);
            positions[histogram_index].write(target as u16);
            positions[moved_index].write(position as u16);
        }
        target += 1;
    }
}

fn try_index_rgba_frames_exact(
    rgba_stream: &[u8],
    alpha_threshold: u8,
    exact_only: bool,
) -> Result<Option<IndexedPalette>, String> {
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
) -> Result<IndexedPalette, String> {
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
    if mapper.palette_rgb.len() <= 16 {
        return map_rgba_frame_to_palette_cached::<16>(rgba_stream, mapper, indexed);
    }
    indexed.clear();
    let pointer = rgba_stream.as_ptr();
    let mut offset = 0usize;
    let mut cached_rgb = u32::MAX;
    let mut cached_index = 0u8;
    while offset < rgba_stream.len() {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(pointer.add(offset).cast::<u32>()) });
        let rgb = packed & 0x00ff_ffff;
        let index = if rgb == cached_rgb {
            cached_index
        } else {
            let index = mapper.index_pixel(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8);
            cached_rgb = rgb;
            cached_index = index;
            index
        };
        indexed.push(index);
        offset += 4;
    }
}

fn map_rgba_frame_to_palette_cached<const CACHE_SIZE: usize>(
    rgba_stream: &[u8],
    mapper: &PaletteMapper<'_>,
    indexed: &mut Vec<u8>,
) {
    indexed.clear();
    let pointer = rgba_stream.as_ptr();
    let mut cache_keys = [u32::MAX; CACHE_SIZE];
    let mut cache_values = [0u8; CACHE_SIZE];
    let cache_mask = CACHE_SIZE - 1;
    let mut offset = 0usize;
    while offset < rgba_stream.len() {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(pointer.add(offset).cast::<u32>()) });
        let rgb = packed & 0x00ff_ffff;
        let slot = (rgb as usize).wrapping_mul(2_654_435_761) & cache_mask;
        let index = if cache_keys[slot] == rgb {
            cache_values[slot]
        } else {
            let index = mapper.index_pixel(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8);
            cache_keys[slot] = rgb;
            cache_values[slot] = index;
            index
        };
        indexed.push(index);
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
    if mapper.palette_rgb.len() <= 16 {
        return map_rgba_rect_to_palette_cached::<16>(
            rgba_stream,
            canvas_width,
            rect,
            mapper,
            indexed,
        );
    }
    indexed.clear();
    let row_skip = (canvas_width - rect.width) * 4;
    let mut offset = (rect.y * canvas_width + rect.x) * 4;
    let pointer = rgba_stream.as_ptr();
    let mut cached_rgb = u32::MAX;
    let mut cached_index = 0u8;
    for _ in 0..rect.height {
        for _ in 0..rect.width {
            let packed = u32::from_le(unsafe {
                std::ptr::read_unaligned(pointer.add(offset).cast::<u32>())
            });
            let rgb = packed & 0x00ff_ffff;
            let index = if rgb == cached_rgb {
                cached_index
            } else {
                let index =
                    mapper.index_pixel(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8);
                cached_rgb = rgb;
                cached_index = index;
                index
            };
            indexed.push(index);
            offset += 4;
        }
        offset += row_skip;
    }
}

fn map_rgba_rect_to_palette_cached<const CACHE_SIZE: usize>(
    rgba_stream: &[u8],
    canvas_width: usize,
    rect: ChangedRectU32,
    mapper: &PaletteMapper<'_>,
    indexed: &mut Vec<u8>,
) {
    indexed.clear();
    let row_skip = (canvas_width - rect.width) * 4;
    let pointer = rgba_stream.as_ptr();
    let mut offset = (rect.y * canvas_width + rect.x) * 4;
    let mut cache_keys = [u32::MAX; CACHE_SIZE];
    let mut cache_values = [0u8; CACHE_SIZE];
    let cache_mask = CACHE_SIZE - 1;
    for _ in 0..rect.height {
        for _ in 0..rect.width {
            let packed = u32::from_le(unsafe {
                std::ptr::read_unaligned(pointer.add(offset).cast::<u32>())
            });
            let rgb = packed & 0x00ff_ffff;
            let slot = (rgb as usize).wrapping_mul(2_654_435_761) & cache_mask;
            let index = if cache_keys[slot] == rgb {
                cache_values[slot]
            } else {
                let index =
                    mapper.index_pixel(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8);
                cache_keys[slot] = rgb;
                cache_values[slot] = index;
                index
            };
            indexed.push(index);
            offset += 4;
        }
        offset += row_skip;
    }
}

#[inline(always)]
fn rgba_stream_is_opaque(rgba_stream: &[u8]) -> bool {
    let pointer = rgba_stream.as_ptr();
    const ALPHA_MASK: u64 = 0xff00_0000_ff00_0000;
    let mut offset = 0usize;
    while offset + 8 <= rgba_stream.len() {
        let packed =
            u64::from_le(unsafe { std::ptr::read_unaligned(pointer.add(offset).cast::<u64>()) });
        if packed & ALPHA_MASK != ALPHA_MASK {
            return false;
        }
        offset += 8;
    }
    while offset < rgba_stream.len() {
        if rgba_stream[offset + 3] != 255 {
            return false;
        }
        offset += 4;
    }
    true
}

#[inline(always)]
fn rgba_stream_has_transparent_pixels(rgba_stream: &[u8], alpha_threshold: u8) -> bool {
    if alpha_threshold == 0 {
        return false;
    }
    let pointer = rgba_stream.as_ptr();
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    {
        use core::arch::wasm32::{u32x4_lt, u32x4_shr, u32x4_splat, v128_any_true, v128_load};

        let threshold = u32x4_splat(u32::from(alpha_threshold));
        let mut offset = 0usize;
        while offset + 16 <= rgba_stream.len() {
            let pixels = unsafe { v128_load(pointer.add(offset).cast()) };
            let alpha = u32x4_shr(pixels, 24);
            if v128_any_true(u32x4_lt(alpha, threshold)) {
                return true;
            }
            offset += 16;
        }
        while offset < rgba_stream.len() {
            if unsafe { *pointer.add(offset + 3) < alpha_threshold } {
                return true;
            }
            offset += 4;
        }
        return false;
    }
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    {
        let mut offset = 0usize;
        while offset + 32 <= rgba_stream.len() {
            if unsafe {
                *pointer.add(offset + 3) < alpha_threshold
                    || *pointer.add(offset + 7) < alpha_threshold
                    || *pointer.add(offset + 11) < alpha_threshold
                    || *pointer.add(offset + 15) < alpha_threshold
                    || *pointer.add(offset + 19) < alpha_threshold
                    || *pointer.add(offset + 23) < alpha_threshold
                    || *pointer.add(offset + 27) < alpha_threshold
                    || *pointer.add(offset + 31) < alpha_threshold
            } {
                return true;
            }
            offset += 32;
        }
        while offset < rgba_stream.len() {
            if unsafe { *pointer.add(offset + 3) < alpha_threshold } {
                return true;
            }
            offset += 4;
        }
        false
    }
}

#[inline(always)]
fn rgba_stream_samples_opaque(rgba_stream: &[u8], alpha_threshold: u8) -> bool {
    if alpha_threshold == 0 {
        return true;
    }
    let pixel_count = rgba_stream.len() / 4;
    if pixel_count == 0 {
        return true;
    }
    const MAX_SAMPLE_COUNT: usize = 32;
    let sample_count = pixel_count.min(MAX_SAMPLE_COUNT);
    let last_pixel = pixel_count - 1;
    let pointer = rgba_stream.as_ptr();
    for sample in 0..sample_count {
        let pixel_index = if sample_count == 1 {
            0
        } else {
            sample * last_pixel / (sample_count - 1)
        };
        if unsafe { *pointer.add(pixel_index * 4 + 3) } < alpha_threshold {
            return false;
        }
    }
    true
}

#[inline(always)]
fn rgba_stream_samples_alpha_255(rgba_stream: &[u8]) -> bool {
    let pixel_count = rgba_stream.len() / 4;
    if pixel_count == 0 {
        return true;
    }
    const MAX_SAMPLE_COUNT: usize = 256;
    let sample_count = pixel_count.min(MAX_SAMPLE_COUNT);
    let last_pixel = pixel_count - 1;
    let pointer = rgba_stream.as_ptr();
    for sample in 0..sample_count {
        let pixel_index = if sample_count == 1 {
            0
        } else {
            sample * last_pixel / (sample_count - 1)
        };
        if unsafe { *pointer.add(pixel_index * 4 + 3) } != u8::MAX {
            return false;
        }
    }
    true
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

#[inline(always)]
fn palette_color_distance(color: u32, r: u8, g: u8, b: u8) -> u32 {
    let dr = i32::from(r) - ((color >> 16) & 0xff) as i32;
    let dg = i32::from(g) - ((color >> 8) & 0xff) as i32;
    let db = i32::from(b) - (color & 0xff) as i32;
    (dr * dr + dg * dg + db * db) as u32
}

#[derive(Clone, Copy)]
struct PaletteKdNode {
    red: u8,
    green: u8,
    blue: u8,
    palette_index: u8,
    axis: u8,
    split: u8,
    min_red: u8,
    min_green: u8,
    min_blue: u8,
    max_red: u8,
    max_green: u8,
    max_blue: u8,
    left: u16,
    right: u16,
}

const PALETTE_KD_EMPTY: u16 = u16::MAX;

struct PaletteKdTree {
    nodes: Vec<PaletteKdNode>,
    root: u16,
}

thread_local! {
    static REUSABLE_PALETTE_KD_NODES: std::cell::RefCell<Vec<PaletteKdNode>> =
        const { std::cell::RefCell::new(Vec::new()) };
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
        ) -> u16 {
            if indices.is_empty() {
                return PALETTE_KD_EMPTY;
            }
            let mut min_red = u8::MAX;
            let mut min_green = u8::MAX;
            let mut min_blue = u8::MAX;
            let mut max_red = 0u8;
            let mut max_green = 0u8;
            let mut max_blue = 0u8;
            let mut sum_red = 0u64;
            let mut sum_green = 0u64;
            let mut sum_blue = 0u64;
            let mut sum_red_squared = 0u64;
            let mut sum_green_squared = 0u64;
            let mut sum_blue_squared = 0u64;
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
                let red = u64::from(red);
                let green = u64::from(green);
                let blue = u64::from(blue);
                sum_red += red;
                sum_green += green;
                sum_blue += blue;
                sum_red_squared += red * red;
                sum_green_squared += green * green;
                sum_blue_squared += blue * blue;
            }
            let count = indices.len() as u64;
            let red_variance = count * sum_red_squared - sum_red * sum_red;
            let green_variance = count * sum_green_squared - sum_green * sum_green;
            let blue_variance = count * sum_blue_squared - sum_blue * sum_blue;
            let axis = if red_variance >= green_variance && red_variance >= blue_variance {
                0
            } else if green_variance >= blue_variance {
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
            let node_index = nodes.len() as u16;
            let color = palette_rgb[palette_index];
            nodes.push(PaletteKdNode {
                red: (color >> 16) as u8,
                green: (color >> 8) as u8,
                blue: color as u8,
                palette_index: palette_index as u8,
                axis,
                split: component(color, axis),
                min_red,
                min_green,
                min_blue,
                max_red,
                max_green,
                max_blue,
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
        let mut nodes = REUSABLE_PALETTE_KD_NODES.with(|scratch| {
            let mut nodes = std::mem::take(&mut *scratch.borrow_mut());
            nodes.clear();
            if nodes.capacity() < palette_rgb.len() {
                nodes.reserve(palette_rgb.len() - nodes.capacity());
            }
            nodes
        });
        let root = build(&mut indices[..palette_rgb.len()], palette_rgb, &mut nodes);
        Self { nodes, root }
    }

    /// Update the colors and exact subtree bounds without rebuilding the
    /// median topology. Palette representatives may move after the weighted
    /// assignment pass, but node membership and palette indices stay fixed.
    /// The retained topology supplies traversal order; bounds are recomputed
    /// from the current colors so pruning remains exact even when a
    /// representative crosses an old split plane.
    fn recolor(&mut self, palette_rgb: &[u32]) {
        fn visit(
            nodes: &mut [PaletteKdNode],
            node_index: u16,
            palette_rgb: &[u32],
        ) -> ([u8; 3], [u8; 3]) {
            let index = usize::from(node_index);
            let (left, right, palette_index, axis) = {
                let node = &nodes[index];
                (node.left, node.right, node.palette_index, node.axis)
            };
            let color = palette_rgb[usize::from(palette_index)];
            let current = [(color >> 16) as u8, (color >> 8) as u8, color as u8];
            let mut min = current;
            let mut max = current;
            if left != PALETTE_KD_EMPTY {
                let bounds = visit(nodes, left, palette_rgb);
                for channel in 0..3 {
                    min[channel] = min[channel].min(bounds.0[channel]);
                    max[channel] = max[channel].max(bounds.1[channel]);
                }
            }
            if right != PALETTE_KD_EMPTY {
                let bounds = visit(nodes, right, palette_rgb);
                for channel in 0..3 {
                    min[channel] = min[channel].min(bounds.0[channel]);
                    max[channel] = max[channel].max(bounds.1[channel]);
                }
            }
            let node = &mut nodes[index];
            node.red = current[0];
            node.green = current[1];
            node.blue = current[2];
            node.split = current[usize::from(axis)];
            node.min_red = min[0];
            node.min_green = min[1];
            node.min_blue = min[2];
            node.max_red = max[0];
            node.max_green = max[1];
            node.max_blue = max[2];
            (min, max)
        }

        if self.root != PALETTE_KD_EMPTY {
            visit(&mut self.nodes, self.root, palette_rgb);
        }
    }

    /// Build the exact 4-bit/channel nearest-color table used by the median
    /// cut mapper. Each cell starts its KD search from the result for an
    /// adjacent cell, which is a much tighter seed than the coarser 3-bit
    /// lookup. Duplicate palette colors keep the original coarse seed so the
    /// zero-distance early exit cannot change the existing lowest-index tie.
    #[cfg(all(test, not(feature = "encode-only")))]
    fn coarse_nearest_table(&self, palette_rgb: &[u32]) -> [u8; 1 << 12] {
        self.coarse_nearest_table_for_cells(palette_rgb, &[true; 1 << 12])
    }

    fn coarse_nearest_table_for_cells(
        &self,
        palette_rgb: &[u32],
        requested_cells: &[bool; 1 << 12],
    ) -> [u8; 1 << 12] {
        let mut requested_coarse_cells = [false; 512];
        for red in 0..16usize {
            for green in 0..16usize {
                for blue in 0..16usize {
                    let index = (red << 8) | (green << 4) | blue;
                    if requested_cells[index] {
                        requested_coarse_cells[(red >> 1) << 6 | (green >> 1) << 3 | (blue >> 1)] =
                            true;
                    }
                }
            }
        }
        let coarse_hints = self.coarse_hint_table_for_cells(palette_rgb, &requested_coarse_cells);
        let mut has_duplicate = false;
        'duplicate_search: for (index, &color) in palette_rgb.iter().enumerate() {
            if palette_rgb[..index].contains(&color) {
                has_duplicate = true;
                break 'duplicate_search;
            }
        }
        let use_neighbor_hints = !has_duplicate;
        let mut table = [0u8; 1 << 12];
        for red in 0..16u8 {
            for green in 0..16u8 {
                for blue in 0..16u8 {
                    let index =
                        (usize::from(red) << 8) | (usize::from(green) << 4) | usize::from(blue);
                    if !requested_cells[index] {
                        continue;
                    }
                    let coarse_index = (usize::from(red >> 1) << 6)
                        | (usize::from(green >> 1) << 3)
                        | usize::from(blue >> 1);
                    let hint_index = if use_neighbor_hints {
                        if blue != 0 && requested_cells[index - 1] {
                            table[index - 1]
                        } else if green != 0 && requested_cells[index - (1 << 4)] {
                            table[index - (1 << 4)]
                        } else if red != 0 && requested_cells[index - (1 << 8)] {
                            table[index - (1 << 8)]
                        } else {
                            coarse_hints[coarse_index]
                        }
                    } else {
                        coarse_hints[coarse_index]
                    };
                    table[index] = self.nearest_with_hint_index_split(
                        (red << 4) | 8,
                        (green << 4) | 8,
                        (blue << 4) | 8,
                        hint_index,
                        palette_rgb[usize::from(hint_index)],
                    );
                }
            }
        }
        table
    }

    fn coarse_hint_table_for_cells(
        &self,
        palette_rgb: &[u32],
        requested_cells: &[bool; 512],
    ) -> [u8; 512] {
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
            if !requested_cells[cell] {
                continue;
            }
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
                    if !requested_cells[index] {
                        continue;
                    }
                    if !occupied[index] {
                        table[index] = self.nearest_with_hint_split(
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

    #[cfg(all(test, not(feature = "encode-only")))]
    #[inline]
    fn nearest(&self, r: u8, g: u8, b: u8) -> u8 {
        self.nearest_with_hint(r, g, b, None)
    }

    #[cfg(all(test, not(feature = "encode-only")))]
    #[inline]
    fn nearest_with_hint(&self, r: u8, g: u8, b: u8, hint: Option<(u8, u32)>) -> u8 {
        self.nearest_with_hint_impl::<true>(r, g, b, hint)
    }

    #[inline(always)]
    fn nearest_with_hint_split(&self, r: u8, g: u8, b: u8, hint: Option<(u8, u32)>) -> u8 {
        self.nearest_with_hint_impl::<false>(r, g, b, hint)
    }

    #[inline(always)]
    fn nearest_with_seed_bounds(
        &self,
        r: u8,
        g: u8,
        b: u8,
        best_index: u8,
        best_distance: u32,
    ) -> u8 {
        self.nearest_with_seed::<true>(r, g, b, best_index, best_distance)
    }

    #[inline(always)]
    fn nearest_with_hint_index_split(
        &self,
        r: u8,
        g: u8,
        b: u8,
        hint_index: u8,
        hint_color: u32,
    ) -> u8 {
        self.nearest_with_seed::<true>(
            r,
            g,
            b,
            hint_index,
            palette_color_distance(hint_color, r, g, b),
        )
    }

    #[inline(always)]
    fn nearest_with_hint_impl<const USE_BOUNDS: bool>(
        &self,
        r: u8,
        g: u8,
        b: u8,
        hint: Option<(u8, u32)>,
    ) -> u8 {
        if self.root == PALETTE_KD_EMPTY {
            return 0;
        }
        let (best_index, best_distance) = hint
            .map(|(index, color)| (index, palette_color_distance(color, r, g, b)))
            .unwrap_or((0, u32::MAX));
        self.nearest_with_seed::<USE_BOUNDS>(r, g, b, best_index, best_distance)
    }

    #[inline(always)]
    fn nearest_with_seed<const USE_BOUNDS: bool>(
        &self,
        r: u8,
        g: u8,
        b: u8,
        mut best_index: u8,
        mut best_distance: u32,
    ) -> u8 {
        if self.root == PALETTE_KD_EMPTY || best_distance == 0 {
            return best_index;
        }
        let r = i32::from(r);
        let g = i32::from(g);
        let b = i32::from(b);
        // A balanced 256-entry tree has depth at most eight; one slot per
        // level is enough for pending far branches without zeroing a larger
        // stack for every histogram color lookup.
        // The stack entries are written before they are read.  Avoiding a
        // per-lookup zeroing pass matters here because the median-cut mapper
        // performs one nearest-color search for every occupied histogram bin.
        let mut stack = [std::mem::MaybeUninit::<(u16, u32)>::uninit(); 8];
        let mut stack_len = 0usize;
        let mut next = self.root;
        while next != PALETTE_KD_EMPTY {
            let node_index = usize::from(next);
            let node = unsafe { *self.nodes.get_unchecked(node_index) };
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
                    let far_node = unsafe { *self.nodes.get_unchecked(usize::from(far)) };
                    let min_red = i32::from(far_node.min_red);
                    let min_green = i32::from(far_node.min_green);
                    let min_blue = i32::from(far_node.min_blue);
                    let max_red = i32::from(far_node.max_red);
                    let max_green = i32::from(far_node.max_green);
                    let max_blue = i32::from(far_node.max_blue);
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
                        + blue_distance * blue_distance) as u32
                } else {
                    let delta = (value - split).unsigned_abs();
                    delta * delta
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

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
fn encode_indexed_literal_gif_inner_with_output(
    output: Vec<u8>,
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    transparent_index: Option<u8>,
) -> Result<Vec<u8>, String> {
    encode_indexed_literal_gif_inner_with_output_impl::<true>(
        output,
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

#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn encode_indexed_literal_gif_inner_with_output_unchecked(
    output: Vec<u8>,
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    transparent_index: Option<u8>,
) -> Result<Vec<u8>, String> {
    encode_indexed_literal_gif_inner_with_output_impl::<false>(
        output,
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

#[allow(clippy::too_many_arguments)]
fn encode_indexed_literal_gif_inner_with_output_impl<const VALIDATE: bool>(
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
    // Native thread setup and per-frame scratch allocation are slower than the
    // direct writer for ordinary 128x128 animations. Parallelism starts to
    // repay that fixed cost only once the indexed input is around one MiB.
    if VALIDATE && min_code_size > 2 && frame_count >= 8 && expected_len >= 1_000_000 {
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
        if VALIDATE {
            encode_indexed_literal_lzw_direct_to(&mut output, frame, min_code_size, color_count)?;
        } else {
            encode_indexed_literal_lzw_direct_to_unchecked(
                &mut output,
                frame,
                min_code_size,
                color_count,
            )?;
        }
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
    output.resize(output_length, 0);
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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
    let thread_cap = if total_frame_pixels < 100_000 || metadata.frames.len() <= 12 {
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
    output.resize(output_length, 0);
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
    let mut frame_output = Vec::new();
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
        &mut frame_output,
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
    frame_output: &mut Vec<u8>,
) -> Result<(), String> {
    loop {
        let frame_index = next_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(frame) = metadata.frames.get(frame_index) else {
            return Ok(());
        };
        let (destination, expected_length) = frame_layout[frame_index];
        decode_frame_indices_reusing_output(data, frame, image_data, lzw_scratch, indices)?;
        frame_output.clear();
        if frame_output.capacity() < expected_length {
            frame_output.reserve(expected_length);
        }
        write_reencoded_frame_literal_to(
            frame_output,
            data,
            metadata,
            frame,
            indices,
            compressed_scratch,
        )?;
        if frame_output.len() != expected_length {
            return Err("Predicted reencoded frame size differs".to_string());
        }
        // Each worker receives a distinct frame range from the atomic index.
        // The backing output vector is fully sized before workers start and
        // is never resized while these disjoint copies run.
        unsafe {
            std::ptr::copy_nonoverlapping(
                frame_output.as_ptr(),
                (output_address as *mut u8).add(destination),
                expected_length,
            );
        }
    }
}

impl Drop for PaletteKdTree {
    fn drop(&mut self) {
        let nodes = std::mem::take(&mut self.nodes);
        REUSABLE_PALETTE_KD_NODES.with(|scratch| {
            *scratch.borrow_mut() = nodes;
        });
    }
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
struct ReencodeDispatchContext<'a> {
    data: &'a [u8],
    metadata: &'a GifMetadata,
    frame_layout: &'a [(usize, usize)],
    output_address: usize,
    next_frame: std::sync::atomic::AtomicUsize,
    error: std::sync::Mutex<Option<String>>,
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
unsafe extern "C" fn reencode_dispatch_worker(context: *mut std::ffi::c_void, _iteration: usize) {
    let context = &*(context.cast::<ReencodeDispatchContext<'_>>());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        reencode_gif_literal_worker(
            context.data,
            context.metadata,
            context.frame_layout,
            context.output_address,
            &context.next_frame,
        )
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
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let next_frame = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..thread_count)
                .map(|_| {
                    let next_frame = &next_frame;
                    let frame_layout = &frame_layout;
                    scope.spawn(move || -> Result<(), String> {
                        reencode_gif_literal_worker(
                            data,
                            metadata,
                            frame_layout,
                            output_address,
                            next_frame,
                        )
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[cfg(not(feature = "encode-only"))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[allow(clippy::too_many_arguments)]
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
                let rect_length = rect.width * rect.height;
                if literal {
                    rect_scratch.clear();
                    if rect_scratch.capacity() < rect_length {
                        rect_scratch.reserve(rect_length - rect_scratch.capacity());
                    }
                    for row in 0..rect.height {
                        let start = (rect.y + row) * canvas_width + rect.x;
                        rect_scratch.extend_from_slice(&frame[start..start + rect.width]);
                    }
                    encode_indexed_literal_lzw_direct_to(
                        &mut output,
                        &rect_scratch,
                        min_code_size,
                        color_count,
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
                // A no-op frame has one pixel of payload. The literal stream
                // is the same minimal CLEAR/pixel/EOI code sequence without
                // paying for a dictionary reset and table lookup.
                encode_indexed_literal_lzw_direct_to(
                    &mut output,
                    &frame[..1],
                    min_code_size,
                    color_count,
                )?;
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
                encode_indexed_literal_lzw_direct_to(
                    &mut output,
                    frame,
                    min_code_size,
                    color_count,
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

#[allow(clippy::too_many_arguments)]
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
            entries: Vec::new(),
            epoch: 0,
        }
    }

    #[inline]
    fn ensure_capacity(&mut self, color_count: usize) {
        let required = (1usize << 12).saturating_mul(color_count);
        debug_assert!(required <= LZW_DIRECT_ENTRY_COUNT);
        if self.entries.len() < required {
            self.entries.resize(required, 0);
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

struct LzwEncodeScratch {
    // JavaScript overwrites the complete requested byte range before any
    // encoder reads it. Keep the reusable input allocation uninitialized so
    // growing it does not first zero bytes that the caller immediately
    // replaces. `MaybeUninit<u32>` preserves four-byte alignment for packed
    // RGBA loads.
    input: Vec<std::mem::MaybeUninit<u32>>,
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
        const { std::cell::RefCell::new(Vec::new()) };
    static REUSABLE_QUALITY_HISTOGRAM_U32: std::cell::RefCell<Vec<RgbHistogramBin32>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static REUSABLE_QUALITY_HISTOGRAM_U64: std::cell::RefCell<Vec<RgbHistogramBin>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static REUSABLE_QUALITY_HISTOGRAM_TO_PALETTE: std::cell::RefCell<Vec<u8>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static REUSABLE_QUALITY_COLORS: std::cell::RefCell<Vec<QuantizedColor>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static REUSABLE_QUALITY_PALETTE: std::cell::RefCell<Vec<u32>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static REUSABLE_QUALITY_COLOR_INDEX: std::cell::RefCell<ColorIndexTable> =
        std::cell::RefCell::new(ColorIndexTable::empty());
    static REUSABLE_QUANTIZED_INDEXED: std::cell::RefCell<Vec<u16>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static REUSABLE_QUANTIZED_BYTES: std::cell::RefCell<Vec<u8>> =
        const { std::cell::RefCell::new(Vec::new()) };
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
        if length > input.len() * std::mem::size_of::<u32>() {
            return Err("Indexed input scratch length exceeds capacity".to_string());
        }
        let index_stream =
            unsafe { std::slice::from_raw_parts(input.as_ptr().cast::<u8>(), length) };
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn encode_indexed_literal_lzw_to(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
    compressed: &mut Vec<u8>,
) -> Result<(), String> {
    if !(2..=8).contains(&min_code_size) {
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
    encode_indexed_literal_lzw_direct_to_impl::<true>(
        output,
        index_stream,
        min_code_size,
        color_count,
    )
}

#[inline(always)]
fn encode_indexed_literal_lzw_direct_to_unchecked(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<(), String> {
    encode_indexed_literal_lzw_direct_to_impl::<false>(
        output,
        index_stream,
        min_code_size,
        color_count,
    )
}

#[inline(always)]
fn encode_indexed_literal_lzw_direct_to_impl<const VALIDATE: bool>(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<(), String> {
    if min_code_size == 7 && color_count <= 128 {
        return encode_eight_bit_literal_lzw_direct_to::<VALIDATE>(
            output,
            index_stream,
            color_count,
        );
    }
    if min_code_size == 3 {
        return encode_four_bit_literal_lzw_direct_to::<VALIDATE>(
            output,
            index_stream,
            color_count,
        );
    }
    if min_code_size == 5 {
        return encode_six_bit_literal_lzw_direct_to::<VALIDATE>(output, index_stream, color_count);
    }
    if min_code_size == 6 {
        return encode_seven_bit_literal_lzw_direct_to::<VALIDATE>(
            output,
            index_stream,
            color_count,
        );
    }
    if min_code_size == 8 {
        return encode_nine_bit_literal_lzw_direct_to::<VALIDATE>(
            output,
            index_stream,
            color_count,
        );
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

fn encode_six_bit_literal_lzw_direct_to<const VALIDATE: bool>(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    color_count: usize,
) -> Result<(), String> {
    if color_count == 0 || color_count > 32 {
        return Err("Invalid color count".to_string());
    }
    if index_stream.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    if VALIDATE && !indices_fit_color_count(index_stream, color_count) {
        return Err("Pixel index out of range".to_string());
    }

    const CODE_SIZE: usize = 6;
    const CLEAR: u16 = 32;
    const EOI: u16 = 33;
    const LITERALS_PER_CLEAR: usize = 30;
    let clear_count = index_stream.len().div_ceil(LITERALS_PER_CLEAR);
    let raw_length = ((index_stream.len() + clear_count + 1) * CODE_SIZE).div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + 2 + block_count + raw_length);
    output[output_start] = 5;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;

    for literals in index_stream.chunks(LITERALS_PER_CLEAR) {
        append_six_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, CLEAR);
        let mut groups = literals.chunks_exact(8);
        for group in &mut groups {
            let packed = u64::from(group[0])
                | (u64::from(group[1]) << 6)
                | (u64::from(group[2]) << 12)
                | (u64::from(group[3]) << 18)
                | (u64::from(group[4]) << 24)
                | (u64::from(group[5]) << 30)
                | (u64::from(group[6]) << 36)
                | (u64::from(group[7]) << 42);
            let combined = bits | (packed << bit_count);
            let total_bits = bit_count + 48;
            let byte_count = total_bits / 8;
            writer.write_fixed_u64(combined, byte_count);
            bits = combined >> (byte_count * 8);
            bit_count = total_bits - byte_count * 8;
        }
        for &pixel in groups.remainder() {
            append_six_bit_literal_code_to_direct(
                &mut writer,
                &mut bits,
                &mut bit_count,
                u16::from(pixel),
            );
        }
    }
    append_six_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, EOI);
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
    output[output_start + 1 + block_count + raw_length] = 0;
    Ok(())
}

fn encode_seven_bit_literal_lzw_direct_to<const VALIDATE: bool>(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    color_count: usize,
) -> Result<(), String> {
    if color_count == 0 || color_count > 64 {
        return Err("Invalid color count".to_string());
    }
    if index_stream.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    if VALIDATE && !indices_fit_color_count(index_stream, color_count) {
        return Err("Pixel index out of range".to_string());
    }

    const CODE_SIZE: usize = 7;
    const CLEAR: u16 = 64;
    const EOI: u16 = 65;
    const LITERALS_PER_CLEAR: usize = 62;
    let clear_count = index_stream.len().div_ceil(LITERALS_PER_CLEAR);
    let raw_length = ((index_stream.len() + clear_count + 1) * CODE_SIZE).div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + 2 + block_count + raw_length);
    output[output_start] = 6;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;

    for literals in index_stream.chunks(LITERALS_PER_CLEAR) {
        append_seven_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, CLEAR);
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
            let combined = bits | (packed << bit_count);
            let total_bits = bit_count + 56;
            let byte_count = total_bits / 8;
            writer.write_fixed_u64(combined, byte_count);
            bits = combined >> (byte_count * 8);
            bit_count = total_bits - byte_count * 8;
        }
        for &pixel in groups.remainder() {
            append_seven_bit_literal_code_to_direct(
                &mut writer,
                &mut bits,
                &mut bit_count,
                u16::from(pixel),
            );
        }
    }
    append_seven_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, EOI);
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
    output[output_start + 1 + block_count + raw_length] = 0;
    Ok(())
}

fn encode_eight_bit_literal_lzw_direct_to<const VALIDATE: bool>(
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
    if VALIDATE && !indices_fit_color_count(index_stream, color_count) {
        return Err("Pixel index out of range".to_string());
    }

    let raw_length = index_stream.len() + index_stream.len().div_ceil(126) + 1;
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    let output_length = output_start + 2 + block_count + raw_length;
    resize_output_uninitialized(output, output_length);
    output[output_start] = 7;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    for literals in index_stream.chunks(126) {
        writer.write_byte(128);
        writer.write_slice(literals);
    }
    writer.write_byte(129);
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
    }
    Ok(())
}

struct DirectGifSubblockWriter<'a> {
    output: &'a mut [u8],
    position: usize,
    block_remaining: usize,
    raw_position: usize,
}

/// Extend an output buffer without paying to zero bytes that the caller will
/// immediately overwrite. Every caller must fill the entire new range before
/// it is observed by safe code.
#[inline(always)]
fn resize_output_uninitialized(output: &mut Vec<u8>, new_len: usize) {
    debug_assert!(new_len >= output.len());
    if output.capacity() < new_len {
        output.reserve(new_len - output.len());
    }
    // SAFETY: callers of this helper write every byte in the extended range
    // before reading it or exposing it to safe code.
    unsafe { output.set_len(new_len) };
}

impl DirectGifSubblockWriter<'_> {
    #[inline(always)]
    fn write_byte(&mut self, value: u8) {
        if self.block_remaining == 0 {
            self.position += 1;
            self.block_remaining = 255;
        }
        self.output[self.position] = value;
        self.position += 1;
        self.block_remaining -= 1;
        self.raw_position += 1;
    }

    #[inline(always)]
    fn write_fixed_u64(&mut self, mut value: u64, length: usize) {
        if length > self.block_remaining {
            for _ in 0..length {
                self.write_byte(value as u8);
                value >>= 8;
            }
            return;
        }
        if length == 8 {
            // The common literal packer writes complete eight-byte chunks.
            // Store them directly when the chunk stays inside the current
            // GIF sub-block; crossing a boundary still uses the byte-wise
            // fallback above so the length marker cannot be overwritten.
            unsafe {
                std::ptr::write_unaligned(
                    self.output.as_mut_ptr().add(self.position).cast::<u64>(),
                    value.to_le(),
                );
            }
            self.position += 8;
            self.block_remaining -= 8;
            self.raw_position += 8;
            return;
        }
        if length == 7 && self.block_remaining >= 8 {
            // A seven-byte literal group has one more raw byte available in
            // this sub-block. Store the whole u64 and advance only seven
            // bytes; the next write overwrites the eighth byte. This keeps
            // the common non-boundary path on the same unaligned-store fast
            // path as eight-byte groups.
            unsafe {
                std::ptr::write_unaligned(
                    self.output.as_mut_ptr().add(self.position).cast::<u64>(),
                    value.to_le(),
                );
            }
            self.position += 7;
            self.block_remaining -= 7;
            self.raw_position += 7;
            return;
        }
        let bytes = value.to_le_bytes();
        self.output[self.position..self.position + length].copy_from_slice(&bytes[..length]);
        self.position += length;
        self.block_remaining -= length;
        self.raw_position += length;
    }

    #[inline(always)]
    fn write_slice(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            if self.block_remaining == 0 {
                self.position += 1;
                self.block_remaining = 255;
            }
            let length = bytes.len().min(self.block_remaining);
            self.output[self.position..self.position + length].copy_from_slice(&bytes[..length]);
            self.position += length;
            self.block_remaining -= length;
            self.raw_position += length;
            bytes = &bytes[length..];
        }
    }
}

fn encode_four_bit_literal_lzw_direct_to<const VALIDATE: bool>(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    color_count: usize,
) -> Result<(), String> {
    if color_count == 0 || color_count > 8 {
        return Err("Invalid color count".to_string());
    }
    if index_stream.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    if VALIDATE && !indices_fit_color_count(index_stream, color_count) {
        return Err("Pixel index out of range".to_string());
    }

    let clear_count = index_stream.len().div_ceil(6);
    let raw_length = ((index_stream.len() + clear_count + 1) * 4).div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + 2 + block_count + raw_length);
    output[output_start] = 3;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let mut groups = index_stream.chunks_exact(6);
    for group in &mut groups {
        let packed = 8u32
            | (u32::from(group[0]) << 4)
            | (u32::from(group[1]) << 8)
            | (u32::from(group[2]) << 12)
            | (u32::from(group[3]) << 16)
            | (u32::from(group[4]) << 20)
            | (u32::from(group[5]) << 24);
        let total_bits = bit_count + 28;
        let combined = bits | (u64::from(packed) << bit_count);
        let byte_count = total_bits / 8;
        writer.write_fixed_u64(combined, byte_count);
        bits = combined >> (byte_count * 8);
        bit_count = total_bits - byte_count * 8;
    }
    if !groups.remainder().is_empty() {
        bits |= 8u64 << bit_count;
        bit_count += 4;
        while bit_count >= 8 {
            writer.write_byte(bits as u8);
            bits >>= 8;
            bit_count -= 8;
        }
        for &pixel in groups.remainder() {
            bits |= u64::from(pixel) << bit_count;
            bit_count += 4;
            while bit_count >= 8 {
                writer.write_byte(bits as u8);
                bits >>= 8;
                bit_count -= 8;
            }
        }
    }
    bits |= 9u64 << bit_count;
    bit_count += 4;
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
    output[output_start + 1 + block_count + raw_length] = 0;
    Ok(())
}

fn encode_nine_bit_literal_lzw_direct_to<const VALIDATE: bool>(
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
    if VALIDATE && !indices_fit_color_count(index_stream, color_count) {
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
    resize_output_uninitialized(output, output_start + 2 + block_count + raw_length + 8);
    output[output_start] = 8;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
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
            if total_bits >= 64 {
                writer.write_fixed_u64(combined, 8);
                bits = if bit_count == 0 {
                    0
                } else {
                    packed >> (64 - bit_count)
                };
                bit_count = total_bits - 64;
            } else {
                debug_assert_eq!(bit_count, 0);
                writer.write_fixed_u64(combined, 7);
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
    let final_length = output_start + 2 + block_count + raw_length;
    output[final_length - 1] = 0;
    output.truncate(final_length);
    Ok(())
}

/// Encode a 256-color literal stream directly from opaque RGBA input using an
/// exact supplied palette. This is the known-palette counterpart to the
/// quality histogram mapper below; it avoids a full intermediate index pass.
fn encode_nine_bit_literal_lzw_palette_mapped_to(
    output: &mut Vec<u8>,
    rgba_stream: &[u8],
    mapper: &PaletteMapper<'_>,
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
    let output_length = output_start + 2 + block_count + raw_length;
    // The direct writer fills every byte in this range, including the
    // sub-block length slots written after the literal stream. Avoid asking
    // Wasm's allocator to zero the encoded payload before overwriting it.
    resize_output_uninitialized(output, output_length);
    output[output_start] = 8;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let rgba_pointer = rgba_stream.as_ptr();
    let mut cache_keys = [u32::MAX; 256];
    let mut cache_values = [0u8; 256];

    for chunk_start in (0..pixel_count).step_by(254) {
        append_nine_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, 256);
        let chunk_end = (chunk_start + 254).min(pixel_count);
        let mut pixel_index = chunk_start;
        while pixel_index + 7 <= chunk_end {
            let rgba_offset = pixel_index * 4;
            let (packed0, packed1) = unsafe { read_rgba_pair(rgba_pointer, rgba_offset) };
            let (packed2, packed3) = unsafe { read_rgba_pair(rgba_pointer, rgba_offset + 8) };
            let (packed4, packed5) = unsafe { read_rgba_pair(rgba_pointer, rgba_offset + 16) };
            let packed6 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(rgba_offset + 24).cast())
            });
            let code0 = exact_palette_index_packed_cached(
                mapper,
                packed0,
                &mut cache_keys,
                &mut cache_values,
            )?;
            let code1 = exact_palette_index_packed_cached(
                mapper,
                packed1,
                &mut cache_keys,
                &mut cache_values,
            )?;
            let code2 = exact_palette_index_packed_cached(
                mapper,
                packed2,
                &mut cache_keys,
                &mut cache_values,
            )?;
            let code3 = exact_palette_index_packed_cached(
                mapper,
                packed3,
                &mut cache_keys,
                &mut cache_values,
            )?;
            let code4 = exact_palette_index_packed_cached(
                mapper,
                packed4,
                &mut cache_keys,
                &mut cache_values,
            )?;
            let code5 = exact_palette_index_packed_cached(
                mapper,
                packed5,
                &mut cache_keys,
                &mut cache_values,
            )?;
            let code6 = exact_palette_index_packed_cached(
                mapper,
                packed6,
                &mut cache_keys,
                &mut cache_values,
            )?;
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
                writer.write_fixed_u64(combined, 8);
                bits = if bit_count == 0 {
                    0
                } else {
                    packed_codes >> (64 - bit_count)
                };
                bit_count = total_bits - 64;
            } else {
                debug_assert_eq!(bit_count, 0);
                writer.write_fixed_u64(combined, 7);
                bits = combined >> 56;
                bit_count = total_bits - 56;
            }
            pixel_index += 7;
        }
        while pixel_index < chunk_end {
            let packed = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
            });
            let code = exact_palette_index_packed_cached(
                mapper,
                packed,
                &mut cache_keys,
                &mut cache_values,
            )?;
            append_nine_bit_literal_code_to_direct(
                &mut writer,
                &mut bits,
                &mut bit_count,
                u16::from(code),
            );
            pixel_index += 1;
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
    output[output_start + 1 + block_count + raw_length] = 0;
    Ok(())
}

#[inline(always)]
fn exact_palette_index_packed(mapper: &PaletteMapper<'_>, packed: u32) -> Result<u8, String> {
    mapper
        .exact_index(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8)
        .ok_or_else(|| {
            "Pixel-perfect GIF encoding found an RGBA color outside the supplied palette"
                .to_string()
        })
}

#[inline(always)]
fn exact_palette_index_packed_cached(
    mapper: &PaletteMapper<'_>,
    packed: u32,
    cache_keys: &mut [u32; 256],
    cache_values: &mut [u8; 256],
) -> Result<u8, String> {
    let rgb = packed & 0x00ff_ffff;
    let slot = (rgb as usize).wrapping_mul(2_654_435_761) & 255;
    if cache_keys[slot] == rgb {
        return Ok(cache_values[slot]);
    }
    let index = exact_palette_index_packed(mapper, packed)?;
    cache_keys[slot] = rgb;
    cache_values[slot] = index;
    Ok(index)
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn append_nine_bit_mapped_quality_group<const BITS: usize, const HAS_TRANSPARENT: bool>(
    writer: &mut DirectGifSubblockWriter,
    bits: &mut u64,
    bit_count: &mut usize,
    rgba_pointer: *const u8,
    pixel_index: usize,
    alpha_threshold: u8,
    transparent_index: u8,
    histogram_to_palette_pointer: *const u8,
) {
    let rgba_offset = pixel_index * 4;
    let (packed0, packed1) = unsafe { read_rgba_pair(rgba_pointer, rgba_offset) };
    let (packed2, packed3) = unsafe { read_rgba_pair(rgba_pointer, rgba_offset + 8) };
    let (packed4, packed5) = unsafe { read_rgba_pair(rgba_pointer, rgba_offset + 16) };
    let packed6 = u32::from_le(unsafe {
        std::ptr::read_unaligned(rgba_pointer.add(rgba_offset + 24).cast())
    });
    let code0 = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
        packed0,
        alpha_threshold,
        transparent_index,
        histogram_to_palette_pointer,
    );
    let code1 = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
        packed1,
        alpha_threshold,
        transparent_index,
        histogram_to_palette_pointer,
    );
    let code2 = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
        packed2,
        alpha_threshold,
        transparent_index,
        histogram_to_palette_pointer,
    );
    let code3 = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
        packed3,
        alpha_threshold,
        transparent_index,
        histogram_to_palette_pointer,
    );
    let code4 = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
        packed4,
        alpha_threshold,
        transparent_index,
        histogram_to_palette_pointer,
    );
    let code5 = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
        packed5,
        alpha_threshold,
        transparent_index,
        histogram_to_palette_pointer,
    );
    let code6 = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
        packed6,
        alpha_threshold,
        transparent_index,
        histogram_to_palette_pointer,
    );
    let packed_codes = u64::from(code0)
        | (u64::from(code1) << 9)
        | (u64::from(code2) << 18)
        | (u64::from(code3) << 27)
        | (u64::from(code4) << 36)
        | (u64::from(code5) << 45)
        | (u64::from(code6) << 54);
    let combined = *bits | (packed_codes << *bit_count);
    let total_bits = *bit_count + 63;
    if total_bits >= 64 {
        writer.write_fixed_u64(combined, 8);
        *bits = if *bit_count == 0 {
            0
        } else {
            packed_codes >> (64 - *bit_count)
        };
        *bit_count = total_bits - 64;
    } else {
        debug_assert_eq!(*bit_count, 0);
        writer.write_fixed_u64(combined, 7);
        *bits = combined >> 56;
        *bit_count = total_bits - 56;
    }
}

/// Encode the 256-color literal stream directly from RGBA input. The quality
/// palette has already been built, so retaining a full indexed scratch buffer
#[inline(never)]
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
    resize_output_uninitialized(output, output_start + 2 + block_count + raw_length);
    output[output_start] = 8;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let rgba_pointer = rgba_stream.as_ptr();
    let histogram_to_palette_pointer = histogram_to_palette.as_ptr();

    for chunk_start in (0..pixel_count).step_by(254) {
        append_nine_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, 256);
        let chunk_end = (chunk_start + 254).min(pixel_count);
        let mut pixel_index = chunk_start;
        while pixel_index + 14 <= chunk_end {
            append_nine_bit_mapped_quality_group::<BITS, HAS_TRANSPARENT>(
                &mut writer,
                &mut bits,
                &mut bit_count,
                rgba_pointer,
                pixel_index,
                alpha_threshold,
                transparent_index,
                histogram_to_palette_pointer,
            );
            append_nine_bit_mapped_quality_group::<BITS, HAS_TRANSPARENT>(
                &mut writer,
                &mut bits,
                &mut bit_count,
                rgba_pointer,
                pixel_index + 7,
                alpha_threshold,
                transparent_index,
                histogram_to_palette_pointer,
            );
            pixel_index += 14;
        }
        while pixel_index + 7 <= chunk_end {
            append_nine_bit_mapped_quality_group::<BITS, HAS_TRANSPARENT>(
                &mut writer,
                &mut bits,
                &mut bit_count,
                rgba_pointer,
                pixel_index,
                alpha_threshold,
                transparent_index,
                histogram_to_palette_pointer,
            );
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
                histogram_to_palette_pointer,
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
    output[output_start + 1 + block_count + raw_length] = 0;
    Ok(())
}

#[inline(always)]
fn append_nine_bit_histogram_quality_group<const HAS_TRANSPARENT: bool>(
    writer: &mut DirectGifSubblockWriter,
    bits: &mut u64,
    bit_count: &mut usize,
    histogram_pointer: *const u16,
    pixel_index: usize,
    transparent_index: u8,
    histogram_to_palette: &[u8],
) {
    let packed_indices = u64::from_le(unsafe {
        std::ptr::read_unaligned(histogram_pointer.add(pixel_index).cast::<u64>())
    });
    let code0 = mapped_quality_histogram_pixel::<HAS_TRANSPARENT>(
        packed_indices as u16,
        transparent_index,
        histogram_to_palette,
    );
    let code1 = mapped_quality_histogram_pixel::<HAS_TRANSPARENT>(
        (packed_indices >> 16) as u16,
        transparent_index,
        histogram_to_palette,
    );
    let code2 = mapped_quality_histogram_pixel::<HAS_TRANSPARENT>(
        (packed_indices >> 32) as u16,
        transparent_index,
        histogram_to_palette,
    );
    let code3 = mapped_quality_histogram_pixel::<HAS_TRANSPARENT>(
        (packed_indices >> 48) as u16,
        transparent_index,
        histogram_to_palette,
    );
    let code4 = mapped_quality_histogram_pixel::<HAS_TRANSPARENT>(
        u16::from_le(unsafe {
            std::ptr::read_unaligned(histogram_pointer.add(pixel_index + 4).cast::<u16>())
        }),
        transparent_index,
        histogram_to_palette,
    );
    let code5 = mapped_quality_histogram_pixel::<HAS_TRANSPARENT>(
        u16::from_le(unsafe {
            std::ptr::read_unaligned(histogram_pointer.add(pixel_index + 5).cast::<u16>())
        }),
        transparent_index,
        histogram_to_palette,
    );
    let code6 = mapped_quality_histogram_pixel::<HAS_TRANSPARENT>(
        u16::from_le(unsafe {
            std::ptr::read_unaligned(histogram_pointer.add(pixel_index + 6).cast::<u16>())
        }),
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
    let combined = *bits | (packed_codes << *bit_count);
    let total_bits = *bit_count + 63;
    if total_bits >= 64 {
        writer.write_fixed_u64(combined, 8);
        *bits = if *bit_count == 0 {
            0
        } else {
            packed_codes >> (64 - *bit_count)
        };
        *bit_count = total_bits - 64;
    } else {
        debug_assert_eq!(*bit_count, 0);
        writer.write_fixed_u64(combined, 7);
        *bits = combined >> 56;
        *bit_count = total_bits - 56;
    }
}

#[inline(always)]
fn encode_nine_bit_literal_lzw_histogram_indices(
    output: &mut Vec<u8>,
    histogram_indices: &[u16],
    transparent_index: u8,
    has_transparent: bool,
    histogram_to_palette: &[u8],
) -> Result<(), String> {
    if has_transparent {
        encode_nine_bit_literal_lzw_histogram_indices_impl::<true>(
            output,
            histogram_indices,
            transparent_index,
            histogram_to_palette,
        )
    } else {
        encode_nine_bit_literal_lzw_histogram_indices_impl::<false>(
            output,
            histogram_indices,
            transparent_index,
            histogram_to_palette,
        )
    }
}

#[inline(always)]
fn encode_nine_bit_literal_lzw_histogram_indices_impl<const HAS_TRANSPARENT: bool>(
    output: &mut Vec<u8>,
    histogram_indices: &[u16],
    transparent_index: u8,
    histogram_to_palette: &[u8],
) -> Result<(), String> {
    if histogram_indices.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    let pixel_count = histogram_indices.len();
    let clear_count = pixel_count.div_ceil(254);
    let raw_length = pixel_count
        .checked_add(clear_count)
        .and_then(|codes| codes.checked_add(1))
        .and_then(|codes| codes.checked_mul(9))
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?
        .div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + 2 + block_count + raw_length);
    output[output_start] = 8;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let histogram_pointer = histogram_indices.as_ptr();

    for chunk_start in (0..pixel_count).step_by(254) {
        append_nine_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, 256);
        let chunk_end = (chunk_start + 254).min(pixel_count);
        let mut pixel_index = chunk_start;
        while pixel_index + 14 <= chunk_end {
            append_nine_bit_histogram_quality_group::<HAS_TRANSPARENT>(
                &mut writer,
                &mut bits,
                &mut bit_count,
                histogram_pointer,
                pixel_index,
                transparent_index,
                histogram_to_palette,
            );
            append_nine_bit_histogram_quality_group::<HAS_TRANSPARENT>(
                &mut writer,
                &mut bits,
                &mut bit_count,
                histogram_pointer,
                pixel_index + 7,
                transparent_index,
                histogram_to_palette,
            );
            pixel_index += 14;
        }
        while pixel_index + 7 <= chunk_end {
            append_nine_bit_histogram_quality_group::<HAS_TRANSPARENT>(
                &mut writer,
                &mut bits,
                &mut bit_count,
                histogram_pointer,
                pixel_index,
                transparent_index,
                histogram_to_palette,
            );
            pixel_index += 7;
        }
        while pixel_index < chunk_end {
            let code = mapped_quality_histogram_pixel::<HAS_TRANSPARENT>(
                unsafe { *histogram_pointer.add(pixel_index) },
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
    output[output_start + 1 + block_count + raw_length] = 0;
    Ok(())
}

#[inline(always)]
fn mapped_quality_histogram_pixel<const HAS_TRANSPARENT: bool>(
    histogram_index: u16,
    transparent_index: u8,
    histogram_to_palette: &[u8],
) -> u8 {
    if HAS_TRANSPARENT && histogram_index == u16::MAX {
        transparent_index
    } else {
        unsafe { *histogram_to_palette.get_unchecked(usize::from(histogram_index)) }
    }
}

#[inline(always)]
fn mapped_quality_pixel<const BITS: usize, const HAS_TRANSPARENT: bool>(
    packed: u32,
    alpha_threshold: u8,
    transparent_index: u8,
    histogram_to_palette: *const u8,
) -> u8 {
    if HAS_TRANSPARENT && ((packed >> 24) as u8) < alpha_threshold {
        transparent_index
    } else {
        let index = quality_histogram_index_packed::<BITS>(packed);
        unsafe { *histogram_to_palette.add(index) }
    }
}

#[inline(always)]
fn append_six_bit_literal_code_to_direct(
    writer: &mut DirectGifSubblockWriter,
    bits: &mut u64,
    bit_count: &mut usize,
    code: u16,
) {
    *bits |= u64::from(code) << *bit_count;
    *bit_count += 6;
    while *bit_count >= 8 {
        writer.write_byte(*bits as u8);
        *bits >>= 8;
        *bit_count -= 8;
    }
}

#[inline(always)]
fn append_seven_bit_literal_code_to_direct(
    writer: &mut DirectGifSubblockWriter,
    bits: &mut u64,
    bit_count: &mut usize,
    code: u16,
) {
    *bits |= u64::from(code) << *bit_count;
    *bit_count += 7;
    while *bit_count >= 8 {
        writer.write_byte(*bits as u8);
        *bits >>= 8;
        *bit_count -= 8;
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
    if !(2..=8).contains(&min_code_size) {
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
    if min_code_size == 3 {
        return encode_four_bit_literal_codes(output, index_stream);
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
    if min_code_size == 5 && color_count == 32 {
        return encode_six_bit_literal_codes(output, index_stream);
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

fn encode_four_bit_literal_codes(output: &mut Vec<u8>, index_stream: &[u8]) -> Result<(), String> {
    let clear_count = index_stream.len().div_ceil(6);
    let raw_length = ((index_stream.len() + clear_count + 1) * 4).div_ceil(8);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + raw_length);
    let output_pointer = unsafe { output.as_mut_ptr().add(output_start) };
    let mut output_position = 0usize;
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let mut groups = index_stream.chunks_exact(6);

    for group in &mut groups {
        let packed = 8u32
            | (u32::from(group[0]) << 4)
            | (u32::from(group[1]) << 8)
            | (u32::from(group[2]) << 12)
            | (u32::from(group[3]) << 16)
            | (u32::from(group[4]) << 20)
            | (u32::from(group[5]) << 24);
        let total_bits = bit_count + 28;
        let combined = bits | (u64::from(packed) << bit_count);
        let byte_count = total_bits / 8;
        let bytes = combined.to_le_bytes();
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                output_pointer.add(output_position),
                byte_count,
            );
        }
        output_position += byte_count;
        bits = combined >> (byte_count * 8);
        bit_count = total_bits - byte_count * 8;
    }

    if !groups.remainder().is_empty() {
        append_fixed_literal_code(
            output_pointer,
            &mut output_position,
            &mut bits,
            &mut bit_count,
            8,
            4,
        );
        for &pixel in groups.remainder() {
            append_fixed_literal_code(
                output_pointer,
                &mut output_position,
                &mut bits,
                &mut bit_count,
                pixel,
                4,
            );
        }
    }
    append_fixed_literal_code(
        output_pointer,
        &mut output_position,
        &mut bits,
        &mut bit_count,
        9,
        4,
    );
    while bit_count > 0 {
        unsafe { output_pointer.add(output_position).write(bits as u8) };
        output_position += 1;
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(output_position, raw_length);
    Ok(())
}

#[inline(always)]
fn append_fixed_literal_code(
    output: *mut u8,
    output_position: &mut usize,
    bits: &mut u64,
    bit_count: &mut usize,
    code: u8,
    code_bits: usize,
) {
    *bits |= u64::from(code) << *bit_count;
    *bit_count += code_bits;
    while *bit_count >= 8 {
        unsafe { output.add(*output_position).write(*bits as u8) };
        *output_position += 1;
        *bits >>= 8;
        *bit_count -= 8;
    }
}

#[inline(always)]
fn encode_fixed_width_literal_codes<const CODE_BITS: usize>(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    clear_code: u8,
    eoi_code: u8,
    literals_per_clear: usize,
) -> Result<(), String> {
    let clear_count = index_stream.len().div_ceil(literals_per_clear);
    let raw_length = ((index_stream.len() + clear_count + 1) * CODE_BITS).div_ceil(8);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + raw_length);
    let output_pointer = unsafe { output.as_mut_ptr().add(output_start) };
    let mut output_position = 0usize;
    let mut bits = 0u64;
    let mut bit_count = 0usize;

    for literals in index_stream.chunks(literals_per_clear) {
        append_fixed_literal_code(
            output_pointer,
            &mut output_position,
            &mut bits,
            &mut bit_count,
            clear_code,
            CODE_BITS,
        );
        let mut groups = literals.chunks_exact(8);
        for group in &mut groups {
            let packed = u64::from(group[0])
                | (u64::from(group[1]) << CODE_BITS)
                | (u64::from(group[2]) << (CODE_BITS * 2))
                | (u64::from(group[3]) << (CODE_BITS * 3))
                | (u64::from(group[4]) << (CODE_BITS * 4))
                | (u64::from(group[5]) << (CODE_BITS * 5))
                | (u64::from(group[6]) << (CODE_BITS * 6))
                | (u64::from(group[7]) << (CODE_BITS * 7));
            let total_bits = bit_count + CODE_BITS * 8;
            let combined = bits | (packed << bit_count);
            let byte_count = total_bits / 8;
            let bytes = combined.to_le_bytes();
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    output_pointer.add(output_position),
                    byte_count,
                );
            }
            output_position += byte_count;
            bits = combined >> (byte_count * 8);
            bit_count = total_bits - byte_count * 8;
        }
        for &pixel in groups.remainder() {
            append_fixed_literal_code(
                output_pointer,
                &mut output_position,
                &mut bits,
                &mut bit_count,
                pixel,
                CODE_BITS,
            );
        }
    }

    append_fixed_literal_code(
        output_pointer,
        &mut output_position,
        &mut bits,
        &mut bit_count,
        eoi_code,
        CODE_BITS,
    );
    while bit_count > 0 {
        unsafe { output_pointer.add(output_position).write(bits as u8) };
        output_position += 1;
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(output_position, raw_length);
    Ok(())
}

fn encode_six_bit_literal_codes(output: &mut Vec<u8>, index_stream: &[u8]) -> Result<(), String> {
    encode_fixed_width_literal_codes::<6>(output, index_stream, 32, 33, 30)
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
    resize_output_uninitialized(output, output_start + raw_length);
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
            output.extend_from_slice(&bits.to_le_bytes()[..7]);
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
    resize_output_uninitialized(output, output_start + writable_length);
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

#[inline(always)]
fn encode_rgba_two_bit_literal_source_to<F>(
    output: &mut Vec<u8>,
    pixel_count: usize,
    mut next_index: F,
) -> Result<(), String>
where
    F: FnMut() -> Option<u8>,
{
    if pixel_count == 0 {
        return Err("Indexed pixel stream is empty".to_string());
    }
    let pair_count = pixel_count / 2;
    let raw_bit_length = pair_count * 9 + usize::from(pixel_count % 2 != 0) * 6 + 3;
    let raw_length = raw_bit_length.div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    let output_length = output_start + 2 + block_count + raw_length;
    resize_output_uninitialized(output, output_length);
    output[output_start] = 2;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let mut remaining_pairs = pair_count;
    while remaining_pairs >= 7 {
        let mut packed = 0u64;
        for pair in 0..7 {
            let first =
                u64::from(next_index().ok_or_else(|| "Indexed pixel stream is empty".to_string())?);
            let second =
                u64::from(next_index().ok_or_else(|| "Indexed pixel stream is empty".to_string())?);
            packed |= (4 | (first << 3) | (second << 6)) << (pair * 9);
        }
        let combined = bits | (packed << bit_count);
        let total_bits = bit_count + 63;
        if total_bits >= 64 {
            writer.write_fixed_u64(combined, 8);
            bits = if bit_count == 0 {
                0
            } else {
                packed >> (64 - bit_count)
            };
            bit_count = total_bits - 64;
        } else {
            debug_assert_eq!(bit_count, 0);
            writer.write_fixed_u64(combined, 7);
            bits = combined >> 56;
            bit_count = total_bits - 56;
        }
        remaining_pairs -= 7;
    }
    while remaining_pairs > 0 {
        let first =
            u64::from(next_index().ok_or_else(|| "Indexed pixel stream is empty".to_string())?);
        let second =
            u64::from(next_index().ok_or_else(|| "Indexed pixel stream is empty".to_string())?);
        let packed = 4 | (first << 3) | (second << 6);
        let combined = bits | (packed << bit_count);
        let total_bits = bit_count + 9;
        let byte_count = total_bits / 8;
        writer.write_fixed_u64(combined, byte_count);
        bits = combined >> (byte_count * 8);
        bit_count = total_bits - byte_count * 8;
        remaining_pairs -= 1;
    }
    if pixel_count % 2 != 0 {
        let pixel =
            u64::from(next_index().ok_or_else(|| "Indexed pixel stream is empty".to_string())?);
        bits |= (4 | (pixel << 3)) << bit_count;
        bit_count += 6;
    }
    bits |= 5 << bit_count;
    bit_count += 3;
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
    output[output_start + 1 + block_count + raw_length] = 0;
    Ok(())
}

#[inline(always)]
fn encode_rgba_five_bit_literal_source_to<F>(
    output: &mut Vec<u8>,
    pixel_count: usize,
    mut next_index: F,
) -> Result<(), String>
where
    F: FnMut() -> Option<u8>,
{
    if pixel_count == 0 {
        return Err("Indexed pixel stream is empty".to_string());
    }
    const CODE_BITS: usize = 5;
    const CLEAR: u16 = 16;
    const EOI: u16 = 17;
    const LITERALS_PER_CLEAR: usize = 14;
    let clear_count = pixel_count.div_ceil(LITERALS_PER_CLEAR);
    let raw_length = ((pixel_count + clear_count + 1) * CODE_BITS).div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + 2 + block_count + raw_length + 8);
    output[output_start] = CODE_BITS as u8 - 1;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let mut remaining = pixel_count;
    while remaining > 0 {
        bits |= u64::from(CLEAR) << bit_count;
        bit_count += CODE_BITS;
        while bit_count >= 8 {
            writer.write_byte(bits as u8);
            bits >>= 8;
            bit_count -= 8;
        }
        let literals = remaining.min(LITERALS_PER_CLEAR);
        let mut groups = literals / 8;
        while groups > 0 {
            let mut packed = 0u64;
            for index in 0..8 {
                let pixel = u64::from(
                    next_index().ok_or_else(|| "Indexed pixel stream is empty".to_string())?,
                );
                packed |= pixel << (index * CODE_BITS);
            }
            let combined = bits | (packed << bit_count);
            let total_bits = bit_count + CODE_BITS * 8;
            let byte_count = total_bits / 8;
            writer.write_fixed_u64(combined, byte_count);
            bits = combined >> (byte_count * 8);
            bit_count = total_bits - byte_count * 8;
            groups -= 1;
        }
        for _ in 0..(literals % 8) {
            let pixel =
                u64::from(next_index().ok_or_else(|| "Indexed pixel stream is empty".to_string())?);
            bits |= pixel << bit_count;
            bit_count += CODE_BITS;
            while bit_count >= 8 {
                writer.write_byte(bits as u8);
                bits >>= 8;
                bit_count -= 8;
            }
        }
        remaining -= literals;
    }
    bits |= u64::from(EOI) << bit_count;
    bit_count += CODE_BITS;
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
    let final_length = output_start + 2 + block_count + raw_length;
    output[final_length - 1] = 0;
    output.truncate(final_length);
    Ok(())
}

#[inline(always)]
fn encode_rgba_two_bit_literal_frame_to(
    output: &mut Vec<u8>,
    frame: &[u8],
    mapper: &PaletteMapper<'_>,
    alpha_threshold: u8,
    exact_alpha: bool,
    exact_palette: bool,
) -> Result<(), String> {
    let pointer = frame.as_ptr();
    let pixel_count = frame.len() / 4;
    let mut offset = 0usize;
    let mut invalid_alpha = false;
    let mut invalid_palette = false;
    let mut cache_keys = [u32::MAX; 16];
    let mut cache_values = [0u8; 16];
    encode_rgba_two_bit_literal_source_to(output, pixel_count, || {
        let packed =
            unsafe { u32::from_le(std::ptr::read_unaligned(pointer.add(offset).cast::<u32>())) };
        offset += 4;
        let alpha = (packed >> 24) as u8;
        if alpha < alpha_threshold || (exact_alpha && alpha != 255) {
            invalid_alpha = true;
        }
        let rgb = packed & 0x00ff_ffff;
        let slot = (rgb as usize).wrapping_mul(2_654_435_761) & 15;
        let index = if cache_keys[slot] == rgb {
            cache_values[slot]
        } else {
            let index = if exact_palette {
                mapper.exact_index(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8)
            } else {
                Some(mapper.index_pixel(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8))
            };
            let index = index.unwrap_or_else(|| {
                invalid_palette = true;
                0
            });
            cache_keys[slot] = rgb;
            cache_values[slot] = index;
            index
        };
        Some(index)
    })?;
    if invalid_alpha || invalid_palette {
        return Err(RGBA_DELTA_FALLBACK.to_string());
    }
    Ok(())
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn encode_rgba_two_bit_literal_rect_to(
    output: &mut Vec<u8>,
    rgba_stream: &[u8],
    canvas_width: usize,
    rect: ChangedRectU32,
    mapper: &PaletteMapper<'_>,
    alpha_threshold: u8,
    exact_alpha: bool,
    exact_palette: bool,
) -> Result<(), String> {
    let pointer = rgba_stream.as_ptr();
    let pixel_count = rect.width * rect.height;
    let row_skip = (canvas_width - rect.width) * 4;
    let mut offset = (rect.y * canvas_width + rect.x) * 4;
    let mut row_remaining = rect.width;
    let mut invalid_alpha = false;
    let mut invalid_palette = false;
    let mut cache_keys = [u32::MAX; 16];
    let mut cache_values = [0u8; 16];
    encode_rgba_two_bit_literal_source_to(output, pixel_count, || {
        let packed =
            unsafe { u32::from_le(std::ptr::read_unaligned(pointer.add(offset).cast::<u32>())) };
        offset += 4;
        let alpha = (packed >> 24) as u8;
        if alpha < alpha_threshold || (exact_alpha && alpha != 255) {
            invalid_alpha = true;
        }
        row_remaining -= 1;
        if row_remaining == 0 {
            offset += row_skip;
            row_remaining = rect.width;
        }
        let rgb = packed & 0x00ff_ffff;
        let slot = (rgb as usize).wrapping_mul(2_654_435_761) & 15;
        let index = if cache_keys[slot] == rgb {
            cache_values[slot]
        } else {
            let index = if exact_palette {
                mapper.exact_index(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8)
            } else {
                Some(mapper.index_pixel(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8))
            };
            let index = index.unwrap_or_else(|| {
                invalid_palette = true;
                0
            });
            cache_keys[slot] = rgb;
            cache_values[slot] = index;
            index
        };
        Some(index)
    })?;
    if invalid_alpha || invalid_palette {
        return Err(RGBA_DELTA_FALLBACK.to_string());
    }
    Ok(())
}

#[inline(always)]
fn encode_rgba_five_bit_literal_frame_to(
    output: &mut Vec<u8>,
    frame: &[u8],
    mapper: &PaletteMapper<'_>,
    alpha_threshold: u8,
    exact_alpha: bool,
    exact_palette: bool,
) -> Result<(), String> {
    let pointer = frame.as_ptr();
    let pixel_count = frame.len() / 4;
    let mut offset = 0usize;
    let mut invalid_alpha = false;
    let mut invalid_palette = false;
    // The small-palette exact path sees at most sixteen distinct RGB keys;
    // a 64-slot direct-mapped cache removes most collisions without making
    // each frame pay for a large cold cache.
    let mut cache_keys = [u32::MAX; 64];
    let mut cache_values = [0u8; 64];
    encode_rgba_five_bit_literal_source_to(output, pixel_count, || {
        let packed =
            unsafe { u32::from_le(std::ptr::read_unaligned(pointer.add(offset).cast::<u32>())) };
        offset += 4;
        let alpha = (packed >> 24) as u8;
        if alpha < alpha_threshold || (exact_alpha && alpha != 255) {
            invalid_alpha = true;
        }
        let rgb = packed & 0x00ff_ffff;
        let slot = (rgb as usize).wrapping_mul(2_654_435_761) & 63;
        let index = if cache_keys[slot] == rgb {
            cache_values[slot]
        } else {
            let index = if exact_palette {
                mapper.exact_index(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8)
            } else {
                Some(mapper.index_pixel(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8))
            };
            let index = index.unwrap_or_else(|| {
                invalid_palette = true;
                0
            });
            cache_keys[slot] = rgb;
            cache_values[slot] = index;
            index
        };
        Some(index)
    })?;
    if invalid_alpha || invalid_palette {
        return Err(RGBA_DELTA_FALLBACK.to_string());
    }
    Ok(())
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
    resize_output_uninitialized(output, output_start + raw_length);
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
    tables.ensure_capacity(color_count);
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
            if next_code > code_mask && code_size < 12 {
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
    tables.ensure_capacity(256);
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

#[allow(clippy::too_many_arguments)]
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
    tables.ensure_capacity(color_count);

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
            if next_code > code_mask && code_size < 12 {
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

#[allow(clippy::too_many_arguments)]
fn encode_rgba_lzw_rect_to_palette(
    output: &mut Vec<u8>,
    rgba_stream: &[u8],
    canvas_width: usize,
    rect: ChangedRectU32,
    mapper: &PaletteMapper<'_>,
    min_code_size: u8,
    color_count: usize,
    tables: &mut LzwEncodeTables,
) -> Result<(), String> {
    let pixel_count = rect.width * rect.height;
    let row_skip = (canvas_width - rect.width) * 4;
    let pointer = rgba_stream.as_ptr();
    let start = (rect.y * canvas_width + rect.x) * 4;
    if mapper.palette_rgb.len() <= 16 {
        let mut offset = start;
        let mut row_remaining = rect.width;
        let mut cache_keys = [u32::MAX; 16];
        let mut cache_values = [0u8; 16];
        return encode_indexed_lzw_source_to(
            output,
            pixel_count,
            min_code_size,
            color_count,
            tables,
            || {
                let packed = unsafe {
                    u32::from_le(std::ptr::read_unaligned(pointer.add(offset).cast::<u32>()))
                };
                offset += 4;
                row_remaining -= 1;
                if row_remaining == 0 {
                    offset += row_skip;
                    row_remaining = rect.width;
                }
                let rgb = packed & 0x00ff_ffff;
                let slot = (rgb as usize).wrapping_mul(2_654_435_761) & 15;
                let index = if cache_keys[slot] == rgb {
                    cache_values[slot]
                } else {
                    let index =
                        mapper.index_pixel(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8);
                    cache_keys[slot] = rgb;
                    cache_values[slot] = index;
                    index
                };
                Some(index)
            },
        );
    }

    let mut offset = start;
    let mut row_remaining = rect.width;
    let mut cached_rgb = u32::MAX;
    let mut cached_index = 0u8;
    encode_indexed_lzw_source_to(
        output,
        pixel_count,
        min_code_size,
        color_count,
        tables,
        || {
            let packed = unsafe {
                u32::from_le(std::ptr::read_unaligned(pointer.add(offset).cast::<u32>()))
            };
            offset += 4;
            row_remaining -= 1;
            if row_remaining == 0 {
                offset += row_skip;
                row_remaining = rect.width;
            }
            let rgb = packed & 0x00ff_ffff;
            let index = if rgb == cached_rgb {
                cached_index
            } else {
                let index =
                    mapper.index_pixel(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8);
                cached_rgb = rgb;
                cached_index = index;
                index
            };
            Some(index)
        },
    )
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

#[cfg(not(feature = "encode-only"))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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
            #[cfg(not(feature = "encode-only"))]
            PixelFormat::Bgra => b | (g << 8) | (r << 16) | (255 << 24),
        });
    }

    Ok(palette)
}

#[cfg(not(feature = "encode-only"))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn blit_indices_to_uninit_full_canvas_u32(
    palette: &[u32],
    indices: &[u8],
    output: &mut [std::mem::MaybeUninit<u32>],
) -> Result<(), String> {
    if indices.len() != output.len() {
        return Err("Decoded index buffer is the wrong size".to_string());
    }
    if palette.len() == 256 {
        let indices_pointer = indices.as_ptr();
        let palette_pointer = palette.as_ptr();
        let output_pointer = output.as_mut_ptr().cast::<u32>();
        let mut pixel_index = 0usize;
        while pixel_index + 8 <= indices.len() {
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
                    output_pointer.add(pixel_index),
                    colors.len(),
                );
            }
            pixel_index += 8;
        }
        while pixel_index < indices.len() {
            let index = unsafe { *indices_pointer.add(pixel_index) };
            if usize::from(index) >= palette.len() {
                return Err(format!("Palette index {index} exceeds palette size"));
            }
            unsafe {
                output_pointer
                    .add(pixel_index)
                    .write(*palette_pointer.add(usize::from(index)));
            }
            pixel_index += 1;
        }
        return Ok(());
    }

    for (pixel, &index) in output.iter_mut().zip(indices) {
        let color = palette
            .get(usize::from(index))
            .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
        pixel.write(*color);
    }
    Ok(())
}

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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
#[cfg(not(feature = "encode-only"))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn lzw_decode_to_indices_direct(
    min_code_size: u8,
    image_data: &[u8],
    output: &mut [u8],
) -> Result<(), String> {
    let mut scratch = LzwStackScratch::default();
    lzw_decode_to_indices_direct_with_scratch(min_code_size, image_data, output, &mut scratch)
}

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn lzw_decode_to_indices_direct_with_scratch(
    min_code_size: u8,
    image_data: &[u8],
    output: &mut [u8],
    scratch: &mut LzwStackScratch,
) -> Result<(), String> {
    if min_code_size > 11 {
        return Err(format!("Invalid LZW minimum code size {min_code_size}"));
    }
    // wtfgif's literal writer emits a fixed-width 9-bit stream for a full
    // 256-color palette: CLEAR, raw indices, periodic CLEAR, then EOI.  It is
    // still a normal GIF LZW stream, so this path is safe for any input; the
    // parser falls through to the complete dictionary decoder as soon as it
    // sees a dictionary code.  Keeping this check here also makes decoding
    // our larger lossless GIFs a straight bit-unpack instead of constructing
    // one dictionary entry per pixel.
    if min_code_size == 8
        && image_data.len() >= output.len()
        && decode_literal_9_bit_stream(image_data, output)
    {
        return Ok(());
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

                if next_code > code_mask && code_size < 12 {
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

/// Decode a fixed-width 9-bit literal-only LZW stream. Return `false` for a
/// normal dictionary-compressed stream so the caller can use the general
/// decoder without changing its validation or tie behavior.
#[inline(always)]
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn decode_literal_9_bit_stream(image_data: &[u8], output: &mut [u8]) -> bool {
    const CLEAR: u16 = 256;
    const EOI: u16 = 257;
    let clear_count = output.len().div_ceil(254);
    let Some(code_count) = output
        .len()
        .checked_add(clear_count)
        .and_then(|count| count.checked_add(1))
    else {
        return false;
    };
    let Some(expected_length) = code_count.checked_mul(9).map(|bits| bits.div_ceil(8)) else {
        return false;
    };
    if image_data.len() != expected_length {
        return false;
    }
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let mut offset = 0usize;
    let mut output_index = 0usize;
    let mut saw_clear = false;
    let output_length = output.len();
    let output_pointer = output.as_mut_ptr();

    loop {
        while bit_count < 9 && offset < image_data.len() {
            if bit_count <= 32 && image_data.len().saturating_sub(offset) >= 4 {
                let word = unsafe {
                    std::ptr::read_unaligned(image_data.as_ptr().add(offset) as *const u32)
                };
                bits |= u64::from(u32::from_le(word)) << bit_count;
                bit_count += 32;
                offset += 4;
            } else {
                bits |= u64::from(image_data[offset]) << bit_count;
                bit_count += 8;
                offset += 1;
            }
        }
        if bit_count < 9 {
            return false;
        }
        let code = (bits & 0x01ff) as u16;
        bits >>= 9;
        bit_count -= 9;

        if code == CLEAR {
            saw_clear = true;
            continue;
        }
        if code == EOI {
            return saw_clear && output_index == output_length;
        }
        if code >= CLEAR || output_index >= output_length {
            return false;
        }
        unsafe { output_pointer.add(output_index).write(code as u8) };
        output_index += 1;
    }
}

/// Decode a literal-only GIF stream whose code width is one bit wider than
/// the minimum code size.  wtfgif uses these streams for small indexed
/// palettes; validating the exact packed length and every clear/EOI marker
/// keeps this a safe probe for ordinary dictionary-compressed GIFs too.
#[inline(always)]
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn decode_fixed_literal_stream<const CODE_BITS: usize>(
    image_data: &[u8],
    output: &mut [u8],
) -> bool {
    if !(3..=9).contains(&CODE_BITS) {
        return false;
    }
    let min_code_size = CODE_BITS - 1;
    let clear = 1usize << min_code_size;
    let literals_per_clear = clear - 2;
    let clear_count = output.len().div_ceil(literals_per_clear);
    let Some(code_count) = output
        .len()
        .checked_add(clear_count)
        .and_then(|count| count.checked_add(1))
    else {
        return false;
    };
    let Some(expected_length) = code_count
        .checked_mul(CODE_BITS)
        .map(|bits| bits.div_ceil(8))
    else {
        return false;
    };
    if image_data.len() != expected_length {
        return false;
    }

    let eoi = clear + 1;
    let code_mask = (1usize << CODE_BITS) - 1;
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let mut offset = 0usize;
    let mut output_index = 0usize;
    let mut saw_clear = false;
    let output_pointer = output.as_mut_ptr();

    loop {
        while bit_count < CODE_BITS && offset < image_data.len() {
            if bit_count <= 32 && image_data.len().saturating_sub(offset) >= 4 {
                let word = unsafe {
                    std::ptr::read_unaligned(image_data.as_ptr().add(offset) as *const u32)
                };
                bits |= u64::from(u32::from_le(word)) << bit_count;
                bit_count += 32;
                offset += 4;
            } else {
                bits |= u64::from(image_data[offset]) << bit_count;
                bit_count += 8;
                offset += 1;
            }
        }
        if bit_count < CODE_BITS {
            return false;
        }
        let code = (bits as usize) & code_mask;
        bits >>= CODE_BITS;
        bit_count -= CODE_BITS;

        if code == clear {
            saw_clear = true;
            continue;
        }
        if code == eoi {
            return saw_clear && output_index == output.len();
        }
        if code >= clear || output_index >= output.len() {
            return false;
        }
        unsafe { output_pointer.add(output_index).write(code as u8) };
        output_index += 1;
    }
}

/// Decode a fixed-width literal stream directly into palette-colored u32
/// output.  The byte portion of the destination is used as temporary index
/// storage and is consumed backwards, so no second frame-sized allocation is
/// needed.
#[inline(always)]
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn decode_fixed_literal_pixels<const CODE_BITS: usize>(
    image_data: &[u8],
    palette: &[u32],
    output: &mut [u32],
) -> bool {
    let output_bytes =
        unsafe { std::slice::from_raw_parts_mut(output.as_mut_ptr().cast::<u8>(), output.len()) };
    if !decode_fixed_literal_stream::<CODE_BITS>(image_data, output_bytes) {
        return false;
    }
    let output_pointer = output.as_mut_ptr();
    let palette_pointer = palette.as_ptr();
    let palette_length = palette.len();
    for pixel in (0..output.len()).rev() {
        let index = usize::from(unsafe { *output_bytes.get_unchecked(pixel) });
        if index >= palette_length {
            return false;
        }
        unsafe { output_pointer.add(pixel).write(*palette_pointer.add(index)) };
    }
    true
}

#[inline(always)]
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[inline(always)]
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
unsafe fn copy_lzw_dictionary_color_string(
    source: *const u32,
    destination: *mut u32,
    length: usize,
) {
    std::ptr::copy_nonoverlapping(source, destination, length);
}

/// Decode an opaque full-canvas frame directly into palette colors. The
/// ordinary decoder first writes one byte index per pixel and then performs a
/// second palette lookup pass; this keeps the same LZW dictionary algorithm
/// while making that mapping part of the output loop.
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn lzw_decode_to_pixels_copy_with_scratch(
    min_code_size: u8,
    image_data: &[u8],
    palette: &[u32],
    output: &mut [u32],
    scratch: &mut LzwStackScratch,
) -> Result<(), String> {
    if min_code_size > 11 {
        return Err(format!("Invalid LZW minimum code size {min_code_size}"));
    }
    let fixed_literal = match min_code_size {
        2 => decode_fixed_literal_pixels::<3>(image_data, palette, output),
        3 => decode_fixed_literal_pixels::<4>(image_data, palette, output),
        4 => decode_fixed_literal_pixels::<5>(image_data, palette, output),
        5 => decode_fixed_literal_pixels::<6>(image_data, palette, output),
        6 => decode_fixed_literal_pixels::<7>(image_data, palette, output),
        7 => decode_fixed_literal_pixels::<8>(image_data, palette, output),
        _ => false,
    };
    if fixed_literal {
        return Ok(());
    }
    if min_code_size == 8
        && image_data.len() >= output.len()
        && decode_literal_9_bit_pixels(image_data, palette, output)
    {
        return Ok(());
    }

    let clear = 1usize << min_code_size;
    let eoi = clear + 1;
    let mut next_code = eoi + 1;
    let mut code_size = usize::from(min_code_size) + 1;
    let mut code_mask = (1usize << code_size) - 1;
    let string_length = &mut scratch.string_length;
    let string_start = &mut scratch.string_start;

    let mut output_index = 0usize;
    let mut q = 0usize;
    let mut bits = 0usize;
    let mut bit_count = 0usize;
    let mut have_previous = false;
    let mut previous_start = 0usize;
    let mut previous_length = 0usize;
    let mut previous_first = 0u32;

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
            let color = palette
                .get(code)
                .copied()
                .ok_or_else(|| format!("Palette index {code} exceeds palette size"))?;
            output[output_index] = color;
            output_index += 1;
            (1usize, color)
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
                copy_lzw_dictionary_color_string(
                    output.as_ptr().add(source_start),
                    output.as_mut_ptr().add(output_index),
                    decoded_length,
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
                copy_lzw_dictionary_color_string(
                    output.as_ptr().add(previous_start),
                    output.as_mut_ptr().add(output_index),
                    previous_length,
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
            if next_code > code_mask && code_size < 12 {
                code_size += 1;
                code_mask = (1usize << code_size) - 1;
            }
        }
        previous_start = current_start;
        previous_length = decoded_length;
        previous_first = out_first;
        have_previous = true;
    }

    if output_index != output.len() {
        return Err("LZW decoded output is shorter than frame dimensions".to_string());
    }
    Ok(())
}

#[inline(always)]
#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn decode_literal_9_bit_pixels(image_data: &[u8], palette: &[u32], output: &mut [u32]) -> bool {
    const CLEAR: u16 = 256;
    const EOI: u16 = 257;
    let clear_count = output.len().div_ceil(254);
    let Some(code_count) = output
        .len()
        .checked_add(clear_count)
        .and_then(|count| count.checked_add(1))
    else {
        return false;
    };
    let Some(expected_length) = code_count.checked_mul(9).map(|bits| bits.div_ceil(8)) else {
        return false;
    };
    if image_data.len() != expected_length {
        return false;
    }
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let mut offset = 0usize;
    let mut output_index = 0usize;
    let mut saw_clear = false;
    let output_length = output.len();
    let output_pointer = output.as_mut_ptr();
    let palette_length = palette.len();
    let palette_pointer = palette.as_ptr();

    loop {
        while bit_count < 9 && offset < image_data.len() {
            if bit_count <= 32 && image_data.len().saturating_sub(offset) >= 4 {
                let word = unsafe {
                    std::ptr::read_unaligned(image_data.as_ptr().add(offset) as *const u32)
                };
                bits |= u64::from(u32::from_le(word)) << bit_count;
                bit_count += 32;
                offset += 4;
            } else {
                bits |= u64::from(image_data[offset]) << bit_count;
                bit_count += 8;
                offset += 1;
            }
        }
        if bit_count < 9 {
            return false;
        }
        let code = (bits & 0x01ff) as u16;
        bits >>= 9;
        bit_count -= 9;

        if code == CLEAR {
            saw_clear = true;
            continue;
        }
        if code == EOI {
            return saw_clear && output_index == output_length;
        }
        if code >= CLEAR || output_index >= output_length {
            return false;
        }
        let palette_index = usize::from(code);
        if palette_index >= palette_length {
            return false;
        }
        unsafe {
            output_pointer
                .add(output_index)
                .write(*palette_pointer.add(palette_index));
        }
        output_index += 1;
    }
}

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn lzw_decode_to_indices_copy_with_scratch(
    min_code_size: u8,
    image_data: &[u8],
    output: &mut [u8],
    scratch: &mut LzwStackScratch,
) -> Result<(), String> {
    if min_code_size > 11 {
        return Err(format!("Invalid LZW minimum code size {min_code_size}"));
    }
    let fixed_literal = match min_code_size {
        2 => decode_fixed_literal_stream::<3>(image_data, output),
        3 => decode_fixed_literal_stream::<4>(image_data, output),
        4 => decode_fixed_literal_stream::<5>(image_data, output),
        5 => decode_fixed_literal_stream::<6>(image_data, output),
        6 => decode_fixed_literal_stream::<7>(image_data, output),
        7 => decode_fixed_literal_stream::<8>(image_data, output),
        _ => false,
    };
    if fixed_literal {
        return Ok(());
    }
    // wtfgif's literal writer emits one fixed-width 9-bit code per output
    // index, making the payload at least as large as the decoded frame. This
    // conservative size gate avoids probing ordinary compressed GIF streams
    // while letting our own full-palette output skip dictionary construction.
    if min_code_size == 8
        && image_data.len() >= output.len()
        && decode_literal_9_bit_stream(image_data, output)
    {
        return Ok(());
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
            if next_code > code_mask && code_size < 12 {
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
struct LzwStackScratch {
    prefix: [std::mem::MaybeUninit<u16>; 4096],
    suffix: [std::mem::MaybeUninit<u8>; 4096],
    stack: [std::mem::MaybeUninit<u8>; 4096],
    string_length: [std::mem::MaybeUninit<u16>; 4096],
    string_start: [std::mem::MaybeUninit<u32>; 4096],
}

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

                if next_code > code_mask && code_size < 12 {
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
fn read_u16(data: &[u8], offset: usize, context: &str) -> Result<u16, String> {
    let end = checked_add(offset, 2, data.len(), context)?;
    Ok(u16::from_le_bytes([data[offset], data[end - 1]]))
}

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
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

#[cfg(not(feature = "encode-only"))]
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

#[cfg(not(feature = "encode-only"))]
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

#[cfg(not(feature = "encode-only"))]
fn push_json_field_str(json: &mut String, key: &str, value: &str) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":\"");
    json.push_str(value);
    json.push('"');
}

#[cfg(not(feature = "encode-only"))]
fn push_json_field_bool(json: &mut String, key: &str, value: bool) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":");
    json.push_str(if value { "true" } else { "false" });
}

#[cfg(not(feature = "encode-only"))]
fn push_json_field_u8(json: &mut String, key: &str, value: u8) {
    push_json_field_number(json, key, usize::from(value));
}

#[cfg(not(feature = "encode-only"))]
fn push_json_field_u16(json: &mut String, key: &str, value: u16) {
    push_json_field_number(json, key, usize::from(value));
}

#[cfg(not(feature = "encode-only"))]
fn push_json_field_usize(json: &mut String, key: &str, value: usize) {
    push_json_field_number(json, key, value);
}

#[cfg(not(feature = "encode-only"))]
fn push_json_field_u8_option(json: &mut String, key: &str, value: Option<u8>) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":");
    match value {
        Some(value) => json.push_str(&value.to_string()),
        None => json.push_str("null"),
    }
}

#[cfg(not(feature = "encode-only"))]
fn push_json_field_usize_option(json: &mut String, key: &str, value: Option<usize>) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":");
    match value {
        Some(value) => json.push_str(&value.to_string()),
        None => json.push_str("null"),
    }
}

#[cfg(not(feature = "encode-only"))]
fn push_json_field_number(json: &mut String, key: &str, value: usize) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":");
    json.push_str(&value.to_string());
}

#[cfg(all(test, not(feature = "encode-only")))]
mod tests;

#[cfg(all(test, feature = "encode-only"))]
mod encode_only_tests;
