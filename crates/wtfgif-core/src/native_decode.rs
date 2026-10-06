//! Multithreaded native decoding and compositing pipelines.

use super::*;

pub(crate) fn frames_are_independent_native(metadata: &GifMetadata) -> bool {
    metadata.frames.iter().enumerate().all(|(index, frame)| {
        let starts_clear = index == 0
            || (metadata.frames[index - 1].disposal == 2
                && frame_covers_canvas(&metadata.frames[index - 1], metadata));
        starts_clear || (frame_covers_canvas(frame, metadata) && frame.transparent_index.is_none())
    })
}

#[inline]
pub(crate) fn frame_covers_canvas(frame: &FrameMetadata, metadata: &GifMetadata) -> bool {
    frame.x == 0 && frame.y == 0 && frame.width == metadata.width && frame.height == metadata.height
}

pub(crate) fn independent_frame_segments_native(
    metadata: &GifMetadata,
) -> Option<Vec<(usize, usize)>> {
    let mut segments = Vec::new();
    let mut start = 0usize;
    for (frame_index, frame) in metadata.frames.iter().enumerate() {
        let clears_canvas = frame.disposal == 2
            && frame.x == 0
            && frame.y == 0
            && frame.width == metadata.width
            && frame.height == metadata.height;
        if clears_canvas && frame_index + 1 < metadata.frames.len() {
            segments.push((start, frame_index + 1));
            start = frame_index + 1;
        }
    }
    segments.push((start, metadata.frames.len()));
    let output_pixels = usize::from(metadata.width)
        .saturating_mul(usize::from(metadata.height))
        .saturating_mul(metadata.frames.len());
    let longest_segment = segments
        .iter()
        .map(|(segment_start, segment_end)| segment_end - segment_start)
        .max()
        .unwrap_or(0);
    (segments.len() >= 2 && output_pixels >= 100_000 && longest_segment <= 8).then_some(segments)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_segments_worker(
    data: &[u8],
    metadata: &GifMetadata,
    segments: &[(usize, usize)],
    palettes: &[Vec<u32>],
    palettes_share_table: bool,
    canvas_pixels: usize,
    output_address: usize,
    next_segment: &std::sync::atomic::AtomicUsize,
) -> Result<(), String> {
    let mut image_data = Vec::new();
    let mut lzw_scratch = LzwStackScratch::default();
    let mut indices = Vec::new();
    loop {
        let segment_index = next_segment.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(&(start, end)) = segments.get(segment_index) else {
            return Ok(());
        };
        if end == start + 1 {
            let frame = &metadata.frames[start];
            if frame_covers_canvas(frame, metadata) && !frame.interlaced {
                let palette = &palettes[if palettes_share_table { 0 } else { start }];
                decode_frame_indices_reusing_output(
                    data,
                    frame,
                    &mut image_data,
                    &mut lzw_scratch,
                    &mut indices,
                )?;
                let destination = unsafe {
                    std::slice::from_raw_parts_mut(
                        (output_address as *mut std::mem::MaybeUninit<u32>)
                            .add(start * canvas_pixels),
                        canvas_pixels,
                    )
                };
                if palette.len() == 256 {
                    let palette_address = palette.as_ptr();
                    let indices_address = indices.as_ptr();
                    let destination_address = destination.as_mut_ptr().cast::<u32>();
                    let mut pixel_index = 0usize;
                    match frame.transparent_index {
                        None => {
                            while pixel_index + 8 <= canvas_pixels {
                                let packed = unsafe {
                                    std::ptr::read_unaligned(
                                        indices_address.add(pixel_index).cast::<u64>(),
                                    )
                                };
                                let colors = unsafe {
                                    [
                                        *palette_address.add((packed & 0xff) as usize),
                                        *palette_address.add(((packed >> 8) & 0xff) as usize),
                                        *palette_address.add(((packed >> 16) & 0xff) as usize),
                                        *palette_address.add(((packed >> 24) & 0xff) as usize),
                                        *palette_address.add(((packed >> 32) & 0xff) as usize),
                                        *palette_address.add(((packed >> 40) & 0xff) as usize),
                                        *palette_address.add(((packed >> 48) & 0xff) as usize),
                                        *palette_address.add(((packed >> 56) & 0xff) as usize),
                                    ]
                                };
                                unsafe {
                                    std::ptr::copy_nonoverlapping(
                                        colors.as_ptr(),
                                        destination_address.add(pixel_index),
                                        colors.len(),
                                    );
                                }
                                pixel_index += 8;
                            }
                        }
                        Some(transparent_index) => {
                            while pixel_index + 8 <= canvas_pixels {
                                let packed = unsafe {
                                    std::ptr::read_unaligned(
                                        indices_address.add(pixel_index).cast::<u64>(),
                                    )
                                };
                                let indices = [
                                    (packed & 0xff) as u8,
                                    ((packed >> 8) & 0xff) as u8,
                                    ((packed >> 16) & 0xff) as u8,
                                    ((packed >> 24) & 0xff) as u8,
                                    ((packed >> 32) & 0xff) as u8,
                                    ((packed >> 40) & 0xff) as u8,
                                    ((packed >> 48) & 0xff) as u8,
                                    ((packed >> 56) & 0xff) as u8,
                                ];
                                let colors = indices.map(|index| {
                                    if index == transparent_index {
                                        0
                                    } else {
                                        unsafe { *palette_address.add(usize::from(index)) }
                                    }
                                });
                                unsafe {
                                    std::ptr::copy_nonoverlapping(
                                        colors.as_ptr(),
                                        destination_address.add(pixel_index),
                                        colors.len(),
                                    );
                                }
                                pixel_index += 8;
                            }
                        }
                    }
                    while pixel_index < canvas_pixels {
                        let index = unsafe { *indices_address.add(pixel_index) };
                        let color = if frame.transparent_index == Some(index) {
                            0
                        } else {
                            unsafe { *palette_address.add(usize::from(index)) }
                        };
                        unsafe {
                            destination_address.add(pixel_index).write(color);
                        }
                        pixel_index += 1;
                    }
                    continue;
                }
                match frame.transparent_index {
                    None => {
                        for (pixel, &index) in destination.iter_mut().zip(&indices) {
                            pixel.write(*palette.get(usize::from(index)).ok_or_else(|| {
                                format!("Palette index {index} exceeds palette size")
                            })?);
                        }
                    }
                    Some(transparent_index) => {
                        for (pixel, &index) in destination.iter_mut().zip(&indices) {
                            pixel.write(if index == transparent_index {
                                0
                            } else {
                                *palette.get(usize::from(index)).ok_or_else(|| {
                                    format!("Palette index {index} exceeds palette size")
                                })?
                            });
                        }
                    }
                }
                continue;
            }
        }
        let mut canvas = vec![0u32; canvas_pixels];
        for frame_index in start..end {
            let frame = &metadata.frames[frame_index];
            let restore = (frame.disposal == 3).then(|| canvas.clone());
            let palette = &palettes[if palettes_share_table { 0 } else { frame_index }];
            decode_frame_indices_reusing_output(
                data,
                frame,
                &mut image_data,
                &mut lzw_scratch,
                &mut indices,
            )?;
            blit_indices_to_canvas_u32(palette, metadata.width, frame, &indices, &mut canvas)?;
            unsafe {
                let destination = (output_address as *mut std::mem::MaybeUninit<u32>)
                    .add(frame_index * canvas_pixels)
                    .cast::<u32>();
                std::ptr::copy_nonoverlapping(canvas.as_ptr(), destination, canvas_pixels);
            }
            apply_frame_disposal_u32(&mut canvas, metadata.width, metadata.height, frame, restore);
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
pub(crate) struct DecodeSegmentsDispatchContext<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) metadata: &'a GifMetadata,
    pub(crate) segments: &'a [(usize, usize)],
    pub(crate) palettes: &'a [Vec<u32>],
    pub(crate) palettes_share_table: bool,
    pub(crate) canvas_pixels: usize,
    pub(crate) output_address: usize,
    pub(crate) next_segment: std::sync::atomic::AtomicUsize,
    pub(crate) error: std::sync::Mutex<Option<String>>,
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
unsafe extern "C" fn decode_segments_dispatch_worker(
    context: *mut std::ffi::c_void,
    _iteration: usize,
) {
    let context = &*(context.cast::<DecodeSegmentsDispatchContext<'_>>());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        decode_segments_worker(
            context.data,
            context.metadata,
            context.segments,
            context.palettes,
            context.palettes_share_table,
            context.canvas_pixels,
            context.output_address,
            &context.next_segment,
        )
    }));
    let error = match result {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error,
        Err(_) => "Parallel GIF segment decoder panicked".to_string(),
    };
    if let Ok(mut stored_error) = context.error.lock() {
        if stored_error.is_none() {
            *stored_error = Some(error);
        }
    }
}

pub(crate) fn decode_segmented_frames_into_native(
    data: &[u8],
    metadata: &GifMetadata,
    segments: &[(usize, usize)],
    output: &mut [std::mem::MaybeUninit<u32>],
) -> Result<(), String> {
    let canvas_pixels = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_pixels = metadata
        .frames
        .len()
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    if output.len() != output_pixels {
        return Err("Host output buffer has the wrong size".to_string());
    }
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let thread_count = available_threads.min(segments.len());
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
    let output_address = output.as_mut_ptr() as usize;
    let next_segment = std::sync::atomic::AtomicUsize::new(0);

    #[cfg(target_os = "macos")]
    {
        let mut context = DecodeSegmentsDispatchContext {
            data,
            metadata,
            segments,
            palettes: &palettes,
            palettes_share_table,
            canvas_pixels,
            output_address,
            next_segment,
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
                (&mut context as *mut DecodeSegmentsDispatchContext<'_>).cast(),
                decode_segments_dispatch_worker,
            );
        }
        if let Some(error) = context
            .error
            .into_inner()
            .map_err(|_| "Parallel GIF segment decoder error lock was poisoned".to_string())?
        {
            return Err(error);
        }
    }

    #[cfg(not(target_os = "macos"))]
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..thread_count)
            .map(|_| {
                let next_segment = &next_segment;
                let palettes = &palettes;
                scope.spawn(move || {
                    decode_segments_worker(
                        data,
                        metadata,
                        segments,
                        palettes,
                        palettes_share_table,
                        canvas_pixels,
                        output_address,
                        next_segment,
                    )
                })
            })
            .collect();
        for handle in handles {
            handle
                .join()
                .map_err(|_| "Parallel GIF segment decoder panicked".to_string())??;
        }
        Ok::<_, String>(())
    })?;

    Ok(())
}

pub(crate) fn decode_segmented_frames_native(
    data: &[u8],
    metadata: &GifMetadata,
    segments: &[(usize, usize)],
) -> Result<Vec<u32>, String> {
    let canvas_pixels = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_pixels = metadata
        .frames
        .len()
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    let mut output = uninitialized(output_pixels);
    decode_segmented_frames_into_native(data, metadata, segments, &mut output)?;
    Ok(unsafe { assume_initialized(output) })
}

pub(crate) fn should_fuse_sequential_decode_native(metadata: &GifMetadata) -> bool {
    let total_frame_pixels = metadata.frames.iter().fold(0usize, |total, frame| {
        total.saturating_add(usize::from(frame.width) * usize::from(frame.height))
    });
    metadata.frames.len() < 8 || total_frame_pixels < 30_000
}

pub(crate) fn should_pipeline_decode_native(metadata: &GifMetadata) -> bool {
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    if available_threads < 4 || metadata.frames.len() < 8 {
        return false;
    }
    let canvas_pixels = usize::from(metadata.width) * usize::from(metadata.height);
    let output_pixels = canvas_pixels.saturating_mul(metadata.frames.len());
    let decoded_pixels = metadata.frames.iter().fold(0usize, |total, frame| {
        total.saturating_add(usize::from(frame.width) * usize::from(frame.height))
    });
    output_pixels >= 50_000 && decoded_pixels >= 50_000
}

pub(crate) struct PipelineDecodedFrame {
    pub(crate) state: std::sync::atomic::AtomicU8,
    pub(crate) result: std::sync::OnceLock<Result<Vec<u8>, String>>,
    pub(crate) direct_mapped: std::sync::atomic::AtomicBool,
    pub(crate) offset: usize,
    pub(crate) length: usize,
}

pub(crate) fn decode_pipeline_frame_into(
    data: &[u8],
    frame: &FrameMetadata,
    image_data: &mut Vec<u8>,
    lzw_scratch: &mut LzwStackScratch,
    decoded_indices_address: usize,
    layout: &PipelineDecodedFrame,
) -> Result<(), String> {
    if frame.interlaced {
        let indices = decode_frame_indices_with_scratches(data, frame, image_data, lzw_scratch)?;
        if indices.len() != layout.length {
            return Err("Decoded frame length does not match dimensions".to_string());
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                indices.as_ptr(),
                (decoded_indices_address as *mut u8).add(layout.offset),
                layout.length,
            );
        }
        return Ok(());
    }
    if image_data.capacity() < frame.data_length {
        image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
    }
    collect_image_data_into(data, frame.data_offset, image_data)?;
    unsafe {
        std::ptr::write_bytes(
            (decoded_indices_address as *mut u8).add(layout.offset),
            0,
            layout.length,
        );
    }
    let output = unsafe {
        std::slice::from_raw_parts_mut(
            (decoded_indices_address as *mut u8).add(layout.offset),
            layout.length,
        )
    };
    if should_decode_lzw_direct(frame, layout.length) {
        if should_decode_lzw_copy(frame, layout.length) {
            lzw_decode_to_indices_copy_with_scratch(
                frame.min_code_size,
                image_data,
                output,
                lzw_scratch,
            )
        } else {
            lzw_decode_to_indices_direct_with_scratch(
                frame.min_code_size,
                image_data,
                output,
                lzw_scratch,
            )
        }
    } else {
        lzw_decode_to_indices_stack_with_scratch(
            frame.min_code_size,
            image_data,
            output,
            lzw_scratch,
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_pipeline_frame_result(
    data: &[u8],
    metadata: &GifMetadata,
    frame_index: usize,
    frame: &FrameMetadata,
    layout: &PipelineDecodedFrame,
    image_data: &mut Vec<u8>,
    lzw_scratch: &mut LzwStackScratch,
    decoded_indices_address: usize,
    flat_decoded_indices: bool,
    palette: &[u32],
    output_address: usize,
    canvas_pixels: usize,
    parallel_direct_mapping: bool,
) -> Result<Vec<u8>, String> {
    if flat_decoded_indices {
        decode_pipeline_frame_into(
            data,
            frame,
            image_data,
            lzw_scratch,
            decoded_indices_address,
            layout,
        )?;
        return Ok(Vec::new());
    }
    if parallel_direct_mapping && !frame.interlaced {
        if image_data.capacity() < frame.data_length {
            image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
        }
        collect_image_data_into(data, frame.data_offset, image_data)?;
        let destination = unsafe {
            (output_address as *mut std::mem::MaybeUninit<u32>)
                .add(frame_index * canvas_pixels)
                .cast::<u32>()
        };
        unsafe {
            std::ptr::write_bytes(destination.cast::<u8>(), 0, canvas_pixels);
        }
        let destination_bytes =
            unsafe { std::slice::from_raw_parts_mut(destination.cast::<u8>(), canvas_pixels) };
        if should_decode_lzw_direct(frame, canvas_pixels) {
            if should_decode_lzw_copy(frame, canvas_pixels) {
                lzw_decode_to_indices_copy_with_scratch(
                    frame.min_code_size,
                    image_data,
                    destination_bytes,
                    lzw_scratch,
                )?;
            } else {
                lzw_decode_to_indices_direct_with_scratch(
                    frame.min_code_size,
                    image_data,
                    destination_bytes,
                    lzw_scratch,
                )?;
            }
        } else {
            lzw_decode_to_indices_stack_with_scratch(
                frame.min_code_size,
                image_data,
                destination_bytes,
                lzw_scratch,
            )?;
        }
        if let Some(transparent_index) = frame.transparent_index {
            if destination_bytes.contains(&transparent_index) {
                return Ok(destination_bytes.to_vec());
            }
        }
        if palette.len() == 256 {
            let palette_address = palette.as_ptr();
            let indices_address = destination.cast::<u8>();
            let mut pixel_index = canvas_pixels;
            while pixel_index >= 8 {
                pixel_index -= 8;
                let packed_indices = unsafe {
                    std::ptr::read_unaligned(indices_address.add(pixel_index).cast::<u64>())
                };
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
                        destination.add(pixel_index),
                        colors.len(),
                    );
                }
            }
            while pixel_index > 0 {
                pixel_index -= 1;
                let index = unsafe { usize::from(*indices_address.add(pixel_index)) };
                unsafe {
                    destination
                        .add(pixel_index)
                        .write(*palette_address.add(index));
                }
            }
        } else {
            for pixel_index in (0..canvas_pixels).rev() {
                let index = destination_bytes[pixel_index];
                let color = *palette
                    .get(usize::from(index))
                    .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
                unsafe {
                    destination.add(pixel_index).write(color);
                }
            }
        }
        layout
            .direct_mapped
            .store(true, std::sync::atomic::Ordering::Release);
        return Ok(Vec::new());
    }
    let indices = decode_frame_indices_with_scratches(data, frame, image_data, lzw_scratch)?;
    if !parallel_direct_mapping {
        return Ok(indices);
    }
    debug_assert!(frame_covers_canvas(frame, metadata));
    if frame
        .transparent_index
        .is_some_and(|transparent_index| indices.contains(&transparent_index))
    {
        return Ok(indices);
    }
    if indices.len() != canvas_pixels {
        return Err("Decoded full frame length does not match canvas".to_string());
    }
    let destination = unsafe {
        (output_address as *mut std::mem::MaybeUninit<u32>)
            .add(frame_index * canvas_pixels)
            .cast::<u32>()
    };
    if palette.len() == 256 {
        for (pixel_index, &index) in indices.iter().enumerate() {
            unsafe {
                destination
                    .add(pixel_index)
                    .write(*palette.get_unchecked(usize::from(index)));
            }
        }
    } else {
        for (pixel_index, &index) in indices.iter().enumerate() {
            let color = *palette
                .get(usize::from(index))
                .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
            unsafe {
                destination.add(pixel_index).write(color);
            }
        }
    }
    layout
        .direct_mapped
        .store(true, std::sync::atomic::Ordering::Release);
    Ok(Vec::new())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_pipeline_worker(
    data: &[u8],
    metadata: &GifMetadata,
    decoded: &[PipelineDecodedFrame],
    decoded_indices_address: usize,
    flat_decoded_indices: bool,
    palettes: &[Vec<u32>],
    palettes_share_table: bool,
    output_address: usize,
    canvas_pixels: usize,
    parallel_direct_mapping: bool,
    next_frame: &std::sync::atomic::AtomicUsize,
    assist_decode: bool,
) {
    let mut image_data = Vec::new();
    let mut lzw_scratch = LzwStackScratch::default();
    loop {
        let frame_index = next_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(frame) = metadata.frames.get(frame_index) else {
            return;
        };
        let layout = &decoded[frame_index];
        if assist_decode
            && layout
                .state
                .compare_exchange(
                    0,
                    1,
                    std::sync::atomic::Ordering::Acquire,
                    std::sync::atomic::Ordering::Relaxed,
                )
                .is_err()
        {
            continue;
        }
        let palette = &palettes[if palettes_share_table { 0 } else { frame_index }];
        let result = decode_pipeline_frame_result(
            data,
            metadata,
            frame_index,
            frame,
            layout,
            &mut image_data,
            &mut lzw_scratch,
            decoded_indices_address,
            flat_decoded_indices,
            palette,
            output_address,
            canvas_pixels,
            parallel_direct_mapping,
        );
        let _ = layout.result.set(result);
        if assist_decode {
            layout.state.store(2, std::sync::atomic::Ordering::Release);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn compose_pipeline_stripe(
    data: &[u8],
    metadata: &GifMetadata,
    decoded: &[PipelineDecodedFrame],
    decoded_indices_address: usize,
    flat_decoded_indices: bool,
    parallel_direct_mapping: bool,
    palettes: &[Vec<u32>],
    palettes_share_table: bool,
    canvas_width: usize,
    canvas_height: usize,
    canvas_pixels: usize,
    rows_per_thread: usize,
    thread_index: usize,
    output_address: usize,
    next_frame: &std::sync::atomic::AtomicUsize,
    assist_decode: bool,
    direct_overlay_output: bool,
) -> Result<(), String> {
    let row_start = thread_index * rows_per_thread;
    let row_end = (row_start + rows_per_thread).min(canvas_height);
    let stripe_pixels = (row_end - row_start) * canvas_width;
    let mut canvas = if direct_overlay_output {
        Vec::new()
    } else {
        vec![0u32; stripe_pixels]
    };
    let mut image_data = Vec::new();
    let mut lzw_scratch = LzwStackScratch::default();
    for (frame_index, frame) in metadata.frames.iter().enumerate() {
        let palette = &palettes[if palettes_share_table { 0 } else { frame_index }];
        let layout = &decoded[frame_index];
        let frame_result = if assist_decode {
            'frame_ready: loop {
                if let Some(result) = layout.result.get() {
                    break result;
                }
                if layout
                    .state
                    .compare_exchange(
                        0,
                        1,
                        std::sync::atomic::Ordering::Acquire,
                        std::sync::atomic::Ordering::Relaxed,
                    )
                    .is_ok()
                {
                    let result = decode_pipeline_frame_result(
                        data,
                        metadata,
                        frame_index,
                        frame,
                        layout,
                        &mut image_data,
                        &mut lzw_scratch,
                        decoded_indices_address,
                        flat_decoded_indices,
                        palette,
                        output_address,
                        canvas_pixels,
                        parallel_direct_mapping,
                    );
                    let _ = layout.result.set(result);
                    layout.state.store(2, std::sync::atomic::Ordering::Release);
                    break layout.result.get().unwrap();
                }

                let background_index =
                    next_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if let Some(background_frame) = metadata.frames.get(background_index) {
                    let background_layout = &decoded[background_index];
                    if background_layout
                        .state
                        .compare_exchange(
                            0,
                            1,
                            std::sync::atomic::Ordering::Acquire,
                            std::sync::atomic::Ordering::Relaxed,
                        )
                        .is_ok()
                    {
                        let background_palette = &palettes[if palettes_share_table {
                            0
                        } else {
                            background_index
                        }];
                        let result = decode_pipeline_frame_result(
                            data,
                            metadata,
                            background_index,
                            background_frame,
                            background_layout,
                            &mut image_data,
                            &mut lzw_scratch,
                            decoded_indices_address,
                            flat_decoded_indices,
                            background_palette,
                            output_address,
                            canvas_pixels,
                            parallel_direct_mapping,
                        );
                        let _ = background_layout.result.set(result);
                        background_layout
                            .state
                            .store(2, std::sync::atomic::Ordering::Release);
                    }
                    continue;
                }

                let mut spins = 0usize;
                loop {
                    if let Some(result) = layout.result.get() {
                        break 'frame_ready result;
                    }
                    if spins < 64 {
                        std::hint::spin_loop();
                        spins += 1;
                    } else {
                        std::thread::yield_now();
                    }
                }
            }
        } else {
            let wait_for_result = || {
                let mut spins = 0usize;
                loop {
                    if let Some(result) = layout.result.get() {
                        return result;
                    }
                    if spins < 64 {
                        std::hint::spin_loop();
                        spins += 1;
                    } else {
                        std::thread::yield_now();
                    }
                }
            };
            if canvas_pixels >= 16_000 {
                match layout.result.get() {
                    Some(result) => result,
                    None => wait_for_result(),
                }
            } else {
                wait_for_result()
            }
        };
        let stored_indices = frame_result.as_ref().map_err(Clone::clone)?;
        if layout
            .direct_mapped
            .load(std::sync::atomic::Ordering::Acquire)
        {
            continue;
        }
        let indices = if flat_decoded_indices {
            unsafe {
                std::slice::from_raw_parts(
                    (decoded_indices_address as *const u8).add(layout.offset),
                    layout.length,
                )
            }
        } else {
            stored_indices
        };
        if direct_overlay_output {
            let frame_covers_canvas = frame_covers_canvas(frame, metadata);
            let source_start = row_start * canvas_width;
            let destination = unsafe {
                (output_address as *mut std::mem::MaybeUninit<u32>)
                    .add(frame_index * canvas_pixels + source_start)
                    .cast::<u32>()
            };
            let previous = (frame_index != 0).then(|| unsafe {
                (output_address as *const std::mem::MaybeUninit<u32>)
                    .add((frame_index - 1) * canvas_pixels + source_start)
                    .cast::<u32>()
            });
            if !frame_covers_canvas {
                unsafe {
                    if let Some(previous) = previous {
                        std::ptr::copy_nonoverlapping(previous, destination, stripe_pixels);
                    } else {
                        std::ptr::write_bytes(destination, 0, stripe_pixels);
                    }
                }
                let destination =
                    unsafe { std::slice::from_raw_parts_mut(destination, stripe_pixels) };
                blit_indices_to_canvas_stripe_u32(
                    palette,
                    canvas_width,
                    frame,
                    indices,
                    row_start,
                    row_end,
                    destination,
                )?;
                continue;
            }
            let source = indices
                .get(source_start..source_start + stripe_pixels)
                .ok_or_else(|| "Decoded index buffer is too short".to_string())?;
            match frame.transparent_index {
                None => {
                    for (offset, &index) in source.iter().enumerate() {
                        let color = *palette
                            .get(usize::from(index))
                            .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
                        unsafe {
                            destination.add(offset).write(color);
                        }
                    }
                }
                Some(transparent_index) => {
                    for (offset, &index) in source.iter().enumerate() {
                        let color = if index == transparent_index {
                            previous
                                .map(|previous| unsafe { *previous.add(offset) })
                                .unwrap_or(0)
                        } else {
                            *palette.get(usize::from(index)).ok_or_else(|| {
                                format!("Palette index {index} exceeds palette size")
                            })?
                        };
                        unsafe {
                            destination.add(offset).write(color);
                        }
                    }
                }
            }
            continue;
        }
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
            std::ptr::copy_nonoverlapping(canvas.as_ptr(), destination, stripe_pixels);
        }
        if frame.disposal == 2 {
            clear_frame_rect_stripe_u32(&mut canvas, canvas_width, frame, row_start, row_end);
        } else if frame.disposal == 3 {
            canvas = restore.expect("restore canvas exists");
        }
    }
    Ok(())
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
pub(crate) struct DecodePipelineDispatchContext<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) metadata: &'a GifMetadata,
    pub(crate) decoded: &'a [PipelineDecodedFrame],
    pub(crate) decoded_indices_address: usize,
    pub(crate) flat_decoded_indices: bool,
    pub(crate) parallel_direct_mapping: bool,
    pub(crate) palettes: &'a [Vec<u32>],
    pub(crate) palettes_share_table: bool,
    pub(crate) canvas_width: usize,
    pub(crate) canvas_height: usize,
    pub(crate) canvas_pixels: usize,
    pub(crate) rows_per_thread: usize,
    pub(crate) decode_thread_count: usize,
    pub(crate) output_address: usize,
    pub(crate) next_frame: &'a std::sync::atomic::AtomicUsize,
    pub(crate) assist_decode: bool,
    pub(crate) direct_overlay_output: bool,
    pub(crate) error: std::sync::Mutex<Option<String>>,
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
unsafe extern "C" fn decode_pipeline_dispatch_worker(
    context: *mut std::ffi::c_void,
    iteration: usize,
) {
    let context = &*(context.cast::<DecodePipelineDispatchContext<'_>>());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if iteration < context.decode_thread_count {
            decode_pipeline_worker(
                context.data,
                context.metadata,
                context.decoded,
                context.decoded_indices_address,
                context.flat_decoded_indices,
                context.palettes,
                context.palettes_share_table,
                context.output_address,
                context.canvas_pixels,
                context.parallel_direct_mapping,
                context.next_frame,
                context.assist_decode,
            );
            Ok(())
        } else {
            compose_pipeline_stripe(
                context.data,
                context.metadata,
                context.decoded,
                context.decoded_indices_address,
                context.flat_decoded_indices,
                context.parallel_direct_mapping,
                context.palettes,
                context.palettes_share_table,
                context.canvas_width,
                context.canvas_height,
                context.canvas_pixels,
                context.rows_per_thread,
                iteration - context.decode_thread_count,
                context.output_address,
                context.next_frame,
                context.assist_decode,
                context.direct_overlay_output,
            )
        }
    }));
    let error = match result {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error,
        Err(_) => "Parallel GIF pipeline panicked".to_string(),
    };
    if let Ok(mut stored_error) = context.error.lock() {
        if stored_error.is_none() {
            *stored_error = Some(error);
        }
    }
}

pub(crate) fn decode_and_compose_pipeline_native(
    data: &[u8],
    metadata: &GifMetadata,
) -> Result<Vec<u32>, String> {
    let canvas_width = usize::from(metadata.width);
    let canvas_height = usize::from(metadata.height);
    let canvas_pixels = canvas_width
        .checked_mul(canvas_height)
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_pixels = metadata
        .frames
        .len()
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    let mut output = uninitialized(output_pixels);
    decode_and_compose_pipeline_into_native(data, metadata, &mut output)?;
    Ok(unsafe { assume_initialized(output) })
}

pub(crate) fn decode_and_compose_pipeline_into_native(
    data: &[u8],
    metadata: &GifMetadata,
    output: &mut [std::mem::MaybeUninit<u32>],
) -> Result<(), String> {
    let canvas_width = usize::from(metadata.width);
    let canvas_height = usize::from(metadata.height);
    let canvas_pixels = canvas_width
        .checked_mul(canvas_height)
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_pixels = metadata
        .frames
        .len()
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    if output.len() != output_pixels {
        return Err("Host output buffer has the wrong size".to_string());
    }
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let assist_decode = (32..=128).contains(&metadata.frames.len()) && canvas_pixels >= 40_000;
    let decode_thread_count = if assist_decode || canvas_pixels < 10_000 {
        available_threads * 2 / 3
    } else {
        available_threads / 2
    };
    let compose_thread_count = available_threads - decode_thread_count;
    let direct_overlay_output = metadata
        .frames
        .iter()
        .all(|frame| frame.disposal <= 1 && frame_covers_canvas(frame, metadata));
    let rows_per_thread = canvas_height.div_ceil(compose_thread_count);
    let next_frame = std::sync::atomic::AtomicUsize::new(0);
    let mut decoded_indices_length = 0usize;
    let decoded: Vec<PipelineDecodedFrame> = metadata
        .frames
        .iter()
        .map(|frame| {
            let length = usize::from(frame.width)
                .checked_mul(usize::from(frame.height))
                .ok_or_else(|| "Decoded frame size overflow".to_string())?;
            let offset = decoded_indices_length;
            decoded_indices_length = decoded_indices_length
                .checked_add(length)
                .ok_or_else(|| "Decoded frame stream overflow".to_string())?;
            Ok(PipelineDecodedFrame {
                state: std::sync::atomic::AtomicU8::new(0),
                result: std::sync::OnceLock::new(),
                direct_mapped: std::sync::atomic::AtomicBool::new(false),
                offset,
                length,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let flat_decoded_indices = metadata.frames.len() >= 128 && canvas_pixels < 10_000;
    let mut decoded_indices = if flat_decoded_indices {
        uninitialized::<u8>(decoded_indices_length)
    } else {
        Vec::new()
    };
    let decoded_indices_address = if flat_decoded_indices {
        decoded_indices.as_mut_ptr() as usize
    } else {
        0
    };
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
    let output_address = output.as_mut_ptr() as usize;
    let parallel_direct_mapping = direct_overlay_output && metadata.frames.len() >= 100;

    #[cfg(target_os = "macos")]
    {
        let mut context = DecodePipelineDispatchContext {
            data,
            metadata,
            decoded: &decoded,
            decoded_indices_address,
            flat_decoded_indices,
            parallel_direct_mapping,
            palettes: &palettes,
            palettes_share_table,
            canvas_width,
            canvas_height,
            canvas_pixels,
            rows_per_thread,
            decode_thread_count,
            output_address,
            next_frame: &next_frame,
            assist_decode,
            direct_overlay_output,
            error: std::sync::Mutex::new(None),
        };
        unsafe {
            let queue = dispatch_get_global_queue(0, 0);
            if queue.is_null() {
                return Err("Could not acquire the system worker queue".to_string());
            }
            dispatch_apply_f(
                available_threads,
                queue,
                (&mut context as *mut DecodePipelineDispatchContext<'_>).cast(),
                decode_pipeline_dispatch_worker,
            );
        }
        if let Some(error) = context
            .error
            .into_inner()
            .map_err(|_| "Parallel GIF pipeline error lock was poisoned".to_string())?
        {
            return Err(error);
        }
    }

    #[cfg(not(target_os = "macos"))]
    std::thread::scope(|scope| {
        let decode_handles: Vec<_> = (0..decode_thread_count)
            .map(|_| {
                scope.spawn(|| {
                    decode_pipeline_worker(
                        data,
                        metadata,
                        &decoded,
                        decoded_indices_address,
                        flat_decoded_indices,
                        &palettes,
                        palettes_share_table,
                        output_address,
                        canvas_pixels,
                        parallel_direct_mapping,
                        &next_frame,
                        assist_decode,
                    )
                })
            })
            .collect();
        let compose_handles: Vec<_> = (0..compose_thread_count)
            .map(|thread_index| {
                let decoded = &decoded;
                let palettes = &palettes;
                let next_frame = &next_frame;
                scope.spawn(move || {
                    compose_pipeline_stripe(
                        data,
                        metadata,
                        decoded,
                        decoded_indices_address,
                        flat_decoded_indices,
                        parallel_direct_mapping,
                        palettes,
                        palettes_share_table,
                        canvas_width,
                        canvas_height,
                        canvas_pixels,
                        rows_per_thread,
                        thread_index,
                        output_address,
                        next_frame,
                        assist_decode,
                        direct_overlay_output,
                    )
                })
            })
            .collect();
        for handle in decode_handles {
            handle
                .join()
                .map_err(|_| "Parallel GIF decoder panicked".to_string())?;
        }
        for handle in compose_handles {
            handle
                .join()
                .map_err(|_| "Parallel GIF compositor panicked".to_string())??;
        }
        Ok::<_, String>(())
    })?;

    Ok(())
}

pub(crate) fn decode_small_independent_frames_into_native(
    data: &[u8],
    metadata: &GifMetadata,
    canvas_pixels: usize,
    output: &mut [std::mem::MaybeUninit<u32>],
) -> Result<(), String> {
    let output_pixels = metadata
        .frames
        .len()
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    if output.len() != output_pixels {
        return Err("Host output buffer has the wrong size".to_string());
    }
    let mut image_data = Vec::new();
    let mut fixed_image_data = [std::mem::MaybeUninit::<u8>::uninit(); 512];
    let mut lzw_scratch = LzwStackScratch::default();
    let mut indices = Vec::new();
    for (frame_index, frame) in metadata.frames.iter().enumerate() {
        let palette = build_palette_u32(data, frame, PixelFormat::Rgba)?;
        let destination =
            &mut output[frame_index * canvas_pixels..(frame_index + 1) * canvas_pixels];
        if frame_covers_canvas(frame, metadata) && !frame.interlaced {
            let image_bytes = if frame.data_length <= fixed_image_data.len() {
                collect_image_data_into_fixed(data, frame.data_offset, &mut fixed_image_data)?
            } else {
                if image_data.capacity() < frame.data_length {
                    image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
                }
                collect_image_data_into(data, frame.data_offset, &mut image_data)?;
                &image_data
            };
            let destination_bytes = unsafe {
                std::slice::from_raw_parts_mut(destination.as_mut_ptr().cast::<u8>(), canvas_pixels)
            };
            destination_bytes.fill(0);
            if should_decode_lzw_direct(frame, canvas_pixels) {
                lzw_decode_to_indices_direct_with_scratch(
                    frame.min_code_size,
                    image_bytes,
                    destination_bytes,
                    &mut lzw_scratch,
                )?;
            } else {
                lzw_decode_to_indices_stack_with_scratch(
                    frame.min_code_size,
                    image_bytes,
                    destination_bytes,
                    &mut lzw_scratch,
                )?;
            }
            for pixel_index in (0..canvas_pixels).rev() {
                let index = usize::from(destination_bytes[pixel_index]);
                let color = if frame.transparent_index == Some(index as u8) {
                    0
                } else {
                    *palette
                        .get(index)
                        .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?
                };
                destination[pixel_index].write(color);
            }
            continue;
        }
        if !frame.interlaced && frame.data_length <= fixed_image_data.len() {
            let image_bytes =
                collect_image_data_into_fixed(data, frame.data_offset, &mut fixed_image_data)?;
            let frame_size = usize::from(frame.width) * usize::from(frame.height);
            indices.resize(frame_size, 0);
            indices.fill(0);
            if should_decode_lzw_direct(frame, frame_size) {
                lzw_decode_to_indices_direct_with_scratch(
                    frame.min_code_size,
                    image_bytes,
                    &mut indices,
                    &mut lzw_scratch,
                )?;
            } else {
                lzw_decode_to_indices_stack_with_scratch(
                    frame.min_code_size,
                    image_bytes,
                    &mut indices,
                    &mut lzw_scratch,
                )?;
            }
        } else {
            decode_frame_indices_reusing_output(
                data,
                frame,
                &mut image_data,
                &mut lzw_scratch,
                &mut indices,
            )?;
        }
        if !frame_covers_canvas(frame, metadata) {
            destination.fill(std::mem::MaybeUninit::new(0));
            let destination = unsafe {
                std::slice::from_raw_parts_mut(
                    destination.as_mut_ptr().cast::<u32>(),
                    destination.len(),
                )
            };
            blit_indices_to_canvas_u32(&palette, metadata.width, frame, &indices, destination)?;
            continue;
        }
        match frame.transparent_index {
            None => {
                for (pixel, &index) in destination.iter_mut().zip(&indices) {
                    pixel.write(
                        *palette
                            .get(usize::from(index))
                            .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?,
                    );
                }
            }
            Some(transparent_index) => {
                for (pixel, &index) in destination.iter_mut().zip(&indices) {
                    pixel.write(if index == transparent_index {
                        0
                    } else {
                        *palette
                            .get(usize::from(index))
                            .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?
                    });
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn decode_independent_frames_native(
    data: &[u8],
    metadata: &GifMetadata,
) -> Result<Vec<u32>, String> {
    let canvas_pixels = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_pixels = metadata
        .frames
        .len()
        .checked_mul(canvas_pixels)
        .ok_or_else(|| "Prepared frame output overflow".to_string())?;
    if output_pixels < 50_000 {
        let mut output = uninitialized(output_pixels);
        decode_small_independent_frames_into_native(data, metadata, canvas_pixels, &mut output)?;
        return Ok(unsafe { assume_initialized(output) });
    }
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let thread_count = available_threads.min(metadata.frames.len());
    let mut output = uninitialized(output_pixels);
    let output_address = output.as_mut_ptr() as usize;
    let next_frame = std::sync::atomic::AtomicUsize::new(0);

    let worker = || {
        let mut image_data = Vec::new();
        loop {
            let frame_index = next_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let Some(frame) = metadata.frames.get(frame_index) else {
                break;
            };
            let palette = build_palette_u32(data, frame, PixelFormat::Rgba)?;
            let destination = unsafe {
                (output_address as *mut std::mem::MaybeUninit<u32>).add(frame_index * canvas_pixels)
            };
            if frame.interlaced
                || canvas_pixels < 100_000
                || frame.transparent_index.is_some()
                || !frame_covers_canvas(frame, metadata)
            {
                let indices =
                    decode_frame_indices_with_image_scratch(data, frame, &mut image_data)?;
                let destination =
                    unsafe { std::slice::from_raw_parts_mut(destination, canvas_pixels) };
                if !frame_covers_canvas(frame, metadata) {
                    destination.fill(std::mem::MaybeUninit::new(0));
                    let destination = unsafe {
                        std::slice::from_raw_parts_mut(
                            destination.as_mut_ptr().cast::<u32>(),
                            destination.len(),
                        )
                    };
                    blit_indices_to_canvas_u32(
                        &palette,
                        metadata.width,
                        frame,
                        &indices,
                        destination,
                    )?;
                    continue;
                }
                match frame.transparent_index {
                    None => {
                        for (pixel, index) in destination.iter_mut().zip(indices) {
                            pixel.write(*palette.get(usize::from(index)).ok_or_else(|| {
                                format!("Palette index {index} exceeds palette size")
                            })?);
                        }
                    }
                    Some(transparent_index) => {
                        for (pixel, index) in destination.iter_mut().zip(indices) {
                            let color = if index == transparent_index {
                                0
                            } else {
                                *palette.get(usize::from(index)).ok_or_else(|| {
                                    format!("Palette index {index} exceeds palette size")
                                })?
                            };
                            pixel.write(color);
                        }
                    }
                }
                continue;
            }

            if image_data.capacity() < frame.data_length {
                image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
            }
            collect_image_data_into(data, frame.data_offset, &mut image_data)?;
            let destination_bytes =
                unsafe { std::slice::from_raw_parts_mut(destination.cast::<u8>(), canvas_pixels) };
            lzw_decode_to_indices_direct(frame.min_code_size, &image_data, destination_bytes)?;
            if palette.len() == 256 {
                let palette_address = palette.as_ptr();
                let indices_address = destination.cast::<u8>();
                let mut pixel_index = canvas_pixels;
                while pixel_index >= 8 {
                    pixel_index -= 8;
                    let packed_indices = unsafe {
                        std::ptr::read_unaligned(indices_address.add(pixel_index).cast::<u64>())
                    };
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
                            destination.add(pixel_index).cast::<u32>(),
                            colors.len(),
                        );
                    }
                }
                while pixel_index > 0 {
                    pixel_index -= 1;
                    let index = unsafe { usize::from(*indices_address.add(pixel_index)) };
                    let color = unsafe { *palette_address.add(index) };
                    unsafe {
                        destination
                            .add(pixel_index)
                            .write(std::mem::MaybeUninit::new(color));
                    }
                }
                continue;
            }

            for pixel_index in (0..canvas_pixels).rev() {
                let index = unsafe { *destination.cast::<u8>().add(pixel_index) };
                let color = *palette
                    .get(usize::from(index))
                    .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
                unsafe {
                    destination
                        .add(pixel_index)
                        .write(std::mem::MaybeUninit::new(color));
                }
            }
        }
        Ok::<_, String>(())
    };
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..thread_count).map(|_| scope.spawn(worker)).collect();
        for handle in handles {
            handle
                .join()
                .map_err(|_| "Parallel GIF decoder panicked".to_string())??;
        }
        Ok::<_, String>(())
    })?;
    Ok(unsafe { assume_initialized(output) })
}

pub(crate) fn decode_indices_worker(
    data: &[u8],
    metadata: &GifMetadata,
    results_address: usize,
    next_frame: &std::sync::atomic::AtomicUsize,
) {
    loop {
        let frame_index = next_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(frame) = metadata.frames.get(frame_index) else {
            return;
        };
        unsafe {
            (results_address as *const std::sync::OnceLock<Result<Vec<u8>, String>>)
                .add(frame_index)
                .as_ref()
                .expect("parallel result slot exists")
                .set(decode_frame_indices_inner(data, frame))
                .expect("parallel frame is decoded once");
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
pub(crate) struct DecodeIndicesDispatchContext<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) metadata: &'a GifMetadata,
    pub(crate) results_address: usize,
    pub(crate) next_frame: std::sync::atomic::AtomicUsize,
    pub(crate) panicked: std::sync::atomic::AtomicBool,
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
unsafe extern "C" fn decode_indices_dispatch_worker(
    context: *mut std::ffi::c_void,
    _iteration: usize,
) {
    let context = &*(context.cast::<DecodeIndicesDispatchContext<'_>>());
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        decode_indices_worker(
            context.data,
            context.metadata,
            context.results_address,
            &context.next_frame,
        );
    }))
    .is_err();
    if panicked {
        context
            .panicked
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(crate) fn decode_frame_indices_parallel_native(
    data: &[u8],
    metadata: &GifMetadata,
) -> Result<Vec<Vec<u8>>, String> {
    let total_frame_pixels = metadata.frames.iter().try_fold(0usize, |total, frame| {
        usize::from(frame.width)
            .checked_mul(usize::from(frame.height))
            .and_then(|pixels| total.checked_add(pixels))
            .ok_or_else(|| "Decoded frame size overflow".to_string())
    })?;
    let available_threads = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let thread_cap = if total_frame_pixels < 100_000 {
        6
    } else {
        usize::MAX
    };
    let thread_count = available_threads.min(metadata.frames.len()).min(thread_cap);
    if thread_count <= 1 || metadata.frames.len() < 8 || total_frame_pixels < 30_000 {
        let mut image_data = Vec::new();
        let mut decoded = Vec::with_capacity(metadata.frames.len());
        for frame in &metadata.frames {
            decoded.push(decode_frame_indices_with_image_scratch(
                data,
                frame,
                &mut image_data,
            )?);
        }
        return Ok(decoded);
    }

    let results: Vec<std::sync::OnceLock<Result<Vec<u8>, String>>> = (0..metadata.frames.len())
        .map(|_| std::sync::OnceLock::new())
        .collect();
    let results_address = results.as_ptr() as usize;
    let next_frame = std::sync::atomic::AtomicUsize::new(0);
    #[cfg(target_os = "macos")]
    let joined = {
        let mut context = DecodeIndicesDispatchContext {
            data,
            metadata,
            results_address,
            next_frame,
            panicked: std::sync::atomic::AtomicBool::new(false),
        };
        unsafe {
            let queue = dispatch_get_global_queue(0, 0);
            if queue.is_null() {
                return Err("Could not acquire the system worker queue".to_string());
            }
            dispatch_apply_f(
                thread_count,
                queue,
                (&mut context as *mut DecodeIndicesDispatchContext<'_>).cast(),
                decode_indices_dispatch_worker,
            );
        }
        !context.panicked.load(std::sync::atomic::Ordering::Relaxed)
    };
    #[cfg(not(target_os = "macos"))]
    let joined = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..thread_count)
            .map(|_| {
                let next_frame = &next_frame;
                scope.spawn(move || {
                    decode_indices_worker(data, metadata, results_address, next_frame)
                })
            })
            .collect();
        handles.into_iter().all(|handle| handle.join().is_ok())
    });
    if !joined {
        return Err("Parallel GIF decoder panicked".to_string());
    }
    results
        .into_iter()
        .map(|result| {
            result
                .into_inner()
                .ok_or_else(|| "Parallel GIF decoder skipped a frame".to_string())?
        })
        .collect()
}
