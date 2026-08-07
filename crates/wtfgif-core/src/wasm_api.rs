// wasm-bindgen exports mirror the explicit JavaScript ABI, whose scalar
// parameters are validated before entering the internal codec.
#![allow(clippy::too_many_arguments)]

use super::*;

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
#[wasm_bindgen]
pub fn encode_indexed_lzw(
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<Vec<u8>, JsValue> {
    encode_indexed_lzw_inner(index_stream, min_code_size, color_count)
        .map_err(|message| JsValue::from_str(&message))
}

#[cfg(not(feature = "quality-only"))]
#[wasm_bindgen]
pub fn encode_indexed_lzw_scratch(
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<usize, JsValue> {
    encode_indexed_lzw_scratch_inner(index_stream, min_code_size, color_count, false)
        .map_err(|message| JsValue::from_str(&message))
}

#[cfg(not(feature = "quality-only"))]
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
    let units = length.div_ceil(std::mem::size_of::<u32>());
    REUSABLE_LZW_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        if units > scratch.input.len() {
            let current_len = scratch.input.len();
            scratch.input.reserve(units - current_len);
            // Callers must overwrite every requested byte before invoking an
            // encoder, so growing this range needs no initialization pass.
            unsafe { scratch.input.set_len(units) };
        }
        scratch.input.as_mut_ptr().cast::<u8>() as usize
    })
}

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
#[wasm_bindgen]
pub fn indexed_lzw_scratch_ptr() -> usize {
    REUSABLE_LZW_SCRATCH.with(|scratch| scratch.borrow().output.as_ptr() as usize)
}

#[cfg(not(feature = "quality-only"))]
#[wasm_bindgen]
pub fn wasm_memory() -> JsValue {
    wasm_bindgen::memory()
}

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
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

/// Scratch-input/output form of the constant-delay literal indexed encoder.
/// JavaScript copies the caller's indices into the reusable input range and
/// then copies the returned GIF range before the next encode.
#[cfg(not(feature = "quality-only"))]
#[wasm_bindgen]
pub fn encode_indexed_literal_gif_scratch_from_input(
    length: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delay: u16,
    loop_count: i32,
) -> Result<usize, JsValue> {
    let input_ptr = REUSABLE_LZW_SCRATCH.with(|scratch| {
        let scratch = scratch.borrow();
        if length > scratch.input.len() * std::mem::size_of::<u32>() {
            return Err(JsValue::from_str(
                "Indexed input scratch buffer is too short",
            ));
        }
        Ok(scratch.input.as_ptr().cast::<u8>())
    })?;
    let index_stream = unsafe { std::slice::from_raw_parts(input_ptr, length) };
    let output = REUSABLE_GIF_OUTPUT.with(|scratch| std::mem::take(&mut *scratch.borrow_mut()));
    let encoded = encode_indexed_literal_gif_inner_with_output(
        output,
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::Constant(delay),
        loop_count,
        None,
    )
    .map_err(|message| JsValue::from_str(&message))?;
    let length = encoded.len();
    REUSABLE_GIF_OUTPUT.with(|scratch| {
        *scratch.borrow_mut() = encoded;
    });
    Ok(length)
}

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
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

#[cfg(not(feature = "quality-only"))]
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
            if length > scratch.input.len() * std::mem::size_of::<u32>() {
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
#[cfg(not(feature = "quality-only"))]
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
        if length > scratch.input.len() * std::mem::size_of::<u32>() {
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
/// Constant-delay variant of the specialized quality encoder. Keeping the
/// scalar delay scalar across the Wasm boundary avoids allocating a temporary
/// per-frame delay array for the common animation API.
#[wasm_bindgen]
pub fn encode_rgba_quality_gif_constant_delay_scratch_from_input(
    length: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    delay: u16,
    loop_count: i32,
    alpha_threshold: u8,
) -> usize {
    let input_ptr = REUSABLE_LZW_SCRATCH.with(|scratch| {
        let scratch = scratch.borrow();
        (length <= scratch.input.len() * std::mem::size_of::<u32>())
            .then(|| scratch.input.as_ptr().cast::<u8>())
    });
    let Some(input_ptr) = input_ptr else {
        return 0;
    };
    let rgba_stream = unsafe { std::slice::from_raw_parts(input_ptr, length) };
    let output = REUSABLE_GIF_OUTPUT.with(|scratch| std::mem::take(&mut *scratch.borrow_mut()));
    let Ok(encoded) = encode_rgba_quality_gif_inner_with_output(
        rgba_stream,
        width,
        height,
        frame_count,
        DelaySource::Constant(delay),
        loop_count,
        alpha_threshold,
        output,
    ) else {
        return 0;
    };
    let length = encoded.len();
    REUSABLE_GIF_OUTPUT.with(|scratch| {
        *scratch.borrow_mut() = encoded;
    });
    length
}

/// Reserves an aligned per-frame delay array immediately after the RGBA input
/// in the reusable input scratch buffer and returns its pointer.
#[wasm_bindgen]
pub fn quality_delay_scratch_reserve(input_length: usize, delay_count: usize) -> usize {
    let input_units = input_length.div_ceil(std::mem::size_of::<u32>());
    let delay_units = delay_count.div_ceil(2);
    REUSABLE_LZW_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let required_units = input_units.saturating_add(delay_units);
        if scratch.input.len() < required_units {
            let additional = required_units - scratch.input.len();
            scratch.input.reserve(additional);
            unsafe { scratch.input.set_len(required_units) };
        }
        unsafe { scratch.input.as_mut_ptr().add(input_units).cast::<u16>() as usize }
    })
}

/// Scratch-output form of the specialized quality encoder. The returned
/// length refers to `gif_output_scratch_ptr()` in Wasm memory.
#[wasm_bindgen]
pub fn encode_rgba_quality_gif_scratch_from_input(
    length: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    delay_count: usize,
    loop_count: i32,
    alpha_threshold: u8,
) -> usize {
    let input_ptr = REUSABLE_LZW_SCRATCH.with(|scratch| {
        let scratch = scratch.borrow();
        (length <= scratch.input.len() * std::mem::size_of::<u32>())
            .then(|| scratch.input.as_ptr().cast::<u8>())
    });
    let Some(input_ptr) = input_ptr else {
        return 0;
    };
    let delays_ptr = REUSABLE_LZW_SCRATCH.with(|scratch| {
        let scratch = scratch.borrow();
        let input_units = length.div_ceil(std::mem::size_of::<u32>());
        let delay_units = delay_count.div_ceil(2);
        (input_units.saturating_add(delay_units) <= scratch.input.len())
            .then(|| unsafe { scratch.input.as_ptr().add(input_units).cast::<u16>() })
    });
    let Some(delays_ptr) = delays_ptr else {
        return 0;
    };
    let rgba_stream = unsafe { std::slice::from_raw_parts(input_ptr, length) };
    let delays = unsafe { std::slice::from_raw_parts(delays_ptr, delay_count) };
    let output = REUSABLE_GIF_OUTPUT.with(|scratch| std::mem::take(&mut *scratch.borrow_mut()));
    let Ok(encoded) = encode_rgba_quality_gif_inner_with_output(
        rgba_stream,
        width,
        height,
        frame_count,
        DelaySource::PerFrame(delays),
        loop_count,
        alpha_threshold,
        output,
    ) else {
        return 0;
    };
    let length = encoded.len();
    REUSABLE_GIF_OUTPUT.with(|scratch| {
        *scratch.borrow_mut() = encoded;
    });
    length
}

#[wasm_bindgen]
pub fn gif_output_scratch_ptr() -> usize {
    REUSABLE_GIF_OUTPUT.with(|scratch| scratch.borrow().as_ptr() as usize)
}
