//! LZW decoders for GIF image data.

use super::*;

pub(crate) fn collect_image_data_into(
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
pub(crate) fn collect_image_data_into_fixed<'a>(
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

pub(crate) fn lzw_decode_to_indices_direct(
    min_code_size: u8,
    image_data: &[u8],
    output: &mut [u8],
) -> Result<(), String> {
    let mut scratch = LzwStackScratch::default();
    lzw_decode_to_indices_direct_with_scratch(min_code_size, image_data, output, &mut scratch)
}

pub(crate) fn lzw_decode_to_indices_direct_with_scratch(
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
pub(crate) fn decode_literal_9_bit_stream(image_data: &[u8], output: &mut [u8]) -> bool {
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
    let mut since_clear = 0u16;
    let output_length = output.len();
    let output_pointer = output.as_mut_ptr();

    loop {
        // A decoder reads 10-bit codes once a clear interval holds 255 codes,
        // so this fixed-width parse is only valid while intervals are shorter.
        if since_clear >= CLEAR - 1 {
            return false;
        }
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
            since_clear = 0;
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
        since_clear += 1;
    }
}

/// Decode a literal-only GIF stream whose code width is one bit wider than
/// the minimum code size.  wtfgif uses these streams for small indexed
/// palettes; validating the exact packed length, every clear/EOI marker, and
/// that no clear interval reaches the length at which a decoder widens its
/// codes keeps this a safe probe for ordinary dictionary-compressed GIFs too.
#[inline(always)]
pub(crate) fn decode_fixed_literal_stream<const CODE_BITS: usize>(
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
    let mut since_clear = 0usize;
    let output_pointer = output.as_mut_ptr();

    loop {
        // After `clear - 1` codes without a clear, a decoder reads codes one
        // bit wider, which this fixed-width parse cannot follow.
        if since_clear >= clear - 1 {
            return false;
        }
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
            since_clear = 0;
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
        since_clear += 1;
    }
}

/// Decode a fixed-width literal stream directly into palette-colored u32
/// output.  The byte portion of the destination is used as temporary index
/// storage and is consumed backwards, so no second frame-sized allocation is
/// needed.
#[inline(always)]
pub(crate) fn decode_fixed_literal_pixels<const CODE_BITS: usize>(
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
pub(crate) unsafe fn copy_lzw_dictionary_string(
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
pub(crate) unsafe fn copy_lzw_dictionary_color_string(
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
pub(crate) fn lzw_decode_to_pixels_copy_with_scratch(
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
pub(crate) fn decode_literal_9_bit_pixels(
    image_data: &[u8],
    palette: &[u32],
    output: &mut [u32],
) -> bool {
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
    let mut since_clear = 0u16;
    let output_length = output.len();
    let output_pointer = output.as_mut_ptr();
    let palette_length = palette.len();
    let palette_pointer = palette.as_ptr();

    loop {
        // A decoder reads 10-bit codes once a clear interval holds 255 codes,
        // so this fixed-width parse is only valid while intervals are shorter.
        if since_clear >= CLEAR - 1 {
            return false;
        }
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
            since_clear = 0;
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
        since_clear += 1;
    }
}

pub(crate) fn lzw_decode_to_indices_copy_with_scratch(
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

pub(crate) struct LzwStackScratch {
    pub(crate) prefix: [std::mem::MaybeUninit<u16>; 4096],
    pub(crate) suffix: [std::mem::MaybeUninit<u8>; 4096],
    pub(crate) stack: [std::mem::MaybeUninit<u8>; 4096],
    pub(crate) string_length: [std::mem::MaybeUninit<u16>; 4096],
    pub(crate) string_start: [std::mem::MaybeUninit<u32>; 4096],
}

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

pub(crate) fn lzw_decode_to_indices_stack_with_scratch(
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
