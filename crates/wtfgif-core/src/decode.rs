//! Single-frame decoding into indices, pixels, and reusable scratch.

use super::*;

pub(crate) fn decode_frame_indices_inner(
    data: &[u8],
    frame: &FrameMetadata,
) -> Result<Vec<u8>, String> {
    let mut image_data = Vec::new();
    decode_frame_indices_with_image_scratch(data, frame, &mut image_data)
}

pub(crate) fn decode_frame_indices_with_image_scratch(
    data: &[u8],
    frame: &FrameMetadata,
    image_data: &mut Vec<u8>,
) -> Result<Vec<u8>, String> {
    let frame_size = usize::from(frame.width) * usize::from(frame.height);
    if should_decode_lzw_direct(frame, frame_size) {
        let image_data_slice;
        if let Some(range) = single_image_data_range(data, frame.data_offset) {
            image_data_slice = &data[range];
        } else {
            if image_data.capacity() < frame.data_length {
                image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
            }
            collect_image_data_into(data, frame.data_offset, image_data)?;
            image_data_slice = image_data.as_slice();
        }
        let mut output = vec![0; frame_size];
        lzw_decode_to_indices_direct(frame.min_code_size, image_data_slice, &mut output)?;
        return deinterlace_frame_indices(output, frame);
    }
    let mut lzw_scratch = LzwStackScratch::default();
    decode_frame_indices_with_scratches(data, frame, image_data, &mut lzw_scratch)
}

pub(crate) fn decode_frame_indices_with_scratches(
    data: &[u8],
    frame: &FrameMetadata,
    image_data: &mut Vec<u8>,
    lzw_scratch: &mut LzwStackScratch,
) -> Result<Vec<u8>, String> {
    let image_data_slice;
    if let Some(range) = single_image_data_range(data, frame.data_offset) {
        image_data_slice = &data[range];
    } else {
        if image_data.capacity() < frame.data_length {
            image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
        }
        collect_image_data_into(data, frame.data_offset, image_data)?;
        image_data_slice = image_data.as_slice();
    }
    let frame_size = usize::from(frame.width) * usize::from(frame.height);
    let linear = if should_decode_lzw_direct(frame, frame_size) {
        let mut output = vec![0; frame_size];
        if should_decode_lzw_copy(frame, frame_size) {
            lzw_decode_to_indices_copy_with_scratch(
                frame.min_code_size,
                image_data_slice,
                &mut output,
                lzw_scratch,
            )?;
        } else {
            lzw_decode_to_indices_direct_with_scratch(
                frame.min_code_size,
                image_data_slice,
                &mut output,
                lzw_scratch,
            )?;
        }
        output
    } else {
        let mut output = vec![0; frame_size];
        lzw_decode_to_indices_stack_with_scratch(
            frame.min_code_size,
            image_data_slice,
            &mut output,
            lzw_scratch,
        )?;
        output
    };
    deinterlace_frame_indices(linear, frame)
}

pub(crate) fn decode_frame_indices_reusing_output(
    data: &[u8],
    frame: &FrameMetadata,
    image_data: &mut Vec<u8>,
    lzw_scratch: &mut LzwStackScratch,
    output: &mut Vec<u8>,
) -> Result<(), String> {
    if frame.interlaced {
        *output = decode_frame_indices_with_scratches(data, frame, image_data, lzw_scratch)?;
        return Ok(());
    }
    let image_data_slice;
    if let Some(range) = single_image_data_range(data, frame.data_offset) {
        image_data_slice = &data[range];
    } else {
        if image_data.capacity() < frame.data_length {
            image_data.reserve(frame.data_length.saturating_sub(image_data.len()));
        }
        collect_image_data_into(data, frame.data_offset, image_data)?;
        image_data_slice = image_data.as_slice();
    }
    let frame_size = usize::from(frame.width) * usize::from(frame.height);
    output.resize(frame_size, 0);
    if should_decode_lzw_direct(frame, frame_size) {
        if should_decode_lzw_copy(frame, frame_size) {
            lzw_decode_to_indices_copy_with_scratch(
                frame.min_code_size,
                image_data_slice,
                output,
                lzw_scratch,
            )
        } else {
            lzw_decode_to_indices_direct_with_scratch(
                frame.min_code_size,
                image_data_slice,
                output,
                lzw_scratch,
            )
        }
    } else {
        lzw_decode_to_indices_stack_with_scratch(
            frame.min_code_size,
            image_data_slice,
            output,
            lzw_scratch,
        )
    }
}

#[inline]
pub(crate) fn single_image_data_range(
    data: &[u8],
    data_offset: usize,
) -> Option<std::ops::Range<usize>> {
    let length_offset = data_offset.checked_add(1)?;
    let payload_start = length_offset.checked_add(1)?;
    let length = usize::from(*data.get(length_offset)?);
    if length == 0 {
        return Some(payload_start..payload_start);
    }
    let payload_end = payload_start.checked_add(length)?;
    (data.get(payload_end) == Some(&0)).then_some(payload_start..payload_end)
}

#[inline]
pub(crate) fn should_decode_lzw_direct(frame: &FrameMetadata, frame_size: usize) -> bool {
    (frame.min_code_size == 8 && frame_size >= 256)
        || frame_size >= 40_000
        || (frame_size >= 1_000
            && frame.data_length.saturating_mul(20) < frame_size.saturating_mul(13))
        || (frame_size >= 256 && frame.data_length.saturating_mul(5) < frame_size.saturating_mul(2))
}

#[inline]
pub(crate) fn should_decode_lzw_copy(frame: &FrameMetadata, frame_size: usize) -> bool {
    frame.min_code_size <= 6
        || frame_size >= 10_000
        || frame.data_length.saturating_mul(5) < frame_size.saturating_mul(2)
}

pub(crate) fn deinterlace_frame_indices(
    linear: Vec<u8>,
    frame: &FrameMetadata,
) -> Result<Vec<u8>, String> {
    if !frame.interlaced {
        return Ok(linear);
    }

    let frame_size = usize::from(frame.width) * usize::from(frame.height);
    let mut deinterlaced = vec![0; frame_size];
    let width = usize::from(frame.width);
    let height = usize::from(frame.height);
    let mut src = 0usize;

    for (y_start, y_stride) in [(0usize, 8usize), (4, 8), (2, 4), (1, 2)] {
        let mut row = y_start;
        while row < height {
            let dst = row * width;
            for x in 0..width {
                if src >= linear.len() {
                    return Ok(deinterlaced);
                }
                deinterlaced[dst + x] = linear[src];
                src += 1;
            }
            row += y_stride;
        }
    }

    Ok(deinterlaced)
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn decode_frame_pixels_inner(
    data: &[u8],
    metadata: &GifMetadata,
    frame_index: usize,
    format: PixelFormat,
) -> Result<Vec<u8>, String> {
    let frame = metadata
        .frames
        .get(frame_index)
        .ok_or_else(|| "Frame index out of range".to_string())?;
    let canvas_len = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let mut pixels = vec![0; canvas_len];
    let palette = build_palette_pixels(data, frame, format)?;
    let indices = decode_frame_indices_inner(data, frame)?;

    blit_indices_to_pixels(&palette, metadata.width, frame, &indices, &mut pixels)?;

    Ok(pixels)
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn decode_frame_to_scratch(
    data: &[u8],
    metadata: &GifMetadata,
    frame_index: usize,
    format: PixelFormat,
    scratch: &mut FrameDecodeScratch,
) -> Result<usize, String> {
    let frame = metadata
        .frames
        .get(frame_index)
        .ok_or_else(|| "Frame index out of range".to_string())?;
    if frame.interlaced
        || frame.transparent_index.is_some()
        || frame.x != 0
        || frame.y != 0
        || frame.width != metadata.width
        || frame.height != metadata.height
    {
        return Err("Scratch decode requires an opaque full-canvas frame".to_string());
    }

    let canvas_pixels = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    let output_len = canvas_pixels
        .checked_mul(4)
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    if scratch.output.len() != output_len {
        scratch.output.resize(output_len, 0);
    }

    let (prefix, canvas, suffix) = unsafe { scratch.output.align_to_mut::<u32>() };
    if prefix.is_empty() && suffix.is_empty() {
        if scratch.palette_offset != frame.palette_offset
            || scratch.palette_size != frame.palette_size
            || scratch.palette_format != Some(format)
        {
            scratch.palette = build_palette_u32(data, frame, format)?;
            scratch.palette_offset = frame.palette_offset;
            scratch.palette_size = frame.palette_size;
            scratch.palette_format = Some(format);
        }
        let image_data_slice;
        if let Some(range) = single_image_data_range(data, frame.data_offset) {
            image_data_slice = &data[range];
        } else {
            if scratch.image_data.capacity() < frame.data_length {
                scratch
                    .image_data
                    .reserve(frame.data_length.saturating_sub(scratch.image_data.len()));
            }
            collect_image_data_into(data, frame.data_offset, &mut scratch.image_data)?;
            image_data_slice = scratch.image_data.as_slice();
        }
        if lzw_decode_to_pixels_copy_with_scratch(
            frame.min_code_size,
            image_data_slice,
            &scratch.palette,
            canvas,
            &mut scratch.lzw,
        )
        .is_ok()
        {
            return Ok(output_len);
        }
    }

    decode_frame_indices_reusing_output(
        data,
        frame,
        &mut scratch.image_data,
        &mut scratch.lzw,
        &mut scratch.indices,
    )?;
    if scratch.palette_offset != frame.palette_offset
        || scratch.palette_size != frame.palette_size
        || scratch.palette_format != Some(format)
    {
        scratch.palette = build_palette_u32(data, frame, format)?;
        scratch.palette_offset = frame.palette_offset;
        scratch.palette_size = frame.palette_size;
        scratch.palette_format = Some(format);
    }

    let (prefix, canvas, suffix) = unsafe { scratch.output.align_to_mut::<u32>() };
    if prefix.is_empty() && suffix.is_empty() {
        blit_indices_to_canvas_u32(
            &scratch.palette,
            metadata.width,
            frame,
            &scratch.indices,
            canvas,
        )?;
    } else {
        let palette = build_palette_pixels(data, frame, format)?;
        blit_indices_to_pixels(
            &palette,
            metadata.width,
            frame,
            &scratch.indices,
            &mut scratch.output,
        )?;
    }
    Ok(output_len)
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn decode_frame_rect_to_scratch(
    data: &[u8],
    metadata: &GifMetadata,
    frame_index: usize,
    format: PixelFormat,
    scratch: &mut FrameDecodeScratch,
) -> Result<usize, String> {
    let frame = metadata
        .frames
        .get(frame_index)
        .ok_or_else(|| "Frame index out of range".to_string())?;
    let frame_pixels = usize::from(frame.width)
        .checked_mul(usize::from(frame.height))
        .ok_or_else(|| "Decoded frame size overflow".to_string())?;

    scratch.composited_output.resize(frame_pixels, 0);
    if !frame.interlaced {
        if scratch.palette_offset != frame.palette_offset
            || scratch.palette_size != frame.palette_size
            || scratch.palette_format != Some(format)
        {
            scratch.palette = build_palette_u32(data, frame, format)?;
            scratch.palette_offset = frame.palette_offset;
            scratch.palette_size = frame.palette_size;
            scratch.palette_format = Some(format);
        }
        let image_data_slice;
        if let Some(range) = single_image_data_range(data, frame.data_offset) {
            image_data_slice = &data[range];
        } else {
            if scratch.image_data.capacity() < frame.data_length {
                scratch
                    .image_data
                    .reserve(frame.data_length.saturating_sub(scratch.image_data.len()));
            }
            collect_image_data_into(data, frame.data_offset, &mut scratch.image_data)?;
            image_data_slice = scratch.image_data.as_slice();
        }
        if frame.transparent_index.is_none()
            && lzw_decode_to_pixels_copy_with_scratch(
                frame.min_code_size,
                image_data_slice,
                &scratch.palette,
                &mut scratch.composited_output,
                &mut scratch.lzw,
            )
            .is_ok()
        {
            return Ok(frame_pixels);
        }
    }

    decode_frame_indices_reusing_output(
        data,
        frame,
        &mut scratch.image_data,
        &mut scratch.lzw,
        &mut scratch.indices,
    )?;
    if scratch.palette_offset != frame.palette_offset
        || scratch.palette_size != frame.palette_size
        || scratch.palette_format != Some(format)
    {
        scratch.palette = build_palette_u32(data, frame, format)?;
        scratch.palette_offset = frame.palette_offset;
        scratch.palette_size = frame.palette_size;
        scratch.palette_format = Some(format);
    }

    let transparent_index = frame.transparent_index;
    for (destination, &index) in scratch
        .composited_output
        .iter_mut()
        .zip(scratch.indices.iter().take(frame_pixels))
    {
        *destination = if transparent_index == Some(index) {
            0
        } else {
            *scratch
                .palette
                .get(usize::from(index))
                .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?
        };
    }
    Ok(frame_pixels)
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn decode_and_blit_frame_reusing_scratch(
    data: &[u8],
    metadata: &GifMetadata,
    frame_index: usize,
    format: PixelFormat,
    pixels: &mut [u8],
    scratch: &mut FrameDecodeScratch,
) -> Result<(), String> {
    let expected_length = usize::from(metadata.width)
        .checked_mul(usize::from(metadata.height))
        .and_then(|pixel_count| pixel_count.checked_mul(4))
        .ok_or_else(|| "Canvas size overflow".to_string())?;
    if pixels.len() < expected_length {
        return Err("Pixel buffer is too small".to_string());
    }
    let frame = metadata
        .frames
        .get(frame_index)
        .ok_or_else(|| "Frame index out of range".to_string())?;

    let target = &mut pixels[..expected_length];
    let (prefix, canvas, suffix) = unsafe { target.align_to_mut::<u32>() };
    if prefix.is_empty()
        && suffix.is_empty()
        && !frame.interlaced
        && frame.transparent_index.is_none()
        && frame.x == 0
        && frame.y == 0
        && frame.width == metadata.width
        && frame.height == metadata.height
    {
        if scratch.palette_offset != frame.palette_offset
            || scratch.palette_size != frame.palette_size
            || scratch.palette_format != Some(format)
        {
            scratch.palette = build_palette_u32(data, frame, format)?;
            scratch.palette_offset = frame.palette_offset;
            scratch.palette_size = frame.palette_size;
            scratch.palette_format = Some(format);
        }
        let image_data_slice;
        if let Some(range) = single_image_data_range(data, frame.data_offset) {
            image_data_slice = &data[range];
        } else {
            if scratch.image_data.capacity() < frame.data_length {
                scratch
                    .image_data
                    .reserve(frame.data_length.saturating_sub(scratch.image_data.len()));
            }
            collect_image_data_into(data, frame.data_offset, &mut scratch.image_data)?;
            image_data_slice = scratch.image_data.as_slice();
        }
        if lzw_decode_to_pixels_copy_with_scratch(
            frame.min_code_size,
            image_data_slice,
            &scratch.palette,
            canvas,
            &mut scratch.lzw,
        )
        .is_ok()
        {
            return Ok(());
        }
    }

    decode_frame_indices_reusing_output(
        data,
        frame,
        &mut scratch.image_data,
        &mut scratch.lzw,
        &mut scratch.indices,
    )?;
    let (prefix, canvas, suffix) = unsafe { target.align_to_mut::<u32>() };
    if prefix.is_empty() && suffix.is_empty() {
        if scratch.palette_offset != frame.palette_offset
            || scratch.palette_size != frame.palette_size
            || scratch.palette_format != Some(format)
        {
            scratch.palette = build_palette_u32(data, frame, format)?;
            scratch.palette_offset = frame.palette_offset;
            scratch.palette_size = frame.palette_size;
            scratch.palette_format = Some(format);
        }
        return blit_indices_to_canvas_u32(
            &scratch.palette,
            metadata.width,
            frame,
            &scratch.indices,
            canvas,
        );
    }
    let palette = build_palette_pixels(data, frame, format)?;
    blit_indices_to_pixels(&palette, metadata.width, frame, &scratch.indices, target)
}
