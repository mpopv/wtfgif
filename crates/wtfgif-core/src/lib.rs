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
    frames: Vec<FrameMetadata>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PixelFormat {
    Rgba,
    Bgra,
}

const COMPOSITED_DELTA_MAGIC: u32 = 0x3144_4757;
const COMPOSITED_DELTA_VERSION: u32 = 1;
const COMPOSITED_DELTA_HEADER_LEN: usize = 4;
const COMPOSITED_DELTA_ENTRY_LEN: usize = 9;
const LZW_TABLE_CAP: usize = 16_384;
const COLOR_INDEX_CAP: usize = 1_024;

#[wasm_bindgen]
pub struct WtfGifCore {
    data: Vec<u8>,
    metadata: GifMetadata,
}

#[wasm_bindgen]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[wasm_bindgen]
pub fn parse_metadata_json(data: &[u8]) -> Result<String, JsValue> {
    parse_metadata(data)
        .map(|metadata| metadata.to_json())
        .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn decode_frame_indices(data: &[u8], frame_index: usize) -> Result<Vec<u8>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    let frame = metadata
        .frames
        .get(frame_index)
        .ok_or_else(|| JsValue::from_str("Frame index out of range"))?;
    decode_frame_indices_inner(data, frame).map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn decode_frame_rgba(data: &[u8], frame_index: usize) -> Result<Vec<u8>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    decode_frame_pixels_inner(data, &metadata, frame_index, PixelFormat::Rgba)
        .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn decode_frame_bgra(data: &[u8], frame_index: usize) -> Result<Vec<u8>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    decode_frame_pixels_inner(data, &metadata, frame_index, PixelFormat::Bgra)
        .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn prepare_composited_rgba(data: &[u8], requested_frames: &[u8]) -> Result<Vec<u32>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    prepare_composited_frames_inner(data, &metadata, requested_frames, PixelFormat::Rgba)
        .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn prepare_composited_bgra(data: &[u8], requested_frames: &[u8]) -> Result<Vec<u32>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    prepare_composited_frames_inner(data, &metadata, requested_frames, PixelFormat::Bgra)
        .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
pub fn prepare_composited_delta_rgba(
    data: &[u8],
    requested_frames: &[u8],
) -> Result<Vec<u32>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
    prepare_composited_delta_frames_inner(data, &metadata, requested_frames, PixelFormat::Rgba)
        .map_err(|message| JsValue::from_str(&message))
}

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
        delay,
        loop_count,
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
        delay,
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
        delay,
        loop_count,
        deltas,
    )
    .map_err(|message| JsValue::from_str(&message))
}

#[wasm_bindgen]
impl WtfGifCore {
    #[wasm_bindgen(constructor)]
    pub fn new(data: &[u8]) -> Result<WtfGifCore, JsValue> {
        let metadata = parse_metadata(data).map_err(|message| JsValue::from_str(&message))?;
        Ok(WtfGifCore {
            data: data.to_vec(),
            metadata,
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
        frames,
    })
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
    let image_data = collect_image_data(data, frame.data_offset)?;
    let frame_size = usize::from(frame.width) * usize::from(frame.height);
    let linear = lzw_decode_to_indices(frame.min_code_size, &image_data, frame_size)?;

    if !frame.interlaced {
        return Ok(linear);
    }

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

fn prepare_composited_frames_inner(
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
    let output_pixels = requested_count
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    let mut output = Vec::with_capacity(output_pixels);
    let mut canvas = vec![0u32; canvas_pixels];

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
            output.extend_from_slice(&canvas);
        }

        apply_frame_disposal_u32(&mut canvas, metadata.width, metadata.height, frame, restore);
    }

    Ok(output)
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

struct ColorIndexTable {
    keys: Vec<i32>,
    values: Vec<u8>,
}

impl ColorIndexTable {
    fn new() -> Self {
        Self {
            keys: vec![-1; COLOR_INDEX_CAP],
            values: vec![0; COLOR_INDEX_CAP],
        }
    }

    #[inline]
    fn get(&self, key: u32) -> Option<u8> {
        let key = key as i32;
        let mut slot = (key as usize).wrapping_mul(2_654_435_761) & (COLOR_INDEX_CAP - 1);
        loop {
            let stored = self.keys[slot];
            if stored == key {
                return Some(self.values[slot]);
            }
            if stored == -1 {
                return None;
            }
            slot = (slot + 1) & (COLOR_INDEX_CAP - 1);
        }
    }

    #[inline]
    fn insert_if_absent(&mut self, key: u32, value: u8) {
        let key = key as i32;
        let mut slot = (key as usize).wrapping_mul(2_654_435_761) & (COLOR_INDEX_CAP - 1);
        loop {
            let stored = self.keys[slot];
            if stored == key {
                return;
            }
            if stored == -1 {
                self.keys[slot] = key;
                self.values[slot] = value;
                return;
            }
            slot = (slot + 1) & (COLOR_INDEX_CAP - 1);
        }
    }
}

struct PaletteMapper<'a> {
    palette_rgb: &'a [u32],
    exact: ColorIndexTable,
}

impl<'a> PaletteMapper<'a> {
    fn new(palette_rgb: &'a [u32]) -> Self {
        let mut exact = ColorIndexTable::new();
        for (index, color) in palette_rgb.iter().enumerate() {
            exact.insert_if_absent(color & 0x00ff_ffff, index as u8);
        }
        Self { palette_rgb, exact }
    }

    #[inline]
    fn index_pixel(&self, r: u8, g: u8, b: u8) -> u8 {
        let rgb = rgb_key(r, g, b);
        self.exact
            .get(rgb)
            .unwrap_or_else(|| nearest_palette_index(r, g, b, self.palette_rgb))
    }
}

fn encode_rgba_gif_inner(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delay: u16,
    loop_count: i32,
    deltas: bool,
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

    if deltas && !palette_rgb.is_empty() {
        return encode_rgba_delta_gif_to_palette_inner(
            rgba_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delay,
            loop_count,
        );
    }

    let (palette, indexed) = index_rgba_frames(rgba_stream, palette_rgb)?;
    encode_indexed_gif_inner_with_rects(
        &indexed,
        width,
        height,
        frame_count,
        &palette,
        delay,
        loop_count,
        deltas,
    )
}

fn encode_rgba_delta_gif_to_palette_inner(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delay: u16,
    loop_count: i32,
) -> Result<Vec<u8>, String> {
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
    let mut lzw_tables = LzwEncodeTables::new();
    let mut mapped = Vec::with_capacity(frame_pixels);

    write_indexed_gif_header(&mut output, width, height, palette_rgb, color_count);
    write_loop_extension(&mut output, loop_count);

    let mut previous_frame: Option<&[u8]> = None;
    for frame in rgba_stream.chunks_exact(frame_bytes) {
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
                );
                map_rgba_rect_to_palette(frame, canvas_width, rect, &mapper, &mut mapped);
                encode_indexed_lzw_to_with_tables(
                    &mut output,
                    &mapped,
                    min_code_size,
                    color_count,
                    &mut lzw_tables,
                )?;
            } else {
                write_indexed_gif_frame_header(&mut output, 0, 0, 1, 1, delay);
                let noop = [mapper.index_pixel(frame[0], frame[1], frame[2])];
                encode_indexed_lzw_to_with_tables(
                    &mut output,
                    &noop,
                    min_code_size,
                    color_count,
                    &mut lzw_tables,
                )?;
            }
        } else {
            write_indexed_gif_frame_header(&mut output, 0, 0, width, height, delay);
            map_rgba_frame_to_palette(frame, &mapper, &mut mapped);
            encode_indexed_lzw_to_with_tables(
                &mut output,
                &mapped,
                min_code_size,
                color_count,
                &mut lzw_tables,
            )?;
        }
        previous_frame = Some(frame);
    }

    output.push(0x3b);
    Ok(output)
}

fn index_rgba_frames(
    rgba_stream: &[u8],
    palette_rgb: &[u32],
) -> Result<(Vec<u32>, Vec<u8>), String> {
    if !palette_rgb.is_empty() {
        checked_palette_color_count(palette_rgb.len())?;
        return Ok((
            palette_rgb
                .iter()
                .map(|color| color & 0x00ff_ffff)
                .collect(),
            index_rgba_frames_to_palette(rgba_stream, palette_rgb),
        ));
    }

    if let Some(exact) = try_index_rgba_frames_exact(rgba_stream) {
        return Ok(exact);
    }

    Ok(index_rgba_frames_332(rgba_stream))
}

fn try_index_rgba_frames_exact(rgba_stream: &[u8]) -> Option<(Vec<u32>, Vec<u8>)> {
    let mut table = ColorIndexTable::new();
    let mut palette = Vec::with_capacity(256);
    let mut indexed = Vec::with_capacity(rgba_stream.len() / 4);

    let mut offset = 0usize;
    while offset < rgba_stream.len() {
        let rgb = rgb_key(
            rgba_stream[offset],
            rgba_stream[offset + 1],
            rgba_stream[offset + 2],
        );
        if let Some(index) = table.get(rgb) {
            indexed.push(index);
            offset += 4;
            continue;
        }
        if palette.len() == 256 {
            return None;
        }

        let index = palette.len() as u8;
        table.insert_if_absent(rgb, index);
        palette.push(rgb);
        indexed.push(index);
        offset += 4;
    }

    Some((palette, indexed))
}

fn index_rgba_frames_to_palette(rgba_stream: &[u8], palette_rgb: &[u32]) -> Vec<u8> {
    let mapper = PaletteMapper::new(palette_rgb);
    let mut indexed = Vec::with_capacity(rgba_stream.len() / 4);
    map_rgba_frame_to_palette(rgba_stream, &mapper, &mut indexed);
    indexed
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

fn index_rgba_frames_332(rgba_stream: &[u8]) -> (Vec<u32>, Vec<u8>) {
    let mut indexed = Vec::with_capacity(rgba_stream.len() / 4);
    let mut offset = 0usize;
    while offset < rgba_stream.len() {
        indexed.push(rgb332_index(
            rgba_stream[offset],
            rgba_stream[offset + 1],
            rgba_stream[offset + 2],
        ));
        offset += 4;
    }
    (fixed_332_palette(), indexed)
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

fn encode_indexed_gif_inner(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delay: u16,
    loop_count: i32,
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
    let mut lzw_tables = LzwEncodeTables::new();

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

    for frame in index_stream.chunks_exact(frame_len) {
        if delay != 0 {
            output.extend_from_slice(&[0x21, 0xf9, 0x04, 0x00]);
            push_u16_le(&mut output, delay);
            output.extend_from_slice(&[0x00, 0x00]);
        }

        output.push(0x2c);
        push_u16_le(&mut output, 0);
        push_u16_le(&mut output, 0);
        push_u16_le(&mut output, width);
        push_u16_le(&mut output, height);
        output.push(0);

        encode_indexed_lzw_to_with_tables(
            &mut output,
            frame,
            min_code_size,
            color_count,
            &mut lzw_tables,
        )?;
    }

    output.push(0x3b);
    Ok(output)
}

fn encode_indexed_delta_gif_inner(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delay: u16,
    loop_count: i32,
) -> Result<Vec<u8>, String> {
    encode_indexed_gif_inner_with_rects(
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        delay,
        loop_count,
        true,
    )
}

fn encode_indexed_gif_inner_with_rects(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delay: u16,
    loop_count: i32,
    deltas: bool,
) -> Result<Vec<u8>, String> {
    if !deltas {
        return encode_indexed_gif_inner(
            index_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delay,
            loop_count,
        );
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
    let mut lzw_tables = LzwEncodeTables::new();

    write_indexed_gif_header(&mut output, width, height, palette_rgb, color_count);
    write_loop_extension(&mut output, loop_count);

    let mut previous_frame: Option<&[u8]> = None;
    for frame in index_stream.chunks_exact(frame_len) {
        if let Some(previous) = previous_frame {
            if let Some(rect) = find_changed_rect_u8(previous, frame, canvas_width, canvas_height) {
                write_indexed_gif_frame_header(
                    &mut output,
                    rect.x as u16,
                    rect.y as u16,
                    rect.width as u16,
                    rect.height as u16,
                    delay,
                );
                encode_indexed_lzw_rect_to(
                    &mut output,
                    frame,
                    rect.y * canvas_width + rect.x,
                    rect.width,
                    rect.height,
                    canvas_width,
                    min_code_size,
                    color_count,
                    &mut lzw_tables,
                )?;
            } else {
                write_indexed_gif_frame_header(&mut output, 0, 0, 1, 1, delay);
                encode_indexed_lzw_to_with_tables(
                    &mut output,
                    &frame[..1],
                    min_code_size,
                    color_count,
                    &mut lzw_tables,
                )?;
            }
        } else {
            write_indexed_gif_frame_header(&mut output, 0, 0, width, height, delay);
            encode_indexed_lzw_to_with_tables(
                &mut output,
                frame,
                min_code_size,
                color_count,
                &mut lzw_tables,
            )?;
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
) {
    if delay != 0 {
        output.extend_from_slice(&[0x21, 0xf9, 0x04, 0x00]);
        push_u16_le(output, delay);
        output.extend_from_slice(&[0x00, 0x00]);
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
    keys: Vec<i32>,
    values: Vec<u16>,
    generations: Vec<u32>,
    epoch: u32,
}

impl LzwEncodeTables {
    fn new() -> Self {
        Self {
            keys: vec![0; LZW_TABLE_CAP],
            values: vec![0; LZW_TABLE_CAP],
            generations: vec![0; LZW_TABLE_CAP],
            epoch: 0,
        }
    }

    fn reset(&mut self) -> u32 {
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.generations.fill(0);
            self.epoch = 1;
        }
        self.epoch
    }
}

fn encode_indexed_lzw_inner(
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<Vec<u8>, String> {
    let mut output = Vec::with_capacity(index_stream.len() / 2 + 16);
    encode_indexed_lzw_to(&mut output, index_stream, min_code_size, color_count)?;
    Ok(output)
}

fn encode_indexed_lzw_to(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<(), String> {
    let mut tables = LzwEncodeTables::new();
    encode_indexed_lzw_to_with_tables(
        output,
        index_stream,
        min_code_size,
        color_count,
        &mut tables,
    )
}

fn encode_indexed_lzw_to_with_tables(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
    tables: &mut LzwEncodeTables,
) -> Result<(), String> {
    let mut index = 0usize;
    encode_indexed_lzw_source_to(
        output,
        index_stream.len(),
        min_code_size,
        color_count,
        tables,
        || {
            let pixel = index_stream.get(index).copied();
            index += 1;
            pixel
        },
    )
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

        let key = ((ib << 8) | k) as i32;
        let mut slot = (key as usize) & (LZW_TABLE_CAP - 1);
        let mut found = None;
        while tables.generations[slot] == epoch {
            if tables.keys[slot] == key {
                found = Some(usize::from(tables.values[slot]));
                break;
            }
            slot = (slot + 1) & (LZW_TABLE_CAP - 1);
        }

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
            tables.generations[slot] = epoch;
            tables.keys[slot] = key;
            tables.values[slot] = next_code as u16;
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
            tables.generations[slot] = epoch;
            tables.keys[slot] = key;
            tables.values[slot] = next_code as u16;
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

#[inline]
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

#[inline]
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
    let mut palette = vec![0u32; frame.palette_size];

    for (index, color) in palette.iter_mut().enumerate() {
        let palette_offset = frame
            .palette_offset
            .checked_add(index * 3)
            .ok_or_else(|| "Palette offset overflow".to_string())?;
        let end = checked_add(palette_offset, 3, data.len(), "palette color")?;
        let r = u32::from(data[palette_offset]);
        let g = u32::from(data[palette_offset + 1]);
        let b = u32::from(data[end - 1]);
        *color = match format {
            PixelFormat::Rgba => r | (g << 8) | (b << 16) | (255 << 24),
            PixelFormat::Bgra => b | (g << 8) | (r << 16) | (255 << 24),
        };
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
    let transparent_index = frame.transparent_index;
    let mut src = 0usize;

    for y in 0..frame_height {
        let mut dst = (frame_y + y)
            .checked_mul(canvas_width)
            .and_then(|row| row.checked_add(frame_x))
            .ok_or_else(|| "Frame destination offset overflow".to_string())?;

        for _ in 0..frame_width {
            let index = *indices
                .get(src)
                .ok_or_else(|| "Decoded index buffer is too short".to_string())?;
            src += 1;

            if Some(index) != transparent_index {
                let color = palette
                    .get(usize::from(index))
                    .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
                let pixel = canvas
                    .get_mut(dst)
                    .ok_or_else(|| "Frame destination exceeds canvas bounds".to_string())?;
                *pixel = *color;
            }
            dst += 1;
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

fn collect_image_data(data: &[u8], data_offset: usize) -> Result<Vec<u8>, String> {
    if data_offset >= data.len() {
        return Err("Image data is missing an LZW minimum code size".to_string());
    }

    let mut offset = data_offset + 1;
    let mut image_data = Vec::new();
    loop {
        if offset >= data.len() {
            return Err("Truncated image data".to_string());
        }
        let length = usize::from(data[offset]);
        offset += 1;
        if length == 0 {
            return Ok(image_data);
        }
        let end = checked_add(offset, length, data.len(), "image data")?;
        image_data.extend_from_slice(&data[offset..end]);
        offset = end;
    }
}

fn lzw_decode_to_indices(
    min_code_size: u8,
    image_data: &[u8],
    output_len: usize,
) -> Result<Vec<u8>, String> {
    if min_code_size > 11 {
        return Err(format!("Invalid LZW minimum code size {min_code_size}"));
    }

    let clear = 1usize << min_code_size;
    let eoi = clear + 1;
    let mut next_code = eoi + 1;
    let mut code_size = usize::from(min_code_size) + 1;
    let mut code_mask = (1usize << code_size) - 1;

    let mut prefix = [0u16; 4096];
    let mut suffix = [0u8; 4096];
    let mut first_byte = [0u8; 4096];
    for i in 0..clear {
        first_byte[i] = i as u8;
    }

    let mut output = vec![0; output_len];
    let mut output_index = 0usize;
    let mut stack = [0u8; 4096];

    let mut q = 0usize;
    let mut bits = 0usize;
    let mut bit_count = 0usize;
    let mut prev_code: Option<usize> = None;

    loop {
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
            for i in 0..clear {
                first_byte[i] = i as u8;
            }
            continue;
        }

        if code == eoi {
            break;
        }

        let out_first;
        let mut cur = code;

        if cur < clear {
            out_first = cur as u8;
            write_index(&mut output, &mut output_index, out_first);
        } else {
            let mut sp = 0usize;
            if cur >= next_code {
                let Some(prev) = prev_code else {
                    break;
                };
                out_first = first_byte[prev];
                stack[sp] = out_first;
                sp += 1;
                cur = prev;
            } else {
                out_first = first_byte[cur];
            }

            while cur >= clear {
                if sp >= stack.len() {
                    return Err("LZW decode stack overflow".to_string());
                }
                stack[sp] = suffix[cur];
                sp += 1;
                cur = usize::from(prefix[cur]);
            }

            write_index(&mut output, &mut output_index, cur as u8);
            while sp > 0 {
                sp -= 1;
                write_index(&mut output, &mut output_index, stack[sp]);
            }
        }

        if let Some(prev) = prev_code {
            if next_code < 4096 {
                prefix[next_code] = prev as u16;
                suffix[next_code] = out_first;
                first_byte[next_code] = first_byte[prev];
                next_code += 1;

                if next_code >= code_mask + 1 && code_size < 12 {
                    code_size += 1;
                    code_mask = (1usize << code_size) - 1;
                }
            }
        }

        prev_code = Some(code);
    }

    Ok(output)
}

#[inline]
fn write_index(output: &mut [u8], output_index: &mut usize, value: u8) {
    if *output_index < output.len() {
        output[*output_index] = value;
        *output_index += 1;
    }
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
        let encoded =
            encode_indexed_gif_inner(&[1, 1, 1, 1, 2, 0, 0, 2], 2, 2, 2, &palette, 5, 0).unwrap();
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
            4,
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
            6,
            0,
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
            4,
            0,
            true,
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

        let encoded = encode_rgba_gif_inner(&rgba, 257, 1, 1, &[], 0, -1, false).unwrap();
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
}
