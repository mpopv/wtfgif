//! Composited playback frames: disposal, canvas blits, and delta streams.

use super::*;

#[cfg(not(feature = "encode-only"))]
pub(crate) fn prepare_composited_frames_inner(
    data: &[u8],
    metadata: &GifMetadata,
    requested_frames: &[u8],
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    prepare_composited_frames_selected(data, metadata, Some(requested_frames), format)
}

pub(crate) fn prepare_all_composited_frames_inner(
    data: &[u8],
    metadata: &GifMetadata,
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    prepare_composited_frames_selected(data, metadata, None, format)
}

pub(crate) fn prepare_composited_frames_selected(
    data: &[u8],
    metadata: &GifMetadata,
    requested_frames: Option<&[u8]>,
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    let all_requested = requested_frames.is_none()
        || requested_frames.is_some_and(|frames| {
            frames.len() == metadata.frames.len() && frames.iter().all(|flag| *flag != 0)
        });
    if all_requested && all_frames_full_opaque(metadata) {
        return decode_full_opaque_frames_direct(data, metadata, format);
    }
    if let Some(requested_frames) = requested_frames {
        if requested_frames.len() > metadata.frames.len() {
            return Err("Requested frame flags exceed frame count".to_string());
        }
    }

    let canvas_width = usize::from(metadata.width);
    let canvas_height = usize::from(metadata.height);
    let canvas_pixels = canvas_width
        .checked_mul(canvas_height)
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let requested_count = requested_frames.map_or(metadata.frames.len(), |requested_frames| {
        requested_frames.iter().filter(|flag| **flag != 0).count()
    });
    let output_pixels = requested_count
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    let mut output = Vec::with_capacity(output_pixels);
    let mut canvas = vec![0u32; canvas_pixels];
    let mut image_data = Vec::new();
    let mut lzw_scratch = LzwStackScratch::default();
    let mut indices_scratch = Vec::new();
    let palettes_share_table = metadata.frames.first().is_some_and(|first| {
        metadata.frames.iter().all(|frame| {
            frame.palette_offset == first.palette_offset && frame.palette_size == first.palette_size
        })
    });
    let shared_palette = if palettes_share_table {
        Some(build_palette_u32(data, &metadata.frames[0], format)?)
    } else {
        None
    };

    let frame_limit = requested_frames.map_or(metadata.frames.len(), <[u8]>::len);
    for frame_index in 0..frame_limit {
        let requested = requested_frames
            .and_then(|frames| frames.get(frame_index))
            .copied()
            .unwrap_or(1);
        let frame = metadata
            .frames
            .get(frame_index)
            .ok_or_else(|| "Frame index out of range".to_string())?;
        let restore = (frame.disposal == 3).then(|| canvas.clone());
        let palette_storage;
        let palette = if let Some(palette) = shared_palette.as_ref() {
            palette
        } else {
            palette_storage = build_palette_u32(data, frame, format)?;
            &palette_storage
        };
        decode_frame_indices_reusing_output(
            data,
            frame,
            &mut image_data,
            &mut lzw_scratch,
            &mut indices_scratch,
        )?;
        blit_indices_to_canvas_u32(
            palette,
            metadata.width,
            frame,
            &indices_scratch,
            &mut canvas,
        )?;

        if requested != 0 {
            output.extend_from_slice(&canvas);
        }

        apply_frame_disposal_u32(&mut canvas, metadata.width, metadata.height, frame, restore);
    }

    Ok(output)
}

#[inline]
pub(crate) fn all_frames_full_opaque(metadata: &GifMetadata) -> bool {
    !metadata.frames.is_empty()
        && metadata.frames.iter().all(|frame| {
            frame.x == 0
                && frame.y == 0
                && frame.width == metadata.width
                && frame.height == metadata.height
                && frame.transparent_index.is_none()
        })
}

/// Decode animations whose frames overwrite the entire canvas without
/// transparency directly into the returned frame stream. The general
/// compositor first maps each frame into a reusable canvas and then copies
/// that canvas into the output; this common independent-frame shape only
/// needs the palette mapping once per pixel.
pub(crate) fn decode_full_opaque_frames_direct(
    data: &[u8],
    metadata: &GifMetadata,
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    let canvas_pixels = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_pixels = canvas_pixels
        .checked_mul(metadata.frames.len())
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    let palettes_share_table = metadata.frames.first().is_some_and(|first| {
        metadata.frames.iter().all(|frame| {
            frame.palette_offset == first.palette_offset && frame.palette_size == first.palette_size
        })
    });
    let shared_palette = if palettes_share_table {
        Some(build_palette_u32(data, &metadata.frames[0], format)?)
    } else {
        None
    };
    let mut output = uninitialized(output_pixels);
    let mut image_data = Vec::new();
    let mut lzw_scratch = LzwStackScratch::default();
    let mut indices_scratch = Vec::new();
    for (frame_index, frame) in metadata.frames.iter().enumerate() {
        let palette_storage;
        let palette = if let Some(palette) = shared_palette.as_ref() {
            palette
        } else {
            palette_storage = build_palette_u32(data, frame, format)?;
            &palette_storage
        };
        let destination =
            &mut output[frame_index * canvas_pixels..(frame_index + 1) * canvas_pixels];
        let destination_u32 = unsafe {
            std::slice::from_raw_parts_mut(destination.as_mut_ptr().cast::<u32>(), canvas_pixels)
        };
        let image_data_slice;
        if let Some(range) = single_image_data_range(data, frame.data_offset) {
            image_data_slice = &data[range];
        } else {
            if image_data.capacity() < frame.data_length {
                image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
            }
            collect_image_data_into(data, frame.data_offset, &mut image_data)?;
            image_data_slice = image_data.as_slice();
        }
        if lzw_decode_to_pixels_copy_with_scratch(
            frame.min_code_size,
            image_data_slice,
            palette,
            destination_u32,
            &mut lzw_scratch,
        )
        .is_ok()
        {
            continue;
        }
        decode_frame_indices_reusing_output(
            data,
            frame,
            &mut image_data,
            &mut lzw_scratch,
            &mut indices_scratch,
        )?;
        blit_indices_to_uninit_full_canvas_u32(palette, &indices_scratch, destination)?;
    }
    Ok(unsafe { assume_initialized(output) })
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn compose_decoded_frames_native(
    data: &[u8],
    metadata: &GifMetadata,
    decoded_indices: &[Vec<u8>],
) -> Result<Vec<u32>, String> {
    let canvas_pixels = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_pixels = metadata
        .frames
        .len()
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    if available_threads > 1 && output_pixels >= 200_000 {
        return compose_decoded_frames_by_rows_native(
            data,
            metadata,
            decoded_indices,
            available_threads,
            output_pixels,
        );
    }
    let mut output = Vec::with_capacity(output_pixels);
    let mut canvas = vec![0u32; canvas_pixels];

    for (frame, indices) in metadata.frames.iter().zip(decoded_indices.iter()) {
        let restore = (frame.disposal == 3).then(|| canvas.clone());
        let palette = build_palette_u32(data, frame, PixelFormat::Rgba)?;
        blit_indices_to_canvas_u32(&palette, metadata.width, frame, indices, &mut canvas)?;
        output.extend_from_slice(&canvas);
        apply_frame_disposal_u32(&mut canvas, metadata.width, metadata.height, frame, restore);
    }
    Ok(output)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn compose_decoded_frames_by_rows_native(
    data: &[u8],
    metadata: &GifMetadata,
    decoded_indices: &[Vec<u8>],
    available_threads: usize,
    output_pixels: usize,
) -> Result<Vec<u32>, String> {
    let canvas_width = usize::from(metadata.width);
    let canvas_height = usize::from(metadata.height);
    let canvas_pixels = canvas_width * canvas_height;
    let thread_count = available_threads.min(canvas_height);
    let rows_per_thread = canvas_height.div_ceil(thread_count);
    let palettes_share_table = metadata.frames.first().is_some_and(|first| {
        metadata.frames.iter().all(|frame| {
            frame.palette_offset == first.palette_offset && frame.palette_size == first.palette_size
        })
    });
    let palettes = if palettes_share_table {
        vec![build_palette_u32(
            data,
            &metadata.frames[0],
            PixelFormat::Rgba,
        )?]
    } else {
        metadata
            .frames
            .iter()
            .map(|frame| build_palette_u32(data, frame, PixelFormat::Rgba))
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut output = uninitialized(output_pixels);
    let output_address = output.as_mut_ptr() as usize;

    let result = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..thread_count)
            .map(|thread_index| {
                let row_start = thread_index * rows_per_thread;
                let row_end = (row_start + rows_per_thread).min(canvas_height);
                let palettes = &palettes;
                scope.spawn(move || {
                    let stripe_pixels = (row_end - row_start) * canvas_width;
                    let mut canvas = vec![0u32; stripe_pixels];
                    for (frame_index, (frame, indices)) in metadata
                        .frames
                        .iter()
                        .zip(decoded_indices.iter())
                        .enumerate()
                    {
                        let palette = &palettes[if palettes_share_table { 0 } else { frame_index }];
                        let restore = (frame.disposal == 3).then(|| canvas.clone());
                        blit_indices_to_canvas_stripe_u32(
                            palette,
                            canvas_width,
                            frame,
                            indices,
                            row_start,
                            row_end,
                            &mut canvas,
                        )?;
                        unsafe {
                            let destination = (output_address as *mut std::mem::MaybeUninit<u32>)
                                .add(frame_index * canvas_pixels + row_start * canvas_width)
                                .cast::<u32>();
                            std::ptr::copy_nonoverlapping(
                                canvas.as_ptr(),
                                destination,
                                stripe_pixels,
                            );
                        }
                        if frame.disposal == 2 {
                            clear_frame_rect_stripe_u32(
                                &mut canvas,
                                canvas_width,
                                frame,
                                row_start,
                                row_end,
                            );
                        } else if frame.disposal == 3 {
                            canvas = restore.expect("restore canvas exists");
                        }
                    }
                    Ok::<_, String>(())
                })
            })
            .collect();
        for handle in handles {
            handle
                .join()
                .map_err(|_| "Parallel GIF compositor panicked".to_string())??;
        }
        Ok::<_, String>(())
    });
    result?;

    Ok(unsafe { assume_initialized(output) })
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn blit_indices_to_canvas_stripe_u32(
    palette: &[u32],
    canvas_width: usize,
    frame: &FrameMetadata,
    indices: &[u8],
    row_start: usize,
    row_end: usize,
    canvas: &mut [u32],
) -> Result<(), String> {
    let frame_y = usize::from(frame.y);
    let frame_bottom = frame_y + usize::from(frame.height);
    let first_row = frame_y.max(row_start);
    let last_row = frame_bottom.min(row_end);
    if first_row >= last_row {
        return Ok(());
    }
    let frame_width = usize::from(frame.width);
    let frame_x = usize::from(frame.x);
    let frame_pixels = frame_width
        .checked_mul(usize::from(frame.height))
        .ok_or_else(|| "Decoded frame size overflow".to_string())?;
    if indices.len() < frame_pixels {
        return Err("Decoded index buffer is too short".to_string());
    }
    if frame_x.saturating_add(frame_width) > canvas_width
        || canvas_width == 0
        || row_end.saturating_sub(row_start) > canvas.len() / canvas_width
    {
        return Err("Frame destination exceeds canvas bounds".to_string());
    }
    let full_byte_palette = palette.len() == 256;
    match frame.transparent_index {
        None => {
            for global_y in first_row..last_row {
                let source = (global_y - frame_y) * frame_width;
                let source_row = &indices[source..source + frame_width];
                let destination = (global_y - row_start) * canvas_width + frame_x;
                let destination_row = &mut canvas[destination..destination + frame_width];
                if full_byte_palette {
                    for (pixel, &index) in destination_row.iter_mut().zip(source_row) {
                        *pixel = unsafe { *palette.get_unchecked(usize::from(index)) };
                    }
                    continue;
                }
                for (pixel, &index) in destination_row.iter_mut().zip(source_row) {
                    *pixel = *palette
                        .get(usize::from(index))
                        .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
                }
            }
        }
        Some(transparent_index) => {
            for global_y in first_row..last_row {
                let source = (global_y - frame_y) * frame_width;
                let source_row = &indices[source..source + frame_width];
                let destination = (global_y - row_start) * canvas_width + frame_x;
                let destination_row = &mut canvas[destination..destination + frame_width];
                if full_byte_palette {
                    let palette_address = palette.as_ptr();
                    let transparent_bytes =
                        u64::from(transparent_index).wrapping_mul(0x0101_0101_0101_0101);
                    let mut pixel_index = 0usize;
                    while pixel_index + 8 <= frame_width {
                        let packed_indices = unsafe {
                            std::ptr::read_unaligned(
                                source_row.as_ptr().add(pixel_index).cast::<u64>(),
                            )
                        };
                        if packed_indices == transparent_bytes {
                            pixel_index += 8;
                            continue;
                        }
                        let compared = packed_indices ^ transparent_bytes;
                        let transparent_lanes = compared.wrapping_sub(0x0101_0101_0101_0101)
                            & !compared
                            & 0x8080_8080_8080_8080;
                        if transparent_lanes == 0 {
                            let colors = unsafe {
                                [
                                    *palette_address.add((packed_indices & 0xff) as usize),
                                    *palette_address.add(((packed_indices >> 8) & 0xff) as usize),
                                    *palette_address.add(((packed_indices >> 16) & 0xff) as usize),
                                    *palette_address.add(((packed_indices >> 24) & 0xff) as usize),
                                    *palette_address.add(((packed_indices >> 32) & 0xff) as usize),
                                    *palette_address.add(((packed_indices >> 40) & 0xff) as usize),
                                    *palette_address.add(((packed_indices >> 48) & 0xff) as usize),
                                    *palette_address.add(((packed_indices >> 56) & 0xff) as usize),
                                ]
                            };
                            unsafe {
                                std::ptr::copy_nonoverlapping(
                                    colors.as_ptr(),
                                    destination_row.as_mut_ptr().add(pixel_index),
                                    colors.len(),
                                );
                            }
                            pixel_index += 8;
                            continue;
                        }
                        for lane in 0..8 {
                            let index = unsafe { *source_row.get_unchecked(pixel_index + lane) };
                            if index != transparent_index {
                                destination_row[pixel_index + lane] =
                                    unsafe { *palette_address.add(usize::from(index)) };
                            }
                        }
                        pixel_index += 8;
                    }
                    while pixel_index < frame_width {
                        let index = source_row[pixel_index];
                        if index != transparent_index {
                            destination_row[pixel_index] =
                                unsafe { *palette_address.add(usize::from(index)) };
                        }
                        pixel_index += 1;
                    }
                    continue;
                }
                for (pixel, &index) in destination_row.iter_mut().zip(source_row) {
                    if index != transparent_index {
                        *pixel = *palette
                            .get(usize::from(index))
                            .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn clear_frame_rect_stripe_u32(
    canvas: &mut [u32],
    canvas_width: usize,
    frame: &FrameMetadata,
    row_start: usize,
    row_end: usize,
) {
    let frame_y = usize::from(frame.y);
    let first_row = frame_y.max(row_start);
    let last_row = (frame_y + usize::from(frame.height)).min(row_end);
    let x = usize::from(frame.x);
    let width = usize::from(frame.width);
    for global_y in first_row..last_row {
        let start = (global_y - row_start) * canvas_width + x;
        canvas[start..start + width].fill(0);
    }
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn prepare_composited_delta_frames_inner(
    data: &[u8],
    metadata: &GifMetadata,
    requested_frames: &[u8],
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    if requested_frames.len() > metadata.frames.len() {
        return Err("Requested frame flags exceed frame count".to_string());
    }

    let canvas_width = usize::from(metadata.width);
    let canvas_height = usize::from(metadata.height);
    let canvas_pixels = canvas_width
        .checked_mul(canvas_height)
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let requested_count = requested_frames.iter().filter(|flag| **flag != 0).count();
    let table_len = COMPOSITED_DELTA_ENTRY_LEN
        .checked_mul(requested_count)
        .and_then(|len| len.checked_add(COMPOSITED_DELTA_HEADER_LEN))
        .ok_or_else(|| "Prepared frame table overflow".to_string())?;
    let output_pixels = requested_count
        .checked_mul(canvas_pixels)
        .and_then(|len| len.checked_add(table_len))
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    let mut output = vec![0u32; table_len];
    output[0] = COMPOSITED_DELTA_MAGIC;
    output[1] = COMPOSITED_DELTA_VERSION;
    output[2] = requested_count as u32;
    output[3] = canvas_pixels as u32;
    output.reserve(output_pixels.saturating_sub(table_len));

    let mut canvas = vec![0u32; canvas_pixels];
    let mut previous_requested_canvas: Option<Vec<u32>> = None;
    let mut output_frame_index = 0usize;

    for (frame_index, requested) in requested_frames.iter().enumerate() {
        let frame = metadata
            .frames
            .get(frame_index)
            .ok_or_else(|| "Frame index out of range".to_string())?;
        let restore = (frame.disposal == 3).then(|| canvas.clone());
        let palette = build_palette_u32(data, frame, format)?;
        let indices = decode_frame_indices_inner(data, frame)?;

        blit_indices_to_canvas_u32(&palette, metadata.width, frame, &indices, &mut canvas)?;

        if *requested != 0 {
            let entry =
                COMPOSITED_DELTA_HEADER_LEN + output_frame_index * COMPOSITED_DELTA_ENTRY_LEN;
            let full_start = output.len();
            output.extend_from_slice(&canvas);
            let changed_rect = previous_requested_canvas.as_deref().and_then(|previous| {
                find_changed_rect_u32(previous, &canvas, canvas_width, canvas_height)
            });
            let delta_start = output.len();
            let delta_len = if let Some(rect) = changed_rect {
                push_rect_pixels_u32(&canvas, canvas_width, rect, &mut output);
                rect.width * rect.height
            } else {
                0
            };

            output[entry] = frame_index as u32;
            output[entry + 1] = full_start as u32;
            output[entry + 2] = canvas_pixels as u32;
            if let Some(rect) = changed_rect {
                output[entry + 3] = rect.x as u32;
                output[entry + 4] = rect.y as u32;
                output[entry + 5] = rect.width as u32;
                output[entry + 6] = rect.height as u32;
            }
            output[entry + 7] = delta_start as u32;
            output[entry + 8] = delta_len as u32;

            match &mut previous_requested_canvas {
                Some(previous) => previous.copy_from_slice(&canvas),
                None => previous_requested_canvas = Some(canvas.clone()),
            }
            output_frame_index += 1;
        }

        apply_frame_disposal_u32(&mut canvas, metadata.width, metadata.height, frame, restore);
    }

    Ok(output)
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn find_changed_rect_u32(
    previous: &[u32],
    current: &[u32],
    width: usize,
    height: usize,
) -> Option<ChangedRectU32> {
    let mut top = 0usize;
    let mut bottom = height.saturating_sub(1);

    while top < height {
        let row = top * width;
        if previous[row..row + width] != current[row..row + width] {
            break;
        }
        top += 1;
    }

    if top == height {
        return None;
    }

    while bottom > top {
        let row = bottom * width;
        if previous[row..row + width] != current[row..row + width] {
            break;
        }
        bottom -= 1;
    }

    let mut left = width - 1;
    let mut right = 0usize;
    let previous_pointer = previous.as_ptr();
    let current_pointer = current.as_ptr();
    for y in top..=bottom {
        let row = y * width;
        let mut x = 0usize;
        while x + 2 <= width {
            let previous_word =
                unsafe { std::ptr::read_unaligned(previous_pointer.add(row + x).cast::<u64>()) };
            let current_word =
                unsafe { std::ptr::read_unaligned(current_pointer.add(row + x).cast::<u64>()) };
            if previous_word != current_word {
                for offset in 0..2 {
                    if previous[row + x + offset] != current[row + x + offset] {
                        let changed = x + offset;
                        left = left.min(changed);
                        right = right.max(changed);
                    }
                }
            }
            x += 2;
        }
        while x < width {
            if previous[row + x] != current[row + x] {
                left = left.min(x);
                right = right.max(x);
            }
            x += 1;
        }
    }

    Some(ChangedRectU32 {
        x: left,
        y: top,
        width: right - left + 1,
        height: bottom - top + 1,
    })
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn push_rect_pixels_u32(
    source: &[u32],
    source_width: usize,
    rect: ChangedRectU32,
    output: &mut Vec<u32>,
) {
    for y in 0..rect.height {
        let start = (rect.y + y) * source_width + rect.x;
        output.extend_from_slice(&source[start..start + rect.width]);
    }
}
