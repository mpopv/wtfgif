//! Re-encoding and remuxing existing GIFs with literal LZW codes.

use super::*;

pub(crate) fn reencode_gif_literal_sequential(
    data: &[u8],
    metadata: &GifMetadata,
    loop_count: i32,
    decoded_size: usize,
) -> Result<Vec<u8>, String> {
    if loop_count < -1 || loop_count > i32::from(u16::MAX) {
        return Err("Loop count invalid".to_string());
    }

    let mut output = write_reencoded_gif_header(
        data,
        metadata,
        loop_count,
        data.len()
            .saturating_add(decoded_size)
            .saturating_add(metadata.frames.len() * 32),
    )?;
    let mut image_data = Vec::new();
    let mut lzw_scratch = LzwStackScratch::default();
    let mut indices = Vec::new();
    let mut compressed_scratch = Vec::new();

    for frame in &metadata.frames {
        decode_frame_indices_reusing_output(
            data,
            frame,
            &mut image_data,
            &mut lzw_scratch,
            &mut indices,
        )?;
        write_reencoded_frame_literal_to(
            &mut output,
            data,
            metadata,
            frame,
            &indices,
            &mut compressed_scratch,
        )?;
    }

    output.push(0x3b);
    Ok(output)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn reencode_gif_literal_parallel_native(
    data: &[u8],
    metadata: &GifMetadata,
    loop_count: i32,
) -> Result<Vec<u8>, String> {
    let total_frame_pixels = metadata.frames.iter().try_fold(0usize, |total, frame| {
        usize::from(frame.width)
            .checked_mul(usize::from(frame.height))
            .and_then(|pixels| total.checked_add(pixels))
            .ok_or_else(|| "Decoded frame size overflow".to_string())
    })?;
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let thread_cap = if total_frame_pixels < 100_000 || metadata.frames.len() <= 12 {
        6
    } else {
        usize::MAX
    };
    let thread_count = available_threads.min(metadata.frames.len()).min(thread_cap);
    if thread_count <= 1 || metadata.frames.len() < 8 || total_frame_pixels < 30_000 {
        return reencode_gif_literal_sequential(data, metadata, loop_count, total_frame_pixels);
    }

    let mut output = write_reencoded_gif_header(data, metadata, loop_count, 0)?;
    let mut output_length = output.len();
    let mut frame_layout = Vec::with_capacity(metadata.frames.len());
    for frame in &metadata.frames {
        let frame_length = reencoded_frame_literal_size(metadata, frame)?;
        frame_layout.push((output_length, frame_length));
        output_length = output_length
            .checked_add(frame_length)
            .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    }
    output_length = output_length
        .checked_add(1)
        .ok_or_else(|| "Reencoded GIF size overflow".to_string())?;
    output.resize(output_length, 0);
    fill_reencoded_gif_literal_parallel_native(
        data,
        metadata,
        &mut output,
        &frame_layout,
        output_length,
        thread_count,
    )?;
    Ok(output)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn reencode_gif_literal_worker(
    data: &[u8],
    metadata: &GifMetadata,
    frame_layout: &[(usize, usize)],
    output_address: usize,
    next_frame: &std::sync::atomic::AtomicUsize,
) -> Result<(), String> {
    let mut image_data = Vec::new();
    let mut lzw_scratch = LzwStackScratch::default();
    let mut indices = Vec::new();
    let mut compressed_scratch = Vec::new();
    let mut frame_output = Vec::new();
    reencode_gif_literal_worker_with_scratch(
        data,
        metadata,
        frame_layout,
        output_address,
        next_frame,
        &mut image_data,
        &mut lzw_scratch,
        &mut indices,
        &mut compressed_scratch,
        &mut frame_output,
    )
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn reencode_gif_literal_worker_with_scratch(
    data: &[u8],
    metadata: &GifMetadata,
    frame_layout: &[(usize, usize)],
    output_address: usize,
    next_frame: &std::sync::atomic::AtomicUsize,
    image_data: &mut Vec<u8>,
    lzw_scratch: &mut LzwStackScratch,
    indices: &mut Vec<u8>,
    compressed_scratch: &mut Vec<u8>,
    frame_output: &mut Vec<u8>,
) -> Result<(), String> {
    loop {
        let frame_index = next_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(frame) = metadata.frames.get(frame_index) else {
            return Ok(());
        };
        let (destination, expected_length) = frame_layout[frame_index];
        decode_frame_indices_reusing_output(data, frame, image_data, lzw_scratch, indices)?;
        frame_output.clear();
        if frame_output.capacity() < expected_length {
            frame_output.reserve(expected_length);
        }
        write_reencoded_frame_literal_to(
            frame_output,
            data,
            metadata,
            frame,
            indices,
            compressed_scratch,
        )?;
        if frame_output.len() != expected_length {
            return Err("Predicted reencoded frame size differs".to_string());
        }
        // Each worker receives a distinct frame range from the atomic index.
        // The backing output vector is fully sized before workers start and
        // is never resized while these disjoint copies run.
        unsafe {
            std::ptr::copy_nonoverlapping(
                frame_output.as_ptr(),
                (output_address as *mut u8).add(destination),
                expected_length,
            );
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
pub(crate) struct ReencodeDispatchContext<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) metadata: &'a GifMetadata,
    pub(crate) frame_layout: &'a [(usize, usize)],
    pub(crate) output_address: usize,
    pub(crate) next_frame: std::sync::atomic::AtomicUsize,
    pub(crate) error: std::sync::Mutex<Option<String>>,
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
unsafe extern "C" fn reencode_dispatch_worker(context: *mut std::ffi::c_void, _iteration: usize) {
    let context = &*(context.cast::<ReencodeDispatchContext<'_>>());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        reencode_gif_literal_worker(
            context.data,
            context.metadata,
            context.frame_layout,
            context.output_address,
            &context.next_frame,
        )
    }));
    let error = match result {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error,
        Err(_) => "Parallel GIF transcoder panicked".to_string(),
    };
    if let Ok(mut stored_error) = context.error.lock() {
        if stored_error.is_none() {
            *stored_error = Some(error);
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
pub(crate) fn fill_reencoded_gif_literal_dispatch(
    data: &[u8],
    metadata: &GifMetadata,
    output_address: usize,
    frame_layout: &[(usize, usize)],
    thread_count: usize,
) -> Result<(), String> {
    let mut context = ReencodeDispatchContext {
        data,
        metadata,
        frame_layout,
        output_address,
        next_frame: std::sync::atomic::AtomicUsize::new(0),
        error: std::sync::Mutex::new(None),
    };
    unsafe {
        let queue = dispatch_get_global_queue(0, 0);
        if queue.is_null() {
            return Err("Could not acquire the system worker queue".to_string());
        }
        dispatch_apply_f(
            thread_count,
            queue,
            (&mut context as *mut ReencodeDispatchContext<'_>).cast(),
            reencode_dispatch_worker,
        );
    }
    context
        .error
        .into_inner()
        .map_err(|_| "Parallel GIF transcoder error lock was poisoned".to_string())?
        .map_or(Ok(()), Err)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn fill_reencoded_gif_literal_parallel_native(
    data: &[u8],
    metadata: &GifMetadata,
    output: &mut Vec<u8>,
    frame_layout: &[(usize, usize)],
    output_length: usize,
    thread_count: usize,
) -> Result<(), String> {
    let output_address = output.as_mut_ptr() as usize;
    #[cfg(target_os = "macos")]
    {
        fill_reencoded_gif_literal_dispatch(
            data,
            metadata,
            output_address,
            frame_layout,
            thread_count,
        )?;
        output[output_length - 1] = 0x3b;
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let next_frame = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..thread_count)
                .map(|_| {
                    let next_frame = &next_frame;
                    let frame_layout = &frame_layout;
                    scope.spawn(move || -> Result<(), String> {
                        reencode_gif_literal_worker(
                            data,
                            metadata,
                            frame_layout,
                            output_address,
                            next_frame,
                        )
                    })
                })
                .collect();
            for handle in handles {
                handle
                    .join()
                    .map_err(|_| "Parallel GIF transcoder panicked".to_string())??;
            }
            Ok::<_, String>(())
        })?;
        output[output_length - 1] = 0x3b;
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn reencoded_frame_literal_size(
    metadata: &GifMetadata,
    frame: &FrameMetadata,
) -> Result<usize, String> {
    let color_count = checked_palette_color_count(frame.palette_size)?;
    let pixel_count = usize::from(frame.width)
        .checked_mul(usize::from(frame.height))
        .ok_or_else(|| "Frame size overflow".to_string())?;
    let min_code_size = (log2_pow2(color_count) as u8).max(2);
    let lzw_length = literal_lzw_block_size(pixel_count, min_code_size)?;
    let gce_length =
        usize::from(frame.delay != 0 || frame.disposal != 0 || frame.transparent_index.is_some())
            * 8;
    let uses_global_palette = metadata.global_palette_offset == Some(frame.palette_offset)
        && metadata.global_palette_size == frame.palette_size;
    let palette_length = if uses_global_palette {
        0
    } else {
        color_count
            .checked_mul(3)
            .ok_or_else(|| "Palette size overflow".to_string())?
    };
    gce_length
        .checked_add(10)
        .and_then(|length| length.checked_add(palette_length))
        .and_then(|length| length.checked_add(lzw_length))
        .ok_or_else(|| "Reencoded GIF size overflow".to_string())
}

pub(crate) fn write_reencoded_gif_header(
    data: &[u8],
    metadata: &GifMetadata,
    loop_count: i32,
    capacity: usize,
) -> Result<Vec<u8>, String> {
    if loop_count < -1 || loop_count > i32::from(u16::MAX) {
        return Err("Loop count invalid".to_string());
    }
    let mut output = Vec::with_capacity(capacity);
    write_reencoded_gif_header_to(&mut output, data, metadata, loop_count)?;
    Ok(output)
}

pub(crate) fn write_reencoded_gif_header_to(
    output: &mut Vec<u8>,
    data: &[u8],
    metadata: &GifMetadata,
    loop_count: i32,
) -> Result<(), String> {
    if loop_count < -1 || loop_count > i32::from(u16::MAX) {
        return Err("Loop count invalid".to_string());
    }
    output.extend_from_slice(b"GIF89a");
    push_u16_le(output, metadata.width);
    push_u16_le(output, metadata.height);
    output.extend_from_slice(&data[10..13]);
    if let Some(global_palette_offset) = metadata.global_palette_offset {
        let global_palette_length = metadata
            .global_palette_size
            .checked_mul(3)
            .ok_or_else(|| "Global palette size overflow".to_string())?;
        let global_palette_end = checked_add(
            global_palette_offset,
            global_palette_length,
            data.len(),
            "global color table",
        )?;
        output.extend_from_slice(&data[global_palette_offset..global_palette_end]);
    }
    write_loop_extension(output, loop_count);
    Ok(())
}

pub(crate) fn write_reencoded_frame_literal_to(
    output: &mut Vec<u8>,
    data: &[u8],
    metadata: &GifMetadata,
    frame: &FrameMetadata,
    indices: &[u8],
    compressed_scratch: &mut Vec<u8>,
) -> Result<(), String> {
    if frame.palette_size == 0 {
        return Err("GIF frame has no color table".to_string());
    }
    let color_count = checked_palette_color_count(frame.palette_size)?;
    let palette_length = color_count
        .checked_mul(3)
        .ok_or_else(|| "Palette size overflow".to_string())?;
    let palette_end = checked_add(
        frame.palette_offset,
        palette_length,
        data.len(),
        "frame color table",
    )?;
    let expected_indices = usize::from(frame.width)
        .checked_mul(usize::from(frame.height))
        .ok_or_else(|| "Frame size overflow".to_string())?;
    if indices.len() != expected_indices {
        return Err("Decoded frame length does not match dimensions".to_string());
    }

    let uses_global_palette = metadata.global_palette_offset == Some(frame.palette_offset)
        && metadata.global_palette_size == frame.palette_size;
    write_reencoded_frame_header(output, frame, color_count, !uses_global_palette);
    if !uses_global_palette {
        output.extend_from_slice(&data[frame.palette_offset..palette_end]);
    }
    let min_code_size = (log2_pow2(color_count) as u8).max(2);
    let direct_output = indices.len() >= 100_000;
    if direct_output {
        encode_indexed_literal_lzw_direct_to(output, indices, min_code_size, color_count)
    } else {
        encode_indexed_literal_lzw_to(
            output,
            indices,
            min_code_size,
            color_count,
            compressed_scratch,
        )
    }
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn remux_gif_pixel_perfect_inner(
    data: &[u8],
    metadata: &GifMetadata,
    loop_count: i32,
) -> Result<Vec<u8>, String> {
    let mut output = write_reencoded_gif_header(data, metadata, loop_count, data.len())?;
    for frame in &metadata.frames {
        if frame.palette_size == 0 {
            return Err("GIF frame has no color table".to_string());
        }
        let color_count = checked_palette_color_count(frame.palette_size)?;
        let uses_global_palette = metadata.global_palette_offset == Some(frame.palette_offset)
            && metadata.global_palette_size == frame.palette_size;
        write_reencoded_frame_header(&mut output, frame, color_count, !uses_global_palette);
        if frame.interlaced {
            let packed_offset = output
                .len()
                .checked_sub(1)
                .ok_or_else(|| "Missing image descriptor".to_string())?;
            output[packed_offset] |= 0x40;
        }
        if !uses_global_palette {
            let palette_length = color_count
                .checked_mul(3)
                .ok_or_else(|| "Palette size overflow".to_string())?;
            let palette_end = checked_add(
                frame.palette_offset,
                palette_length,
                data.len(),
                "frame color table",
            )?;
            output.extend_from_slice(&data[frame.palette_offset..palette_end]);
        }
        let data_end = checked_add(
            frame.data_offset,
            frame.data_length,
            data.len(),
            "frame image data",
        )?;
        output.extend_from_slice(&data[frame.data_offset..data_end]);
    }
    output.push(0x3b);
    Ok(output)
}

pub(crate) fn write_reencoded_frame_header(
    output: &mut Vec<u8>,
    frame: &FrameMetadata,
    color_count: usize,
    write_local_palette: bool,
) {
    if frame.delay != 0 || frame.disposal != 0 || frame.transparent_index.is_some() {
        output.extend_from_slice(&[
            0x21,
            0xf9,
            0x04,
            ((frame.disposal & 0x07) << 2)
                | if frame.transparent_index.is_some() {
                    0x01
                } else {
                    0x00
                },
        ]);
        push_u16_le(output, frame.delay);
        output.push(frame.transparent_index.unwrap_or(0));
        output.push(0);
    }

    output.push(0x2c);
    push_u16_le(output, frame.x);
    push_u16_le(output, frame.y);
    push_u16_le(output, frame.width);
    push_u16_le(output, frame.height);
    let color_table_size_bits = (log2_pow2(color_count) as u8 - 1) & 7;
    output.push(if write_local_palette {
        0x80 | color_table_size_bits
    } else {
        0
    });
}
