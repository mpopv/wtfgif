//! Literal-code LZW writers and GIF sub-block packing.

use super::*;

pub(crate) struct LzwEncodeScratch {
    // JavaScript overwrites the complete requested byte range before any
    // encoder reads it. Keep the reusable input allocation uninitialized so
    // growing it does not first zero bytes that the caller immediately
    // replaces. `MaybeUninit<u32>` preserves four-byte alignment for packed
    // RGBA loads.
    pub(crate) input: Vec<std::mem::MaybeUninit<u32>>,
    pub(crate) output: Vec<u8>,
}

pub(crate) fn encode_indexed_literal_lzw_scratch_inner(
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<usize, String> {
    REUSABLE_LZW_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let output = &mut scratch.output;
        output.clear();
        output.reserve(index_stream.len() / 2 + 16);
        encode_indexed_literal_lzw_direct_to(output, index_stream, min_code_size, color_count)?;
        Ok(output.len())
    })
}

pub(crate) fn encode_indexed_literal_lzw_scratch_from_input_inner(
    length: usize,
    min_code_size: u8,
    color_count: usize,
) -> Result<usize, String> {
    REUSABLE_LZW_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let LzwEncodeScratch { input, output } = &mut *scratch;
        if length > input.len() * std::mem::size_of::<u32>() {
            return Err("Indexed input scratch length exceeds capacity".to_string());
        }
        let index_stream =
            unsafe { std::slice::from_raw_parts(input.as_ptr().cast::<u8>(), length) };
        output.clear();
        output.reserve(index_stream.len() / 2 + 16);
        encode_indexed_literal_lzw_direct_to(output, index_stream, min_code_size, color_count)?;
        Ok(output.len())
    })
}

#[cfg(not(all(feature = "encode-only", target_arch = "wasm32")))]
pub(crate) fn encode_indexed_literal_lzw_to(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
    compressed: &mut Vec<u8>,
) -> Result<(), String> {
    if !(2..=8).contains(&min_code_size) {
        return Err(format!("Invalid LZW minimum code size {min_code_size}"));
    }
    if color_count == 0 || color_count > 256 {
        return Err("Invalid color count".to_string());
    }
    if index_stream.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    let code_size = usize::from(min_code_size) + 1;
    let literals_per_clear = (1usize << min_code_size) - 2;
    let estimated_bytes = index_stream
        .len()
        .saturating_mul(code_size)
        .saturating_mul(literals_per_clear + 1)
        / literals_per_clear
        / 8
        + 8;
    compressed.clear();
    if compressed.capacity() < estimated_bytes {
        compressed.reserve(estimated_bytes - compressed.capacity());
    }
    encode_indexed_literal_codes_raw(compressed, index_stream, min_code_size, color_count)?;
    output.push(min_code_size);
    for block in compressed.chunks(255) {
        output.push(block.len() as u8);
        output.extend_from_slice(block);
    }
    output.push(0);
    Ok(())
}

pub(crate) fn encode_indexed_literal_lzw_direct_to(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<(), String> {
    encode_indexed_literal_lzw_direct_to_impl::<true>(
        output,
        index_stream,
        min_code_size,
        color_count,
    )
}

#[inline(always)]
pub(crate) fn encode_indexed_literal_lzw_direct_to_unchecked(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<(), String> {
    encode_indexed_literal_lzw_direct_to_impl::<false>(
        output,
        index_stream,
        min_code_size,
        color_count,
    )
}

#[inline(always)]
pub(crate) fn encode_indexed_literal_lzw_direct_to_impl<const VALIDATE: bool>(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<(), String> {
    if min_code_size == 7 && color_count <= 128 {
        return encode_eight_bit_literal_lzw_direct_to::<VALIDATE>(
            output,
            index_stream,
            color_count,
        );
    }
    if min_code_size == 3 {
        return encode_four_bit_literal_lzw_direct_to::<VALIDATE>(
            output,
            index_stream,
            color_count,
        );
    }
    if min_code_size == 5 {
        return encode_six_bit_literal_lzw_direct_to::<VALIDATE>(output, index_stream, color_count);
    }
    if min_code_size == 6 {
        return encode_seven_bit_literal_lzw_direct_to::<VALIDATE>(
            output,
            index_stream,
            color_count,
        );
    }
    if min_code_size == 8 {
        return encode_nine_bit_literal_lzw_direct_to::<VALIDATE>(
            output,
            index_stream,
            color_count,
        );
    }
    output.push(min_code_size);
    let compressed_start = output.len();
    encode_indexed_literal_codes_raw(output, index_stream, min_code_size, color_count)?;
    let compressed_length = output.len() - compressed_start;
    let block_count = compressed_length.div_ceil(255);
    let final_length = output
        .len()
        .checked_add(block_count + 1)
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?;
    output.resize(final_length, 0);
    for block in (0..block_count).rev() {
        let source_start = compressed_start + block * 255;
        let length = (compressed_length - block * 255).min(255);
        let destination_start = compressed_start + block * 256;
        output.copy_within(source_start..source_start + length, destination_start + 1);
        output[destination_start] = length as u8;
    }
    Ok(())
}

pub(crate) fn encode_six_bit_literal_lzw_direct_to<const VALIDATE: bool>(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    color_count: usize,
) -> Result<(), String> {
    if color_count == 0 || color_count > 32 {
        return Err("Invalid color count".to_string());
    }
    if index_stream.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    if VALIDATE && !indices_fit_color_count(index_stream, color_count) {
        return Err("Pixel index out of range".to_string());
    }

    const CODE_SIZE: usize = 6;
    const CLEAR: u16 = 32;
    const EOI: u16 = 33;
    const LITERALS_PER_CLEAR: usize = 30;
    let clear_count = index_stream.len().div_ceil(LITERALS_PER_CLEAR);
    let raw_length = ((index_stream.len() + clear_count + 1) * CODE_SIZE).div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + 2 + block_count + raw_length);
    output[output_start] = 5;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;

    for literals in index_stream.chunks(LITERALS_PER_CLEAR) {
        append_six_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, CLEAR);
        let mut groups = literals.chunks_exact(8);
        for group in &mut groups {
            let packed = u64::from(group[0])
                | (u64::from(group[1]) << 6)
                | (u64::from(group[2]) << 12)
                | (u64::from(group[3]) << 18)
                | (u64::from(group[4]) << 24)
                | (u64::from(group[5]) << 30)
                | (u64::from(group[6]) << 36)
                | (u64::from(group[7]) << 42);
            let combined = bits | (packed << bit_count);
            let total_bits = bit_count + 48;
            let byte_count = total_bits / 8;
            writer.write_fixed_u64(combined, byte_count);
            bits = combined >> (byte_count * 8);
            bit_count = total_bits - byte_count * 8;
        }
        for &pixel in groups.remainder() {
            append_six_bit_literal_code_to_direct(
                &mut writer,
                &mut bits,
                &mut bit_count,
                u16::from(pixel),
            );
        }
    }
    append_six_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, EOI);
    while bit_count > 0 {
        writer.write_byte(bits as u8);
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(writer.raw_position, raw_length);

    let mut raw_offset = 0usize;
    for block in 0..block_count {
        let length = (raw_length - raw_offset).min(255);
        unsafe {
            output
                .as_mut_ptr()
                .add(output_start + 1 + block * 256)
                .write(length as u8);
        }
        raw_offset += length;
    }
    output[output_start + 1 + block_count + raw_length] = 0;
    Ok(())
}

pub(crate) fn encode_seven_bit_literal_lzw_direct_to<const VALIDATE: bool>(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    color_count: usize,
) -> Result<(), String> {
    if color_count == 0 || color_count > 64 {
        return Err("Invalid color count".to_string());
    }
    if index_stream.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    if VALIDATE && !indices_fit_color_count(index_stream, color_count) {
        return Err("Pixel index out of range".to_string());
    }

    const CODE_SIZE: usize = 7;
    const CLEAR: u16 = 64;
    const EOI: u16 = 65;
    const LITERALS_PER_CLEAR: usize = 62;
    let clear_count = index_stream.len().div_ceil(LITERALS_PER_CLEAR);
    let raw_length = ((index_stream.len() + clear_count + 1) * CODE_SIZE).div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + 2 + block_count + raw_length);
    output[output_start] = 6;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;

    for literals in index_stream.chunks(LITERALS_PER_CLEAR) {
        append_seven_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, CLEAR);
        let mut groups = literals.chunks_exact(8);
        for group in &mut groups {
            let packed = u64::from(group[0])
                | (u64::from(group[1]) << 7)
                | (u64::from(group[2]) << 14)
                | (u64::from(group[3]) << 21)
                | (u64::from(group[4]) << 28)
                | (u64::from(group[5]) << 35)
                | (u64::from(group[6]) << 42)
                | (u64::from(group[7]) << 49);
            let combined = bits | (packed << bit_count);
            let total_bits = bit_count + 56;
            let byte_count = total_bits / 8;
            writer.write_fixed_u64(combined, byte_count);
            bits = combined >> (byte_count * 8);
            bit_count = total_bits - byte_count * 8;
        }
        for &pixel in groups.remainder() {
            append_seven_bit_literal_code_to_direct(
                &mut writer,
                &mut bits,
                &mut bit_count,
                u16::from(pixel),
            );
        }
    }
    append_seven_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, EOI);
    while bit_count > 0 {
        writer.write_byte(bits as u8);
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(writer.raw_position, raw_length);

    let mut raw_offset = 0usize;
    for block in 0..block_count {
        let length = (raw_length - raw_offset).min(255);
        unsafe {
            output
                .as_mut_ptr()
                .add(output_start + 1 + block * 256)
                .write(length as u8);
        }
        raw_offset += length;
    }
    output[output_start + 1 + block_count + raw_length] = 0;
    Ok(())
}

pub(crate) fn encode_eight_bit_literal_lzw_direct_to<const VALIDATE: bool>(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    color_count: usize,
) -> Result<(), String> {
    if index_stream.is_empty() {
        return Ok(());
    }
    if color_count == 0 || color_count > 128 {
        return Err("Invalid color count".to_string());
    }
    if VALIDATE && !indices_fit_color_count(index_stream, color_count) {
        return Err("Pixel index out of range".to_string());
    }

    let raw_length = index_stream.len() + index_stream.len().div_ceil(126) + 1;
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    let output_length = output_start + 2 + block_count + raw_length;
    resize_output_uninitialized(output, output_length);
    output[output_start] = 7;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    for literals in index_stream.chunks(126) {
        writer.write_byte(128);
        writer.write_slice(literals);
    }
    writer.write_byte(129);
    debug_assert_eq!(writer.raw_position, raw_length);
    let mut raw_offset = 0usize;
    for block in 0..block_count {
        let length = (raw_length - raw_offset).min(255);
        unsafe {
            output
                .as_mut_ptr()
                .add(output_start + 1 + block * 256)
                .write(length as u8);
        }
        raw_offset += length;
    }
    unsafe {
        output
            .as_mut_ptr()
            .add(output_start + 1 + block_count + raw_length)
            .write(0);
    }
    Ok(())
}

pub(crate) struct DirectGifSubblockWriter<'a> {
    pub(crate) output: &'a mut [u8],
    pub(crate) position: usize,
    pub(crate) block_remaining: usize,
    pub(crate) raw_position: usize,
}

/// Extend an output buffer without paying to zero bytes that the caller will
/// immediately overwrite. Every caller must fill the entire new range before
/// it is observed by safe code.
#[inline(always)]
pub(crate) fn resize_output_uninitialized(output: &mut Vec<u8>, new_len: usize) {
    debug_assert!(new_len >= output.len());
    if output.capacity() < new_len {
        output.reserve(new_len - output.len());
    }
    // SAFETY: callers of this helper write every byte in the extended range
    // before reading it or exposing it to safe code.
    unsafe { output.set_len(new_len) };
}

impl DirectGifSubblockWriter<'_> {
    #[inline(always)]
    pub(crate) fn write_byte(&mut self, value: u8) {
        if self.block_remaining == 0 {
            self.position += 1;
            self.block_remaining = 255;
        }
        self.output[self.position] = value;
        self.position += 1;
        self.block_remaining -= 1;
        self.raw_position += 1;
    }

    #[inline(always)]
    pub(crate) fn write_fixed_u64(&mut self, mut value: u64, length: usize) {
        if length > self.block_remaining {
            for _ in 0..length {
                self.write_byte(value as u8);
                value >>= 8;
            }
            return;
        }
        if length == 8 {
            // The common literal packer writes complete eight-byte chunks.
            // Store them directly when the chunk stays inside the current
            // GIF sub-block; crossing a boundary still uses the byte-wise
            // fallback above so the length marker cannot be overwritten.
            unsafe {
                std::ptr::write_unaligned(
                    self.output.as_mut_ptr().add(self.position).cast::<u64>(),
                    value.to_le(),
                );
            }
            self.position += 8;
            self.block_remaining -= 8;
            self.raw_position += 8;
            return;
        }
        if length == 7 && self.block_remaining >= 8 {
            // A seven-byte literal group has one more raw byte available in
            // this sub-block. Store the whole u64 and advance only seven
            // bytes; the next write overwrites the eighth byte. This keeps
            // the common non-boundary path on the same unaligned-store fast
            // path as eight-byte groups.
            unsafe {
                std::ptr::write_unaligned(
                    self.output.as_mut_ptr().add(self.position).cast::<u64>(),
                    value.to_le(),
                );
            }
            self.position += 7;
            self.block_remaining -= 7;
            self.raw_position += 7;
            return;
        }
        let bytes = value.to_le_bytes();
        self.output[self.position..self.position + length].copy_from_slice(&bytes[..length]);
        self.position += length;
        self.block_remaining -= length;
        self.raw_position += length;
    }

    #[inline(always)]
    pub(crate) fn write_slice(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            if self.block_remaining == 0 {
                self.position += 1;
                self.block_remaining = 255;
            }
            let length = bytes.len().min(self.block_remaining);
            self.output[self.position..self.position + length].copy_from_slice(&bytes[..length]);
            self.position += length;
            self.block_remaining -= length;
            self.raw_position += length;
            bytes = &bytes[length..];
        }
    }
}

pub(crate) fn encode_four_bit_literal_lzw_direct_to<const VALIDATE: bool>(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    color_count: usize,
) -> Result<(), String> {
    if color_count == 0 || color_count > 8 {
        return Err("Invalid color count".to_string());
    }
    if index_stream.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    if VALIDATE && !indices_fit_color_count(index_stream, color_count) {
        return Err("Pixel index out of range".to_string());
    }

    let clear_count = index_stream.len().div_ceil(6);
    let raw_length = ((index_stream.len() + clear_count + 1) * 4).div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + 2 + block_count + raw_length);
    output[output_start] = 3;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let mut paired_groups = index_stream.chunks_exact(12);
    for group in &mut paired_groups {
        let packed = 8u64
            | (u64::from(group[0]) << 4)
            | (u64::from(group[1]) << 8)
            | (u64::from(group[2]) << 12)
            | (u64::from(group[3]) << 16)
            | (u64::from(group[4]) << 20)
            | (u64::from(group[5]) << 24)
            | (8u64 << 28)
            | (u64::from(group[6]) << 32)
            | (u64::from(group[7]) << 36)
            | (u64::from(group[8]) << 40)
            | (u64::from(group[9]) << 44)
            | (u64::from(group[10]) << 48)
            | (u64::from(group[11]) << 52);
        writer.write_fixed_u64(packed, 7);
    }
    let mut groups = paired_groups.remainder().chunks_exact(6);
    for group in &mut groups {
        let packed = 8u32
            | (u32::from(group[0]) << 4)
            | (u32::from(group[1]) << 8)
            | (u32::from(group[2]) << 12)
            | (u32::from(group[3]) << 16)
            | (u32::from(group[4]) << 20)
            | (u32::from(group[5]) << 24);
        let total_bits = bit_count + 28;
        let combined = bits | (u64::from(packed) << bit_count);
        let byte_count = total_bits / 8;
        writer.write_fixed_u64(combined, byte_count);
        bits = combined >> (byte_count * 8);
        bit_count = total_bits - byte_count * 8;
    }
    if !groups.remainder().is_empty() {
        bits |= 8u64 << bit_count;
        bit_count += 4;
        while bit_count >= 8 {
            writer.write_byte(bits as u8);
            bits >>= 8;
            bit_count -= 8;
        }
        for &pixel in groups.remainder() {
            bits |= u64::from(pixel) << bit_count;
            bit_count += 4;
            while bit_count >= 8 {
                writer.write_byte(bits as u8);
                bits >>= 8;
                bit_count -= 8;
            }
        }
    }
    bits |= 9u64 << bit_count;
    bit_count += 4;
    while bit_count > 0 {
        writer.write_byte(bits as u8);
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(writer.raw_position, raw_length);

    let mut raw_offset = 0usize;
    for block in 0..block_count {
        let length = (raw_length - raw_offset).min(255);
        unsafe {
            output
                .as_mut_ptr()
                .add(output_start + 1 + block * 256)
                .write(length as u8);
        }
        raw_offset += length;
    }
    output[output_start + 1 + block_count + raw_length] = 0;
    Ok(())
}

pub(crate) fn encode_nine_bit_literal_lzw_direct_to<const VALIDATE: bool>(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    color_count: usize,
) -> Result<(), String> {
    if color_count == 0 || color_count > 256 {
        return Err("Invalid color count".to_string());
    }
    if index_stream.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    if VALIDATE && !indices_fit_color_count(index_stream, color_count) {
        return Err("Pixel index out of range".to_string());
    }

    let clear_count = index_stream.len().div_ceil(254);
    let raw_length = index_stream
        .len()
        .checked_add(clear_count)
        .and_then(|codes| codes.checked_add(1))
        .and_then(|codes| codes.checked_mul(9))
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?
        .div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + 2 + block_count + raw_length + 8);
    output[output_start] = 8;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;

    for literals in index_stream.chunks(254) {
        append_nine_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, 256);

        let mut groups = literals.chunks_exact(7);
        for group in &mut groups {
            let packed = u64::from(group[0])
                | (u64::from(group[1]) << 9)
                | (u64::from(group[2]) << 18)
                | (u64::from(group[3]) << 27)
                | (u64::from(group[4]) << 36)
                | (u64::from(group[5]) << 45)
                | (u64::from(group[6]) << 54);
            let combined = bits | (packed << bit_count);
            let total_bits = bit_count + 63;
            if total_bits >= 64 {
                writer.write_fixed_u64(combined, 8);
                bits = if bit_count == 0 {
                    0
                } else {
                    packed >> (64 - bit_count)
                };
                bit_count = total_bits - 64;
            } else {
                debug_assert_eq!(bit_count, 0);
                writer.write_fixed_u64(combined, 7);
                bits = combined >> 56;
                bit_count = total_bits - 56;
            }
        }
        for &pixel in groups.remainder() {
            append_nine_bit_literal_code_to_direct(
                &mut writer,
                &mut bits,
                &mut bit_count,
                u16::from(pixel),
            );
        }
    }
    append_nine_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, 257);
    while bit_count > 0 {
        writer.write_byte(bits as u8);
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(writer.raw_position, raw_length);

    let mut raw_offset = 0usize;
    for block in 0..block_count {
        let length = (raw_length - raw_offset).min(255);
        unsafe {
            output
                .as_mut_ptr()
                .add(output_start + 1 + block * 256)
                .write(length as u8);
        }
        raw_offset += length;
    }
    let final_length = output_start + 2 + block_count + raw_length;
    output[final_length - 1] = 0;
    output.truncate(final_length);
    Ok(())
}

/// Encode a 256-color literal stream directly from opaque RGBA input using an
/// exact supplied palette. This is the known-palette counterpart to the
/// quality histogram mapper below; it avoids a full intermediate index pass.
pub(crate) fn encode_nine_bit_literal_lzw_palette_mapped_to(
    output: &mut Vec<u8>,
    rgba_stream: &[u8],
    mapper: &PaletteMapper<'_>,
) -> Result<(), String> {
    if rgba_stream.is_empty() || !rgba_stream.len().is_multiple_of(4) {
        return Err("RGBA frame stream is empty or misaligned".to_string());
    }
    let pixel_count = rgba_stream.len() / 4;
    let clear_count = pixel_count.div_ceil(254);
    let raw_length = pixel_count
        .checked_add(clear_count)
        .and_then(|codes| codes.checked_add(1))
        .and_then(|codes| codes.checked_mul(9))
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?
        .div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    let output_length = output_start + 2 + block_count + raw_length;
    // The direct writer fills every byte in this range, including the
    // sub-block length slots written after the literal stream. Avoid asking
    // Wasm's allocator to zero the encoded payload before overwriting it.
    resize_output_uninitialized(output, output_length);
    output[output_start] = 8;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let rgba_pointer = rgba_stream.as_ptr();
    let mut cache_keys = [u32::MAX; 256];
    let mut cache_values = [0u8; 256];

    for chunk_start in (0..pixel_count).step_by(254) {
        append_nine_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, 256);
        let chunk_end = (chunk_start + 254).min(pixel_count);
        let mut pixel_index = chunk_start;
        while pixel_index + 7 <= chunk_end {
            let rgba_offset = pixel_index * 4;
            let (packed0, packed1) = unsafe { read_rgba_pair(rgba_pointer, rgba_offset) };
            let (packed2, packed3) = unsafe { read_rgba_pair(rgba_pointer, rgba_offset + 8) };
            let (packed4, packed5) = unsafe { read_rgba_pair(rgba_pointer, rgba_offset + 16) };
            let packed6 = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(rgba_offset + 24).cast())
            });
            let code0 = exact_palette_index_packed_cached(
                mapper,
                packed0,
                &mut cache_keys,
                &mut cache_values,
            )?;
            let code1 = exact_palette_index_packed_cached(
                mapper,
                packed1,
                &mut cache_keys,
                &mut cache_values,
            )?;
            let code2 = exact_palette_index_packed_cached(
                mapper,
                packed2,
                &mut cache_keys,
                &mut cache_values,
            )?;
            let code3 = exact_palette_index_packed_cached(
                mapper,
                packed3,
                &mut cache_keys,
                &mut cache_values,
            )?;
            let code4 = exact_palette_index_packed_cached(
                mapper,
                packed4,
                &mut cache_keys,
                &mut cache_values,
            )?;
            let code5 = exact_palette_index_packed_cached(
                mapper,
                packed5,
                &mut cache_keys,
                &mut cache_values,
            )?;
            let code6 = exact_palette_index_packed_cached(
                mapper,
                packed6,
                &mut cache_keys,
                &mut cache_values,
            )?;
            let packed_codes = u64::from(code0)
                | (u64::from(code1) << 9)
                | (u64::from(code2) << 18)
                | (u64::from(code3) << 27)
                | (u64::from(code4) << 36)
                | (u64::from(code5) << 45)
                | (u64::from(code6) << 54);
            let combined = bits | (packed_codes << bit_count);
            let total_bits = bit_count + 63;
            if total_bits >= 64 {
                writer.write_fixed_u64(combined, 8);
                bits = if bit_count == 0 {
                    0
                } else {
                    packed_codes >> (64 - bit_count)
                };
                bit_count = total_bits - 64;
            } else {
                debug_assert_eq!(bit_count, 0);
                writer.write_fixed_u64(combined, 7);
                bits = combined >> 56;
                bit_count = total_bits - 56;
            }
            pixel_index += 7;
        }
        while pixel_index < chunk_end {
            let packed = u32::from_le(unsafe {
                std::ptr::read_unaligned(rgba_pointer.add(pixel_index * 4).cast())
            });
            let code = exact_palette_index_packed_cached(
                mapper,
                packed,
                &mut cache_keys,
                &mut cache_values,
            )?;
            append_nine_bit_literal_code_to_direct(
                &mut writer,
                &mut bits,
                &mut bit_count,
                u16::from(code),
            );
            pixel_index += 1;
        }
    }
    append_nine_bit_literal_code_to_direct(&mut writer, &mut bits, &mut bit_count, 257);
    while bit_count > 0 {
        writer.write_byte(bits as u8);
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(writer.raw_position, raw_length);
    let mut raw_offset = 0usize;
    for block in 0..block_count {
        let length = (raw_length - raw_offset).min(255);
        unsafe {
            output
                .as_mut_ptr()
                .add(output_start + 1 + block * 256)
                .write(length as u8);
        }
        raw_offset += length;
    }
    output[output_start + 1 + block_count + raw_length] = 0;
    Ok(())
}

#[inline(always)]
pub(crate) fn append_six_bit_literal_code_to_direct(
    writer: &mut DirectGifSubblockWriter,
    bits: &mut u64,
    bit_count: &mut usize,
    code: u16,
) {
    *bits |= u64::from(code) << *bit_count;
    *bit_count += 6;
    while *bit_count >= 8 {
        writer.write_byte(*bits as u8);
        *bits >>= 8;
        *bit_count -= 8;
    }
}

#[inline(always)]
pub(crate) fn append_seven_bit_literal_code_to_direct(
    writer: &mut DirectGifSubblockWriter,
    bits: &mut u64,
    bit_count: &mut usize,
    code: u16,
) {
    *bits |= u64::from(code) << *bit_count;
    *bit_count += 7;
    while *bit_count >= 8 {
        writer.write_byte(*bits as u8);
        *bits >>= 8;
        *bit_count -= 8;
    }
}

#[inline(always)]
pub(crate) fn append_nine_bit_literal_code_to_direct(
    writer: &mut DirectGifSubblockWriter,
    bits: &mut u64,
    bit_count: &mut usize,
    code: u16,
) {
    *bits |= u64::from(code) << *bit_count;
    *bit_count += 9;
    while *bit_count >= 8 {
        writer.write_byte(*bits as u8);
        *bits >>= 8;
        *bit_count -= 8;
    }
}

pub(crate) fn encode_indexed_literal_codes_raw(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    min_code_size: u8,
    color_count: usize,
) -> Result<(), String> {
    if !(2..=8).contains(&min_code_size) {
        return Err(format!("Invalid LZW minimum code size {min_code_size}"));
    }
    if color_count == 0 || color_count > 256 {
        return Err("Invalid color count".to_string());
    }
    if index_stream.is_empty() {
        return Err("Indexed pixel stream is empty".to_string());
    }
    if !indices_fit_color_count(index_stream, color_count) {
        return Err("Pixel index out of range".to_string());
    }
    let clear = 1usize << min_code_size;
    let eoi = clear + 1;
    let code_size = usize::from(min_code_size) + 1;
    // Reset before a literal-only decoder dictionary would require wider codes.
    let literals_per_clear = clear - 2;
    let mut bits = 0u64;
    let mut bit_count = 0usize;

    if min_code_size == 2 {
        return encode_two_bit_literal_codes(output, index_stream, color_count);
    }
    if min_code_size == 3 {
        return encode_four_bit_literal_codes(output, index_stream);
    }
    if min_code_size == 7 {
        return encode_eight_bit_literal_codes(output, index_stream);
    }
    if min_code_size == 8 {
        return encode_nine_bit_literal_codes(output, index_stream);
    }
    if min_code_size == 4 && color_count == 16 {
        return encode_five_bit_literal_codes(output, index_stream);
    }
    if min_code_size == 5 && color_count == 32 {
        return encode_six_bit_literal_codes(output, index_stream);
    }
    if min_code_size == 6 && color_count == 64 {
        return encode_seven_bit_literal_codes(output, index_stream);
    }
    for literals in index_stream.chunks(literals_per_clear) {
        emit_raw_lzw_code(output, &mut bits, &mut bit_count, code_size, clear);
        for &pixel in literals {
            emit_raw_lzw_code(
                output,
                &mut bits,
                &mut bit_count,
                code_size,
                usize::from(pixel),
            );
        }
    }
    emit_raw_lzw_code(output, &mut bits, &mut bit_count, code_size, eoi);
    while bit_count > 0 {
        output.push((bits & 0xff) as u8);
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    Ok(())
}

#[inline]
pub(crate) fn indices_fit_color_count(index_stream: &[u8], color_count: usize) -> bool {
    if color_count >= 256 {
        return true;
    }
    if color_count.is_power_of_two() {
        let invalid_bits = !(color_count as u8 - 1);
        let invalid_mask = u64::from(invalid_bits) * 0x0101_0101_0101_0101;
        let mut chunks = index_stream.chunks_exact(8);
        for chunk in &mut chunks {
            let packed = u64::from_le_bytes(chunk.try_into().unwrap());
            if packed & invalid_mask != 0 {
                return false;
            }
        }
        return chunks
            .remainder()
            .iter()
            .all(|&pixel| usize::from(pixel) < color_count);
    }
    index_stream
        .iter()
        .all(|&pixel| usize::from(pixel) < color_count)
}

pub(crate) fn encode_eight_bit_literal_codes(
    output: &mut Vec<u8>,
    index_stream: &[u8],
) -> Result<(), String> {
    for literals in index_stream.chunks(126) {
        output.push(128);
        output.extend_from_slice(literals);
    }
    output.push(129);
    Ok(())
}

pub(crate) fn encode_four_bit_literal_codes(
    output: &mut Vec<u8>,
    index_stream: &[u8],
) -> Result<(), String> {
    let clear_count = index_stream.len().div_ceil(6);
    let raw_length = ((index_stream.len() + clear_count + 1) * 4).div_ceil(8);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + raw_length);
    let output_pointer = unsafe { output.as_mut_ptr().add(output_start) };
    let mut output_position = 0usize;
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let mut groups = index_stream.chunks_exact(6);

    for group in &mut groups {
        let packed = 8u32
            | (u32::from(group[0]) << 4)
            | (u32::from(group[1]) << 8)
            | (u32::from(group[2]) << 12)
            | (u32::from(group[3]) << 16)
            | (u32::from(group[4]) << 20)
            | (u32::from(group[5]) << 24);
        let total_bits = bit_count + 28;
        let combined = bits | (u64::from(packed) << bit_count);
        let byte_count = total_bits / 8;
        let bytes = combined.to_le_bytes();
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                output_pointer.add(output_position),
                byte_count,
            );
        }
        output_position += byte_count;
        bits = combined >> (byte_count * 8);
        bit_count = total_bits - byte_count * 8;
    }

    if !groups.remainder().is_empty() {
        append_fixed_literal_code(
            output_pointer,
            &mut output_position,
            &mut bits,
            &mut bit_count,
            8,
            4,
        );
        for &pixel in groups.remainder() {
            append_fixed_literal_code(
                output_pointer,
                &mut output_position,
                &mut bits,
                &mut bit_count,
                pixel,
                4,
            );
        }
    }
    append_fixed_literal_code(
        output_pointer,
        &mut output_position,
        &mut bits,
        &mut bit_count,
        9,
        4,
    );
    while bit_count > 0 {
        unsafe { output_pointer.add(output_position).write(bits as u8) };
        output_position += 1;
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(output_position, raw_length);
    Ok(())
}

#[inline(always)]
pub(crate) fn append_fixed_literal_code(
    output: *mut u8,
    output_position: &mut usize,
    bits: &mut u64,
    bit_count: &mut usize,
    code: u8,
    code_bits: usize,
) {
    *bits |= u64::from(code) << *bit_count;
    *bit_count += code_bits;
    while *bit_count >= 8 {
        unsafe { output.add(*output_position).write(*bits as u8) };
        *output_position += 1;
        *bits >>= 8;
        *bit_count -= 8;
    }
}

#[inline(always)]
pub(crate) fn encode_fixed_width_literal_codes<const CODE_BITS: usize>(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    clear_code: u8,
    eoi_code: u8,
    literals_per_clear: usize,
) -> Result<(), String> {
    let clear_count = index_stream.len().div_ceil(literals_per_clear);
    let raw_length = ((index_stream.len() + clear_count + 1) * CODE_BITS).div_ceil(8);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + raw_length);
    let output_pointer = unsafe { output.as_mut_ptr().add(output_start) };
    let mut output_position = 0usize;
    let mut bits = 0u64;
    let mut bit_count = 0usize;

    for literals in index_stream.chunks(literals_per_clear) {
        append_fixed_literal_code(
            output_pointer,
            &mut output_position,
            &mut bits,
            &mut bit_count,
            clear_code,
            CODE_BITS,
        );
        let mut groups = literals.chunks_exact(8);
        for group in &mut groups {
            let packed = u64::from(group[0])
                | (u64::from(group[1]) << CODE_BITS)
                | (u64::from(group[2]) << (CODE_BITS * 2))
                | (u64::from(group[3]) << (CODE_BITS * 3))
                | (u64::from(group[4]) << (CODE_BITS * 4))
                | (u64::from(group[5]) << (CODE_BITS * 5))
                | (u64::from(group[6]) << (CODE_BITS * 6))
                | (u64::from(group[7]) << (CODE_BITS * 7));
            let total_bits = bit_count + CODE_BITS * 8;
            let combined = bits | (packed << bit_count);
            let byte_count = total_bits / 8;
            let bytes = combined.to_le_bytes();
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    output_pointer.add(output_position),
                    byte_count,
                );
            }
            output_position += byte_count;
            bits = combined >> (byte_count * 8);
            bit_count = total_bits - byte_count * 8;
        }
        for &pixel in groups.remainder() {
            append_fixed_literal_code(
                output_pointer,
                &mut output_position,
                &mut bits,
                &mut bit_count,
                pixel,
                CODE_BITS,
            );
        }
    }

    append_fixed_literal_code(
        output_pointer,
        &mut output_position,
        &mut bits,
        &mut bit_count,
        eoi_code,
        CODE_BITS,
    );
    while bit_count > 0 {
        unsafe { output_pointer.add(output_position).write(bits as u8) };
        output_position += 1;
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(output_position, raw_length);
    Ok(())
}

pub(crate) fn encode_six_bit_literal_codes(
    output: &mut Vec<u8>,
    index_stream: &[u8],
) -> Result<(), String> {
    encode_fixed_width_literal_codes::<6>(output, index_stream, 32, 33, 30)
}

pub(crate) const fn build_five_bit_pair_table() -> [u16; 4096] {
    let mut table = [0u16; 4096];
    let mut index = 0usize;
    while index < table.len() {
        table[index] = ((index & 0x0f) | (((index >> 8) & 0x0f) << 5)) as u16;
        index += 1;
    }
    table
}

pub(crate) static FIVE_BIT_PAIR_TABLE: [u16; 4096] = build_five_bit_pair_table();

#[inline(always)]
pub(crate) unsafe fn five_bit_pair(input: *const u8) -> u64 {
    let index = u16::from_le(unsafe { input.cast::<u16>().read_unaligned() }) as usize;
    u64::from(*unsafe { FIVE_BIT_PAIR_TABLE.get_unchecked(index) })
}

pub(crate) fn encode_five_bit_literal_codes(
    output: &mut Vec<u8>,
    index_stream: &[u8],
) -> Result<(), String> {
    let clear_count = index_stream.len().div_ceil(14);
    let raw_length = ((index_stream.len() + clear_count + 1) * 5).div_ceil(8);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + raw_length);
    let output_pointer = unsafe { output.as_mut_ptr().add(output_start) };
    let input_pointer = index_stream.as_ptr();
    let mut output_position = 0usize;
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let full_group_count = index_stream.len() / 14;
    for group in 0..full_group_count {
        let literals = unsafe { input_pointer.add(group * 14) };
        bits |= 16u64 << bit_count;
        bit_count += 5;
        while bit_count >= 8 {
            unsafe { output_pointer.add(output_position).write(bits as u8) };
            output_position += 1;
            bits >>= 8;
            bit_count -= 8;
        }

        let first = unsafe {
            five_bit_pair(literals)
                | (five_bit_pair(literals.add(2)) << 10)
                | (five_bit_pair(literals.add(4)) << 20)
                | (five_bit_pair(literals.add(6)) << 30)
        };
        let combined = bits | (first << bit_count);
        let first_bytes = combined.to_le_bytes();
        unsafe {
            std::ptr::copy_nonoverlapping(
                first_bytes.as_ptr(),
                output_pointer.add(output_position),
                5,
            )
        };
        output_position += 5;
        bits = combined >> 40;

        let second = unsafe {
            five_bit_pair(literals.add(8))
                | (five_bit_pair(literals.add(10)) << 10)
                | (five_bit_pair(literals.add(12)) << 20)
        };
        bits |= second << bit_count;
        bit_count += 30;
        while bit_count >= 8 {
            unsafe { output_pointer.add(output_position).write(bits as u8) };
            output_position += 1;
            bits >>= 8;
            bit_count -= 8;
        }
    }

    let remainder = &index_stream[full_group_count * 14..];
    if !remainder.is_empty() {
        bits |= 16u64 << bit_count;
        bit_count += 5;
        while bit_count >= 8 {
            unsafe { output_pointer.add(output_position).write(bits as u8) };
            output_position += 1;
            bits >>= 8;
            bit_count -= 8;
        }
        for &pixel in remainder {
            bits |= u64::from(pixel) << bit_count;
            bit_count += 5;
            while bit_count >= 8 {
                unsafe { output_pointer.add(output_position).write(bits as u8) };
                output_position += 1;
                bits >>= 8;
                bit_count -= 8;
            }
        }
    }

    bits |= 17u64 << bit_count;
    bit_count += 5;
    while bit_count > 0 {
        unsafe { output_pointer.add(output_position).write(bits as u8) };
        output_position += 1;
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(output_position, raw_length);
    Ok(())
}

pub(crate) fn encode_seven_bit_literal_codes(
    output: &mut Vec<u8>,
    index_stream: &[u8],
) -> Result<(), String> {
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    for literals in index_stream.chunks(62) {
        bits |= 64u64 << bit_count;
        bit_count += 7;
        while bit_count >= 8 {
            output.push(bits as u8);
            bits >>= 8;
            bit_count -= 8;
        }
        let mut groups = literals.chunks_exact(8);
        for group in &mut groups {
            let packed = u64::from(group[0])
                | (u64::from(group[1]) << 7)
                | (u64::from(group[2]) << 14)
                | (u64::from(group[3]) << 21)
                | (u64::from(group[4]) << 28)
                | (u64::from(group[5]) << 35)
                | (u64::from(group[6]) << 42)
                | (u64::from(group[7]) << 49);
            bits |= packed << bit_count;
            bit_count += 56;
            output.extend_from_slice(&bits.to_le_bytes()[..7]);
            bits >>= 56;
            bit_count -= 56;
        }
        for &pixel in groups.remainder() {
            bits |= u64::from(pixel) << bit_count;
            bit_count += 7;
            while bit_count >= 8 {
                output.push(bits as u8);
                bits >>= 8;
                bit_count -= 8;
            }
        }
    }
    bits |= 65u64 << bit_count;
    bit_count += 7;
    while bit_count > 0 {
        output.push(bits as u8);
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    Ok(())
}

pub(crate) fn encode_nine_bit_literal_codes(
    output: &mut Vec<u8>,
    index_stream: &[u8],
) -> Result<(), String> {
    let clear_count = index_stream.len().div_ceil(254);
    let raw_length = index_stream
        .len()
        .checked_add(clear_count)
        .and_then(|codes| codes.checked_add(1))
        .and_then(|codes| codes.checked_mul(9))
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?
        .div_ceil(8);
    let output_start = output.len();
    let writable_length = raw_length
        .checked_add(8)
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?;
    resize_output_uninitialized(output, output_start + writable_length);
    let output_pointer = unsafe { output.as_mut_ptr().add(output_start) };
    let mut output_position = 0usize;
    let mut bits = 0u64;
    let mut bit_count = 0usize;

    for literals in index_stream.chunks(254) {
        append_nine_bit_literal_code(
            output_pointer,
            &mut output_position,
            &mut bits,
            &mut bit_count,
            256,
        );

        let mut groups = literals.chunks_exact(7);
        for group in &mut groups {
            let packed = u64::from(group[0])
                | (u64::from(group[1]) << 9)
                | (u64::from(group[2]) << 18)
                | (u64::from(group[3]) << 27)
                | (u64::from(group[4]) << 36)
                | (u64::from(group[5]) << 45)
                | (u64::from(group[6]) << 54);
            let combined = bits | (packed << bit_count);
            let total_bits = bit_count + 63;
            if total_bits >= 64 {
                unsafe {
                    output_pointer
                        .add(output_position)
                        .cast::<u64>()
                        .write_unaligned(combined.to_le());
                };
                output_position += 8;
                bits = if bit_count == 0 {
                    0
                } else {
                    packed >> (64 - bit_count)
                };
                bit_count = total_bits - 64;
            } else {
                debug_assert_eq!(bit_count, 0);
                unsafe {
                    output_pointer
                        .add(output_position)
                        .cast::<u64>()
                        .write_unaligned(combined.to_le());
                };
                output_position += 7;
                bits = combined >> 56;
                bit_count = total_bits - 56;
            }
        }
        for &pixel in groups.remainder() {
            append_nine_bit_literal_code(
                output_pointer,
                &mut output_position,
                &mut bits,
                &mut bit_count,
                u16::from(pixel),
            );
        }
    }
    append_nine_bit_literal_code(
        output_pointer,
        &mut output_position,
        &mut bits,
        &mut bit_count,
        257,
    );
    while bit_count > 0 {
        unsafe { output_pointer.add(output_position).write(bits as u8) };
        output_position += 1;
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(output_position, raw_length);
    output.truncate(output_start + raw_length);
    Ok(())
}

#[inline(always)]
pub(crate) fn append_nine_bit_literal_code(
    output: *mut u8,
    output_position: &mut usize,
    bits: &mut u64,
    bit_count: &mut usize,
    code: u16,
) {
    *bits |= u64::from(code) << *bit_count;
    *bit_count += 9;
    while *bit_count >= 8 {
        unsafe { output.add(*output_position).write(*bits as u8) };
        *output_position += 1;
        *bits >>= 8;
        *bit_count -= 8;
    }
}

#[inline(always)]
pub(crate) fn encode_rgba_two_bit_literal_source_to<F>(
    output: &mut Vec<u8>,
    pixel_count: usize,
    mut next_index: F,
) -> Result<(), String>
where
    F: FnMut() -> Option<u8>,
{
    if pixel_count == 0 {
        return Err("Indexed pixel stream is empty".to_string());
    }
    let pair_count = pixel_count / 2;
    let raw_bit_length = pair_count * 9 + usize::from(!pixel_count.is_multiple_of(2)) * 6 + 3;
    let raw_length = raw_bit_length.div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    let output_length = output_start + 2 + block_count + raw_length;
    resize_output_uninitialized(output, output_length);
    output[output_start] = 2;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let mut remaining_pairs = pair_count;
    while remaining_pairs >= 7 {
        let mut packed = 0u64;
        for pair in 0..7 {
            let first =
                u64::from(next_index().ok_or_else(|| "Indexed pixel stream is empty".to_string())?);
            let second =
                u64::from(next_index().ok_or_else(|| "Indexed pixel stream is empty".to_string())?);
            packed |= (4 | (first << 3) | (second << 6)) << (pair * 9);
        }
        let combined = bits | (packed << bit_count);
        let total_bits = bit_count + 63;
        if total_bits >= 64 {
            writer.write_fixed_u64(combined, 8);
            bits = if bit_count == 0 {
                0
            } else {
                packed >> (64 - bit_count)
            };
            bit_count = total_bits - 64;
        } else {
            debug_assert_eq!(bit_count, 0);
            writer.write_fixed_u64(combined, 7);
            bits = combined >> 56;
            bit_count = total_bits - 56;
        }
        remaining_pairs -= 7;
    }
    while remaining_pairs > 0 {
        let first =
            u64::from(next_index().ok_or_else(|| "Indexed pixel stream is empty".to_string())?);
        let second =
            u64::from(next_index().ok_or_else(|| "Indexed pixel stream is empty".to_string())?);
        let packed = 4 | (first << 3) | (second << 6);
        let combined = bits | (packed << bit_count);
        let total_bits = bit_count + 9;
        let byte_count = total_bits / 8;
        writer.write_fixed_u64(combined, byte_count);
        bits = combined >> (byte_count * 8);
        bit_count = total_bits - byte_count * 8;
        remaining_pairs -= 1;
    }
    if !pixel_count.is_multiple_of(2) {
        let pixel =
            u64::from(next_index().ok_or_else(|| "Indexed pixel stream is empty".to_string())?);
        bits |= (4 | (pixel << 3)) << bit_count;
        bit_count += 6;
    }
    bits |= 5 << bit_count;
    bit_count += 3;
    while bit_count > 0 {
        writer.write_byte(bits as u8);
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(writer.raw_position, raw_length);

    let mut raw_offset = 0usize;
    for block in 0..block_count {
        let length = (raw_length - raw_offset).min(255);
        unsafe {
            output
                .as_mut_ptr()
                .add(output_start + 1 + block * 256)
                .write(length as u8);
        }
        raw_offset += length;
    }
    output[output_start + 1 + block_count + raw_length] = 0;
    Ok(())
}

#[inline(always)]
pub(crate) fn encode_rgba_five_bit_literal_source_to<F>(
    output: &mut Vec<u8>,
    pixel_count: usize,
    mut next_index: F,
) -> Result<(), String>
where
    F: FnMut() -> Option<u8>,
{
    if pixel_count == 0 {
        return Err("Indexed pixel stream is empty".to_string());
    }
    const CODE_BITS: usize = 5;
    const CLEAR: u16 = 16;
    const EOI: u16 = 17;
    const LITERALS_PER_CLEAR: usize = 14;
    let clear_count = pixel_count.div_ceil(LITERALS_PER_CLEAR);
    let raw_length = ((pixel_count + clear_count + 1) * CODE_BITS).div_ceil(8);
    let block_count = raw_length.div_ceil(255);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + 2 + block_count + raw_length + 8);
    output[output_start] = CODE_BITS as u8 - 1;
    let mut writer = DirectGifSubblockWriter {
        output: &mut output[output_start..],
        position: 2,
        block_remaining: 255,
        raw_position: 0,
    };
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let mut remaining = pixel_count;
    while remaining > 0 {
        bits |= u64::from(CLEAR) << bit_count;
        bit_count += CODE_BITS;
        while bit_count >= 8 {
            writer.write_byte(bits as u8);
            bits >>= 8;
            bit_count -= 8;
        }
        let literals = remaining.min(LITERALS_PER_CLEAR);
        let mut groups = literals / 8;
        while groups > 0 {
            let mut packed = 0u64;
            for index in 0..8 {
                let pixel = u64::from(
                    next_index().ok_or_else(|| "Indexed pixel stream is empty".to_string())?,
                );
                packed |= pixel << (index * CODE_BITS);
            }
            let combined = bits | (packed << bit_count);
            let total_bits = bit_count + CODE_BITS * 8;
            let byte_count = total_bits / 8;
            writer.write_fixed_u64(combined, byte_count);
            bits = combined >> (byte_count * 8);
            bit_count = total_bits - byte_count * 8;
            groups -= 1;
        }
        for _ in 0..(literals % 8) {
            let pixel =
                u64::from(next_index().ok_or_else(|| "Indexed pixel stream is empty".to_string())?);
            bits |= pixel << bit_count;
            bit_count += CODE_BITS;
            while bit_count >= 8 {
                writer.write_byte(bits as u8);
                bits >>= 8;
                bit_count -= 8;
            }
        }
        remaining -= literals;
    }
    bits |= u64::from(EOI) << bit_count;
    bit_count += CODE_BITS;
    while bit_count > 0 {
        writer.write_byte(bits as u8);
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(writer.raw_position, raw_length);

    let mut raw_offset = 0usize;
    for block in 0..block_count {
        let length = (raw_length - raw_offset).min(255);
        unsafe {
            output
                .as_mut_ptr()
                .add(output_start + 1 + block * 256)
                .write(length as u8);
        }
        raw_offset += length;
    }
    let final_length = output_start + 2 + block_count + raw_length;
    output[final_length - 1] = 0;
    output.truncate(final_length);
    Ok(())
}

#[inline(always)]
pub(crate) fn encode_rgba_two_bit_literal_frame_to(
    output: &mut Vec<u8>,
    frame: &[u8],
    mapper: &PaletteMapper<'_>,
    alpha_threshold: u8,
    exact_alpha: bool,
    exact_palette: bool,
) -> Result<(), String> {
    let pointer = frame.as_ptr();
    let pixel_count = frame.len() / 4;
    let mut offset = 0usize;
    let mut invalid_alpha = false;
    let mut invalid_palette = false;
    let mut cache_keys = [u32::MAX; 16];
    let mut cache_values = [0u8; 16];
    encode_rgba_two_bit_literal_source_to(output, pixel_count, || {
        let packed =
            unsafe { u32::from_le(std::ptr::read_unaligned(pointer.add(offset).cast::<u32>())) };
        offset += 4;
        let alpha = (packed >> 24) as u8;
        if alpha < alpha_threshold || (exact_alpha && alpha != 255) {
            invalid_alpha = true;
        }
        let rgb = packed & 0x00ff_ffff;
        let slot = (rgb as usize).wrapping_mul(2_654_435_761) & 15;
        let index = if cache_keys[slot] == rgb {
            cache_values[slot]
        } else {
            let index = if exact_palette {
                mapper.exact_index(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8)
            } else {
                Some(mapper.index_pixel(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8))
            };
            let index = index.unwrap_or_else(|| {
                invalid_palette = true;
                0
            });
            cache_keys[slot] = rgb;
            cache_values[slot] = index;
            index
        };
        Some(index)
    })?;
    if invalid_alpha || invalid_palette {
        return Err(RGBA_DELTA_FALLBACK.to_string());
    }
    Ok(())
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_rgba_two_bit_literal_rect_to(
    output: &mut Vec<u8>,
    rgba_stream: &[u8],
    canvas_width: usize,
    rect: ChangedRectU32,
    mapper: &PaletteMapper<'_>,
    alpha_threshold: u8,
    exact_alpha: bool,
    exact_palette: bool,
) -> Result<(), String> {
    let pointer = rgba_stream.as_ptr();
    let pixel_count = rect.width * rect.height;
    let row_skip = (canvas_width - rect.width) * 4;
    let mut offset = (rect.y * canvas_width + rect.x) * 4;
    let mut row_remaining = rect.width;
    let mut invalid_alpha = false;
    let mut invalid_palette = false;
    let mut cache_keys = [u32::MAX; 16];
    let mut cache_values = [0u8; 16];
    encode_rgba_two_bit_literal_source_to(output, pixel_count, || {
        let packed =
            unsafe { u32::from_le(std::ptr::read_unaligned(pointer.add(offset).cast::<u32>())) };
        offset += 4;
        let alpha = (packed >> 24) as u8;
        if alpha < alpha_threshold || (exact_alpha && alpha != 255) {
            invalid_alpha = true;
        }
        row_remaining -= 1;
        if row_remaining == 0 {
            offset += row_skip;
            row_remaining = rect.width;
        }
        let rgb = packed & 0x00ff_ffff;
        let slot = (rgb as usize).wrapping_mul(2_654_435_761) & 15;
        let index = if cache_keys[slot] == rgb {
            cache_values[slot]
        } else {
            let index = if exact_palette {
                mapper.exact_index(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8)
            } else {
                Some(mapper.index_pixel(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8))
            };
            let index = index.unwrap_or_else(|| {
                invalid_palette = true;
                0
            });
            cache_keys[slot] = rgb;
            cache_values[slot] = index;
            index
        };
        Some(index)
    })?;
    if invalid_alpha || invalid_palette {
        return Err(RGBA_DELTA_FALLBACK.to_string());
    }
    Ok(())
}

#[inline(always)]
pub(crate) fn encode_rgba_five_bit_literal_frame_to(
    output: &mut Vec<u8>,
    frame: &[u8],
    mapper: &PaletteMapper<'_>,
    alpha_threshold: u8,
    exact_alpha: bool,
    exact_palette: bool,
) -> Result<(), String> {
    let pointer = frame.as_ptr();
    let pixel_count = frame.len() / 4;
    let mut offset = 0usize;
    let mut invalid_alpha = false;
    let mut invalid_palette = false;
    // The small-palette exact path sees at most sixteen distinct RGB keys;
    // a 64-slot direct-mapped cache removes most collisions without making
    // each frame pay for a large cold cache.
    let mut cache_keys = [u32::MAX; 64];
    let mut cache_values = [0u8; 64];
    encode_rgba_five_bit_literal_source_to(output, pixel_count, || {
        let packed =
            unsafe { u32::from_le(std::ptr::read_unaligned(pointer.add(offset).cast::<u32>())) };
        offset += 4;
        let alpha = (packed >> 24) as u8;
        if alpha < alpha_threshold || (exact_alpha && alpha != 255) {
            invalid_alpha = true;
        }
        let rgb = packed & 0x00ff_ffff;
        let slot = (rgb as usize).wrapping_mul(2_654_435_761) & 63;
        let index = if cache_keys[slot] == rgb {
            cache_values[slot]
        } else {
            let index = if exact_palette {
                mapper.exact_index(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8)
            } else {
                Some(mapper.index_pixel(packed as u8, (packed >> 8) as u8, (packed >> 16) as u8))
            };
            let index = index.unwrap_or_else(|| {
                invalid_palette = true;
                0
            });
            cache_keys[slot] = rgb;
            cache_values[slot] = index;
            index
        };
        Some(index)
    })?;
    if invalid_alpha || invalid_palette {
        return Err(RGBA_DELTA_FALLBACK.to_string());
    }
    Ok(())
}

pub(crate) fn encode_two_bit_literal_codes(
    output: &mut Vec<u8>,
    index_stream: &[u8],
    _color_count: usize,
) -> Result<(), String> {
    let pair_count = index_stream.len() / 2;
    let raw_bit_length =
        pair_count * 9 + usize::from(!index_stream.len().is_multiple_of(2)) * 6 + 3;
    let raw_length = raw_bit_length.div_ceil(8);
    let output_start = output.len();
    resize_output_uninitialized(output, output_start + raw_length);
    let output_pointer = unsafe { output.as_mut_ptr().add(output_start) };
    let mut output_position = 0usize;
    let mut bits = 0u64;
    let mut bit_count = 0usize;
    let mut groups = index_stream.chunks_exact(14);
    for group in &mut groups {
        let mut packed = 0u64;
        for pair in 0..7 {
            let first = usize::from(group[pair * 2]);
            let second = usize::from(group[pair * 2 + 1]);
            let reset_group = 4 | (first << 3) | (second << 6);
            packed |= (reset_group as u64) << (pair * 9);
        }

        let combined = bits | (packed << bit_count);
        let total_bits = bit_count + 63;
        if total_bits >= 64 {
            let bytes = combined.to_le_bytes();
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    output_pointer.add(output_position),
                    8,
                )
            };
            output_position += 8;
            bits = packed >> (64 - bit_count);
            bit_count = total_bits - 64;
        } else {
            let bytes = combined.to_le_bytes();
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    output_pointer.add(output_position),
                    7,
                )
            };
            output_position += 7;
            bits = combined >> 56;
            bit_count = total_bits - 56;
        }
    }

    let remainder = groups.remainder();
    let mut index = 0usize;
    while index + 1 < remainder.len() {
        let first = usize::from(remainder[index]);
        let second = usize::from(remainder[index + 1]);
        // Each reset group is CLEAR, literal, literal: three 3-bit codes.
        bits |= ((4 | (first << 3) | (second << 6)) as u64) << bit_count;
        bit_count += 9;
        if bit_count >= 32 {
            let bytes = (bits as u32).to_le_bytes();
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    output_pointer.add(output_position),
                    4,
                )
            };
            output_position += 4;
            bits >>= 32;
            bit_count -= 32;
        }
        index += 2;
    }
    if index < remainder.len() {
        let pixel = usize::from(remainder[index]);
        bits |= ((4 | (pixel << 3)) as u64) << bit_count;
        bit_count += 6;
    }
    // EOI is code 5 at the unchanged 3-bit width.
    bits |= 5u64 << bit_count;
    bit_count += 3;
    while bit_count > 0 {
        unsafe { output_pointer.add(output_position).write(bits as u8) };
        output_position += 1;
        bits >>= 8;
        bit_count = bit_count.saturating_sub(8);
    }
    debug_assert_eq!(output_position, raw_length);
    Ok(())
}

#[inline]
pub(crate) fn emit_raw_lzw_code(
    output: &mut Vec<u8>,
    bits: &mut u64,
    bit_count: &mut usize,
    code_size: usize,
    code: usize,
) {
    *bits |= (code as u64) << *bit_count;
    *bit_count += code_size;
    if *bit_count >= 32 {
        output.extend_from_slice(&(*bits as u32).to_le_bytes());
        *bits >>= 32;
        *bit_count -= 32;
    }
}

pub(crate) fn literal_lzw_block_size(
    pixel_count: usize,
    min_code_size: u8,
) -> Result<usize, String> {
    let code_size = usize::from(min_code_size) + 1;
    let literals_per_clear = (1usize << min_code_size) - 2;
    literal_lzw_block_size_with_interval(pixel_count, code_size, literals_per_clear)
}

pub(crate) fn literal_lzw_block_size_with_interval(
    pixel_count: usize,
    code_size: usize,
    literals_per_clear: usize,
) -> Result<usize, String> {
    let clear_count = pixel_count.div_ceil(literals_per_clear);
    let code_count = pixel_count
        .checked_add(clear_count)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    let compressed_length = code_count
        .checked_mul(code_size)
        .and_then(|bits| bits.checked_add(7))
        .map(|bits| bits / 8)
        .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    let lzw_length = compressed_length
        .checked_add(compressed_length.div_ceil(255))
        .and_then(|length| length.checked_add(2))
        .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    Ok(lzw_length)
}
