//! RGBA encoder entry points and GIF assembly for each palette mode.

use super::*;

#[derive(Default)]
pub(crate) struct ColorIndexTable {
    pub(crate) entries: Vec<u32>,
    pub(crate) mask: usize,
    pub(crate) sentinel_index: u8,
    pub(crate) has_sentinel: bool,
}

pub(crate) const COLOR_INDEX_EMPTY: u32 = 0x00ff_01fe;

impl ColorIndexTable {
    pub(crate) const fn empty() -> Self {
        Self {
            entries: Vec::new(),
            mask: 0,
            sentinel_index: 0,
            has_sentinel: false,
        }
    }

    pub(crate) fn new(capacity: usize) -> Self {
        debug_assert!(capacity.is_power_of_two());
        Self {
            entries: vec![COLOR_INDEX_EMPTY; capacity],
            mask: capacity - 1,
            sentinel_index: 0,
            has_sentinel: false,
        }
    }

    pub(crate) fn reset(&mut self, capacity: usize) {
        debug_assert!(capacity.is_power_of_two());
        if self.entries.len() != capacity {
            self.entries.resize(capacity, COLOR_INDEX_EMPTY);
        } else {
            self.entries.fill(COLOR_INDEX_EMPTY);
        }
        self.mask = capacity - 1;
        self.has_sentinel = false;
    }

    #[inline(always)]
    pub(crate) fn get(&self, key: u32) -> Option<u8> {
        if key == COLOR_INDEX_EMPTY {
            return self.has_sentinel.then_some(self.sentinel_index);
        }
        let mask = self.mask;
        let mut slot = (key as usize).wrapping_mul(2_654_435_761) & mask;
        loop {
            let entry = unsafe { *self.entries.get_unchecked(slot) };
            let stored = entry & 0x00ff_ffff;
            if stored == key {
                return Some((entry >> 24) as u8);
            }
            if stored == COLOR_INDEX_EMPTY {
                return None;
            }
            slot = (slot + 1) & mask;
        }
    }

    #[inline(always)]
    pub(crate) fn insert_if_absent(&mut self, key: u32, value: u8) {
        if key == COLOR_INDEX_EMPTY {
            if !self.has_sentinel {
                self.sentinel_index = value;
                self.has_sentinel = true;
            }
            return;
        }
        let mask = self.mask;
        let mut slot = (key as usize).wrapping_mul(2_654_435_761) & mask;
        loop {
            let entry = unsafe { *self.entries.get_unchecked(slot) };
            let stored = entry & 0x00ff_ffff;
            if stored == key {
                return;
            }
            if stored == COLOR_INDEX_EMPTY {
                unsafe {
                    *self.entries.get_unchecked_mut(slot) = key | (u32::from(value) << 24);
                }
                return;
            }
            slot = (slot + 1) & mask;
        }
    }
}

pub(crate) fn take_quality_color_index_table(capacity: usize) -> ColorIndexTable {
    REUSABLE_QUALITY_COLOR_INDEX.with(|scratch| {
        let mut table = std::mem::take(&mut *scratch.borrow_mut());
        table.reset(capacity);
        table
    })
}

pub(crate) fn recycle_quality_color_index_table(table: ColorIndexTable) {
    REUSABLE_QUALITY_COLOR_INDEX.with(|scratch| {
        *scratch.borrow_mut() = table;
    });
}

pub(crate) struct PaletteMapper<'a> {
    pub(crate) palette_rgb: &'a [u32],
    pub(crate) exact: ColorIndexTable,
}

impl<'a> PaletteMapper<'a> {
    pub(crate) fn new(palette_rgb: &'a [u32]) -> Self {
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
    pub(crate) fn exact_index(&self, r: u8, g: u8, b: u8) -> Option<u8> {
        self.exact.get(rgb_key(r, g, b))
    }

    #[inline]
    pub(crate) fn index_pixel(&self, r: u8, g: u8, b: u8) -> u8 {
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
pub(crate) fn encode_rgba_gif_advanced_inner(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    deltas: bool,
    alpha_threshold: u8,
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
        quantization,
        palette_mode,
        Vec::new(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_rgba_gif_advanced_inner_with_output(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    deltas: bool,
    alpha_threshold: u8,
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
            alpha_threshold,
            quantization == RgbaQuantization::Exact,
            false,
        ) {
            Ok(encoded) => return Ok(encoded),
            Err(message) if message == RGBA_DELTA_FALLBACK => {}
            Err(message) => return Err(message),
        }
    }

    let (palette, indexed, transparent_index) = if deltas
        && palette_rgb.is_empty()
        && quantization == RgbaQuantization::Quality
        && rgba_stream.len() / 4 <= QUALITY_LOW_RES_PIXEL_LIMIT
    {
        index_rgba_frames_quality_low_res_delta(rgba_stream, alpha_threshold)
    } else {
        index_rgba_frames_with_quantization(
            rgba_stream,
            palette_rgb,
            alpha_threshold,
            quantization,
        )?
    };
    if !deltas {
        let encoded = compact::encode_indexed_gif_compact(
            output,
            &indexed,
            width,
            height,
            frame_count,
            &palette,
            delays,
            loop_count,
            transparent_index,
            true,
        );
        recycle_quantized_indexed(indexed);
        return encoded;
    }
    let encoded = encode_indexed_literal_delta_gif_inner(
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_rgba_quality_gif_inner_with_output(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    delays: DelaySource<'_>,
    loop_count: i32,
    alpha_threshold: u8,
    independent_frames: bool,
    output: Vec<u8>,
) -> Result<Vec<u8>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    validate_rgba_stream(rgba_stream, width, height, frame_count, delays, loop_count)?;
    #[cfg(target_arch = "wasm32")]
    debug_assert_eq!(
        rgba_stream.len(),
        usize::from(width) * usize::from(height) * frame_count * 4
    );
    match index_rgba_frames_quality_result(rgba_stream, alpha_threshold) {
        QualityIndexResult::Exact((palette, indexed, transparent_index)) => {
            encode_quality_indexed_compact(
                output,
                palette,
                indexed,
                transparent_index,
                width,
                height,
                frame_count,
                delays,
                loop_count,
                independent_frames,
            )
        }
        QualityIndexResult::Quantized(plan) => encode_quality_plan_compact(
            output,
            plan,
            rgba_stream,
            alpha_threshold,
            width,
            height,
            frame_count,
            delays,
            loop_count,
            independent_frames,
        ),
    }
}

/// Assemble the compact GIF for a quantized quality plan and return the
/// plan's reusable buffers.
#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_quality_plan_compact(
    output: Vec<u8>,
    plan: QualityIndexPlan,
    rgba_stream: &[u8],
    alpha_threshold: u8,
    width: u16,
    height: u16,
    frame_count: usize,
    delays: DelaySource<'_>,
    loop_count: i32,
    independent_frames: bool,
) -> Result<Vec<u8>, String> {
    let QualityIndexPlan {
        palette,
        histogram_to_palette,
        transparent_index,
        mapping_bits,
    } = plan;
    let frame_len = usize::from(width) * usize::from(height);
    let mut indexed = take_quantized_indexed(frame_len * frame_count);
    let (threshold, transparent) = (alpha_threshold, transparent_index.unwrap_or(0));
    for frame in 0..frame_count {
        let (done, rest) = indexed.split_at_mut(frame * frame_len);
        let out = &mut rest[..frame_len];
        let rgba = &rgba_stream[frame * frame_len * 4..(frame + 1) * frame_len * 4];
        // Each frame may copy unchanged groups from the frame before it.
        let previous = (frame > 0).then(|| {
            (
                &rgba_stream[(frame - 1) * frame_len * 4..frame * frame_len * 4],
                &done[(frame - 1) * frame_len..],
            )
        });
        let table = &histogram_to_palette;
        match (mapping_bits, transparent_index.is_some()) {
            (4, false) => {
                map_quality_frame::<4, false>(rgba, previous, threshold, transparent, table, out)
            }
            (4, true) => {
                map_quality_frame::<4, true>(rgba, previous, threshold, transparent, table, out)
            }
            (5, false) => {
                map_quality_frame::<5, false>(rgba, previous, threshold, transparent, table, out)
            }
            (5, true) => {
                map_quality_frame::<5, true>(rgba, previous, threshold, transparent, table, out)
            }
            _ => unreachable!("unsupported quality histogram precision"),
        }
    }
    recycle_quality_histogram_to_palette(histogram_to_palette);
    encode_quality_indexed_compact(
        output,
        palette,
        indexed,
        transparent_index,
        width,
        height,
        frame_count,
        delays,
        loop_count,
        independent_frames,
    )
}

/// Assemble the compact GIF for an indexed quality result and return its
/// reusable palette and index buffers.
#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_quality_indexed_compact(
    output: Vec<u8>,
    palette: Vec<u32>,
    indexed: Vec<u8>,
    transparent_index: Option<u8>,
    width: u16,
    height: u16,
    frame_count: usize,
    delays: DelaySource<'_>,
    loop_count: i32,
    independent_frames: bool,
) -> Result<Vec<u8>, String> {
    let encoded = compact::encode_indexed_gif_compact(
        output,
        &indexed,
        width,
        height,
        frame_count,
        &palette,
        delays,
        loop_count,
        transparent_index,
        independent_frames,
    );
    recycle_quality_palette(palette);
    recycle_quantized_indexed(indexed);
    encoded
}

/// Encode the quantized half of the normal-size quality path without making
/// V8 compile the exact-color and high-resolution branches on the first
/// photographic encode. The Wasm boundary proves the exact probe overflowed
/// before entering here; less common inputs stay on the complete dispatcher.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
pub(crate) fn encode_rgba_quality_low_res_quantized_gif_inner_with_output(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    delays: DelaySource<'_>,
    loop_count: i32,
    alpha_threshold: u8,
    sampled_alpha_255: bool,
    independent_frames: bool,
    output: Vec<u8>,
) -> Result<Vec<u8>, String> {
    // Explicit Wasm initialization enters with an empty sentinel solely to
    // compile this quantized wrapper. Keep that preparation data-independent.
    if rgba_stream.is_empty() {
        return Ok(output);
    }
    #[cfg(not(target_arch = "wasm32"))]
    validate_rgba_stream(rgba_stream, width, height, frame_count, delays, loop_count)?;
    #[cfg(target_arch = "wasm32")]
    debug_assert_eq!(
        rgba_stream.len(),
        usize::from(width) * usize::from(height) * frame_count * 4
    );
    let palette = take_quality_palette(256);
    let frame_len = usize::from(width) * usize::from(height);
    let plan = if temporal_histogram_pays(rgba_stream, frame_len, alpha_threshold) {
        index_rgba_frames_quality_low_res_quantized_temporal(
            rgba_stream,
            frame_len,
            alpha_threshold,
            palette,
        )
    } else if alpha_threshold == 0 {
        index_rgba_frames_quality_low_res_quantized_opaque(rgba_stream, palette)
    } else if sampled_alpha_255 {
        index_rgba_frames_quality_low_res_quantized_sampled_opaque(
            rgba_stream,
            alpha_threshold,
            palette,
        )
    } else {
        index_rgba_frames_quality_low_res_quantized_mixed(rgba_stream, alpha_threshold, palette)
    };
    encode_quality_plan_compact(
        output,
        plan,
        rgba_stream,
        alpha_threshold,
        width,
        height,
        frame_count,
        delays,
        loop_count,
        independent_frames,
    )
}

/// Encode the exact-color half of the normal-size quality path after the
/// caller's stack probe has proved that every opaque RGB value fits in GIF's
/// palette. Keeping this separate also prevents exact illustrations from
/// compiling the quantizer and large-image encoder on their first call.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
pub(crate) fn encode_rgba_quality_low_res_exact_gif_inner_with_output<const COALESCE_RUNS: bool>(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    delays: DelaySource<'_>,
    loop_count: i32,
    alpha_threshold: u8,
    try_small_palette: bool,
    independent_frames: bool,
    output: Vec<u8>,
) -> Result<Vec<u8>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    validate_rgba_stream(rgba_stream, width, height, frame_count, delays, loop_count)?;
    #[cfg(target_arch = "wasm32")]
    debug_assert_eq!(
        rgba_stream.len(),
        usize::from(width) * usize::from(height) * frame_count * 4
    );
    if try_small_palette {
        if let Some((palette, indexed, transparent_index)) = index_rgba_frames_quality_small_exact(
            rgba_stream,
            alpha_threshold,
            usize::from(width) * usize::from(height),
        ) {
            return encode_quality_indexed_compact(
                output,
                palette,
                indexed,
                transparent_index,
                width,
                height,
                frame_count,
                delays,
                loop_count,
                independent_frames,
            );
        }
    }
    match index_rgba_frames_quality_low_res_exact::<COALESCE_RUNS>(
        rgba_stream,
        alpha_threshold,
        usize::from(width),
    ) {
        QualityIndexResult::Exact((palette, indexed, transparent_index)) => {
            encode_quality_indexed_compact(
                output,
                palette,
                indexed,
                transparent_index,
                width,
                height,
                frame_count,
                delays,
                loop_count,
                independent_frames,
            )
        }
        QualityIndexResult::Quantized(plan) => encode_quality_plan_compact(
            output,
            plan,
            rgba_stream,
            alpha_threshold,
            width,
            height,
            frame_count,
            delays,
            loop_count,
            independent_frames,
        ),
    }
}

#[inline(always)]
pub(crate) fn rgba_blocks_equal_16(left: *const u8, right: *const u8) -> bool {
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    {
        use core::arch::wasm32::{v128_any_true, v128_load, v128_xor};
        let left = unsafe { v128_load(left.cast()) };
        let right = unsafe { v128_load(right.cast()) };
        !v128_any_true(v128_xor(left, right))
    }
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    {
        let left = unsafe { std::ptr::read_unaligned(left.cast::<u128>()) };
        let right = unsafe { std::ptr::read_unaligned(right.cast::<u128>()) };
        left == right
    }
}

#[inline(always)]
pub(crate) fn rgba_blocks_equal_64(left: *const u8, right: *const u8) -> bool {
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    {
        use core::arch::wasm32::{v128_any_true, v128_load, v128_or, v128_xor};
        let difference0 = v128_xor(unsafe { v128_load(left.cast()) }, unsafe {
            v128_load(right.cast())
        });
        let difference1 = v128_xor(unsafe { v128_load(left.add(16).cast()) }, unsafe {
            v128_load(right.add(16).cast())
        });
        let difference2 = v128_xor(unsafe { v128_load(left.add(32).cast()) }, unsafe {
            v128_load(right.add(32).cast())
        });
        let difference3 = v128_xor(unsafe { v128_load(left.add(48).cast()) }, unsafe {
            v128_load(right.add(48).cast())
        });
        !v128_any_true(v128_or(
            v128_or(difference0, difference1),
            v128_or(difference2, difference3),
        ))
    }
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    {
        rgba_blocks_equal_16(left, right)
            && rgba_blocks_equal_16(unsafe { left.add(16) }, unsafe { right.add(16) })
            && rgba_blocks_equal_16(unsafe { left.add(32) }, unsafe { right.add(32) })
            && rgba_blocks_equal_16(unsafe { left.add(48) }, unsafe { right.add(48) })
    }
}

#[inline(never)]
pub(crate) fn index_rgba_frames_quality_small_exact(
    rgba_stream: &[u8],
    alpha_threshold: u8,
    frame_pixel_count: usize,
) -> Option<(Vec<u32>, Vec<u8>, Option<u8>)> {
    const SMALL_COLOR_LIMIT: usize = 16;
    let pixel_count = rgba_stream.len() / 4;
    debug_assert!(frame_pixel_count > 0);
    debug_assert_eq!(pixel_count % frame_pixel_count, 0);
    // The initialization hook enters with an empty stream solely to compile
    // this hot function. Keep that data-independent preparation from warming
    // any reusable palette or index allocation.
    if pixel_count == 0 {
        return Some((Vec::new(), Vec::new(), None));
    }
    let mut palette = take_quality_palette(SMALL_COLOR_LIMIT);
    let mut indexed = take_quantized_indexed(pixel_count);
    let indexed_pointer = indexed.as_mut_ptr();
    let rgba_pointer = rgba_stream.as_ptr();
    let mut has_transparent_pixels = false;
    let mut previous_rgb = u32::MAX;
    let mut previous_index = 0u8;
    let mut pixel_index = 0usize;
    // The first frame has nothing to copy from, so index it one run of a
    // color at a time. Each run's first pixel is seen in scan order, so colors
    // join the palette in the same order as the per-pixel scan below, which
    // handles later frames: they are mostly copied from the previous frame,
    // and their changed pixels measured faster one at a time.
    let first_frame_end = frame_pixel_count.min(pixel_count);
    while pixel_index < first_frame_end {
        let packed = u32::from_le(unsafe {
            std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
        });
        let run_end = rgba_run_end(rgba_pointer, pixel_index, first_frame_end, packed);
        let index = if ((packed >> 24) as u8) < alpha_threshold {
            if !has_transparent_pixels && palette.len() == SMALL_COLOR_LIMIT {
                recycle_quality_palette(palette);
                recycle_quantized_indexed(indexed);
                return None;
            }
            has_transparent_pixels = true;
            u8::MAX
        } else {
            let rgb = rgb_key(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8);
            if rgb != previous_rgb {
                previous_index = match palette.iter().position(|&color| color == rgb) {
                    Some(index) => index as u8,
                    None => {
                        let color_limit = SMALL_COLOR_LIMIT - usize::from(has_transparent_pixels);
                        if palette.len() == color_limit {
                            recycle_quality_palette(palette);
                            recycle_quantized_indexed(indexed);
                            return None;
                        }
                        palette.push(rgb);
                        (palette.len() - 1) as u8
                    }
                };
                previous_rgb = rgb;
            }
            previous_index
        };
        unsafe {
            write_index_run(
                indexed_pointer.add(pixel_index),
                index,
                run_end - pixel_index,
            )
        };
        pixel_index = run_end;
    }
    let mut frame_remaining = frame_pixel_count;
    while pixel_index < pixel_count {
        if pixel_index >= frame_pixel_count && frame_remaining >= 16 {
            let previous_pixel = pixel_index - frame_pixel_count;
            if rgba_blocks_equal_64(unsafe { rgba_pointer.add(pixel_index * 4) }, unsafe {
                rgba_pointer.add(previous_pixel * 4)
            }) {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        indexed_pointer.add(previous_pixel),
                        indexed_pointer.add(pixel_index),
                        16,
                    );
                }
                pixel_index += 16;
                frame_remaining -= 16;
                if frame_remaining == 0 {
                    frame_remaining = frame_pixel_count;
                }
                continue;
            }
        }
        if pixel_index >= frame_pixel_count && frame_remaining >= 4 {
            let previous_pixel = pixel_index - frame_pixel_count;
            if rgba_blocks_equal_16(unsafe { rgba_pointer.add(pixel_index * 4) }, unsafe {
                rgba_pointer.add(previous_pixel * 4)
            }) {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        indexed_pointer.add(previous_pixel),
                        indexed_pointer.add(pixel_index),
                        4,
                    );
                }
                pixel_index += 4;
                frame_remaining -= 4;
                if frame_remaining == 0 {
                    frame_remaining = frame_pixel_count;
                }
                continue;
            }
        }
        let packed = u32::from_le(unsafe {
            std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
        });
        if ((packed >> 24) as u8) < alpha_threshold {
            if !has_transparent_pixels && palette.len() == SMALL_COLOR_LIMIT {
                recycle_quality_palette(palette);
                recycle_quantized_indexed(indexed);
                return None;
            }
            has_transparent_pixels = true;
            unsafe { *indexed_pointer.add(pixel_index) = u8::MAX };
            pixel_index += 1;
            frame_remaining -= 1;
            if frame_remaining == 0 {
                frame_remaining = frame_pixel_count;
            }
            continue;
        }
        let rgb = rgb_key(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8);
        if rgb == previous_rgb {
            unsafe { *indexed_pointer.add(pixel_index) = previous_index };
            pixel_index += 1;
            frame_remaining -= 1;
            if frame_remaining == 0 {
                frame_remaining = frame_pixel_count;
            }
            continue;
        }
        let index = palette.iter().position(|&color| color == rgb);
        let index = match index {
            Some(index) => index as u8,
            None => {
                let color_limit = SMALL_COLOR_LIMIT - usize::from(has_transparent_pixels);
                if palette.len() == color_limit {
                    recycle_quality_palette(palette);
                    recycle_quantized_indexed(indexed);
                    return None;
                }
                let index = palette.len() as u8;
                palette.push(rgb);
                index
            }
        };
        previous_rgb = rgb;
        previous_index = index;
        unsafe { *indexed_pointer.add(pixel_index) = index };
        pixel_index += 1;
        frame_remaining -= 1;
        if frame_remaining == 0 {
            frame_remaining = frame_pixel_count;
        }
    }
    let transparent_index = if has_transparent_pixels {
        let index = palette.len() as u8;
        palette.push(0);
        for pixel_index in 0..pixel_count {
            if unsafe { *indexed_pointer.add(pixel_index) } == u8::MAX {
                unsafe { *indexed_pointer.add(pixel_index) = index };
            }
        }
        Some(index)
    } else {
        None
    };
    Some((palette, indexed, transparent_index))
}

pub(crate) fn validate_rgba_stream(
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
pub(crate) fn encode_rgba_local_palette_gif_inner(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    delays: DelaySource<'_>,
    loop_count: i32,
    alpha_threshold: u8,
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
        let result =
            encode_indexed_literal_lzw_direct_to(&mut output, &indexed, min_code_size, color_count);
        recycle_quantized_indexed(indexed);
        result?;
    }
    output.push(0x3b);
    Ok(output)
}

pub(crate) fn write_local_palette_frame_header(
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
pub(crate) fn encode_rgba_gif_inner(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    deltas: bool,
    alpha_threshold: u8,
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
    if !deltas && palette_rgb.len() == 256 && rgba_stream_is_opaque(rgba_stream) {
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

    if !deltas && (9..=16).contains(&palette_rgb.len()) {
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
        index_rgba_frames(rgba_stream, palette_rgb, alpha_threshold, true)?;
    if !deltas {
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
    encode_indexed_literal_delta_gif_inner(
        &indexed,
        width,
        height,
        frame_count,
        &palette,
        delays,
        loop_count,
        transparent_index,
    )
}

pub(crate) fn encode_rgba_literal_gif_to_palette_inner(
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
pub(crate) fn encode_rgba_sixteen_color_literal_gif_inner(
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

pub(crate) fn rgba_alpha_invalid(rgba: &[u8], alpha_threshold: u8, exact_alpha: bool) -> bool {
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

pub(crate) fn rgba_rect_alpha_invalid(
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

pub(crate) const RGBA_DELTA_FALLBACK: &str = "RGBA delta palette path requires the general encoder";

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_rgba_delta_gif_to_palette_inner(
    rgba_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
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
    let mut mapped = Vec::with_capacity(frame_pixels);

    write_indexed_gif_header(&mut output, width, height, palette_rgb, color_count);
    write_loop_extension(&mut output, loop_count);

    let mut previous_frame: Option<&[u8]> = None;
    for (frame_index, frame) in rgba_stream.chunks_exact(frame_bytes).enumerate() {
        let delay = delays.get(frame_index);
        if let Some(previous) = previous_frame {
            if let Some(rect) =
                find_changed_rect_rgba_bytes(previous, frame, canvas_width, canvas_height)
            {
                let direct_two_bit = color_count <= 4;
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
            let direct_two_bit = color_count <= 4;
            if !direct_two_bit && rgba_alpha_invalid(frame, alpha_threshold, exact_alpha) {
                return Err(RGBA_DELTA_FALLBACK.to_string());
            }
            write_indexed_gif_frame_header(&mut output, 0, 0, width, height, delay, None, 0);
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
        }
        previous_frame = Some(frame);
    }

    output.push(0x3b);
    Ok(output)
}

pub(crate) fn index_rgba_frames(
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

pub(crate) fn index_rgba_frames_with_quantization(
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
