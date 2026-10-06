//! Full-dictionary LZW, the image coder of the smallest output mode.
//!
//! Greedy LZW over GIF's 4,096-entry dictionary. Strings live in an
//! open-addressed table whose entries pack the 20-bit key
//! `prefix code << 8 | pixel` above the 12-bit code, so each probe is one
//! load. The writer clears the dictionary when it is full; keeping a full
//! dictionary instead made the benchmark animation larger.

/// Most slots the string table uses: twice the dictionary size.
pub(crate) const DICTIONARY_SLOTS: usize = 1 << 13;
const LAST_CODE: u32 = 4095;

/// Append one image-data block coded with full-dictionary LZW: the minimum
/// code size, sub-blocks, and the block terminator. `table` holds at least
/// `DICTIONARY_SLOTS` entries of scratch.
pub(crate) fn write_dictionary_lzw_image(
    output: &mut Vec<u8>,
    pixels: &[u8],
    minimum_code_size: u8,
    table: &mut [u32],
) {
    debug_assert!((2..=8).contains(&minimum_code_size));
    debug_assert!(table.len() >= DICTIONARY_SLOTS);
    debug_assert!(!pixels.is_empty());
    let clear = 1u32 << minimum_code_size;
    let first_entry = clear + 2;
    // A frame can add at most one entry per pixel, so small frames use a
    // smaller table and clear less memory.
    let entries = pixels.len().min((LAST_CODE + 1 - first_entry) as usize);
    let slot_bits = (entries * 2)
        .next_power_of_two()
        .trailing_zeros()
        .clamp(8, 13);
    let table = &mut table[..1 << slot_bits];
    let slot_mask = table.len() - 1;

    output.push(minimum_code_size);
    let start = output.len();
    // Every code covers at least one pixel and is at most 12 bits wide; add
    // one clear per full dictionary, the first clear, and the end code.
    let max_codes = pixels.len() + pixels.len() / (LAST_CODE + 1 - first_entry) as usize + 3;
    let max_bytes = (max_codes * 12).div_ceil(8);
    output.reserve(max_bytes + max_bytes.div_ceil(255) + 1 + 8);
    let base = output.as_mut_ptr();
    let mut position = start;
    let mut bits = 0u64;
    let mut bit_count = 0u32;
    let mut width = u32::from(minimum_code_size) + 1;
    // Store all pending bits and advance past each complete byte. The
    // reservation above leaves eight writable bytes past every position.
    let mut emit = |code: u32, width: u32| {
        bits |= u64::from(code) << bit_count;
        bit_count += width;
        unsafe { std::ptr::write_unaligned(base.add(position).cast::<u64>(), bits.to_le()) };
        let bytes = bit_count >> 3;
        position += bytes as usize;
        bits >>= bytes * 8;
        bit_count &= 7;
    };

    table.fill(0);
    emit(clear, width);
    let mut next = first_entry;
    let mut prefix = u32::from(pixels[0]);
    'pixels: for &pixel in &pixels[1..] {
        let key = (prefix << 8) | u32::from(pixel);
        let mut slot = (key.wrapping_mul(0x9E37_79B1) >> (32 - slot_bits)) as usize;
        loop {
            // SAFETY: `slot` is masked to the table length.
            let entry = unsafe { *table.get_unchecked(slot) };
            if entry == 0 {
                break;
            }
            if entry >> 12 == key {
                prefix = entry & 0xfff;
                continue 'pixels;
            }
            slot = (slot + 1) & slot_mask;
        }
        emit(prefix, width);
        if next <= LAST_CODE {
            // Codes start above the clear code, so a stored entry is never 0.
            table[slot] = (key << 12) | next;
            next += 1;
            // The decoder adds each entry one code later than the writer, so
            // it widens its codes when `next` passes the width's last code.
            if next > 1 << width && width < 12 {
                width += 1;
            }
        } else {
            emit(clear, width);
            table.fill(0);
            next = first_entry;
            width = u32::from(minimum_code_size) + 1;
        }
        prefix = u32::from(pixel);
    }
    emit(prefix, width);
    emit(clear + 1, width);
    if bit_count > 0 {
        position += 1;
    }
    frame_sub_blocks(output, start, position - start);
}

/// Split the raw code stream written at `output[start..start + raw_length]`
/// into 255-byte sub-blocks followed by the block terminator. The caller has
/// reserved room for one length byte per sub-block and the terminator.
pub(crate) fn frame_sub_blocks(output: &mut Vec<u8>, start: usize, raw_length: usize) {
    let block_count = raw_length.div_ceil(255);
    let final_length = start + raw_length + block_count + 1;
    debug_assert!(final_length <= output.capacity());
    // SAFETY: the reserved capacity holds the raw stream, which is moved
    // backwards into sub-blocks before any byte past it is read.
    unsafe { output.set_len(final_length) };
    for block in (0..block_count).rev() {
        let source = start + block * 255;
        let length = (raw_length - block * 255).min(255);
        let destination = start + block * 256;
        output.copy_within(source..source + length, destination + 1);
        output[destination] = length as u8;
    }
    output[final_length - 1] = 0;
}
