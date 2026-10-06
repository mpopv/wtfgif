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
        .map_err(|message| js_error(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn decode_frame_indices(data: &[u8], frame_index: usize) -> Result<Vec<u8>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| js_error(&message))?;
    let frame = metadata
        .frames
        .get(frame_index)
        .ok_or_else(|| js_error("Frame index out of range"))?;
    decode_frame_indices_inner(data, frame).map_err(|message| js_error(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn decode_frame_rgba(data: &[u8], frame_index: usize) -> Result<Vec<u8>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| js_error(&message))?;
    decode_frame_pixels_inner(data, &metadata, frame_index, PixelFormat::Rgba)
        .map_err(|message| js_error(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn decode_frame_bgra(data: &[u8], frame_index: usize) -> Result<Vec<u8>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| js_error(&message))?;
    decode_frame_pixels_inner(data, &metadata, frame_index, PixelFormat::Bgra)
        .map_err(|message| js_error(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn decode_all_rgba(data: &[u8]) -> Result<Vec<u32>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| js_error(&message))?;
    prepare_all_composited_frames_inner(data, &metadata, PixelFormat::Rgba)
        .map_err(|message| js_error(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn reencode_gif_pixel_perfect(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| js_error(&message))?;
    let loop_count = metadata.loop_count.map(i32::from).unwrap_or(-1);
    let total_frame_pixels = metadata.frames.iter().try_fold(0usize, |total, frame| {
        usize::from(frame.width)
            .checked_mul(usize::from(frame.height))
            .and_then(|pixels| total.checked_add(pixels))
            .ok_or_else(|| "Decoded frame size overflow".to_string())
    });
    let total_frame_pixels = total_frame_pixels.map_err(|message| js_error(&message))?;
    reencode_gif_literal_sequential(data, &metadata, loop_count, total_frame_pixels)
        .map_err(|message| js_error(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn remux_gif_pixel_perfect(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    if data.len() <= 4_096 {
        validate_gif_structure_no_alloc(data).map_err(|message| js_error(&message))?;
        return Ok(data.to_vec());
    }
    let metadata = parse_metadata(data).map_err(|message| js_error(&message))?;
    let loop_count = metadata.loop_count.map(i32::from).unwrap_or(-1);
    remux_gif_pixel_perfect_inner(data, &metadata, loop_count).map_err(|message| js_error(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn prepare_composited_rgba(data: &[u8], requested_frames: &[u8]) -> Result<Vec<u32>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| js_error(&message))?;
    prepare_composited_frames_inner(data, &metadata, requested_frames, PixelFormat::Rgba)
        .map_err(|message| js_error(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn prepare_composited_bgra(data: &[u8], requested_frames: &[u8]) -> Result<Vec<u32>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| js_error(&message))?;
    prepare_composited_frames_inner(data, &metadata, requested_frames, PixelFormat::Bgra)
        .map_err(|message| js_error(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn prepare_composited_delta_rgba(
    data: &[u8],
    requested_frames: &[u8],
) -> Result<Vec<u32>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| js_error(&message))?;
    prepare_composited_delta_frames_inner(data, &metadata, requested_frames, PixelFormat::Rgba)
        .map_err(|message| js_error(&message))
}

#[cfg(not(feature = "encode-only"))]
#[wasm_bindgen]
pub fn prepare_composited_delta_bgra(
    data: &[u8],
    requested_frames: &[u8],
) -> Result<Vec<u32>, JsValue> {
    let metadata = parse_metadata(data).map_err(|message| js_error(&message))?;
    prepare_composited_delta_frames_inner(data, &metadata, requested_frames, PixelFormat::Bgra)
        .map_err(|message| js_error(&message))
}

#[cfg(not(feature = "quality-only"))]
#[wasm_bindgen]
pub fn encode_indexed_literal_lzw_scratch(
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<usize, JsValue> {
    encode_indexed_literal_lzw_scratch_inner(index_stream, min_code_size, color_count)
        .map_err(|message| js_error(&message))
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

#[wasm_bindgen]
/// Compile representative quality-encoder call graphs during explicit Wasm
/// initialization. Every probe is empty and the codec's empty sentinels avoid
/// populating reusable data arenas, so the first user encode keeps cold data.
pub fn prepare_quality_encoder_code() {
    let empty = std::hint::black_box(&[][..]);
    let hints = quality_low_res_hints(empty, 128);
    let impossible = quality_low_res_exact_is_impossible(empty, 128);
    let alpha_255 = rgba_stream_samples_alpha_255(empty);
    let small = index_rgba_frames_quality_small_exact(empty, 128, 1);
    if let Some((palette, indexed, _)) = small {
        recycle_quality_palette(palette);
        recycle_quantized_indexed(indexed);
    }
    let mut histogram = [];
    let mixed =
        accumulate_quality_histogram_u32_bits_remaining_mixed::<4>(&mut histogram, empty, 0, 128);
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    let opaque_spans = accumulate_quality_histogram_u32_bits_remaining_mixed_opaque_spans_four_bit(
        &mut histogram,
        empty,
        0,
        128,
    );
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    let opaque_spans = false;
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    let zero_dense = accumulate_quality_histogram_u32_bits_remaining_mixed_zero_dense::<4>(
        &mut histogram,
        empty,
        0,
        128,
    );
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    let zero_dense = false;
    let opaque = accumulate_quality_histogram_u32_bits_remaining_opaque::<4, false>(
        &mut histogram,
        empty,
        0,
    );
    let colors = quality_colors_from_histogram_u32::<true>(&mut histogram);
    recycle_quality_colors(colors);
    let sampled_opaque_plan =
        index_rgba_frames_quality_low_res_quantized_sampled_opaque(empty, 128, Vec::new());
    recycle_quality_palette(sampled_opaque_plan.palette);
    let mixed_plan = index_rgba_frames_quality_low_res_quantized_mixed(empty, 128, Vec::new());
    recycle_quality_palette(mixed_plan.palette);
    let direct_plan = finish_quality_low_res_quantized(Vec::new(), false, Vec::new());
    recycle_quality_palette(direct_plan.palette);
    recycle_quality_histogram_to_palette(direct_plan.histogram_to_palette);
    let merge_plan = build_quality_single_merge_plan(true, Vec::new(), Vec::new());
    recycle_quality_palette(merge_plan.palette);
    recycle_quality_histogram_to_palette(merge_plan.histogram_to_palette);
    for result in [
        index_rgba_frames_quality_low_res_exact::<false>(empty, 128, 0),
        index_rgba_frames_quality_low_res_exact::<true>(empty, 128, 0),
    ] {
        match result {
            QualityIndexResult::Exact((palette, indexed, _)) => {
                recycle_quality_palette(palette);
                recycle_quantized_indexed(indexed);
            }
            QualityIndexResult::Quantized(plan) => {
                recycle_quality_palette(plan.palette);
                recycle_quality_histogram_to_palette(plan.histogram_to_palette);
            }
        }
    }
    let exact_plain = encode_rgba_quality_low_res_exact_gif_inner_with_output::<false>(
        empty,
        1,
        1,
        0,
        DelaySource::Constant(0),
        0,
        128,
        false,
        false,
        Vec::new(),
    );
    let exact_runs = encode_rgba_quality_low_res_exact_gif_inner_with_output::<true>(
        empty,
        1,
        1,
        0,
        DelaySource::Constant(0),
        0,
        128,
        false,
        false,
        Vec::new(),
    );
    let quantized = encode_rgba_quality_low_res_quantized_gif_inner_with_output(
        empty,
        1,
        1,
        0,
        DelaySource::Constant(0),
        0,
        128,
        true,
        false,
        Vec::new(),
    );
    let public_quality =
        encode_rgba_quality_low_res_constant_delay_scratch_from_input(0, 1, 1, 0, 0, 0, 128, false);
    let compact_prepared = compact::prepare_compact_code();
    let mut lzw_output = Vec::new();
    map_quality_pixels_grouped::<4, false>(empty, 128, 0, empty, &mut []);
    map_quality_pixels_grouped::<4, true>(empty, 128, 0, empty, &mut []);
    map_quality_frame::<4, false>(empty, None, 128, 0, empty, &mut []);
    map_quality_frame::<4, true>(empty, None, 128, 0, empty, &mut []);
    let temporal = temporal_histogram_pays(empty, 1, 128);
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    let sorted_nearest = {
        let mut search = crate::sorted_palette::SortedPaletteSearch::new();
        search.rebuild(std::hint::black_box(&[0x0010_2030]));
        search.nearest(1, 2, 3) ^ search.nearest_seeded(4, 5, 6, 0, 0x0010_2030)
    };
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    let sorted_nearest = 0u8;
    let (palette, mapping) = build_quality_wu_palette(
        false,
        std::hint::black_box(Vec::new()),
        1 << 12,
        256,
        Vec::new(),
    );
    let cube = WuCube {
        minimum: [0; 3],
        maximum: [0; 3],
    };
    let split = split_wu_cube(std::hint::black_box(&[]), std::hint::black_box(&[]), cube);
    let dominant_palette_prepared = prepare_quality_dominant_palette_code();
    let mut tree = PaletteKdTree::new(std::hint::black_box(&[]));
    tree.recolor(std::hint::black_box(&[]));
    let nearest = tree.nearest_with_seed(0, 0, 0, 0, 1);
    write_indexed_gif_header(
        &mut lzw_output,
        1,
        1,
        std::hint::black_box(&[] as &[u32]),
        2,
    );
    write_loop_extension(&mut lzw_output, 0);
    write_indexed_gif_frame_header(&mut lzw_output, 0, 0, 1, 1, 0, None, 0);
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    let dense = {
        let requested = [false; 1 << 12];
        let hints = [0; 1 << 12];
        dense_coarse_nearest_table_simd::<false>(std::hint::black_box(&[]), &requested, &hints)[0]
            ^ dense_coarse_nearest_table_simd::<true>(std::hint::black_box(&[]), &requested, &hints)
                [0]
    };
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    let dense = 0u8;
    let _ = std::hint::black_box(
        palette.capacity()
            ^ mapping.capacity()
            ^ split.is_some() as usize
            ^ usize::from(nearest)
            ^ usize::from(dense)
            ^ usize::from(hints.likely_exact)
            ^ usize::from(impossible)
            ^ usize::from(alpha_255)
            ^ usize::from(mixed)
            ^ usize::from(opaque_spans)
            ^ usize::from(zero_dense)
            ^ usize::from(opaque)
            ^ dominant_palette_prepared
            ^ usize::from(exact_plain.is_ok())
            ^ usize::from(exact_runs.is_ok())
            ^ usize::from(quantized.is_ok())
            ^ public_quality
            ^ compact_prepared
            ^ usize::from(temporal)
            ^ usize::from(sorted_nearest),
    );
}

#[cfg(not(feature = "quality-only"))]
#[wasm_bindgen]
pub fn encode_indexed_lzw_scratch_from_input(
    length: usize,
    min_code_size: u8,
    color_count: usize,
) -> Result<usize, JsValue> {
    encode_indexed_literal_lzw_scratch_from_input_inner(length, min_code_size, color_count)
        .map_err(|message| js_error(&message))
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
    .map_err(|message| js_error(&message))
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
            return Err(js_error("Indexed input scratch buffer is too short"));
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
    .map_err(|message| js_error(&message))?;
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
    .map_err(|message| js_error(&message))
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
        None,
    )
    .map_err(|message| js_error(&message))
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
        None,
    )
    .map_err(|message| js_error(&message))
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
    )
    .map_err(|message| js_error(&message))
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
    )
    .map_err(|message| js_error(&message))
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
    )
    .map_err(|message| js_error(&message))
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
    )
    .map_err(|message| js_error(&message))
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
    quantization: u8,
    palette_mode: u8,
) -> Result<Vec<u8>, JsValue> {
    let quantization =
        RgbaQuantization::from_u8(quantization).map_err(|message| js_error(&message))?;
    let palette_mode =
        RgbaPaletteMode::from_u8(palette_mode).map_err(|message| js_error(&message))?;
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
        quantization,
        palette_mode,
    )
    .map_err(|message| js_error(&message))
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
    quantization: u8,
    palette_mode: u8,
) -> Result<Vec<u8>, JsValue> {
    let quantization =
        RgbaQuantization::from_u8(quantization).map_err(|message| js_error(&message))?;
    let palette_mode =
        RgbaPaletteMode::from_u8(palette_mode).map_err(|message| js_error(&message))?;
    REUSABLE_LZW_SCRATCH.with(|scratch| {
        // Release the RefCell borrow before encoding: the encoder reuses the
        // same scratch object for its indexed/LZW output buffers. The input
        // Vec is not resized while this call runs, so its pointer remains
        // stable for the duration of the encode.
        let input_ptr = {
            let scratch = scratch.borrow();
            if length > scratch.input.len() * std::mem::size_of::<u32>() {
                return Err(js_error("RGBA input scratch buffer is too short"));
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
            quantization,
            palette_mode,
        )
        .map_err(|message| js_error(&message))
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
    quantization: u8,
    palette_mode: u8,
) -> Result<usize, JsValue> {
    let quantization =
        RgbaQuantization::from_u8(quantization).map_err(|message| js_error(&message))?;
    let palette_mode =
        RgbaPaletteMode::from_u8(palette_mode).map_err(|message| js_error(&message))?;
    let input_ptr = REUSABLE_LZW_SCRATCH.with(|scratch| {
        let scratch = scratch.borrow();
        if length > scratch.input.len() * std::mem::size_of::<u32>() {
            return Err(js_error("RGBA input scratch buffer is too short"));
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
        quantization,
        palette_mode,
        output,
    )
    .map_err(|message| js_error(&message))?;
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
// Keep the exported wrapper as its own Wasm function so explicit preparation
// can compile the exact function JavaScript will enter on the first image.
#[inline(never)]
pub fn encode_rgba_quality_low_res_constant_delay_scratch_from_input(
    length: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    delay: u16,
    loop_count: i32,
    alpha_threshold: u8,
    independent_frames: bool,
) -> usize {
    // `prepare_quality_encoder_code` uses this sentinel before any user data
    // exists. No reusable input, output, palette, or histogram is touched.
    if length == 0 {
        return 0;
    }
    let input_ptr = REUSABLE_LZW_SCRATCH.with(|scratch| {
        let scratch = scratch.borrow();
        (length <= scratch.input.len() * std::mem::size_of::<u32>())
            .then(|| scratch.input.as_ptr().cast::<u8>())
    });
    let Some(input_ptr) = input_ptr else {
        return 0;
    };
    let rgba_stream = unsafe { std::slice::from_raw_parts(input_ptr, length) };
    debug_assert!(rgba_stream.len() % 4 == 0);
    debug_assert!(rgba_stream.len() / 4 <= QUALITY_LOW_RES_PIXEL_LIMIT);
    let output = REUSABLE_GIF_OUTPUT.with(|scratch| std::mem::take(&mut *scratch.borrow_mut()));
    let hints = if rgba_stream.len() >= 40_000 * 4
        && quality_low_res_has_uniform_sampled_runs(rgba_stream)
    {
        QualityLowResHints {
            likely_exact: true,
            likely_small_palette: true,
            prefers_run_coalescing: true,
            exact_impossible: false,
            sampled_alpha_255: rgba_stream.get(3).copied() == Some(255),
        }
    } else {
        quality_low_res_hints(rgba_stream, alpha_threshold)
    };
    let tried_small_exact = rgba_stream.len() >= 40_000 * 4 && hints.likely_small_palette;
    if tried_small_exact {
        if let Some((palette, indexed, transparent_index)) = index_rgba_frames_quality_small_exact(
            rgba_stream,
            alpha_threshold,
            usize::from(width) * usize::from(height),
        ) {
            let encoded = encode_quality_indexed_compact(
                output,
                palette,
                indexed,
                transparent_index,
                width,
                height,
                frame_count,
                DelaySource::Constant(delay),
                loop_count,
                independent_frames,
            );
            let Ok(encoded) = encoded else {
                return 0;
            };
            let length = encoded.len();
            REUSABLE_GIF_OUTPUT.with(|scratch| *scratch.borrow_mut() = encoded);
            return length;
        }
    }
    let encoded = if hints.exact_impossible
        || (!hints.likely_exact
            && quality_low_res_exact_is_impossible(rgba_stream, alpha_threshold))
    {
        encode_rgba_quality_low_res_quantized_gif_inner_with_output(
            rgba_stream,
            width,
            height,
            frame_count,
            DelaySource::Constant(delay),
            loop_count,
            alpha_threshold,
            hints.sampled_alpha_255,
            independent_frames,
            output,
        )
    } else {
        if hints.prefers_run_coalescing {
            encode_rgba_quality_low_res_exact_gif_inner_with_output::<true>(
                rgba_stream,
                width,
                height,
                frame_count,
                DelaySource::Constant(delay),
                loop_count,
                alpha_threshold,
                false,
                independent_frames,
                output,
            )
        } else {
            encode_rgba_quality_low_res_exact_gif_inner_with_output::<false>(
                rgba_stream,
                width,
                height,
                frame_count,
                DelaySource::Constant(delay),
                loop_count,
                alpha_threshold,
                false,
                independent_frames,
                output,
            )
        }
    };
    let Ok(encoded) = encoded else {
        return 0;
    };
    let length = encoded.len();
    REUSABLE_GIF_OUTPUT.with(|scratch| {
        *scratch.borrow_mut() = encoded;
    });
    length
}

#[wasm_bindgen]
pub fn encode_rgba_quality_gif_constant_delay_scratch_from_input(
    length: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    delay: u16,
    loop_count: i32,
    alpha_threshold: u8,
    independent_frames: bool,
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
    let low_res =
        rgba_stream.len() % 4 == 0 && rgba_stream.len() / 4 <= QUALITY_LOW_RES_PIXEL_LIMIT;
    let encoded = if low_res {
        let hints = quality_low_res_hints(rgba_stream, alpha_threshold);
        if hints.exact_impossible
            || (!hints.likely_exact
                && quality_low_res_exact_is_impossible(rgba_stream, alpha_threshold))
        {
            encode_rgba_quality_low_res_quantized_gif_inner_with_output(
                rgba_stream,
                width,
                height,
                frame_count,
                DelaySource::Constant(delay),
                loop_count,
                alpha_threshold,
                hints.sampled_alpha_255,
                independent_frames,
                output,
            )
        } else {
            if hints.prefers_run_coalescing {
                encode_rgba_quality_low_res_exact_gif_inner_with_output::<true>(
                    rgba_stream,
                    width,
                    height,
                    frame_count,
                    DelaySource::Constant(delay),
                    loop_count,
                    alpha_threshold,
                    rgba_stream.len() >= 40_000 * 4 && hints.likely_small_palette,
                    independent_frames,
                    output,
                )
            } else {
                encode_rgba_quality_low_res_exact_gif_inner_with_output::<false>(
                    rgba_stream,
                    width,
                    height,
                    frame_count,
                    DelaySource::Constant(delay),
                    loop_count,
                    alpha_threshold,
                    rgba_stream.len() >= 40_000 * 4 && hints.likely_small_palette,
                    independent_frames,
                    output,
                )
            }
        }
    } else {
        encode_rgba_quality_gif_inner_with_output(
            rgba_stream,
            width,
            height,
            frame_count,
            DelaySource::Constant(delay),
            loop_count,
            alpha_threshold,
            independent_frames,
            output,
        )
    };
    let Ok(encoded) = encoded else {
        return 0;
    };
    let length = encoded.len();
    REUSABLE_GIF_OUTPUT.with(|scratch| {
        *scratch.borrow_mut() = encoded;
    });
    length
}

reusable_cells! {
    static REUSABLE_QUALITY_DELAYS: Vec<u16> = Vec::new();
}

/// Reserves space for `delay_count` per-frame delays and returns its pointer.
/// The delays live in their own buffer: growing the RGBA input buffer here
/// would move it out from under the input pointer JavaScript already holds,
/// and the next same-sized encode would write its pixels to freed memory.
#[wasm_bindgen]
pub fn quality_delay_scratch_reserve(input_length: usize, delay_count: usize) -> usize {
    let _ = input_length;
    REUSABLE_QUALITY_DELAYS.with(|delays| {
        let mut delays = delays.borrow_mut();
        if delays.len() < delay_count {
            delays.resize(delay_count, 0);
        }
        delays.as_mut_ptr() as usize
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
    independent_frames: bool,
) -> usize {
    let input_ptr = REUSABLE_LZW_SCRATCH.with(|scratch| {
        let scratch = scratch.borrow();
        (length <= scratch.input.len() * std::mem::size_of::<u32>())
            .then(|| scratch.input.as_ptr().cast::<u8>())
    });
    let Some(input_ptr) = input_ptr else {
        return 0;
    };
    let delays_ptr = REUSABLE_QUALITY_DELAYS.with(|delays| {
        let delays = delays.borrow();
        (delay_count <= delays.len()).then(|| delays.as_ptr())
    });
    let Some(delays_ptr) = delays_ptr else {
        return 0;
    };
    let rgba_stream = unsafe { std::slice::from_raw_parts(input_ptr, length) };
    let delays = unsafe { std::slice::from_raw_parts(delays_ptr, delay_count) };
    let output = REUSABLE_GIF_OUTPUT.with(|scratch| std::mem::take(&mut *scratch.borrow_mut()));
    let low_res =
        rgba_stream.len() % 4 == 0 && rgba_stream.len() / 4 <= QUALITY_LOW_RES_PIXEL_LIMIT;
    let encoded = if low_res {
        let hints = quality_low_res_hints(rgba_stream, alpha_threshold);
        if hints.exact_impossible
            || (!hints.likely_exact
                && quality_low_res_exact_is_impossible(rgba_stream, alpha_threshold))
        {
            encode_rgba_quality_low_res_quantized_gif_inner_with_output(
                rgba_stream,
                width,
                height,
                frame_count,
                DelaySource::PerFrame(delays),
                loop_count,
                alpha_threshold,
                hints.sampled_alpha_255,
                independent_frames,
                output,
            )
        } else {
            encode_rgba_quality_low_res_exact_gif_inner_with_output::<false>(
                rgba_stream,
                width,
                height,
                frame_count,
                DelaySource::PerFrame(delays),
                loop_count,
                alpha_threshold,
                rgba_stream.len() >= 40_000 * 4 && hints.likely_small_palette,
                independent_frames,
                output,
            )
        }
    } else {
        encode_rgba_quality_gif_inner_with_output(
            rgba_stream,
            width,
            height,
            frame_count,
            DelaySource::PerFrame(delays),
            loop_count,
            alpha_threshold,
            independent_frames,
            output,
        )
    };
    let Ok(encoded) = encoded else {
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
