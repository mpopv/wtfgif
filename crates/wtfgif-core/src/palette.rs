//! Exact and fixed palettes, RGBA scans, and nearest-color mapping.

use super::*;

pub(crate) fn try_index_rgba_frames_exact(
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

pub(crate) fn index_rgba_frames_to_palette(
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

pub(crate) fn map_rgba_frame_to_palette(
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

pub(crate) fn map_rgba_frame_to_palette_cached<const CACHE_SIZE: usize>(
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

pub(crate) fn map_rgba_rect_to_palette(
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

pub(crate) fn map_rgba_rect_to_palette_cached<const CACHE_SIZE: usize>(
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
pub(crate) fn rgba_stream_is_opaque(rgba_stream: &[u8]) -> bool {
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
pub(crate) fn rgba_stream_has_transparent_pixels(rgba_stream: &[u8], alpha_threshold: u8) -> bool {
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
pub(crate) fn rgba_stream_samples_alpha_255(rgba_stream: &[u8]) -> bool {
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

pub(crate) fn index_rgba_frames_332(
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
pub(crate) fn rgb_key(r: u8, g: u8, b: u8) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

#[inline]
pub(crate) fn rgb332_index(r: u8, g: u8, b: u8) -> u8 {
    (r & 0xe0) | ((g >> 3) & 0x1c) | (b >> 6)
}

pub(crate) fn fixed_332_palette() -> Vec<u32> {
    let mut palette = Vec::with_capacity(256);
    for index in 0..256u32 {
        let r = (((index >> 5) & 0x07) * 255 + 3) / 7;
        let g = (((index >> 2) & 0x07) * 255 + 3) / 7;
        let b = ((index & 0x03) * 255 + 1) / 3;
        palette.push((r << 16) | (g << 8) | b);
    }
    palette
}

pub(crate) fn nearest_palette_index(r: u8, g: u8, b: u8, palette_rgb: &[u32]) -> u8 {
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
pub(crate) fn palette_color_distance(color: u32, r: u8, g: u8, b: u8) -> u32 {
    let dr = i32::from(r) - ((color >> 16) & 0xff) as i32;
    let dg = i32::from(g) - ((color >> 8) & 0xff) as i32;
    let db = i32::from(b) - (color & 0xff) as i32;
    (dr * dr + dg * dg + db * db) as u32
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
#[cold]
#[inline(never)]
pub(crate) fn build_quality_dominant_index_plan_simd<const HISTOGRAM_BITS: usize>(
    has_transparent_pixels: bool,
    mut colors: Vec<QuantizedColor>,
    mut palette: Vec<u32>,
) -> QualityIndexPlan {
    const DIRECT_PALETTE_CELL: u16 = 1 << 15;
    debug_assert_eq!(HISTOGRAM_BITS, 4);
    debug_assert!((512..=QUALITY_DOMINANT_COLOR_LIMIT).contains(&colors.len()));
    let mut palette_positions = [0u16; QUALITY_DOMINANT_COLOR_LIMIT];
    for (position, slot) in palette_positions[..colors.len()].iter_mut().enumerate() {
        *slot = position as u16;
    }
    let palette_positions = &mut palette_positions[..colors.len()];
    let maximum_count = colors.iter().map(|color| color.count).max().unwrap_or(0);
    let passes = (u32::BITS - maximum_count.leading_zeros()).div_ceil(8);
    let mut scratch = [0u16; QUALITY_DOMINANT_COLOR_LIMIT];
    for pass in 0..passes {
        let shift = pass * 8;
        let (source, destination): (&[u16], &mut [u16]) = if pass & 1 == 0 {
            (&*palette_positions, &mut scratch[..colors.len()])
        } else {
            (&scratch[..colors.len()], &mut *palette_positions)
        };
        let mut frequencies = [0u16; 256];
        for &position in source {
            let digit = (colors[usize::from(position)].count >> shift) as u8;
            frequencies[usize::from(digit)] += 1;
        }
        let mut offset = 0u16;
        for digit in (0..256).rev() {
            let frequency = frequencies[digit];
            frequencies[digit] = offset;
            offset += frequency;
        }
        for &position in source {
            let digit = (colors[usize::from(position)].count >> shift) as u8;
            let target = &mut frequencies[usize::from(digit)];
            destination[usize::from(*target)] = position;
            *target += 1;
        }
    }
    if passes & 1 != 0 {
        palette_positions.copy_from_slice(&scratch[..palette_positions.len()]);
    }

    let palette_len = 256usize - usize::from(has_transparent_pixels);
    palette.clear();
    if palette.capacity() < palette_len + usize::from(has_transparent_pixels) {
        palette.reserve(palette_len + usize::from(has_transparent_pixels) - palette.capacity());
    }
    for &position in &palette_positions[..palette_len] {
        let color = &colors[usize::from(position)];
        palette.push(rgb_key(color.red, color.green, color.blue));
    }
    let mut search = sorted_palette::SortedPaletteSearch::new();
    search.rebuild(&palette);

    let mapping_len = 1usize << (HISTOGRAM_BITS * 3);
    let mut histogram_to_palette = take_quality_histogram_to_palette(mapping_len);
    for (index, &position) in palette_positions[..palette_len].iter().enumerate() {
        let color = &mut colors[usize::from(position)];
        histogram_to_palette[usize::from(color.histogram_index)] = index as u8;
        color.histogram_index |= DIRECT_PALETTE_CELL;
    }
    let mut counts = [0u32; 256];
    let mut red_sums = [0u32; 256];
    let mut green_sums = [0u32; 256];
    let mut blue_sums = [0u32; 256];
    for color in &mut colors {
        let cell = usize::from(color.histogram_index & !DIRECT_PALETTE_CELL);
        let index = if color.histogram_index & DIRECT_PALETTE_CELL != 0 {
            color.histogram_index &= !DIRECT_PALETTE_CELL;
            histogram_to_palette[cell]
        } else {
            search.nearest(color.red, color.green, color.blue)
        };
        histogram_to_palette[cell] = index;
        let count = color.count;
        let index = usize::from(index);
        counts[index] += count;
        red_sums[index] += u32::from(color.red) * count;
        green_sums[index] += u32::from(color.green) * count;
        blue_sums[index] += u32::from(color.blue) * count;
    }

    let mut palette_changed = false;
    for index in 0..palette.len() {
        let count = counts[index];
        if count == 0 {
            continue;
        }
        let reciprocal = (1u64 << 31) / u64::from(count);
        let representative = rgb_key(
            rounded_weighted_average_u32_reciprocal(red_sums[index], count, reciprocal),
            rounded_weighted_average_u32_reciprocal(green_sums[index], count, reciprocal),
            rounded_weighted_average_u32_reciprocal(blue_sums[index], count, reciprocal),
        );
        palette_changed |= palette[index] != representative;
        palette[index] = representative;
    }
    if palette_changed {
        search.rebuild(&palette);
        for color in &colors {
            let cell = usize::from(color.histogram_index);
            let hint_index = histogram_to_palette[cell];
            let hint_color = palette[usize::from(hint_index)];
            histogram_to_palette[cell] = if hint_color
                == rgb_key(color.red, color.green, color.blue)
            {
                hint_index
            } else {
                // Moving each entry to its members' mean rarely changes
                // the nearest entry, so the previous one is a tight seed.
                search.nearest_seeded(color.red, color.green, color.blue, hint_index, hint_color)
            };
        }
    }
    recycle_quality_colors(colors);

    let transparent_index = if has_transparent_pixels {
        let index = palette.len() as u8;
        palette.push(0);
        histogram_to_palette.push(index);
        Some(index)
    } else {
        None
    };
    QualityIndexPlan {
        palette,
        histogram_to_palette,
        transparent_index,
        mapping_bits: HISTOGRAM_BITS,
    }
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
#[inline(never)]
pub(crate) fn dense_coarse_nearest_table_simd<const PRUNE_FROM_HINTS: bool>(
    palette_rgb: &[u32],
    requested_cells: &[bool; 1 << 12],
    initial_hints: &[u8; 1 << 12],
) -> [u8; 1 << 12] {
    use core::arch::wasm32::{
        i32x4_add, i32x4_lt, i32x4_splat, v128_bitselect, v128_load, v128_store,
    };

    let mut best = [i32::MAX as u32; 1 << 12];
    let mut row_max_distance = [0u32; 1 << 8];
    let mut red_max_distance = [0u32; 1 << 4];
    let mut requested_rows = [false; 1 << 8];
    if PRUNE_FROM_HINTS {
        for (cell, requested) in requested_cells.iter().enumerate() {
            if !*requested {
                continue;
            }
            let red = ((cell >> 8) << 4) | 8;
            let green = (((cell >> 4) & 15) << 4) | 8;
            let blue = ((cell & 15) << 4) | 8;
            let hint_index = initial_hints[cell];
            let hint_color = palette_rgb[usize::from(hint_index)];
            let distance = palette_color_distance(hint_color, red as u8, green as u8, blue as u8);
            best[cell] = (distance << 8) | u32::from(hint_index);
            let row = cell >> 4;
            requested_rows[row] = true;
            row_max_distance[row] = row_max_distance[row].max(distance);
            red_max_distance[cell >> 8] = red_max_distance[cell >> 8].max(distance);
        }
    }
    for (palette_index, &color) in palette_rgb.iter().enumerate() {
        let palette_red = ((color >> 16) & 0xff) as i32;
        let palette_green = ((color >> 8) & 0xff) as i32;
        let palette_blue = (color & 0xff) as i32;
        let mut red_distances = [0u32; 16];
        let mut green_distances = [0u32; 16];
        let mut blue_distances_and_index = [0u32; 16];
        for (channel, distance) in red_distances.iter_mut().enumerate() {
            let delta = ((channel << 4) | 8) as i32 - palette_red;
            *distance = (delta * delta) as u32;
        }
        for (channel, distance) in green_distances.iter_mut().enumerate() {
            let delta = ((channel << 4) | 8) as i32 - palette_green;
            *distance = (delta * delta) as u32;
        }
        for (blue, distance) in blue_distances_and_index.iter_mut().enumerate() {
            let delta = ((blue << 4) | 8) as i32 - palette_blue;
            *distance = ((delta * delta) as u32) << 8 | palette_index as u32;
        }
        for red in 0..16usize {
            if PRUNE_FROM_HINTS && red_distances[red] > red_max_distance[red] {
                continue;
            }
            for green in 0..16usize {
                let row = (red << 8) | (green << 4);
                let base_distance_unshifted = red_distances[red] + green_distances[green];
                if PRUNE_FROM_HINTS
                    && (!requested_rows[row >> 4]
                        || base_distance_unshifted > row_max_distance[row >> 4])
                {
                    continue;
                }
                let base_distance = i32x4_splat((base_distance_unshifted << 8) as i32);
                for blue in (0..16usize).step_by(4) {
                    let blue_distance =
                        unsafe { v128_load(blue_distances_and_index.as_ptr().add(blue).cast()) };
                    let candidate = i32x4_add(base_distance, blue_distance);
                    let best_pointer = unsafe { best.as_mut_ptr().add(row | blue) };
                    let previous = unsafe { v128_load(best_pointer.cast()) };
                    let closer = i32x4_lt(candidate, previous);
                    unsafe {
                        v128_store(
                            best_pointer.cast(),
                            v128_bitselect(candidate, previous, closer),
                        )
                    };
                }
            }
        }
    }
    let mut table = [0u8; 1 << 12];
    for (cell, requested) in requested_cells.iter().enumerate() {
        if *requested {
            table[cell] = best[cell] as u8;
        }
    }
    table
}
