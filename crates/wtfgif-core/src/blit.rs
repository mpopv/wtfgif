//! Palette lookup tables and index-to-canvas blitting.

use super::*;

#[cfg(not(feature = "encode-only"))]
pub(crate) fn build_palette_pixels(
    data: &[u8],
    frame: &FrameMetadata,
    format: PixelFormat,
) -> Result<Vec<u8>, String> {
    let byte_len = frame
        .palette_size
        .checked_mul(4)
        .ok_or_else(|| "Palette size overflow".to_string())?;
    let mut palette = vec![0; byte_len];

    for index in 0..frame.palette_size {
        let palette_offset = frame
            .palette_offset
            .checked_add(index * 3)
            .ok_or_else(|| "Palette offset overflow".to_string())?;
        let end = checked_add(palette_offset, 3, data.len(), "palette color")?;
        let dst = index * 4;
        let r = data[palette_offset];
        let g = data[palette_offset + 1];
        let b = data[end - 1];
        match format {
            PixelFormat::Rgba => {
                palette[dst] = r;
                palette[dst + 1] = g;
                palette[dst + 2] = b;
            }
            PixelFormat::Bgra => {
                palette[dst] = b;
                palette[dst + 1] = g;
                palette[dst + 2] = r;
            }
        }
        palette[dst + 3] = 255;
    }

    Ok(palette)
}

pub(crate) fn build_palette_u32(
    data: &[u8],
    frame: &FrameMetadata,
    format: PixelFormat,
) -> Result<Vec<u32>, String> {
    let palette_bytes = frame
        .palette_size
        .checked_mul(3)
        .ok_or_else(|| "Palette size overflow".to_string())?;
    let palette_end = checked_add(
        frame.palette_offset,
        palette_bytes,
        data.len(),
        "palette color table",
    )?;
    let mut palette = Vec::with_capacity(frame.palette_size);
    for color in data[frame.palette_offset..palette_end].chunks_exact(3) {
        let r = u32::from(color[0]);
        let g = u32::from(color[1]);
        let b = u32::from(color[2]);
        palette.push(match format {
            PixelFormat::Rgba => r | (g << 8) | (b << 16) | (255 << 24),
            #[cfg(not(feature = "encode-only"))]
            PixelFormat::Bgra => b | (g << 8) | (r << 16) | (255 << 24),
        });
    }

    Ok(palette)
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn blit_indices_to_pixels(
    palette: &[u8],
    canvas_width: u16,
    frame: &FrameMetadata,
    indices: &[u8],
    pixels: &mut [u8],
) -> Result<(), String> {
    let canvas_width = usize::from(canvas_width);
    let frame_width = usize::from(frame.width);
    let frame_height = usize::from(frame.height);
    let frame_x = usize::from(frame.x);
    let frame_y = usize::from(frame.y);
    let transparent_index = frame.transparent_index;
    let mut src = 0usize;

    for y in 0..frame_height {
        let mut dst = ((frame_y + y) * canvas_width + frame_x)
            .checked_mul(4)
            .ok_or_else(|| "Frame destination offset overflow".to_string())?;

        for _ in 0..frame_width {
            let index = *indices
                .get(src)
                .ok_or_else(|| "Decoded index buffer is too short".to_string())?;
            src += 1;

            if Some(index) == transparent_index {
                dst += 4;
                continue;
            }

            write_palette_pixel(palette, index, pixels, dst)?;
            dst += 4;
        }
    }

    Ok(())
}

pub(crate) fn blit_indices_to_canvas_u32(
    palette: &[u32],
    canvas_width: u16,
    frame: &FrameMetadata,
    indices: &[u8],
    canvas: &mut [u32],
) -> Result<(), String> {
    let canvas_width = usize::from(canvas_width);
    let frame_width = usize::from(frame.width);
    let frame_height = usize::from(frame.height);
    let frame_x = usize::from(frame.x);
    let frame_y = usize::from(frame.y);
    let frame_pixels = frame_width
        .checked_mul(frame_height)
        .ok_or_else(|| "Decoded frame size overflow".to_string())?;
    if indices.len() < frame_pixels {
        return Err("Decoded index buffer is too short".to_string());
    }
    let frame_right = frame_x
        .checked_add(frame_width)
        .ok_or_else(|| "Frame destination offset overflow".to_string())?;
    let frame_bottom = frame_y
        .checked_add(frame_height)
        .ok_or_else(|| "Frame destination offset overflow".to_string())?;
    if frame_right > canvas_width || canvas_width == 0 || frame_bottom > canvas.len() / canvas_width
    {
        return Err("Frame destination exceeds canvas bounds".to_string());
    }
    let full_byte_palette = palette.len() == 256;
    if full_byte_palette
        && frame.transparent_index.is_none()
        && frame_x == 0
        && frame_y == 0
        && frame_width == canvas_width
        && frame_height == canvas.len() / canvas_width
    {
        // The common GIF case is an opaque full-canvas frame with a complete
        // 256-entry palette. Decode eight index bytes per iteration so the
        // palette lookups and stores stay in a tight pointer loop.
        let indices_pointer = indices.as_ptr();
        let palette_pointer = palette.as_ptr();
        let canvas_pointer = canvas.as_mut_ptr();
        let mut pixel_index = 0usize;
        while pixel_index + 8 <= frame_pixels {
            let packed =
                unsafe { std::ptr::read_unaligned(indices_pointer.add(pixel_index).cast::<u64>()) };
            let colors = unsafe {
                [
                    *palette_pointer.add((packed & 0xff) as usize),
                    *palette_pointer.add(((packed >> 8) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 16) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 24) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 32) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 40) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 48) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 56) & 0xff) as usize),
                ]
            };
            unsafe {
                std::ptr::copy_nonoverlapping(
                    colors.as_ptr(),
                    canvas_pointer.add(pixel_index),
                    colors.len(),
                );
            }
            pixel_index += 8;
        }
        while pixel_index < frame_pixels {
            let index = unsafe { *indices_pointer.add(pixel_index) };
            unsafe {
                canvas_pointer
                    .add(pixel_index)
                    .write(*palette_pointer.add(usize::from(index)));
            }
            pixel_index += 1;
        }
        return Ok(());
    }
    match frame.transparent_index {
        None => {
            for y in 0..frame_height {
                let source = y * frame_width;
                let source_row = &indices[source..source + frame_width];
                let destination = (frame_y + y) * canvas_width + frame_x;
                let destination_row = &mut canvas[destination..destination + frame_width];
                if full_byte_palette {
                    let source_pointer = source_row.as_ptr();
                    let palette_pointer = palette.as_ptr();
                    let destination_pointer = destination_row.as_mut_ptr();
                    let mut pixel_index = 0usize;
                    while pixel_index + 8 <= frame_width {
                        let packed = unsafe {
                            std::ptr::read_unaligned(source_pointer.add(pixel_index).cast::<u64>())
                        };
                        let colors = unsafe {
                            [
                                *palette_pointer.add((packed & 0xff) as usize),
                                *palette_pointer.add(((packed >> 8) & 0xff) as usize),
                                *palette_pointer.add(((packed >> 16) & 0xff) as usize),
                                *palette_pointer.add(((packed >> 24) & 0xff) as usize),
                                *palette_pointer.add(((packed >> 32) & 0xff) as usize),
                                *palette_pointer.add(((packed >> 40) & 0xff) as usize),
                                *palette_pointer.add(((packed >> 48) & 0xff) as usize),
                                *palette_pointer.add(((packed >> 56) & 0xff) as usize),
                            ]
                        };
                        unsafe {
                            std::ptr::copy_nonoverlapping(
                                colors.as_ptr(),
                                destination_pointer.add(pixel_index),
                                colors.len(),
                            );
                        }
                        pixel_index += 8;
                    }
                    while pixel_index < frame_width {
                        let index = unsafe { *source_pointer.add(pixel_index) };
                        unsafe {
                            destination_pointer
                                .add(pixel_index)
                                .write(*palette_pointer.add(usize::from(index)));
                        }
                        pixel_index += 1;
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
            for y in 0..frame_height {
                let source = y * frame_width;
                let source_row = &indices[source..source + frame_width];
                let destination = (frame_y + y) * canvas_width + frame_x;
                let destination_row = &mut canvas[destination..destination + frame_width];
                if full_byte_palette {
                    let source_pointer = source_row.as_ptr();
                    let palette_pointer = palette.as_ptr();
                    let destination_pointer = destination_row.as_mut_ptr();
                    let transparent_bytes =
                        u64::from(transparent_index).wrapping_mul(0x0101_0101_0101_0101);
                    let mut pixel_index = 0usize;
                    while pixel_index + 8 <= frame_width {
                        let packed_indices = unsafe {
                            std::ptr::read_unaligned(source_pointer.add(pixel_index).cast::<u64>())
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
                                    *palette_pointer.add((packed_indices & 0xff) as usize),
                                    *palette_pointer.add(((packed_indices >> 8) & 0xff) as usize),
                                    *palette_pointer.add(((packed_indices >> 16) & 0xff) as usize),
                                    *palette_pointer.add(((packed_indices >> 24) & 0xff) as usize),
                                    *palette_pointer.add(((packed_indices >> 32) & 0xff) as usize),
                                    *palette_pointer.add(((packed_indices >> 40) & 0xff) as usize),
                                    *palette_pointer.add(((packed_indices >> 48) & 0xff) as usize),
                                    *palette_pointer.add(((packed_indices >> 56) & 0xff) as usize),
                                ]
                            };
                            unsafe {
                                std::ptr::copy_nonoverlapping(
                                    colors.as_ptr(),
                                    destination_pointer.add(pixel_index),
                                    colors.len(),
                                );
                            }
                            pixel_index += 8;
                            continue;
                        }
                        for lane in 0..8 {
                            let index = unsafe { *source_pointer.add(pixel_index + lane) };
                            if index != transparent_index {
                                unsafe {
                                    destination_pointer
                                        .add(pixel_index + lane)
                                        .write(*palette_pointer.add(usize::from(index)));
                                }
                            }
                        }
                        pixel_index += 8;
                    }
                    while pixel_index < frame_width {
                        let index = unsafe { *source_pointer.add(pixel_index) };
                        if index != transparent_index {
                            unsafe {
                                destination_pointer
                                    .add(pixel_index)
                                    .write(*palette_pointer.add(usize::from(index)));
                            }
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

pub(crate) fn blit_indices_to_uninit_full_canvas_u32(
    palette: &[u32],
    indices: &[u8],
    output: &mut [std::mem::MaybeUninit<u32>],
) -> Result<(), String> {
    if indices.len() != output.len() {
        return Err("Decoded index buffer is the wrong size".to_string());
    }
    if palette.len() == 256 {
        let indices_pointer = indices.as_ptr();
        let palette_pointer = palette.as_ptr();
        let output_pointer = output.as_mut_ptr().cast::<u32>();
        let mut pixel_index = 0usize;
        while pixel_index + 8 <= indices.len() {
            let packed =
                unsafe { std::ptr::read_unaligned(indices_pointer.add(pixel_index).cast::<u64>()) };
            let colors = unsafe {
                [
                    *palette_pointer.add((packed & 0xff) as usize),
                    *palette_pointer.add(((packed >> 8) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 16) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 24) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 32) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 40) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 48) & 0xff) as usize),
                    *palette_pointer.add(((packed >> 56) & 0xff) as usize),
                ]
            };
            unsafe {
                std::ptr::copy_nonoverlapping(
                    colors.as_ptr(),
                    output_pointer.add(pixel_index),
                    colors.len(),
                );
            }
            pixel_index += 8;
        }
        while pixel_index < indices.len() {
            let index = unsafe { *indices_pointer.add(pixel_index) };
            if usize::from(index) >= palette.len() {
                return Err(format!("Palette index {index} exceeds palette size"));
            }
            unsafe {
                output_pointer
                    .add(pixel_index)
                    .write(*palette_pointer.add(usize::from(index)));
            }
            pixel_index += 1;
        }
        return Ok(());
    }

    for (pixel, &index) in output.iter_mut().zip(indices) {
        let color = palette
            .get(usize::from(index))
            .ok_or_else(|| format!("Palette index {index} exceeds palette size"))?;
        pixel.write(*color);
    }
    Ok(())
}

pub(crate) fn apply_frame_disposal_u32(
    canvas: &mut [u32],
    canvas_width: u16,
    canvas_height: u16,
    frame: &FrameMetadata,
    restore: Option<Vec<u32>>,
) {
    if frame.disposal == 2 {
        clear_frame_rect_u32(canvas, canvas_width, canvas_height, frame);
    } else if frame.disposal == 3 {
        if let Some(restore) = restore {
            canvas.copy_from_slice(&restore);
        }
    }
}

pub(crate) fn clear_frame_rect_u32(
    canvas: &mut [u32],
    canvas_width: u16,
    canvas_height: u16,
    frame: &FrameMetadata,
) {
    let canvas_width = usize::from(canvas_width);
    let canvas_height = usize::from(canvas_height);
    let x = usize::from(frame.x).min(canvas_width);
    let y = usize::from(frame.y).min(canvas_height);
    let right = usize::from(frame.x)
        .saturating_add(usize::from(frame.width))
        .min(canvas_width);
    let bottom = usize::from(frame.y)
        .saturating_add(usize::from(frame.height))
        .min(canvas_height);

    if right <= x {
        return;
    }

    for row in y..bottom {
        let start = row * canvas_width + x;
        canvas[start..start + (right - x)].fill(0);
    }
}

#[inline]
#[cfg(not(feature = "encode-only"))]
pub(crate) fn write_palette_pixel(
    palette: &[u8],
    index: u8,
    pixels: &mut [u8],
    dst: usize,
) -> Result<(), String> {
    let color = usize::from(index)
        .checked_mul(4)
        .ok_or_else(|| "Palette index overflow".to_string())?;
    if color + 3 >= palette.len() {
        return Err(format!("Palette index {index} exceeds palette size"));
    }
    if dst + 3 >= pixels.len() {
        return Err("Frame destination exceeds canvas bounds".to_string());
    }

    pixels[dst] = palette[color];
    pixels[dst + 1] = palette[color + 1];
    pixels[dst + 2] = palette[color + 2];
    pixels[dst + 3] = palette[color + 3];

    Ok(())
}
