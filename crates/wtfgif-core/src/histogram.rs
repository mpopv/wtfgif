//! RGB histograms that feed the quality palette planners.

use super::*;

#[derive(Clone, Copy, Default)]
pub(crate) struct RgbHistogramBin {
    pub(crate) count: u64,
    pub(crate) red: u64,
    pub(crate) green: u64,
    pub(crate) blue: u64,
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
pub(crate) struct RgbHistogramBin32 {
    pub(crate) count: u32,
    pub(crate) red: u32,
    pub(crate) green: u32,
    pub(crate) blue: u32,
}

#[cfg(target_arch = "wasm32")]
pub(crate) type QuantizedColorCount = u32;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type QuantizedColorCount = u64;

#[inline(always)]
pub(crate) fn quantized_color_count_u64(count: QuantizedColorCount) -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        u64::from(count)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        count
    }
}

#[inline(always)]
pub(crate) fn quality_histogram_index_packed<const BITS: usize>(packed: u32) -> usize {
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
pub(crate) fn quality_histogram_index_pair_packed<const BITS: usize>(
    packed: u64,
) -> (usize, usize) {
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
pub(crate) unsafe fn read_rgba_pair(pointer: *const u8, offset: usize) -> (u32, u32) {
    let packed = u64::from_le(std::ptr::read_unaligned(pointer.add(offset).cast::<u64>()));
    (packed as u32, (packed >> 32) as u32)
}

// Histogram updates hit a pseudo-random bin for each pixel. Keep each update
// as two adjacent packed-field operations; a SIMD load/modify/store for a
// scattered bin is still slower on the ARM64 Wasm runtime for the common
// split-bin case.
#[inline(always)]
pub(crate) unsafe fn add_quality_histogram_bin32(
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
pub(crate) unsafe fn add_quality_histogram_bin32_packed(
    bin: *mut RgbHistogramBin32,
    count: u32,
    red: u32,
    green: u32,
    blue: u32,
) {
    #[cfg(target_endian = "little")]
    {
        // The split-pair scanner has already proved that adjacent pixels use
        // different bins. Two scalar read-modify-writes beat a SIMD update for
        // this scattered access pattern, while preserving the same four sums.
        let packed = bin.cast::<u64>();
        let count_red =
            std::ptr::read(packed).wrapping_add(u64::from(count) | (u64::from(red) << 32));
        let green_blue =
            std::ptr::read(packed.add(1)).wrapping_add(u64::from(green) | (u64::from(blue) << 32));
        std::ptr::write(packed, count_red);
        std::ptr::write(packed.add(1), green_blue);
    }
    #[cfg(not(target_endian = "little"))]
    {
        let bin = &mut *bin;
        bin.count = bin.count.wrapping_add(count);
        bin.red = bin.red.wrapping_add(red);
        bin.green = bin.green.wrapping_add(green);
        bin.blue = bin.blue.wrapping_add(blue);
    }
}

#[inline(always)]
pub(crate) fn add_quality_histogram_u32_bits_const<const BITS: usize>(
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
pub(crate) fn add_quality_histogram_u32_bits_indexed(
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
pub(crate) fn add_quality_histogram_u32_pair<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    packed0: u32,
    packed1: u32,
) {
    let index0 = quality_histogram_index_packed::<BITS>(packed0);
    let index1 = quality_histogram_index_packed::<BITS>(packed1);
    add_quality_histogram_u32_pair_indexed::<BITS>(histogram, packed0, packed1, index0, index1);
}

#[inline(always)]
pub(crate) fn add_quality_histogram_u32_pair_packed<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    packed: u64,
) {
    let indices = quality_histogram_index_pair_packed::<BITS>(packed);
    add_quality_histogram_u32_pair_packed_indexed::<BITS>(histogram, packed, indices);
}

#[inline(always)]
pub(crate) fn add_quality_histogram_u32_pair_packed_indexed<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    packed: u64,
    indices: (usize, usize),
) {
    add_quality_histogram_u32_pair_indexed::<BITS>(
        histogram,
        packed as u32,
        (packed >> 32) as u32,
        indices.0,
        indices.1,
    );
}

#[inline(always)]
pub(crate) fn add_quality_histogram_u32_pair_indexed<const BITS: usize>(
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
    unsafe {
        add_quality_histogram_bin32(histogram.as_mut_ptr().add(index0), 2, red, green, blue);
    }
}

#[inline(always)]
pub(crate) fn add_quality_histogram_u32_pair_with_alpha<const BITS: usize>(
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
#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
pub(crate) fn add_quality_histogram_u32_pair_packed_with_alpha<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    packed: u64,
    alpha_threshold: u8,
) -> bool {
    add_quality_histogram_u32_pair_with_alpha::<BITS>(
        histogram,
        packed as u32,
        (packed >> 32) as u32,
        alpha_threshold,
    )
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
#[inline(always)]
pub(crate) fn accumulate_quality_histogram_u32_bits_remaining_mixed_transparent_dense<
    const BITS: usize,
>(
    histogram: &mut [RgbHistogramBin32],
    rgba_stream: &[u8],
    start_offset: usize,
    alpha_threshold: u8,
) -> bool {
    use core::arch::wasm32::{i32x4_bitmask, u32x4_lt, u32x4_shr, u32x4_splat, v128_load};

    let rgba_pointer = rgba_stream.as_ptr();
    let threshold_vector = u32x4_splat(u32::from(alpha_threshold));
    let mut has_transparent_pixels = false;
    let mut offset = start_offset;
    while offset + 32 <= rgba_stream.len() {
        let pixels03 = unsafe { v128_load(rgba_pointer.add(offset).cast()) };
        let pixels47 = unsafe { v128_load(rgba_pointer.add(offset + 16).cast()) };
        let transparent03 = i32x4_bitmask(u32x4_lt(u32x4_shr(pixels03, 24), threshold_vector));
        let transparent47 = i32x4_bitmask(u32x4_lt(u32x4_shr(pixels47, 24), threshold_vector));
        if transparent03 == 0b1111 && transparent47 == 0b1111 {
            has_transparent_pixels = true;
            offset += 32;
            continue;
        }
        let (packed0, packed1) = unsafe { read_rgba_pair(rgba_pointer, offset) };
        let (packed2, packed3) = unsafe { read_rgba_pair(rgba_pointer, offset + 8) };
        let (packed4, packed5) = unsafe { read_rgba_pair(rgba_pointer, offset + 16) };
        let (packed6, packed7) = unsafe { read_rgba_pair(rgba_pointer, offset + 24) };
        if transparent03 == 0 && transparent47 == 0 {
            add_quality_histogram_u32_pair::<BITS>(histogram, packed0, packed1);
            add_quality_histogram_u32_pair::<BITS>(histogram, packed2, packed3);
            add_quality_histogram_u32_pair::<BITS>(histogram, packed4, packed5);
            add_quality_histogram_u32_pair::<BITS>(histogram, packed6, packed7);
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

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
#[inline(never)]
/// Skip 32-pixel canvas blocks whose alpha bytes are all exactly zero. The
/// ordinary mixed-alpha scanner handles every nonzero block and the tail, so
/// threshold semantics and histogram sums remain identical.
pub(crate) fn accumulate_quality_histogram_u32_bits_remaining_mixed_zero_dense<
    const BITS: usize,
>(
    histogram: &mut [RgbHistogramBin32],
    rgba_stream: &[u8],
    start_offset: usize,
    alpha_threshold: u8,
) -> bool {
    use core::arch::wasm32::{u32x4_shr, v128_any_true, v128_load, v128_or};

    let rgba_pointer = rgba_stream.as_ptr();
    let mut has_transparent_pixels = false;
    let mut offset = start_offset;
    while offset + 128 <= rgba_stream.len() {
        let pixels03 = unsafe { v128_load(rgba_pointer.add(offset).cast()) };
        let pixels47 = unsafe { v128_load(rgba_pointer.add(offset + 16).cast()) };
        let pixels811 = unsafe { v128_load(rgba_pointer.add(offset + 32).cast()) };
        let pixels1215 = unsafe { v128_load(rgba_pointer.add(offset + 48).cast()) };
        let pixels1619 = unsafe { v128_load(rgba_pointer.add(offset + 64).cast()) };
        let pixels2023 = unsafe { v128_load(rgba_pointer.add(offset + 80).cast()) };
        let pixels2427 = unsafe { v128_load(rgba_pointer.add(offset + 96).cast()) };
        let pixels2831 = unsafe { v128_load(rgba_pointer.add(offset + 112).cast()) };
        let combined = v128_or(
            v128_or(v128_or(pixels03, pixels47), v128_or(pixels811, pixels1215)),
            v128_or(
                v128_or(pixels1619, pixels2023),
                v128_or(pixels2427, pixels2831),
            ),
        );
        if !v128_any_true(u32x4_shr(combined, 24)) {
            has_transparent_pixels = true;
            offset += 128;
            continue;
        }
        has_transparent_pixels |=
            accumulate_quality_histogram_u32_bits_remaining_mixed_transparent_dense::<BITS>(
                histogram,
                &rgba_stream[offset..offset + 32],
                0,
                alpha_threshold,
            );
        offset += 32;
    }
    has_transparent_pixels
        | accumulate_quality_histogram_u32_bits_remaining_mixed_transparent_dense::<BITS>(
            histogram,
            rgba_stream,
            offset,
            alpha_threshold,
        )
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
#[inline(never)]
pub(crate) fn accumulate_quality_histogram_u32_bits_remaining_mixed_opaque_spans_four_bit(
    histogram: &mut [RgbHistogramBin32],
    rgba_stream: &[u8],
    start_offset: usize,
    alpha_threshold: u8,
) -> bool {
    const ALPHA_MASK: u64 = 0xff00_0000_ff00_0000;
    let rgba_pointer = rgba_stream.as_ptr();
    let mut has_transparent_pixels = false;
    let mut offset = start_offset;
    while offset + 32 <= rgba_stream.len() {
        let packed01 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        let packed23 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 8).cast()) });
        let packed45 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 16).cast()) });
        let packed67 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 24).cast()) });
        let combined_alpha = packed01 & packed23 & packed45 & packed67;
        if combined_alpha & ALPHA_MASK == ALPHA_MASK {
            // Keep opaque mixed-image spans branch-free. Checking whether the
            // two pixels share a bin costs more here than issuing both updates.
            add_quality_histogram_u32_pair_packed_split::<4>(histogram, packed01);
            add_quality_histogram_u32_pair_packed_split::<4>(histogram, packed23);
            add_quality_histogram_u32_pair_packed_split::<4>(histogram, packed45);
            add_quality_histogram_u32_pair_packed_split::<4>(histogram, packed67);
        } else if (packed01 | packed23 | packed45 | packed67) & ALPHA_MASK == 0 {
            has_transparent_pixels = true;
        } else {
            has_transparent_pixels |= add_quality_histogram_u32_pair_packed_with_alpha::<4>(
                histogram,
                packed01,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_packed_with_alpha::<4>(
                histogram,
                packed23,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_packed_with_alpha::<4>(
                histogram,
                packed45,
                alpha_threshold,
            );
            has_transparent_pixels |= add_quality_histogram_u32_pair_packed_with_alpha::<4>(
                histogram,
                packed67,
                alpha_threshold,
            );
        }
        offset += 32;
    }
    while offset < rgba_stream.len() {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        if ((packed >> 24) as u8) < alpha_threshold {
            has_transparent_pixels = true;
        } else {
            add_quality_histogram_u32_bits_const::<4>(histogram, packed);
        }
        offset += 4;
    }
    has_transparent_pixels
}

#[inline(never)]
pub(crate) fn accumulate_quality_histogram_u32_bits_remaining_mixed<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    rgba_stream: &[u8],
    start_offset: usize,
    alpha_threshold: u8,
) -> bool {
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    // Transparent-canvas inputs conventionally begin in the clear background.
    // Dispatch once so sparse foregrounds skip clear blocks without charging
    // mixed/opaque-leading images for a SIMD alpha mask on every block.
    if rgba_stream.get(start_offset + 3) == Some(&0) {
        return accumulate_quality_histogram_u32_bits_remaining_mixed_zero_dense::<BITS>(
            histogram,
            rgba_stream,
            start_offset,
            alpha_threshold,
        );
    }
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    if rgba_stream
        .get(start_offset + 3)
        .is_some_and(|alpha| *alpha < alpha_threshold)
    {
        return accumulate_quality_histogram_u32_bits_remaining_mixed_transparent_dense::<BITS>(
            histogram,
            rgba_stream,
            start_offset,
            alpha_threshold,
        );
    }
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    if BITS == 4 {
        // Ordinary mixed images still contain long all-255 alpha spans. One
        // packed decision per eight pixels avoids eight threshold checks there;
        // blocks with any other alpha value retain the exact pairwise path.
        return accumulate_quality_histogram_u32_bits_remaining_mixed_opaque_spans_four_bit(
            histogram,
            rgba_stream,
            start_offset,
            alpha_threshold,
        );
    }
    let rgba_pointer = rgba_stream.as_ptr();
    let mut has_transparent_pixels = false;
    let mut offset = start_offset;
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
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
    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
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

#[inline(always)]
pub(crate) fn accumulate_quality_histogram_u32_bits_remaining_opaque<
    const BITS: usize,
    const PROBE_ALPHA: bool,
>(
    histogram: &mut [RgbHistogramBin32],
    rgba_stream: &[u8],
    start_offset: usize,
) -> bool {
    let rgba_pointer = rgba_stream.as_ptr();
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
        add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed01);
        add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed23);
        add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed45);
        add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed67);
        let packed89 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 32).cast()) });
        let packed1011 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 40).cast()) });
        let packed1213 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 48).cast()) });
        let packed1415 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 56).cast()) });
        if PROBE_ALPHA {
            let combined_alpha = packed01
                & packed23
                & packed45
                & packed67
                & packed89
                & packed1011
                & packed1213
                & packed1415;
            all_alpha_255 &= combined_alpha & ALPHA_MASK == ALPHA_MASK;
        }
        add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed89);
        add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed1011);
        add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed1213);
        add_quality_histogram_u32_pair_packed::<BITS>(histogram, packed1415);
        offset += 64;
    }
    while offset < rgba_stream.len() {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        if PROBE_ALPHA && (packed >> 24) as u8 != u8::MAX {
            all_alpha_255 = false;
        }
        add_quality_histogram_u32_bits_const::<BITS>(histogram, packed);
        offset += 4;
    }
    all_alpha_255
}

#[inline(always)]
pub(crate) fn add_quality_histogram_u32_pair_packed_split<const BITS: usize>(
    histogram: &mut [RgbHistogramBin32],
    packed: u64,
) {
    let packed0 = packed as u32;
    let packed1 = (packed >> 32) as u32;
    let (index0, index1) = quality_histogram_index_pair_packed::<BITS>(packed);
    unsafe {
        let histogram = histogram.as_mut_ptr();
        add_quality_histogram_bin32_packed(
            histogram.add(index0),
            1,
            u32::from(packed0 as u8),
            u32::from((packed0 >> 8) as u8),
            u32::from((packed0 >> 16) as u8),
        );
        add_quality_histogram_bin32_packed(
            histogram.add(index1),
            1,
            u32::from(packed1 as u8),
            u32::from((packed1 >> 8) as u8),
            u32::from((packed1 >> 16) as u8),
        );
    }
}

#[inline(never)]
pub(crate) fn accumulate_quality_histogram_u32_bits_remaining_opaque_split<
    const PROBE_ALPHA: bool,
>(
    histogram: &mut [RgbHistogramBin32],
    rgba_stream: &[u8],
    start_offset: usize,
) -> bool {
    let rgba_pointer = rgba_stream.as_ptr();
    const ALPHA_MASK: u64 = 0xff00_0000_ff00_0000;
    let mut all_alpha_255 = true;
    let mut offset = start_offset;
    while offset + 32 <= rgba_stream.len() {
        let packed01 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        let packed23 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 8).cast()) });
        let packed45 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 16).cast()) });
        let packed67 =
            u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset + 24).cast()) });
        if PROBE_ALPHA {
            all_alpha_255 &= (packed01 & packed23 & packed45 & packed67) & ALPHA_MASK == ALPHA_MASK;
        }
        add_quality_histogram_u32_pair_packed_split::<4>(histogram, packed01);
        add_quality_histogram_u32_pair_packed_split::<4>(histogram, packed23);
        add_quality_histogram_u32_pair_packed_split::<4>(histogram, packed45);
        add_quality_histogram_u32_pair_packed_split::<4>(histogram, packed67);
        offset += 32;
    }
    while offset < rgba_stream.len() {
        let packed =
            u32::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.add(offset).cast()) });
        if PROBE_ALPHA && (packed >> 24) as u8 != u8::MAX {
            all_alpha_255 = false;
        }
        add_quality_histogram_u32_bits_const::<4>(histogram, packed);
        offset += 4;
    }
    all_alpha_255
}

/// A repeated leading pair followed by variation is a cheap signal for the
/// mixed runs that make the ordinary pair-coalescing branch unpredictable.
/// Both complete passes build identical bins; two read-only pair probes keep
/// the selected hot loop aligned at byte zero.
#[inline(always)]
pub(crate) fn accumulate_quality_histogram_u32_bits_opaque_adaptive<const PROBE_ALPHA: bool>(
    histogram: &mut [RgbHistogramBin32],
    rgba_stream: &[u8],
) -> bool {
    const VARIATION_PROBE_OFFSET: usize = 12 * 4;
    const PROBE_BYTES: usize = 16 * 4;
    let split_pairs = rgba_stream.len() >= PROBE_BYTES && {
        let rgba_pointer = rgba_stream.as_ptr();
        let first = u64::from_le(unsafe { std::ptr::read_unaligned(rgba_pointer.cast()) });
        let later = u64::from_le(unsafe {
            std::ptr::read_unaligned(rgba_pointer.add(VARIATION_PROBE_OFFSET).cast())
        });
        first as u32 == (first >> 32) as u32 && later as u32 != (later >> 32) as u32
    };
    if split_pairs {
        accumulate_quality_histogram_u32_bits_remaining_opaque_split::<PROBE_ALPHA>(
            histogram,
            rgba_stream,
            0,
        )
    } else {
        accumulate_quality_histogram_u32_bits_remaining_opaque::<4, PROBE_ALPHA>(
            histogram,
            rgba_stream,
            0,
        )
    }
}

/// Accumulate a wide histogram in chunks whose channel sums are guaranteed to
/// fit in `u32`. The final bins stay `u64` so arbitrarily large RGBA streams
/// retain the same exact counts and weighted sums as the direct wide scan.
pub(crate) fn accumulate_quality_histogram_u64_via_u32(
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
            accumulate_quality_histogram_u32_bits_remaining_opaque::<5, false>(
                &mut chunk_histogram,
                chunk,
                0,
            );
        } else {
            has_transparent_pixels |= accumulate_quality_histogram_u32_bits_remaining_mixed::<5>(
                &mut chunk_histogram,
                chunk,
                0,
                alpha_threshold,
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
