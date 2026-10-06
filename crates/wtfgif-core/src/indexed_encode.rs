//! Indexed-frame GIF encoding, frame headers, and delta frames.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_indexed_literal_gif_inner(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    transparent_index: Option<u8>,
) -> Result<Vec<u8>, String> {
    encode_indexed_literal_gif_inner_with_output(
        Vec::new(),
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        delays,
        loop_count,
        transparent_index,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_indexed_literal_gif_inner_with_output(
    output: Vec<u8>,
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    transparent_index: Option<u8>,
) -> Result<Vec<u8>, String> {
    encode_indexed_literal_gif_inner_with_output_impl::<true>(
        output,
        index_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        delays,
        loop_count,
        transparent_index,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_indexed_literal_gif_inner_with_output_impl<const VALIDATE: bool>(
    mut output: Vec<u8>,
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    transparent_index: Option<u8>,
) -> Result<Vec<u8>, String> {
    if frame_count == 0 && index_stream.is_empty() && palette_rgb.is_empty() {
        return Ok(output);
    }
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

    let color_count = checked_palette_color_count(palette_rgb.len())?;
    let min_code_size = (log2_pow2(color_count) as u8).max(2);
    let frame_len = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| "Frame size overflow".to_string())?;
    let expected_len = frame_len
        .checked_mul(frame_count)
        .ok_or_else(|| "Frame stream overflow".to_string())?;
    if index_stream.len() != expected_len {
        return Err("Indexed frame stream length does not match dimensions".to_string());
    }

    #[cfg(not(target_arch = "wasm32"))]
    // Native thread setup and per-frame scratch allocation are slower than the
    // direct writer for ordinary 128x128 animations. Parallelism starts to
    // repay that fixed cost only once the indexed input is around one MiB.
    if VALIDATE && min_code_size > 2 && frame_count >= 8 && expected_len >= 1_000_000 {
        return encode_indexed_literal_gif_parallel_native(
            index_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delays,
            loop_count,
            transparent_index,
            color_count,
            min_code_size,
            frame_len,
        );
    }

    let palette_bytes = color_count
        .checked_mul(3)
        .ok_or_else(|| "Palette size overflow".to_string())?;
    let lzw_length = literal_lzw_block_size(frame_len, min_code_size)?;
    let frame_capacity = (0..frame_count).try_fold(0usize, |capacity, frame_index| {
        let graphic_control_length =
            usize::from(delays.get(frame_index) != 0 || transparent_index.is_some()) * 8;
        capacity
            .checked_add(10)
            .and_then(|length| length.checked_add(graphic_control_length))
            .and_then(|length| length.checked_add(lzw_length))
            .ok_or_else(|| "Encoded GIF size overflow".to_string())
    })?;
    let output_capacity =
        13 + palette_bytes + usize::from(loop_count >= 0) * 19 + frame_capacity + 1;
    output.clear();
    if output.capacity() < output_capacity {
        output.reserve(output_capacity - output.capacity());
    }
    write_indexed_gif_header(&mut output, width, height, palette_rgb, color_count);
    write_loop_extension(&mut output, loop_count);
    for (frame_index, frame) in index_stream.chunks_exact(frame_len).enumerate() {
        write_indexed_gif_frame_header(
            &mut output,
            0,
            0,
            width,
            height,
            delays.get(frame_index),
            transparent_index,
            if transparent_index.is_some() { 2 } else { 0 },
        );
        if VALIDATE {
            encode_indexed_literal_lzw_direct_to(&mut output, frame, min_code_size, color_count)?;
        } else {
            encode_indexed_literal_lzw_direct_to_unchecked(
                &mut output,
                frame,
                min_code_size,
                color_count,
            )?;
        }
    }
    output.push(0x3b);
    Ok(output)
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_indexed_literal_gif_parallel_native(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    transparent_index: Option<u8>,
    color_count: usize,
    min_code_size: u8,
    frame_len: usize,
) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    write_indexed_gif_header(&mut output, width, height, palette_rgb, color_count);
    write_loop_extension(&mut output, loop_count);
    let lzw_length = literal_lzw_block_size(frame_len, min_code_size)?;
    let mut output_length = output.len();
    let mut frame_layout = Vec::with_capacity(frame_count);
    for frame_index in 0..frame_count {
        let gce_length =
            usize::from(delays.get(frame_index) != 0 || transparent_index.is_some()) * 8;
        let frame_length = 10usize
            .checked_add(gce_length)
            .and_then(|length| length.checked_add(lzw_length))
            .ok_or_else(|| "Encoded GIF size overflow".to_string())?;
        frame_layout.push((output_length, frame_length));
        output_length = output_length
            .checked_add(frame_length)
            .ok_or_else(|| "Encoded GIF size overflow".to_string())?;
    }
    output_length = output_length
        .checked_add(1)
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?;
    output.resize(output_length, 0);
    let output_address = output.as_mut_ptr() as usize;
    let frame_results: Vec<std::sync::OnceLock<Result<(), String>>> = (0..frame_count)
        .map(|_| std::sync::OnceLock::new())
        .collect();
    let next_frame = std::sync::atomic::AtomicUsize::new(0);
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let thread_count = available_threads.min(frame_count);

    let joined = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..thread_count)
            .map(|_| {
                let next_frame = &next_frame;
                let frame_results = &frame_results;
                let frame_layout = &frame_layout;
                scope.spawn(move || loop {
                    let frame_index = next_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if frame_index >= frame_count {
                        break;
                    }
                    let result = (|| {
                        let frame =
                            &index_stream[frame_index * frame_len..(frame_index + 1) * frame_len];
                        let (destination, expected_length) = frame_layout[frame_index];
                        let mut chunk = Vec::with_capacity(expected_length);
                        write_indexed_gif_frame_header(
                            &mut chunk,
                            0,
                            0,
                            width,
                            height,
                            delays.get(frame_index),
                            transparent_index,
                            if transparent_index.is_some() { 2 } else { 0 },
                        );
                        let mut compressed_scratch = Vec::new();
                        encode_indexed_literal_lzw_to(
                            &mut chunk,
                            frame,
                            min_code_size,
                            color_count,
                            &mut compressed_scratch,
                        )?;
                        if chunk.len() != expected_length {
                            return Err("Predicted encoded frame size differs".to_string());
                        }
                        unsafe {
                            std::ptr::copy_nonoverlapping(
                                chunk.as_ptr(),
                                (output_address as *mut u8).add(destination),
                                expected_length,
                            );
                        }
                        Ok(())
                    })();
                    let _ = frame_results[frame_index].set(result);
                })
            })
            .collect();
        handles.into_iter().all(|handle| handle.join().is_ok())
    });
    if !joined {
        return Err("Parallel GIF encoder panicked".to_string());
    }
    for frame_result in frame_results {
        frame_result
            .into_inner()
            .ok_or_else(|| "Parallel GIF encoder skipped a frame".to_string())??;
    }
    output[output_length - 1] = 0x3b;
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_indexed_literal_delta_gif_inner(
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    transparent_index: Option<u8>,
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

    let color_count = checked_palette_color_count(palette_rgb.len())?;
    let min_code_size = (log2_pow2(color_count) as u8).max(2);
    let canvas_width = usize::from(width);
    let canvas_height = usize::from(height);
    let frame_len = canvas_width
        .checked_mul(canvas_height)
        .ok_or_else(|| "Frame size overflow".to_string())?;
    let expected_len = frame_len
        .checked_mul(frame_count)
        .ok_or_else(|| "Frame stream overflow".to_string())?;
    if index_stream.len() != expected_len {
        return Err("Indexed frame stream length does not match dimensions".to_string());
    }

    let palette_bytes = color_count
        .checked_mul(3)
        .ok_or_else(|| "Palette size overflow".to_string())?;
    let estimated_frame_bytes = frame_len / 8 + 48;
    let mut output = Vec::with_capacity(
        13 + palette_bytes + 20 + frame_count.saturating_mul(estimated_frame_bytes) + 1,
    );
    let mut rect_scratch = Vec::new();

    write_indexed_gif_header(&mut output, width, height, palette_rgb, color_count);
    write_loop_extension(&mut output, loop_count);

    let mut previous_frame: Option<&[u8]> = None;
    for (frame_index, frame) in index_stream.chunks_exact(frame_len).enumerate() {
        let delay = delays.get(frame_index);
        if let Some(previous) = previous_frame {
            if let Some(rect) = find_changed_rect_u8(previous, frame, canvas_width, canvas_height) {
                write_indexed_gif_frame_header(
                    &mut output,
                    rect.x as u16,
                    rect.y as u16,
                    rect.width as u16,
                    rect.height as u16,
                    delay,
                    transparent_index,
                    0,
                );
                let rect_length = rect.width * rect.height;
                rect_scratch.clear();
                if rect_scratch.capacity() < rect_length {
                    rect_scratch.reserve(rect_length - rect_scratch.capacity());
                }
                for row in 0..rect.height {
                    let start = (rect.y + row) * canvas_width + rect.x;
                    rect_scratch.extend_from_slice(&frame[start..start + rect.width]);
                }
                encode_indexed_literal_lzw_direct_to(
                    &mut output,
                    &rect_scratch,
                    min_code_size,
                    color_count,
                )?;
            } else {
                write_indexed_gif_frame_header(
                    &mut output,
                    0,
                    0,
                    1,
                    1,
                    delay,
                    transparent_index,
                    0,
                );
                // A no-op frame has one pixel of payload. The literal stream
                // is the same minimal CLEAR/pixel/EOI code sequence without
                // paying for a dictionary reset and table lookup.
                encode_indexed_literal_lzw_direct_to(
                    &mut output,
                    &frame[..1],
                    min_code_size,
                    color_count,
                )?;
            }
        } else {
            write_indexed_gif_frame_header(
                &mut output,
                0,
                0,
                width,
                height,
                delay,
                transparent_index,
                0,
            );
            encode_indexed_literal_lzw_direct_to(&mut output, frame, min_code_size, color_count)?;
        }
        previous_frame = Some(frame);
    }

    output.push(0x3b);
    Ok(output)
}

pub(crate) fn write_indexed_gif_header(
    output: &mut Vec<u8>,
    width: u16,
    height: u16,
    palette_rgb: &[u32],
    color_count: usize,
) {
    let color_table_size_bits = (log2_pow2(color_count) as u8 - 1) & 7;

    output.extend_from_slice(b"GIF89a");
    push_u16_le(output, width);
    push_u16_le(output, height);
    output.push(0x80 | color_table_size_bits);
    output.push(0);
    output.push(0);

    for index in 0..color_count {
        let rgb = palette_rgb.get(index).copied().unwrap_or(0);
        output.push(((rgb >> 16) & 0xff) as u8);
        output.push(((rgb >> 8) & 0xff) as u8);
        output.push((rgb & 0xff) as u8);
    }
}

pub(crate) fn write_loop_extension(output: &mut Vec<u8>, loop_count: i32) {
    if loop_count >= 0 {
        output.extend_from_slice(&[
            0x21, 0xff, 0x0b, b'N', b'E', b'T', b'S', b'C', b'A', b'P', b'E', b'2', b'.', b'0',
            0x03, 0x01,
        ]);
        push_u16_le(output, loop_count as u16);
        output.push(0);
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_indexed_gif_frame_header(
    output: &mut Vec<u8>,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    delay: u16,
    transparent_index: Option<u8>,
    disposal: u8,
) {
    if delay != 0 || transparent_index.is_some() || disposal != 0 {
        output.extend_from_slice(&[
            0x21,
            0xf9,
            0x04,
            (disposal << 2)
                | if transparent_index.is_some() {
                    0x01
                } else {
                    0x00
                },
        ]);
        push_u16_le(output, delay);
        output.push(transparent_index.unwrap_or(0));
        output.push(0x00);
    }

    output.push(0x2c);
    push_u16_le(output, x);
    push_u16_le(output, y);
    push_u16_le(output, width);
    push_u16_le(output, height);
    output.push(0);
}

pub(crate) fn checked_palette_color_count(palette_len: usize) -> Result<usize, String> {
    if palette_len == 0 || palette_len > 256 {
        return Err("Invalid palette size (must be 1..256)".to_string());
    }
    let mut color_count = palette_len.next_power_of_two();
    if color_count < 2 {
        color_count = 2;
    }
    if color_count > 256 {
        return Err("Invalid palette size (must be 1..256)".to_string());
    }
    Ok(color_count)
}

pub(crate) fn log2_pow2(value: usize) -> usize {
    usize::BITS as usize - 1 - value.leading_zeros() as usize
}

#[inline]
pub(crate) fn push_u16_le(output: &mut Vec<u8>, value: u16) {
    output.push((value & 0xff) as u8);
    output.push((value >> 8) as u8);
}
