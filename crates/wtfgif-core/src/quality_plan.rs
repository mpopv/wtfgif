//! Quality palette plans: exact, grid, single-merge, and dominant-color tables.

use super::*;

#[inline(always)]
pub(crate) fn quality_colors_form_equal_count_cartesian_grid(colors: &[QuantizedColor]) -> bool {
    let Some(first) = colors.first() else {
        return false;
    };
    let mut red_cells = 0u16;
    let mut green_cells = 0u16;
    let mut blue_cells = 0u16;
    for color in colors {
        if color.count != first.count {
            return false;
        }
        red_cells |= 1 << (color.histogram_index >> 8);
        green_cells |= 1 << ((color.histogram_index >> 4) & 15);
        blue_cells |= 1 << (color.histogram_index & 15);
    }
    red_cells.count_ones() as usize
        * green_cells.count_ones() as usize
        * blue_cells.count_ones() as usize
        == colors.len()
}

#[inline(never)]
pub(crate) fn build_quality_single_merge_plan(
    has_transparent_pixels: bool,
    colors: Vec<QuantizedColor>,
    mut palette: Vec<u32>,
) -> QualityIndexPlan {
    if colors.is_empty() {
        return QualityIndexPlan {
            palette,
            histogram_to_palette: Vec::new(),
            transparent_index: None,
            mapping_bits: 4,
        };
    }
    let opaque_color_limit = if has_transparent_pixels { 255 } else { 256 };
    debug_assert_eq!(colors.len(), opaque_color_limit + 1);
    let excluded_position = colors
        .iter()
        .enumerate()
        .min_by_key(|(_, color)| {
            (
                quantized_color_count_u64(color.count),
                std::cmp::Reverse(color.histogram_index),
            )
        })
        .map(|(position, _)| position)
        .unwrap_or(0);

    palette.clear();
    if palette.capacity() < opaque_color_limit + 1 {
        palette.reserve(opaque_color_limit + 1 - palette.capacity());
    }
    let mut histogram_to_palette = take_quality_histogram_to_palette(1 << 12);
    for (position, color) in colors.iter().enumerate() {
        if position == excluded_position {
            continue;
        }
        let index = palette.len() as u8;
        palette.push(rgb_key(color.red, color.green, color.blue));
        histogram_to_palette[usize::from(color.histogram_index)] = index;
    }

    let excluded = colors[excluded_position];
    let merged_index = nearest_single_merge_palette_index(
        &colors,
        excluded_position,
        &palette,
        excluded.red,
        excluded.green,
        excluded.blue,
    );
    histogram_to_palette[usize::from(excluded.histogram_index)] = merged_index;
    let retained_position = if usize::from(merged_index) < excluded_position {
        usize::from(merged_index)
    } else {
        usize::from(merged_index) + 1
    };
    let retained = colors[retained_position];
    let retained_count = retained.count as u32;
    let excluded_count = excluded.count as u32;
    let count = retained_count + excluded_count;
    palette[usize::from(merged_index)] = rgb_key(
        ((u32::from(retained.red) * retained_count
            + u32::from(excluded.red) * excluded_count
            + count / 2)
            / count) as u8,
        ((u32::from(retained.green) * retained_count
            + u32::from(excluded.green) * excluded_count
            + count / 2)
            / count) as u8,
        ((u32::from(retained.blue) * retained_count
            + u32::from(excluded.blue) * excluded_count
            + count / 2)
            / count) as u8,
    );
    // Every other retained representative still has an identical palette
    // entry, so its nearest-color assignment cannot change. Only the two
    // colors whose shared representative moved need the general path's final
    // nearest-color refinement.
    histogram_to_palette[usize::from(retained.histogram_index)] =
        nearest_palette_index(retained.red, retained.green, retained.blue, &palette);
    histogram_to_palette[usize::from(excluded.histogram_index)] =
        nearest_palette_index(excluded.red, excluded.green, excluded.blue, &palette);
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
        mapping_bits: 4,
    }
}

#[inline(never)]
pub(crate) fn nearest_single_merge_palette_index(
    colors: &[QuantizedColor],
    excluded_position: usize,
    palette: &[u32],
    red: u8,
    green: u8,
    blue: u8,
) -> u8 {
    let mut best_index = 0u8;
    let mut best_position = usize::from(excluded_position == 0);
    let mut best_distance = u32::MAX;
    let mut palette_index = 0usize;
    for (position, color) in colors.iter().enumerate() {
        if position == excluded_position {
            continue;
        }
        let distance = palette_color_distance(palette[palette_index], red, green, blue);
        let best_color = colors[best_position];
        if distance < best_distance
            || (distance == best_distance
                && (color.count > best_color.count
                    || (color.count == best_color.count
                        && color.histogram_index < best_color.histogram_index)))
        {
            best_index = palette_index as u8;
            best_position = position;
            best_distance = distance;
        }
        palette_index += 1;
    }
    best_index
}

#[inline(never)]
pub(crate) fn build_quality_direct_cell_plan(
    has_transparent_pixels: bool,
    colors: Vec<QuantizedColor>,
    mut palette: Vec<u32>,
) -> QualityIndexPlan {
    if colors.is_empty() {
        return QualityIndexPlan {
            palette,
            histogram_to_palette: Vec::new(),
            transparent_index: None,
            mapping_bits: 4,
        };
    }
    debug_assert!(!colors.is_empty());
    let opaque_color_limit = if has_transparent_pixels { 255 } else { 256 };
    debug_assert!(colors.len() <= opaque_color_limit);
    palette.clear();
    if palette.capacity() < opaque_color_limit {
        palette.reserve(opaque_color_limit - palette.capacity());
    }
    let mut histogram_to_palette = take_quality_histogram_to_palette(1 << 12);
    let color_count = colors.len();
    unsafe { palette.set_len(color_count) };
    let colors_pointer = colors.as_ptr();
    let palette_pointer = palette.as_mut_ptr();
    let mapping_pointer = histogram_to_palette.as_mut_ptr();
    let mut position = 0usize;
    while position + 4 <= color_count {
        let color0 = unsafe { *colors_pointer.add(position) };
        let color1 = unsafe { *colors_pointer.add(position + 1) };
        let color2 = unsafe { *colors_pointer.add(position + 2) };
        let color3 = unsafe { *colors_pointer.add(position + 3) };
        unsafe {
            palette_pointer
                .add(position)
                .write(rgb_key(color0.red, color0.green, color0.blue));
            palette_pointer
                .add(position + 1)
                .write(rgb_key(color1.red, color1.green, color1.blue));
            palette_pointer
                .add(position + 2)
                .write(rgb_key(color2.red, color2.green, color2.blue));
            palette_pointer
                .add(position + 3)
                .write(rgb_key(color3.red, color3.green, color3.blue));
            mapping_pointer
                .add(usize::from(color0.histogram_index))
                .write(position as u8);
            mapping_pointer
                .add(usize::from(color1.histogram_index))
                .write((position + 1) as u8);
            mapping_pointer
                .add(usize::from(color2.histogram_index))
                .write((position + 2) as u8);
            mapping_pointer
                .add(usize::from(color3.histogram_index))
                .write((position + 3) as u8);
        }
        position += 4;
    }
    while position < color_count {
        let color = unsafe { *colors_pointer.add(position) };
        unsafe {
            palette_pointer
                .add(position)
                .write(rgb_key(color.red, color.green, color.blue));
            mapping_pointer
                .add(usize::from(color.histogram_index))
                .write(position as u8);
        }
        position += 1;
    }
    recycle_quality_colors(colors);

    // Above 128 entries the GIF table and literal code width are already 256
    // colors wide. Pad the logical palette too so the encoder can use its
    // direct 4-bit-cell mapper without changing output size or pixel quality.
    if palette.len() + usize::from(has_transparent_pixels) > 128 {
        let padding = *palette.last().unwrap_or(&0);
        palette.resize(opaque_color_limit, padding);
    }
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
        mapping_bits: 4,
    }
}

#[inline(never)]
pub(crate) fn build_quality_flat_grid_plan(
    has_transparent_pixels: bool,
    colors: Vec<QuantizedColor>,
    palette: Vec<u32>,
) -> QualityIndexPlan {
    let opaque_color_limit = if has_transparent_pixels { 255 } else { 256 };
    let (mut palette, mut histogram_to_palette) =
        build_quality_flat_grid_palette(colors, 1 << 12, opaque_color_limit, palette);
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
        mapping_bits: 4,
    }
}

pub(crate) fn finish_quality_exact_indexed(
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

pub(crate) fn prepare_quality_dominant_palette_code() -> usize {
    let mut checksum = 0usize;
    // The SIMD dominant-palette route now handles the complete synthetic encode
    // that follows this preparation. Keep four planner-only probes so the
    // scalar median-cut/KD route independently reaches V8's measured optimizing
    // tier without warming histogram, LZW, or small exact-image call graphs.
    for _ in 0..4 {
        let mut colors = take_quality_colors();
        colors.clear();
        if colors.capacity() < QUALITY_DOMINANT_COLOR_LIMIT {
            colors.reserve(QUALITY_DOMINANT_COLOR_LIMIT - colors.capacity());
        }
        for cell in 0..QUALITY_DOMINANT_COLOR_LIMIT {
            colors.push(QuantizedColor {
                count: ((cell * 73) % 251 + 1) as QuantizedColorCount,
                histogram_index: cell as u16,
                red: ((cell >> 8) << 4 | 8) as u8,
                green: (((cell >> 4) & 15) << 4 | 8) as u8,
                blue: ((cell & 15) << 4 | 8) as u8,
            });
        }
        let plan = build_quality_index_plan_from_colors::<true, 4>(
            false,
            colors,
            take_quality_palette(256),
        );
        checksum ^= plan.palette.capacity() ^ plan.histogram_to_palette.capacity();
        recycle_quality_palette(plan.palette);
        recycle_quality_histogram_to_palette(plan.histogram_to_palette);
    }
    checksum
}

#[inline(never)]
pub(crate) fn build_quality_index_plan_from_colors<
    const SAFE_U32_COUNTS: bool,
    const HISTOGRAM_BITS: usize,
>(
    has_transparent_pixels: bool,
    mut colors: Vec<QuantizedColor>,
    palette_scratch: Vec<u32>,
) -> QualityIndexPlan {
    let mapping_bits = if HISTOGRAM_BITS == 5 && colors.len() > QUALITY_DOMINANT_COLOR_LIMIT {
        4
    } else {
        HISTOGRAM_BITS
    };
    let mapping_len = 1usize << (mapping_bits * 3);
    let opaque_color_limit = if has_transparent_pixels { 255 } else { 256 };

    if colors.len() > QUALITY_DOMINANT_COLOR_LIMIT {
        let (mut palette, mut histogram_to_palette) = if HISTOGRAM_BITS == 4 {
            build_quality_wu_palette(
                has_transparent_pixels,
                colors,
                mapping_len,
                opaque_color_limit,
                palette_scratch,
            )
        } else {
            build_quality_median_cut_palette(
                has_transparent_pixels,
                colors,
                HISTOGRAM_BITS,
                mapping_len,
                opaque_color_limit,
                palette_scratch,
            )
        };
        let transparent_index = if has_transparent_pixels {
            let index = palette.len() as u8;
            palette.push(0);
            histogram_to_palette.push(index);
            Some(index)
        } else {
            None
        };
        if palette.is_empty() {
            palette.push(0);
        }
        return QualityIndexPlan {
            palette,
            histogram_to_palette,
            transparent_index,
            mapping_bits,
        };
    }

    let (mut palette, mut histogram_to_palette) = {
        let palette_len = colors.len().min(opaque_color_limit);
        // Histogram extraction already leaves colors in cell order. Select
        // the palette through compact positions so quickselect does not
        // scramble the color arena and force a second spatial reorder before
        // nearest-color mapping.
        let mut palette_positions = [0u16; QUALITY_DOMINANT_COLOR_LIMIT];
        for (position, slot) in palette_positions[..colors.len()].iter_mut().enumerate() {
            *slot = position as u16;
        }
        let palette_positions = &mut palette_positions[..colors.len()];
        #[cfg(target_arch = "wasm32")]
        {
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
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if colors
                .iter()
                .all(|color| quantized_color_count_u64(color.count) < QUANTIZED_SORT_COUNT_LIMIT)
            {
                let order = |position: &u16| {
                    let color = &colors[usize::from(*position)];
                    ((QUANTIZED_SORT_COUNT_LIMIT - 1 - quantized_color_count_u64(color.count))
                        << 16)
                        | u64::from(color.histogram_index)
                };
                if colors.len() > palette_len {
                    palette_positions.select_nth_unstable_by_key(palette_len - 1, order);
                    palette_positions[..palette_len].sort_unstable_by_key(order);
                } else {
                    palette_positions.sort_unstable_by_key(order);
                }
            } else {
                palette_positions.sort_unstable_by_key(|position| {
                    let color = &colors[usize::from(*position)];
                    (
                        std::cmp::Reverse(quantized_color_count_u64(color.count)),
                        color.histogram_index,
                    )
                });
            }
        }
        let mut palette = palette_scratch;
        palette.clear();
        let palette_capacity = palette_len + usize::from(has_transparent_pixels);
        if palette.capacity() < palette_capacity {
            palette.reserve(palette_capacity - palette.capacity());
        }
        for &position in &palette_positions[..palette_len] {
            let color = &colors[usize::from(position)];
            palette.push(rgb_key(color.red, color.green, color.blue));
        }
        let mut initial_tree = PaletteKdTree::new(&palette);
        let mut histogram_to_palette = take_quality_histogram_to_palette(mapping_len);
        // A histogram cell has exactly one representative color, so a cell
        // occupied by a selected palette entry is an exact lookup. Distinct
        // 4-bit cells have disjoint channel ranges, so their representatives
        // cannot be duplicate RGB values. Keep a compact direct table for
        // every 4-bit plan. Tag selected entries temporarily in the unused
        // high bit of their 12-bit cell index, avoiding both a second lookup
        // structure and the palette-index-255 sentinel collision.
        let use_direct_palette_cells = mapping_bits == 4;
        let mut palette_lookup = if use_direct_palette_cells {
            ColorIndexTable::empty()
        } else {
            take_quality_color_index_table(COLOR_INDEX_CAP)
        };
        if !use_direct_palette_cells {
            for (index, &color) in palette.iter().enumerate() {
                palette_lookup.insert_if_absent(color, index as u8);
            }
        }
        debug_assert!(colors
            .windows(2)
            .all(|pair| pair[0].histogram_index < pair[1].histogram_index));
        const DIRECT_PALETTE_CELL: u16 = 1 << 15;
        if use_direct_palette_cells {
            for (index, &position) in palette_positions[..palette_len].iter().enumerate() {
                let color = &mut colors[usize::from(position)];
                histogram_to_palette[usize::from(color.histogram_index)] = index as u8;
                color.histogram_index |= DIRECT_PALETTE_CELL;
            }
        }
        let mut palette_changed = false;
        let mut previous_hint = None;
        // The low-resolution and u32 histogram callers prove every count and
        // weighted channel sum fits in u32. Specialize that fact at compile
        // time so their cold call graph does not include the wide fallback.
        if SAFE_U32_COUNTS {
            let mut counts = [0u32; 256];
            let mut red_sums = [0u32; 256];
            let mut green_sums = [0u32; 256];
            let mut blue_sums = [0u32; 256];
            let mut previous_green_hints = [u16::MAX; 16];
            let mut previous_red_hints = [u16::MAX; 256];
            let mut current_red_cell = usize::MAX;
            for color in &mut colors {
                let cell = usize::from(color.histogram_index & !DIRECT_PALETTE_CELL);
                if use_direct_palette_cells {
                    let red_cell = cell >> 8;
                    if red_cell != current_red_cell {
                        previous_green_hints.fill(u16::MAX);
                        current_red_cell = red_cell;
                    }
                }
                let index = if use_direct_palette_cells {
                    let candidate = histogram_to_palette[cell];
                    if color.histogram_index & DIRECT_PALETTE_CELL != 0 {
                        color.histogram_index &= !DIRECT_PALETTE_CELL;
                        usize::from(candidate)
                    } else {
                        let (mut hint_index, hint_color) =
                            previous_hint.unwrap_or_else(|| (0, palette[0]));
                        let mut hint_distance =
                            palette_color_distance(hint_color, color.red, color.green, color.blue);
                        for candidate in [
                            previous_green_hints[cell & 15],
                            previous_red_hints[cell & 255],
                        ] {
                            if candidate == u16::MAX {
                                continue;
                            }
                            let candidate = candidate as u8;
                            let distance = palette_color_distance(
                                palette[usize::from(candidate)],
                                color.red,
                                color.green,
                                color.blue,
                            );
                            if distance < hint_distance
                                || (distance == hint_distance && candidate < hint_index)
                            {
                                hint_index = candidate;
                                hint_distance = distance;
                            }
                        }
                        usize::from(initial_tree.nearest_with_seed_bounds(
                            color.red,
                            color.green,
                            color.blue,
                            hint_index,
                            hint_distance,
                        ))
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
                if use_direct_palette_cells {
                    previous_green_hints[cell & 15] = index as u16;
                    previous_red_hints[cell & 255] = index as u16;
                }
                // This branch is fed only by the u32 histogram path; the
                // source pixel limit proves every bin count fits in u32.
                let count = color.count as u32;
                histogram_to_palette[cell] = index as u8;
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
                let reciprocal = (1u64 << 31) / u64::from(count);
                let representative = rgb_key(
                    rounded_weighted_average_u32_reciprocal(red_sums[index], count, reciprocal),
                    rounded_weighted_average_u32_reciprocal(green_sums[index], count, reciprocal),
                    rounded_weighted_average_u32_reciprocal(blue_sums[index], count, reciprocal),
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
            for color in &mut colors {
                let cell = usize::from(color.histogram_index & !DIRECT_PALETTE_CELL);
                let index = if use_direct_palette_cells {
                    let candidate = histogram_to_palette[cell];
                    if color.histogram_index & DIRECT_PALETTE_CELL != 0 {
                        color.histogram_index &= !DIRECT_PALETTE_CELL;
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
                histogram_to_palette[cell] = index as u8;
                counts[index] += quantized_color_count_u64(color.count);
                red_sums[index] += u64::from(color.red) * quantized_color_count_u64(color.count);
                green_sums[index] +=
                    u64::from(color.green) * quantized_color_count_u64(color.count);
                blue_sums[index] += u64::from(color.blue) * quantized_color_count_u64(color.count);
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
    };
    let transparent_index = if has_transparent_pixels {
        let index = palette.len() as u8;
        palette.push(0);
        histogram_to_palette.push(index);
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

#[inline(never)]
pub(crate) fn build_quality_flat_grid_palette(
    colors: Vec<QuantizedColor>,
    mapping_len: usize,
    opaque_color_limit: usize,
    mut palette: Vec<u32>,
) -> (Vec<u32>, Vec<u8>) {
    debug_assert!(!colors.is_empty());
    debug_assert_eq!(mapping_len, 1 << 12);
    let mut minimum = [u8::MAX; 3];
    let mut maximum = [0u8; 3];
    let mut sum = [0u64; 3];
    let mut squared_sum = [0u64; 3];
    debug_assert!(colors
        .first()
        .is_some_and(|first| colors.iter().all(|color| color.count == first.count)));
    // The caller admits only equal-count Cartesian grids. The common weight
    // cancels from both variance comparisons and rounded cell averages, so
    // unit weights produce the exact same levels and palette representatives.
    for color in &colors {
        let channels = [color.red, color.green, color.blue];
        for channel in 0..3 {
            let value = channels[channel];
            minimum[channel] = minimum[channel].min(value);
            maximum[channel] = maximum[channel].max(value);
            sum[channel] += u64::from(value);
            squared_sum[channel] += u64::from(value) * u64::from(value);
        }
    }

    let total = colors.len() as u64;
    let spread = [
        total * squared_sum[0] - sum[0] * sum[0],
        total * squared_sum[1] - sum[1] * sum[1],
        total * squared_sum[2] - sum[2] * sum[2],
    ];
    let mut levels = [1usize; 3];
    loop {
        let mut best_channel = None;
        let mut best_gain = f64::NEG_INFINITY;
        for channel in 0..3 {
            let next = levels[channel] + 1;
            if next > 16 || next > usize::from(maximum[channel] - minimum[channel]) + 1 {
                continue;
            }
            let product = levels
                .iter()
                .enumerate()
                .map(|(index, &level)| if index == channel { next } else { level })
                .product::<usize>();
            if product > opaque_color_limit {
                continue;
            }
            let current = levels[channel] as f64;
            let next = next as f64;
            let gain = spread[channel] as f64 * (1.0 / (current * current) - 1.0 / (next * next));
            if gain > best_gain {
                best_gain = gain;
                best_channel = Some(channel);
            }
        }
        let Some(channel) = best_channel else {
            break;
        };
        levels[channel] += 1;
    }

    let grid_len = levels.iter().product::<usize>();
    let mut counts = [0u32; 256];
    let mut red_sums = [0u32; 256];
    let mut green_sums = [0u32; 256];
    let mut blue_sums = [0u32; 256];
    let mut color_grid_cells = [0u8; WU_HISTOGRAM_LEN];
    for (position, color) in colors.iter().enumerate() {
        let channels = [color.red, color.green, color.blue];
        let mut coordinates = [0usize; 3];
        for channel in 0..3 {
            let range = usize::from(maximum[channel] - minimum[channel]) + 1;
            coordinates[channel] =
                usize::from(channels[channel] - minimum[channel]) * levels[channel] / range;
        }
        let cell = (coordinates[0] * levels[1] + coordinates[1]) * levels[2] + coordinates[2];
        color_grid_cells[position] = cell as u8;
        counts[cell] += 1;
        red_sums[cell] += u32::from(color.red);
        green_sums[cell] += u32::from(color.green);
        blue_sums[cell] += u32::from(color.blue);
    }

    palette.clear();
    if palette.capacity() < opaque_color_limit {
        palette.reserve(opaque_color_limit - palette.capacity());
    }
    let mut grid_to_palette = [u8::MAX; 256];
    for cell in 0..grid_len {
        let count = counts[cell];
        if count == 0 {
            continue;
        }
        let index = palette.len() as u8;
        grid_to_palette[cell] = index;
        palette.push(rgb_key(
            ((red_sums[cell] + count / 2) / count) as u8,
            ((green_sums[cell] + count / 2) / count) as u8,
            ((blue_sums[cell] + count / 2) / count) as u8,
        ));
    }
    let padding = *palette.last().unwrap_or(&0);
    palette.resize(opaque_color_limit, padding);

    let mut histogram_to_palette = take_quality_histogram_to_palette(mapping_len);
    for (position, color) in colors.iter().enumerate() {
        histogram_to_palette[usize::from(color.histogram_index)] =
            grid_to_palette[usize::from(color_grid_cells[position])];
    }
    recycle_quality_colors(colors);
    (palette, histogram_to_palette)
}
