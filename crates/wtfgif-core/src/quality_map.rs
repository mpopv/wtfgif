//! Mapping RGBA pixels through a quality plan to palette indices.

use super::*;

#[inline(always)]
pub(crate) fn exact_palette_index_packed(
    mapper: &PaletteMapper<'_>,
    packed: u32,
) -> Result<u8, String> {
    mapper
        .exact_index(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8)
        .ok_or_else(|| {
            "Pixel-perfect GIF encoding found an RGBA color outside the supplied palette"
                .to_string()
        })
}

#[inline(always)]
pub(crate) fn exact_palette_index_packed_cached(
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
#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
pub(crate) unsafe fn mapped_quality_four_pixels_simd<
    const BITS: usize,
    const HAS_TRANSPARENT: bool,
>(
    rgba_pointer: *const u8,
    alpha_threshold: u8,
    transparent_index: u8,
    histogram_to_palette_pointer: *const u8,
) -> u32 {
    use core::arch::wasm32::{
        i32x4_bitmask, i32x4_extract_lane, u32x4_lt, u32x4_shl, u32x4_shr, u32x4_splat, v128_and,
        v128_bitselect, v128_load, v128_or,
    };

    let pixels = v128_load(rgba_pointer.cast());
    let transparent = if HAS_TRANSPARENT {
        u32x4_lt(
            u32x4_shr(pixels, 24),
            u32x4_splat(u32::from(alpha_threshold)),
        )
    } else {
        u32x4_splat(0)
    };
    if HAS_TRANSPARENT && i32x4_bitmask(transparent) == 0b1111 {
        return u32::from(transparent_index) * 0x0101_0101;
    }
    let indices = if BITS == 5 {
        v128_or(
            v128_or(
                v128_and(u32x4_shl(pixels, 7), u32x4_splat(0x7c00)),
                v128_and(u32x4_shr(pixels, 6), u32x4_splat(0x03e0)),
            ),
            v128_and(u32x4_shr(pixels, 19), u32x4_splat(0x001f)),
        )
    } else {
        v128_or(
            v128_or(
                v128_and(u32x4_shl(pixels, 4), u32x4_splat(0x0f00)),
                v128_and(u32x4_shr(pixels, 8), u32x4_splat(0x00f0)),
            ),
            v128_and(u32x4_shr(pixels, 20), u32x4_splat(0x000f)),
        )
    };
    let indices = if HAS_TRANSPARENT {
        v128_bitselect(u32x4_splat(1 << (BITS * 3)), indices, transparent)
    } else {
        indices
    };
    let code0 = *histogram_to_palette_pointer.add(i32x4_extract_lane::<0>(indices) as usize);
    let code1 = *histogram_to_palette_pointer.add(i32x4_extract_lane::<1>(indices) as usize);
    let code2 = *histogram_to_palette_pointer.add(i32x4_extract_lane::<2>(indices) as usize);
    let code3 = *histogram_to_palette_pointer.add(i32x4_extract_lane::<3>(indices) as usize);
    u32::from_le_bytes([code0, code1, code2, code3])
}

#[inline(always)]
pub(crate) unsafe fn mapped_quality_eight_pixels<const BITS: usize, const HAS_TRANSPARENT: bool>(
    rgba_pointer: *const u8,
    alpha_threshold: u8,
    _transparent_index: u8,
    histogram_to_palette_pointer: *const u8,
) -> u64 {
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    {
        let low = mapped_quality_four_pixels_simd::<BITS, HAS_TRANSPARENT>(
            rgba_pointer,
            alpha_threshold,
            _transparent_index,
            histogram_to_palette_pointer,
        );
        let high = mapped_quality_four_pixels_simd::<BITS, HAS_TRANSPARENT>(
            rgba_pointer.add(16),
            alpha_threshold,
            _transparent_index,
            histogram_to_palette_pointer,
        );
        u64::from(low) | (u64::from(high) << 32)
    }
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    {
        let mut codes = 0u64;
        for lane in 0..8 {
            let packed = u32::from_le(std::ptr::read_unaligned(rgba_pointer.add(lane * 4).cast()));
            let code = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
                packed,
                alpha_threshold,
                _transparent_index,
                histogram_to_palette_pointer,
            );
            codes |= u64::from(code) << (lane * 8);
        }
        codes
    }
}

#[inline(always)]
pub(crate) fn expand_four_literal_codes_to_nine_bits(packed_codes: u32) -> u64 {
    let paired =
        u64::from(packed_codes & 0x0000_ffff) | (u64::from(packed_codes & 0xffff_0000) << 2);
    (paired & 0x0000_0000_03fc_00ff) | ((paired & 0x0000_0003_fc00_ff00) << 1)
}

/// Map one frame through a quality plan. When a sample of the frame shows
/// that most groups of eight pixels with opaque content match the previous
/// frame, those groups copy the previous frame's indices instead. Equal RGBA
/// always maps to the same index, so the result matches a full mapping.
#[inline(never)]
pub(crate) fn map_quality_frame<const BITS: usize, const HAS_TRANSPARENT: bool>(
    rgba: &[u8],
    previous: Option<(&[u8], &[u8])>,
    alpha_threshold: u8,
    transparent_index: u8,
    histogram_to_palette: &[u8],
    out: &mut [u8],
) {
    const GROUP_BYTES: usize = 32;
    const SAMPLES: usize = 32;
    debug_assert_eq!(rgba.len(), out.len() * 4);
    let groups = out.len() / 8;
    // Fully transparent groups are already cheap to map, so only groups with
    // opaque pixels count toward reuse.
    let reuse = previous.filter(|(previous_rgba, _)| {
        let step = (groups / SAMPLES).max(1);
        let samples = groups.min(SAMPLES);
        let unchanged = (0..samples)
            .map(|sample| sample * step * GROUP_BYTES)
            .filter(|&offset| {
                (!HAS_TRANSPARENT
                    || (0..8).any(|pixel| rgba[offset + pixel * 4 + 3] >= alpha_threshold))
                    && rgba[offset..offset + GROUP_BYTES]
                        == previous_rgba[offset..offset + GROUP_BYTES]
            })
            .count();
        unchanged * 2 > samples
    });
    let Some((previous_rgba, previous_indices)) = reuse else {
        map_quality_pixels_grouped::<BITS, HAS_TRANSPARENT>(
            rgba,
            alpha_threshold,
            transparent_index,
            histogram_to_palette,
            out,
        );
        return;
    };
    let rgba_pointer = rgba.as_ptr();
    let previous_pointer = previous_rgba.as_ptr();
    let table = histogram_to_palette.as_ptr();
    let out_pointer = out.as_mut_ptr();
    for group in 0..groups {
        let (offset, pixel) = (group * GROUP_BYTES, group * 8);
        let unchanged = unsafe {
            rgba_blocks_equal_16(rgba_pointer.add(offset), previous_pointer.add(offset))
                && rgba_blocks_equal_16(
                    rgba_pointer.add(offset + 16),
                    previous_pointer.add(offset + 16),
                )
        };
        if unchanged {
            out[pixel..pixel + 8].copy_from_slice(&previous_indices[pixel..pixel + 8]);
            continue;
        }
        let codes = unsafe {
            mapped_quality_eight_pixels::<BITS, HAS_TRANSPARENT>(
                rgba_pointer.add(offset),
                alpha_threshold,
                transparent_index,
                table,
            )
        };
        unsafe { std::ptr::write_unaligned(out_pointer.add(pixel).cast::<u64>(), codes.to_le()) };
    }
    for (pixel, slot) in out.iter_mut().enumerate().skip(groups * 8) {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(pixel * 4).cast()) });
        *slot = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
            packed,
            alpha_threshold,
            transparent_index,
            table,
        );
    }
}

/// Map RGBA pixels to palette indices eight at a time through a quality
/// plan's coarse-cell table. When the plan has a transparent index, the
/// table's final entry holds it; otherwise the table has one entry per cell.
pub(crate) fn map_quality_pixels_grouped<const BITS: usize, const HAS_TRANSPARENT: bool>(
    rgba_stream: &[u8],
    alpha_threshold: u8,
    transparent_index: u8,
    histogram_to_palette: &[u8],
    indexed: &mut [u8],
) {
    debug_assert_eq!(rgba_stream.len(), indexed.len() * 4);
    debug_assert!(
        indexed.is_empty()
            || histogram_to_palette.len() >= (1 << (BITS * 3)) + usize::from(HAS_TRANSPARENT)
    );
    let rgba_pointer = rgba_stream.as_ptr();
    let table = histogram_to_palette.as_ptr();
    let indexed_pointer = indexed.as_mut_ptr();
    let pixel_count = indexed.len();
    let mut pixel = 0usize;
    while pixel + 8 <= pixel_count {
        unsafe {
            let codes = mapped_quality_eight_pixels::<BITS, HAS_TRANSPARENT>(
                rgba_pointer.add(pixel * 4),
                alpha_threshold,
                transparent_index,
                table,
            );
            std::ptr::write_unaligned(indexed_pointer.add(pixel).cast::<u64>(), codes.to_le());
        }
        pixel += 8;
    }
    while pixel < pixel_count {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(pixel * 4).cast()) });
        indexed[pixel] = mapped_quality_pixel::<BITS, HAS_TRANSPARENT>(
            packed,
            alpha_threshold,
            transparent_index,
            table,
        );
        pixel += 1;
    }
}

#[inline(always)]
pub(crate) fn mapped_quality_pixel<const BITS: usize, const HAS_TRANSPARENT: bool>(
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
