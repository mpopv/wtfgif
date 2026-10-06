//! The quality quantizer: palette planning dispatch and index mapping.

use super::*;

pub(crate) const QUALITY_HISTOGRAM_BITS: usize = 5;

pub(crate) const QUALITY_HISTOGRAM_SIDE: usize = 1 << QUALITY_HISTOGRAM_BITS;

pub(crate) const QUALITY_HISTOGRAM_LEN: usize =
    QUALITY_HISTOGRAM_SIDE * QUALITY_HISTOGRAM_SIDE * QUALITY_HISTOGRAM_SIDE;

pub(crate) const QUALITY_DOMINANT_COLOR_LIMIT: usize = 2_048;

pub(crate) const QUALITY_U32_PIXEL_LIMIT: usize = u32::MAX as usize / 255;

pub(crate) const QUALITY_LOW_RES_PIXEL_LIMIT: usize = 1_000_000;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) const QUANTIZED_SORT_COUNT_LIMIT: u64 = 1 << 48;

pub(crate) struct QualityIndexPlan {
    pub(crate) palette: Vec<u32>,
    pub(crate) histogram_to_palette: Vec<u8>,
    pub(crate) transparent_index: Option<u8>,
    pub(crate) mapping_bits: usize,
}

pub(crate) enum QualityIndexResult {
    Exact((Vec<u32>, Vec<u8>, Option<u8>)),
    Quantized(QualityIndexPlan),
}

impl QualityIndexPlan {
    pub(crate) fn into_indexed(
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
        let transparent = transparent_index.unwrap_or(0);
        match (mapping_bits, transparent_index.is_some()) {
            (4, false) => map_quality_pixels_grouped::<4, false>(
                rgba_stream,
                alpha_threshold,
                transparent,
                &histogram_to_palette,
                &mut indexed,
            ),
            (4, true) => map_quality_pixels_grouped::<4, true>(
                rgba_stream,
                alpha_threshold,
                transparent,
                &histogram_to_palette,
                &mut indexed,
            ),
            (5, false) => map_quality_pixels_grouped::<5, false>(
                rgba_stream,
                alpha_threshold,
                transparent,
                &histogram_to_palette,
                &mut indexed,
            ),
            (5, true) => map_quality_pixels_grouped::<5, true>(
                rgba_stream,
                alpha_threshold,
                transparent,
                &histogram_to_palette,
                &mut indexed,
            ),
            _ => unreachable!("unsupported quality histogram precision"),
        }
        recycle_quality_histogram_to_palette(histogram_to_palette);
        (palette, indexed, transparent_index)
    }
}

pub(crate) fn index_rgba_frames_quality(
    rgba_stream: &[u8],
    alpha_threshold: u8,
) -> (Vec<u32>, Vec<u8>, Option<u8>) {
    match index_rgba_frames_quality_result(rgba_stream, alpha_threshold) {
        QualityIndexResult::Exact(result) => result,
        QualityIndexResult::Quantized(plan) => plan.into_indexed(rgba_stream, alpha_threshold),
    }
}

pub(crate) fn index_rgba_frames_quality_result(
    rgba_stream: &[u8],
    alpha_threshold: u8,
) -> QualityIndexResult {
    let pixel_count = rgba_stream.len() / 4;
    if pixel_count <= QUALITY_LOW_RES_PIXEL_LIMIT {
        return index_rgba_frames_quality_low_res(rgba_stream, alpha_threshold);
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

pub(crate) fn index_rgba_frames_quality_low_res(
    rgba_stream: &[u8],
    alpha_threshold: u8,
) -> QualityIndexResult {
    if quality_low_res_exact_is_impossible(rgba_stream, alpha_threshold) {
        return QualityIndexResult::Quantized(index_rgba_frames_quality_low_res_quantized(
            rgba_stream,
            alpha_threshold,
            take_quality_palette(256),
        ));
    }
    index_rgba_frames_quality_low_res_exact::<false>(rgba_stream, alpha_threshold, 0)
}

/// The delta encoder is already a distinct advanced path, so give flat source
/// animations the same cheap likely-exact probe as the constant-delay hot
/// path. The authoritative exact scan still catches every sample collision or
/// missed color before output is emitted.
#[inline(never)]
pub(crate) fn index_rgba_frames_quality_low_res_delta(
    rgba_stream: &[u8],
    alpha_threshold: u8,
) -> (Vec<u32>, Vec<u8>, Option<u8>) {
    let hints = quality_low_res_hints(rgba_stream, alpha_threshold);
    let result = if hints.exact_impossible
        || (!hints.likely_exact
            && quality_low_res_exact_is_impossible(rgba_stream, alpha_threshold))
    {
        QualityIndexResult::Quantized(index_rgba_frames_quality_low_res_quantized(
            rgba_stream,
            alpha_threshold,
            take_quality_palette(256),
        ))
    } else {
        index_rgba_frames_quality_low_res_exact::<true>(rgba_stream, alpha_threshold, 0)
    };
    match result {
        QualityIndexResult::Exact(exact) => exact,
        QualityIndexResult::Quantized(plan) => plan.into_indexed(rgba_stream, alpha_threshold),
    }
}

/// Prove that an image needs quantization before allocating the reusable
/// palette and prefix-index buffers. Every occupied hash bucket represents at
/// least one distinct RGB value, so crossing GIF's color limit is conclusive.
/// Collisions can only produce a conservative false negative, which falls
/// through to the complete exact scan and cannot change the encoded result.
#[inline(never)]
pub(crate) fn quality_low_res_exact_is_impossible(rgba_stream: &[u8], alpha_threshold: u8) -> bool {
    const OCCUPANCY_BITS: usize = 1 << 10;
    let mut occupied = [0u64; OCCUPANCY_BITS / u64::BITS as usize];
    let mut color_count = 0usize;
    let mut opaque_color_limit = 256usize;
    let rgba_pointer = rgba_stream.as_ptr();
    for pixel_index in 0..rgba_stream.len() / 4 {
        let packed = u32::from_le(unsafe {
            std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
        });
        if ((packed >> 24) as u8) < alpha_threshold {
            opaque_color_limit = 255;
            if color_count > opaque_color_limit {
                return true;
            }
            continue;
        }
        let rgb = packed & 0x00ff_ffff;
        let bucket = (rgb.wrapping_mul(2_654_435_761) >> (u32::BITS as usize - 10)) as usize;
        let word = bucket / u64::BITS as usize;
        let mask = 1u64 << (bucket % u64::BITS as usize);
        if occupied[word] & mask == 0 {
            occupied[word] |= mask;
            color_count += 1;
            if color_count > opaque_color_limit {
                return true;
            }
        }
    }
    false
}

pub(crate) struct QualityLowResHints {
    pub(crate) likely_exact: bool,
    pub(crate) likely_small_palette: bool,
    pub(crate) prefers_run_coalescing: bool,
    pub(crate) exact_impossible: bool,
    pub(crate) sampled_alpha_255: bool,
}

#[inline(always)]
pub(crate) fn quality_low_res_has_uniform_sampled_runs(rgba_stream: &[u8]) -> bool {
    let pixel_count = rgba_stream.len() / 4;
    let sample_step = 8_191 % pixel_count;
    let rgba_pointer = rgba_stream.as_ptr();
    let first = u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.cast()) });
    if first != u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(4).cast()) }) {
        return false;
    }
    let mut sample_pixel = sample_step;
    for _ in 1..4 {
        let current = u32::from_le(unsafe {
            std::ptr::read_unaligned(rgba_pointer.add(sample_pixel * 4).cast())
        });
        let next = u32::from_le(unsafe {
            std::ptr::read_unaligned(rgba_pointer.add((sample_pixel + 1) * 4).cast())
        });
        if current != first || next != first {
            return false;
        }
        sample_pixel += sample_step;
    }
    true
}

#[inline(never)]
pub(crate) fn quality_low_res_hints(rgba_stream: &[u8], alpha_threshold: u8) -> QualityLowResHints {
    // Classify common inputs while sampling the complete stream. Finding more
    // colors than GIF can represent proves quantization is required and skips
    // the separate occupancy pass. A small sampled set can enter the complete
    // exact scanner directly; that scanner still catches every missed color.
    const SHORT_SAMPLE_COUNT: usize = 256;
    const LONG_SAMPLE_COUNT: usize = 2_048;
    const LONG_SAMPLE_PIXEL_THRESHOLD: usize = 40_000;
    const SAMPLE_TABLE_SIZE: usize = 512;
    const LIKELY_EXACT_COLOR_LIMIT: usize = 192;
    const SMALL_COLOR_LIMIT: usize = 12;
    let pixel_count = rgba_stream.len() / 4;
    let sample_limit = if pixel_count >= LONG_SAMPLE_PIXEL_THRESHOLD {
        LONG_SAMPLE_COUNT
    } else {
        SHORT_SAMPLE_COUNT
    };
    let sample_count = pixel_count.min(sample_limit);
    let mut sample_colors = [u32::MAX; SAMPLE_TABLE_SIZE];
    let mut sample_color_count = 0usize;
    let mut opaque_color_limit = 256usize;
    let mut adjacent_matches = 0usize;
    let mut sampled_alpha_255 = true;
    let rgba_pointer = rgba_stream.as_ptr();
    let mut sample_pixel = 0usize;
    let sample_step = if pixel_count == 0 {
        0
    } else {
        let step = 8_191 % pixel_count;
        if step == 0 {
            1
        } else {
            step
        }
    };
    for sample_index in 0..sample_count {
        let packed = u32::from_le(unsafe {
            std::ptr::read_unaligned(rgba_pointer.add(sample_pixel * 4).cast())
        });
        if sample_pixel + 1 < pixel_count {
            let next = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add((sample_pixel + 1) * 4).cast())
            });
            adjacent_matches += usize::from(packed == next);
        }
        sample_pixel += sample_step;
        if sample_pixel >= pixel_count {
            sample_pixel -= pixel_count;
        }
        sampled_alpha_255 &= (packed >> 24) as u8 == 255;
        if ((packed >> 24) as u8) < alpha_threshold {
            opaque_color_limit = 255;
            if sample_color_count > opaque_color_limit {
                return QualityLowResHints {
                    likely_exact: false,
                    likely_small_palette: false,
                    prefers_run_coalescing: false,
                    exact_impossible: true,
                    sampled_alpha_255,
                };
            }
            continue;
        }
        let rgb = packed & 0x00ff_ffff;
        let mut bucket = (rgb.wrapping_mul(2_654_435_761) >> (u32::BITS as usize - 9)) as usize;
        loop {
            let sampled = sample_colors[bucket];
            if sampled == rgb {
                break;
            }
            if sampled == u32::MAX {
                sample_colors[bucket] = rgb;
                sample_color_count += 1;
                if sample_color_count > opaque_color_limit {
                    return QualityLowResHints {
                        likely_exact: false,
                        likely_small_palette: false,
                        prefers_run_coalescing: false,
                        exact_impossible: true,
                        sampled_alpha_255,
                    };
                }
                break;
            }
            bucket = (bucket + 1) & (SAMPLE_TABLE_SIZE - 1);
        }
        if sample_index == 7 && sample_color_count <= 2 && adjacent_matches >= 7 {
            return QualityLowResHints {
                likely_exact: true,
                likely_small_palette: true,
                prefers_run_coalescing: true,
                exact_impossible: false,
                sampled_alpha_255,
            };
        }
    }
    QualityLowResHints {
        likely_exact: sample_color_count <= LIKELY_EXACT_COLOR_LIMIT,
        likely_small_palette: sample_color_count <= SMALL_COLOR_LIMIT,
        prefers_run_coalescing: sample_count > 0 && adjacent_matches * 4 >= sample_count * 3,
        exact_impossible: false,
        sampled_alpha_255: sample_count > 0 && sampled_alpha_255,
    }
}

#[inline(always)]
pub(crate) fn rgba_run_end(
    rgba_pointer: *const u8,
    pixel_index: usize,
    pixel_count: usize,
    packed: u32,
) -> usize {
    let mut run_end = pixel_index;
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    {
        use core::arch::wasm32::{u32x4_splat, v128_any_true, v128_load, v128_xor};

        let repeated = u32x4_splat(packed);
        while run_end + 4 <= pixel_count {
            let block = unsafe { v128_load(rgba_pointer.add(run_end * 4).cast()) };
            if v128_any_true(v128_xor(block, repeated)) {
                break;
            }
            run_end += 4;
        }
    }
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    {
        let repeated = u128::from(packed) * 0x0000_0001_0000_0001_0000_0001_0000_0001;
        while run_end + 4 <= pixel_count {
            let block = u128::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(run_end * 4).cast())
            });
            if block != repeated {
                break;
            }
            run_end += 4;
        }
    }
    if run_end == pixel_index {
        run_end += 1;
    }
    while run_end < pixel_count {
        let next =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(run_end * 4).cast()) });
        if next != packed {
            break;
        }
        run_end += 1;
    }
    run_end
}

#[inline(always)]
pub(crate) unsafe fn write_index_run(output: *mut u8, index: u8, length: usize) {
    if length > 16 {
        unsafe { std::ptr::write_bytes(output, index, length) };
        return;
    }
    let repeated = u64::from(index) * 0x0101_0101_0101_0101;
    let mut offset = 0usize;
    while offset + 8 <= length {
        unsafe { std::ptr::write_unaligned(output.add(offset).cast(), repeated) };
        offset += 8;
    }
    if offset + 4 <= length {
        unsafe { std::ptr::write_unaligned(output.add(offset).cast(), repeated as u32) };
        offset += 4;
    }
    if offset + 2 <= length {
        unsafe { std::ptr::write_unaligned(output.add(offset).cast(), repeated as u16) };
        offset += 2;
    }
    if offset < length {
        unsafe { output.add(offset).write(index) };
    }
}

#[inline(always)]
pub(crate) fn rgba_ranges_equal(left: *const u8, right: *const u8, pixel_count: usize) -> bool {
    let byte_count = pixel_count * 4;
    let mut offset = 0usize;
    while offset + 64 <= byte_count {
        if !rgba_blocks_equal_64(unsafe { left.add(offset) }, unsafe { right.add(offset) }) {
            return false;
        }
        offset += 64;
    }
    while offset + 16 <= byte_count {
        if !rgba_blocks_equal_16(unsafe { left.add(offset) }, unsafe { right.add(offset) }) {
            return false;
        }
        offset += 16;
    }
    while offset < byte_count {
        let left_pixel = u32::from_le(unsafe { std::ptr::read_unaligned(left.add(offset).cast()) });
        let right_pixel =
            u32::from_le(unsafe { std::ptr::read_unaligned(right.add(offset).cast()) });
        if left_pixel != right_pixel {
            return false;
        }
        offset += 4;
    }
    true
}

#[inline(never)]
pub(crate) fn index_rgba_frames_quality_low_res_exact<const COALESCE_RUNS: bool>(
    rgba_stream: &[u8],
    alpha_threshold: u8,
    row_pixel_count: usize,
) -> QualityIndexResult {
    let pixel_count = rgba_stream.len() / 4;
    // The initialization hook enters with an empty stream solely to compile
    // this hot function. The first real encode must still allocate cold
    // scratch, so do not touch the reusable color table here.
    if pixel_count == 0 {
        return QualityIndexResult::Exact((Vec::new(), Vec::new(), None));
    }
    let mut table = take_quality_color_index_table(COLOR_INDEX_CAP);
    let mut palette = take_quality_palette(256);
    // Delay histogram updates until the exact-color probe overflows. Exact
    // inputs are common for flat/illustrated frames, and their histogram is
    // discarded immediately; overflowing inputs replay only the tiny prefix
    // that was already scanned before the 257th color was discovered.
    // Keep the exact-prefix indices so <=256-color inputs do not require a
    // second full RGBA scan after the palette decision is known.
    // The reusable buffer is already reserved during Wasm initialization for
    // the common image sizes. Give it its final length up front so exact-color
    // inputs can write indices directly instead of growing and updating Vec
    // length once per pixel. Overflow paths discard the partially written u8
    // buffer without reading it and then reuse the same capacity for mapping.
    let mut indexed = take_quantized_indexed(pixel_count);
    let indexed_pointer = indexed.as_mut_ptr();
    let mut has_transparent_pixels = false;
    let rgba_pointer = rgba_stream.as_ptr();
    let mut overflowed = false;
    if COALESCE_RUNS {
        let mut pixel_index = 0usize;
        while pixel_index < pixel_count {
            if row_pixel_count > 0
                && pixel_index >= row_pixel_count
                && pixel_index + row_pixel_count <= pixel_count
                && pixel_index.is_multiple_of(row_pixel_count)
                && rgba_ranges_equal(
                    unsafe { rgba_pointer.add(pixel_index * 4) },
                    unsafe { rgba_pointer.add((pixel_index - row_pixel_count) * 4) },
                    row_pixel_count,
                )
            {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        indexed_pointer.add(pixel_index - row_pixel_count),
                        indexed_pointer.add(pixel_index),
                        row_pixel_count,
                    );
                }
                pixel_index += row_pixel_count;
                continue;
            }
            let packed = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
            });
            let run_end = rgba_run_end(rgba_pointer, pixel_index, pixel_count, packed);
            let run_length = run_end - pixel_index;
            let alpha = (packed >> 24) as u8;
            let index = if alpha < alpha_threshold {
                has_transparent_pixels = true;
                if palette.len() == 256 {
                    overflowed = true;
                    break;
                }
                u8::MAX
            } else {
                let rgb = rgb_key(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8);
                if let Some(index) = table.get(rgb) {
                    index
                } else {
                    let color_limit = if has_transparent_pixels { 255 } else { 256 };
                    if palette.len() == color_limit {
                        overflowed = true;
                        break;
                    }
                    let index = palette.len() as u8;
                    table.insert_if_absent(rgb, index);
                    palette.push(rgb);
                    index
                }
            };
            unsafe { write_index_run(indexed_pointer.add(pixel_index), index, run_length) };
            pixel_index = run_end;
        }
    } else {
        let mut previous_rgb = u32::MAX;
        let mut previous_index = 0u8;
        for pixel_index in 0..pixel_count {
            let packed = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
            });
            let alpha = (packed >> 24) as u8;
            if alpha < alpha_threshold {
                has_transparent_pixels = true;
                unsafe { *indexed_pointer.add(pixel_index) = u8::MAX };
                if palette.len() == 256 {
                    overflowed = true;
                    break;
                }
                continue;
            }
            let rgb = rgb_key(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8);
            if rgb == previous_rgb {
                unsafe { *indexed_pointer.add(pixel_index) = previous_index };
                continue;
            }
            if let Some(index) = table.get(rgb) {
                previous_rgb = rgb;
                previous_index = index;
                unsafe { *indexed_pointer.add(pixel_index) = index };
                continue;
            }
            let color_limit = if has_transparent_pixels { 255 } else { 256 };
            if palette.len() == color_limit {
                overflowed = true;
                break;
            }
            let index = palette.len() as u8;
            table.insert_if_absent(rgb, index);
            palette.push(rgb);
            previous_rgb = rgb;
            previous_index = index;
            unsafe { *indexed_pointer.add(pixel_index) = index };
        }
    }
    if overflowed {
        recycle_quantized_indexed(indexed);
        recycle_quality_color_index_table(table);
        QualityIndexResult::Quantized(index_rgba_frames_quality_low_res_quantized(
            rgba_stream,
            alpha_threshold,
            palette,
        ))
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

#[inline(never)]
pub(crate) fn index_rgba_frames_quality_low_res_quantized(
    rgba_stream: &[u8],
    alpha_threshold: u8,
    palette: Vec<u32>,
) -> QualityIndexPlan {
    if alpha_threshold == 0 {
        index_rgba_frames_quality_low_res_quantized_opaque(rgba_stream, palette)
    } else {
        index_rgba_frames_quality_low_res_quantized_alpha(rgba_stream, alpha_threshold, palette)
    }
}

#[inline(never)]
pub(crate) fn index_rgba_frames_quality_low_res_quantized_alpha(
    rgba_stream: &[u8],
    alpha_threshold: u8,
    palette: Vec<u32>,
) -> QualityIndexPlan {
    if rgba_stream_samples_alpha_255(rgba_stream) {
        index_rgba_frames_quality_low_res_quantized_sampled_opaque(
            rgba_stream,
            alpha_threshold,
            palette,
        )
    } else {
        index_rgba_frames_quality_low_res_quantized_mixed(rgba_stream, alpha_threshold, palette)
    }
}

/// Whether most groups of eight pixels with opaque content hold still from
/// one frame to the next, judged from 64 groups spread over the animation.
pub(crate) fn temporal_histogram_pays(
    rgba_stream: &[u8],
    frame_len: usize,
    alpha_threshold: u8,
) -> bool {
    const SAMPLES: usize = 64;
    let frame_count = rgba_stream.len() / 4 / frame_len.max(1);
    let groups = frame_len / 8;
    if frame_count < 2 || groups == 0 {
        return false;
    }
    let mut opaque = 0usize;
    let mut unchanged = 0usize;
    for sample in 0..SAMPLES {
        let frame = 1 + sample % (frame_count - 1);
        let group = sample.wrapping_mul(2_654_435_761) % groups;
        let offset = (frame * frame_len + group * 8) * 4;
        let current = &rgba_stream[offset..offset + 32];
        if !(0..8).any(|pixel| current[pixel * 4 + 3] >= alpha_threshold) {
            continue;
        }
        opaque += 1;
        let previous_offset = offset - frame_len * 4;
        unchanged += usize::from(current == &rgba_stream[previous_offset..previous_offset + 32]);
    }
    opaque > 0 && unchanged * 2 > opaque
}

/// Build the coarse histogram counting each group of eight pixels that holds
/// still across consecutive frames once, weighted by the number of frames it
/// holds. Sums of integers do not depend on their order, so every bin, and
/// the palette and GIF built from it, match a pixel-by-pixel scan.
#[inline(never)]
pub(crate) fn index_rgba_frames_quality_low_res_quantized_temporal(
    rgba_stream: &[u8],
    frame_len: usize,
    alpha_threshold: u8,
    palette: Vec<u32>,
) -> QualityIndexPlan {
    const HISTOGRAM_BITS: usize = 4;
    const HISTOGRAM_LEN: usize = 1 << (HISTOGRAM_BITS * 3);
    let mut histogram = take_quality_histogram_u32(HISTOGRAM_LEN);
    let frame_count = rgba_stream.len() / 4 / frame_len;
    let groups = frame_len / 8;
    let pointer = rgba_stream.as_ptr();
    // Frames after each group's current one that repeat it exactly.
    let mut held = vec![0u32; groups];
    let mut has_transparent_pixels = false;
    let mut add = |packed: u32, weight: u32| {
        if ((packed >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
            return;
        }
        let index = quality_histogram_index_packed::<HISTOGRAM_BITS>(packed);
        unsafe {
            add_quality_histogram_bin32(
                histogram.as_mut_ptr().add(index),
                weight,
                u32::from(packed as u8) * weight,
                u32::from((packed >> 8) as u8) * weight,
                u32::from((packed >> 16) as u8) * weight,
            );
        }
    };
    for frame in (0..frame_count).rev() {
        let frame_start = frame * frame_len;
        for (group, held) in held.iter_mut().enumerate() {
            let offset = (frame_start + group * 8) * 4;
            if frame > 0 {
                let previous = offset - frame_len * 4;
                let same = unsafe {
                    rgba_blocks_equal_16(pointer.add(offset), pointer.add(previous))
                        && rgba_blocks_equal_16(
                            pointer.add(offset + 16),
                            pointer.add(previous + 16),
                        )
                };
                if same {
                    *held += 1;
                    continue;
                }
            }
            let weight = *held + 1;
            *held = 0;
            for pixel in 0..8 {
                let packed = u32::from_le(unsafe {
                    std::ptr::read_unaligned(pointer.add(offset + pixel * 4).cast())
                });
                add(packed, weight);
            }
        }
        // Pixels past the last whole group are counted every frame.
        for pixel in frame_start + groups * 8..frame_start + frame_len {
            let packed =
                u32::from_le(unsafe { std::ptr::read_unaligned(pointer.add(pixel * 4).cast()) });
            add(packed, 1);
        }
    }
    finish_quality_low_res_quantized(histogram, has_transparent_pixels, palette)
}

#[inline(never)]
pub(crate) fn index_rgba_frames_quality_low_res_quantized_mixed(
    rgba_stream: &[u8],
    alpha_threshold: u8,
    palette: Vec<u32>,
) -> QualityIndexPlan {
    // Explicit Wasm initialization enters with an empty sentinel solely to
    // compile this wrapper without allocating or retaining histogram scratch.
    if rgba_stream.is_empty() {
        return QualityIndexPlan {
            palette,
            histogram_to_palette: Vec::new(),
            transparent_index: None,
            mapping_bits: 4,
        };
    }
    const HISTOGRAM_BITS: usize = 4;
    const HISTOGRAM_LEN: usize = 1 << (HISTOGRAM_BITS * 3);
    let mut histogram = take_quality_histogram_u32(HISTOGRAM_LEN);
    let has_transparent_pixels = accumulate_quality_histogram_u32_bits_remaining_mixed::<
        HISTOGRAM_BITS,
    >(&mut histogram, rgba_stream, 0, alpha_threshold);
    finish_quality_low_res_quantized(histogram, has_transparent_pixels, palette)
}

#[inline(never)]
pub(crate) fn index_rgba_frames_quality_low_res_quantized_sampled_opaque(
    rgba_stream: &[u8],
    alpha_threshold: u8,
    palette: Vec<u32>,
) -> QualityIndexPlan {
    // See `prepare_quality_encoder_code`: compile this real opaque-probe
    // wrapper without allocating or retaining its histogram scratch.
    if rgba_stream.is_empty() {
        return QualityIndexPlan {
            palette,
            histogram_to_palette: Vec::new(),
            transparent_index: None,
            mapping_bits: 4,
        };
    }
    const HISTOGRAM_BITS: usize = 4;
    const HISTOGRAM_LEN: usize = 1 << (HISTOGRAM_BITS * 3);
    let mut histogram = take_quality_histogram_u32(HISTOGRAM_LEN);
    let all_alpha_255 =
        accumulate_quality_histogram_u32_bits_opaque_adaptive::<true>(&mut histogram, rgba_stream);
    let has_transparent_pixels = if all_alpha_255 {
        false
    } else {
        histogram.fill(RgbHistogramBin32::default());
        accumulate_quality_histogram_u32_bits_remaining_mixed::<HISTOGRAM_BITS>(
            &mut histogram,
            rgba_stream,
            0,
            alpha_threshold,
        )
    };
    finish_quality_low_res_quantized(histogram, has_transparent_pixels, palette)
}

#[inline(never)]
pub(crate) fn index_rgba_frames_quality_low_res_quantized_opaque(
    rgba_stream: &[u8],
    palette: Vec<u32>,
) -> QualityIndexPlan {
    const HISTOGRAM_BITS: usize = 4;
    const HISTOGRAM_LEN: usize = 1 << (HISTOGRAM_BITS * 3);
    let mut histogram = take_quality_histogram_u32(HISTOGRAM_LEN);
    accumulate_quality_histogram_u32_bits_remaining_opaque::<HISTOGRAM_BITS, false>(
        &mut histogram,
        rgba_stream,
        0,
    );
    finish_quality_low_res_quantized(histogram, false, palette)
}

#[inline(never)]
pub(crate) fn finish_quality_low_res_quantized(
    mut histogram: Vec<RgbHistogramBin32>,
    has_transparent_pixels: bool,
    palette: Vec<u32>,
) -> QualityIndexPlan {
    const HISTOGRAM_BITS: usize = 4;
    let colors = quality_colors_from_histogram_u32::<true>(&mut histogram);
    recycle_quality_histogram_u32(histogram);
    let opaque_color_limit = if has_transparent_pixels { 255 } else { 256 };
    if colors.len() <= opaque_color_limit {
        return build_quality_direct_cell_plan(has_transparent_pixels, colors, palette);
    }
    if colors.len() == opaque_color_limit + 1 {
        return build_quality_single_merge_plan(has_transparent_pixels, colors, palette);
    }
    if quality_colors_form_equal_count_cartesian_grid(&colors) {
        return build_quality_flat_grid_plan(has_transparent_pixels, colors, palette);
    }
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    if (512..=QUALITY_DOMINANT_COLOR_LIMIT).contains(&colors.len()) {
        return build_quality_dominant_index_plan_simd::<HISTOGRAM_BITS>(
            has_transparent_pixels,
            colors,
            palette,
        );
    }
    build_quality_index_plan_from_colors::<true, HISTOGRAM_BITS>(
        has_transparent_pixels,
        colors,
        palette,
    )
}

pub(crate) fn index_rgba_frames_quality_high_res(
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
#[inline(never)]
pub(crate) fn quality_prefers_high_precision_histogram(
    rgba_stream: &[u8],
    alpha_threshold: u8,
) -> bool {
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

pub(crate) fn take_quality_histogram_u32(length: usize) -> Vec<RgbHistogramBin32> {
    let clean = REUSABLE_QUALITY_HISTOGRAM_U32_CLEAN.with(|clean| clean.replace(false));
    REUSABLE_QUALITY_HISTOGRAM_U32.with(|scratch| {
        let mut histogram = std::mem::take(&mut *scratch.borrow_mut());
        if histogram.len() != length {
            histogram.resize(length, RgbHistogramBin32::default());
        } else if !clean {
            histogram.fill(RgbHistogramBin32::default());
        }
        histogram
    })
}

pub(crate) fn recycle_quality_histogram_u32(mut histogram: Vec<RgbHistogramBin32>) {
    let clean = REUSABLE_QUALITY_HISTOGRAM_U32_CLEAN.with(|clean| clean.replace(false));
    if !clean {
        histogram.fill(RgbHistogramBin32::default());
    }
    REUSABLE_QUALITY_HISTOGRAM_U32_CLEAN.with(|clean| {
        clean.replace(true);
    });
    REUSABLE_QUALITY_HISTOGRAM_U32.with(|scratch| {
        *scratch.borrow_mut() = histogram;
    });
}

pub(crate) fn take_quality_histogram_u64(length: usize) -> Vec<RgbHistogramBin> {
    let clean = REUSABLE_QUALITY_HISTOGRAM_U64_CLEAN.with(|clean| clean.replace(false));
    REUSABLE_QUALITY_HISTOGRAM_U64.with(|scratch| {
        let mut histogram = std::mem::take(&mut *scratch.borrow_mut());
        if histogram.len() != length {
            histogram.resize(length, RgbHistogramBin::default());
        } else if !clean {
            histogram.fill(RgbHistogramBin::default());
        }
        histogram
    })
}

pub(crate) fn recycle_quality_histogram_u64(mut histogram: Vec<RgbHistogramBin>) {
    let clean = REUSABLE_QUALITY_HISTOGRAM_U64_CLEAN.with(|clean| clean.replace(false));
    if !clean {
        histogram.fill(RgbHistogramBin::default());
    }
    REUSABLE_QUALITY_HISTOGRAM_U64_CLEAN.with(|clean| {
        clean.replace(true);
    });
    REUSABLE_QUALITY_HISTOGRAM_U64.with(|scratch| {
        *scratch.borrow_mut() = histogram;
    });
}

pub(crate) fn take_quality_histogram_to_palette(length: usize) -> Vec<u8> {
    REUSABLE_QUALITY_HISTOGRAM_TO_PALETTE.with(|scratch| {
        let mut table = std::mem::take(&mut *scratch.borrow_mut());
        if table.capacity() < length + 1 {
            table.reserve(length + 1 - table.capacity());
        }
        if table.len() != length {
            table.resize(length, 0);
        }
        // Every opaque input bin is rewritten before the table is read; stale
        // entries for bins absent from this image are therefore harmless.
        table
    })
}

pub(crate) fn recycle_quality_histogram_to_palette(table: Vec<u8>) {
    REUSABLE_QUALITY_HISTOGRAM_TO_PALETTE.with(|scratch| {
        *scratch.borrow_mut() = table;
    });
}

pub(crate) fn take_quantized_indexed(pixel_count: usize) -> Vec<u8> {
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

pub(crate) fn recycle_quantized_indexed(indexed: Vec<u8>) {
    REUSABLE_QUANTIZED_BYTES.with(|scratch| {
        *scratch.borrow_mut() = indexed;
    });
}

pub(crate) fn take_quality_probe_indices(capacity: usize) -> Vec<u8> {
    REUSABLE_QUANTIZED_BYTES.with(|scratch| {
        let mut indexed = std::mem::take(&mut *scratch.borrow_mut());
        indexed.clear();
        if indexed.capacity() < capacity {
            indexed.reserve(capacity - indexed.capacity());
        }
        indexed
    })
}

pub(crate) fn index_rgba_frames_quality_u32<const BITS: usize>(
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
        // Map pixels directly from RGBA during literal emission. Retaining a
        // u16 histogram cell for every pixel adds a large allocation, write,
        // and read pass on multi-megapixel inputs; recomputing the compact
        // 4-bit cell while emitting is cheaper and produces the same indices.
        let probe_opaque =
            !all_opaque && alpha_threshold != 0 && rgba_stream_samples_alpha_255(rgba_stream);
        if probe_opaque {
            let all_alpha_255 = accumulate_quality_histogram_u32_bits_remaining_opaque::<BITS, true>(
                &mut histogram,
                rgba_stream,
                0,
            );
            if !all_alpha_255 {
                histogram.fill(RgbHistogramBin32::default());
                has_transparent_pixels = accumulate_quality_histogram_u32_bits_remaining_mixed::<
                    BITS,
                >(
                    &mut histogram, rgba_stream, 0, alpha_threshold
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
                prefix_offset += 4;
            }
            if all_opaque {
                accumulate_quality_histogram_u32_bits_remaining_opaque::<BITS, false>(
                    &mut histogram,
                    rgba_stream,
                    start_offset,
                );
            } else {
                has_transparent_pixels |=
                    accumulate_quality_histogram_u32_bits_remaining_mixed::<BITS>(
                        &mut histogram,
                        rgba_stream,
                        start_offset,
                        alpha_threshold,
                    );
            }
        }
        recycle_quantized_indexed(indexed);

        let colors = quality_colors_from_histogram_u32::<true>(&mut histogram);
        recycle_quality_color_index_table(table);
        recycle_quality_histogram_u32(histogram);
        let plan = build_quality_index_plan_from_colors::<true, BITS>(
            has_transparent_pixels,
            colors,
            palette,
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

pub(crate) fn index_rgba_frames_quality_u64(
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

    let colors = quality_colors_from_histogram_u64(&mut histogram);
    recycle_quality_color_index_table(table);
    recycle_quality_histogram_u64(histogram);
    let plan = build_quality_index_plan_from_colors::<false, QUALITY_HISTOGRAM_BITS>(
        has_transparent_pixels,
        colors,
        palette,
    );
    QualityIndexResult::Quantized(plan)
}
