use super::*;

#[cfg(not(target_arch = "wasm32"))]
#[repr(C)]
pub struct NativeDecodedGif {
    pixels: *mut u8,
    byte_len: usize,
    width: u32,
    height: u32,
    frame_count: u32,
    parse_nanos: u64,
    decode_nanos: u64,
    compose_nanos: u64,
    host_owned: i32,
}

#[cfg(not(target_arch = "wasm32"))]
type NativeRgbaAllocator =
    unsafe extern "C" fn(context: *mut std::ffi::c_void, byte_len: usize) -> *mut u8;

#[cfg(not(target_arch = "wasm32"))]
#[repr(C)]
pub struct NativeEncodedGif {
    bytes: *mut u8,
    byte_len: usize,
    byte_capacity: usize,
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
/// Decodes all GIF frames into a Rust-owned RGBA allocation.
///
/// # Safety
///
/// `data` must reference `data_len` readable bytes and `decoded` must be a
/// valid writable pointer. The returned pixels must be released with
/// `wtfgif_free_rgba` using the reported byte length.
pub unsafe extern "C" fn wtfgif_decode_all_rgba(
    data: *const u8,
    data_len: usize,
    decoded: *mut NativeDecodedGif,
) -> i32 {
    wtfgif_decode_all_rgba_inner(data, data_len, None, std::ptr::null_mut(), decoded)
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
/// Decodes all GIF frames into memory supplied by the host allocator.
///
/// # Safety
///
/// `data` must reference `data_len` readable bytes, `decoded` must be writable,
/// and `allocate` must return a writable allocation of at least the requested
/// size that remains valid after this call returns.
pub unsafe extern "C" fn wtfgif_decode_all_rgba_host(
    data: *const u8,
    data_len: usize,
    allocate: NativeRgbaAllocator,
    allocate_context: *mut std::ffi::c_void,
    decoded: *mut NativeDecodedGif,
) -> i32 {
    wtfgif_decode_all_rgba_inner(data, data_len, Some(allocate), allocate_context, decoded)
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn wtfgif_decode_all_rgba_inner(
    data: *const u8,
    data_len: usize,
    allocate: Option<NativeRgbaAllocator>,
    allocate_context: *mut std::ffi::c_void,
    decoded: *mut NativeDecodedGif,
) -> i32 {
    if data.is_null() || decoded.is_null() {
        return 0;
    }
    let data = std::slice::from_raw_parts(data, data_len);
    let Ok(metadata) = parse_metadata(data) else {
        return 0;
    };
    let canvas_pixels = usize::from(metadata.width).checked_mul(usize::from(metadata.height));
    let output_pixels = canvas_pixels.and_then(|pixels| pixels.checked_mul(metadata.frames.len()));
    if let (Some(allocate), Some(canvas_pixels), Some(output_pixels)) =
        (allocate, canvas_pixels, output_pixels)
    {
        if output_pixels < 50_000 && frames_are_independent_native(&metadata) {
            let byte_len = match output_pixels.checked_mul(std::mem::size_of::<u32>()) {
                Some(byte_len) => byte_len,
                None => return 0,
            };
            let pixels_ptr = allocate(allocate_context, byte_len);
            if pixels_ptr.is_null() {
                return 0;
            }
            let output = std::slice::from_raw_parts_mut(
                pixels_ptr.cast::<std::mem::MaybeUninit<u32>>(),
                output_pixels,
            );
            if decode_small_independent_frames_into_native(data, &metadata, canvas_pixels, output)
                .is_err()
            {
                return 0;
            }
            decoded.write(NativeDecodedGif {
                pixels: pixels_ptr,
                byte_len,
                width: u32::from(metadata.width),
                height: u32::from(metadata.height),
                frame_count: metadata.frames.len() as u32,
                parse_nanos: 0,
                decode_nanos: 0,
                compose_nanos: 0,
                host_owned: 1,
            });
            return 1;
        }
        if let Some(segments) = independent_frame_segments_native(&metadata) {
            let byte_len = match output_pixels.checked_mul(std::mem::size_of::<u32>()) {
                Some(byte_len) => byte_len,
                None => return 0,
            };
            let pixels_ptr = allocate(allocate_context, byte_len);
            if pixels_ptr.is_null() {
                return 0;
            }
            let output = std::slice::from_raw_parts_mut(
                pixels_ptr.cast::<std::mem::MaybeUninit<u32>>(),
                output_pixels,
            );
            if decode_segmented_frames_into_native(data, &metadata, &segments, output).is_err() {
                return 0;
            }
            decoded.write(NativeDecodedGif {
                pixels: pixels_ptr,
                byte_len,
                width: u32::from(metadata.width),
                height: u32::from(metadata.height),
                frame_count: metadata.frames.len() as u32,
                parse_nanos: 0,
                decode_nanos: 0,
                compose_nanos: 0,
                host_owned: 1,
            });
            return 1;
        }
        let mixed_pipeline_output = output_pixels >= 2_000_000
            && metadata
                .frames
                .iter()
                .any(|frame| frame.disposal > 1 || !frame_covers_canvas(frame, &metadata));
        if mixed_pipeline_output && should_pipeline_decode_native(&metadata) {
            let byte_len = match output_pixels.checked_mul(std::mem::size_of::<u32>()) {
                Some(byte_len) => byte_len,
                None => return 0,
            };
            let pixels_ptr = allocate(allocate_context, byte_len);
            if pixels_ptr.is_null() {
                return 0;
            }
            let output = std::slice::from_raw_parts_mut(
                pixels_ptr.cast::<std::mem::MaybeUninit<u32>>(),
                output_pixels,
            );
            if decode_and_compose_pipeline_into_native(data, &metadata, output).is_err() {
                return 0;
            }
            decoded.write(NativeDecodedGif {
                pixels: pixels_ptr,
                byte_len,
                width: u32::from(metadata.width),
                height: u32::from(metadata.height),
                frame_count: metadata.frames.len() as u32,
                parse_nanos: 0,
                decode_nanos: 0,
                compose_nanos: 0,
                host_owned: 1,
            });
            return 1;
        }
    }

    let result = (|| {
        let pixels = if frames_are_independent_native(&metadata) {
            decode_independent_frames_native(data, &metadata)?
        } else if let Some(segments) = independent_frame_segments_native(&metadata) {
            decode_segmented_frames_native(data, &metadata, &segments)?
        } else if should_fuse_sequential_decode_native(&metadata) {
            prepare_all_composited_frames_inner(data, &metadata, PixelFormat::Rgba)?
        } else if should_pipeline_decode_native(&metadata) {
            decode_and_compose_pipeline_native(data, &metadata)?
        } else {
            let decoded_indices = decode_frame_indices_parallel_native(data, &metadata)?;
            compose_decoded_frames_native(data, &metadata, &decoded_indices)?
        };
        Ok::<_, String>(pixels)
    })();
    let Ok(pixels) = result else {
        return 0;
    };
    let mut pixels = pixels.into_boxed_slice();
    let byte_len = pixels.len() * std::mem::size_of::<u32>();
    let pixels_ptr = pixels.as_mut_ptr().cast::<u8>();
    std::mem::forget(pixels);
    decoded.write(NativeDecodedGif {
        pixels: pixels_ptr,
        byte_len,
        width: u32::from(metadata.width),
        height: u32::from(metadata.height),
        frame_count: metadata.frames.len() as u32,
        parse_nanos: 0,
        decode_nanos: 0,
        compose_nanos: 0,
        host_owned: 0,
    });
    1
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
/// Releases pixels returned by `wtfgif_decode_all_rgba`.
///
/// # Safety
///
/// `pixels` and `byte_len` must be the exact pair returned by a successful
/// Rust-owned decode and must not have been released previously.
pub unsafe extern "C" fn wtfgif_free_rgba(pixels: *mut u8, byte_len: usize) {
    if pixels.is_null() || byte_len == 0 || byte_len % std::mem::size_of::<u32>() != 0 {
        return;
    }
    let pixel_len = byte_len / std::mem::size_of::<u32>();
    drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
        pixels.cast::<u32>(),
        pixel_len,
    )));
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
/// Encodes RGBA frames using the fast native profile.
///
/// # Safety
///
/// Every non-null input pointer must reference the number of readable elements
/// given by its corresponding length, and `encoded` must be writable.
pub unsafe extern "C" fn wtfgif_encode_rgba_fast(
    rgba_stream: *const u8,
    rgba_len: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: *const u32,
    palette_len: usize,
    delays: *const u16,
    delay_count: usize,
    loop_count: i32,
    deltas: i32,
    encoded: *mut NativeEncodedGif,
) -> i32 {
    if rgba_stream.is_null()
        || encoded.is_null()
        || (palette_len != 0 && palette_rgb.is_null())
        || (delay_count != 0 && delays.is_null())
    {
        return 0;
    }

    let rgba_stream = std::slice::from_raw_parts(rgba_stream, rgba_len);
    let palette_rgb = if palette_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(palette_rgb, palette_len)
    };
    let delays = if delay_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(delays, delay_count)
    };
    let result = encode_rgba_gif_inner(
        rgba_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::PerFrame(delays),
        loop_count,
        deltas != 0,
        TRANSPARENT_ALPHA_THRESHOLD,
        true,
    );
    let Ok(bytes) = result else {
        return 0;
    };

    let mut bytes = bytes;
    let byte_len = bytes.len();
    let byte_capacity = bytes.capacity();
    let bytes_ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    encoded.write(NativeEncodedGif {
        bytes: bytes_ptr,
        byte_len,
        byte_capacity,
    });
    1
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
/// Encodes RGBA frames using the quality native profile.
///
/// # Safety
///
/// Every non-null input pointer must reference the number of readable elements
/// given by its corresponding length, and `encoded` must be writable.
pub unsafe extern "C" fn wtfgif_encode_rgba_quality(
    rgba_stream: *const u8,
    rgba_len: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    delays: *const u16,
    delay_count: usize,
    loop_count: i32,
    alpha_threshold: u8,
    encoded: *mut NativeEncodedGif,
) -> i32 {
    if rgba_stream.is_null() || encoded.is_null() || (delay_count != 0 && delays.is_null()) {
        return 0;
    }

    let rgba_stream = std::slice::from_raw_parts(rgba_stream, rgba_len);
    let delays = if delay_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(delays, delay_count)
    };
    let Ok(mut bytes) = encode_rgba_quality_gif_inner_with_output(
        rgba_stream,
        width,
        height,
        frame_count,
        delays,
        loop_count,
        alpha_threshold,
        Vec::new(),
    ) else {
        return 0;
    };

    let byte_len = bytes.len();
    let byte_capacity = bytes.capacity();
    let bytes_ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    encoded.write(NativeEncodedGif {
        bytes: bytes_ptr,
        byte_len,
        byte_capacity,
    });
    1
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
/// Encodes RGBA frames using the balanced native profile.
///
/// # Safety
///
/// Every non-null input pointer must reference the number of readable elements
/// given by its corresponding length, and `encoded` must be writable.
pub unsafe extern "C" fn wtfgif_encode_rgba_balanced(
    rgba_stream: *const u8,
    rgba_len: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: *const u32,
    palette_len: usize,
    delays: *const u16,
    delay_count: usize,
    loop_count: i32,
    deltas: i32,
    alpha_threshold: u8,
    quantization: u8,
    encoded: *mut NativeEncodedGif,
) -> i32 {
    if rgba_stream.is_null()
        || encoded.is_null()
        || (palette_len != 0 && palette_rgb.is_null())
        || (delay_count != 0 && delays.is_null())
    {
        return 0;
    }

    let Ok(quantization) = RgbaQuantization::from_u8(quantization) else {
        return 0;
    };
    let rgba_stream = std::slice::from_raw_parts(rgba_stream, rgba_len);
    let palette_rgb = if palette_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(palette_rgb, palette_len)
    };
    let delays = if delay_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(delays, delay_count)
    };
    let result = encode_rgba_gif_advanced_inner(
        rgba_stream,
        width,
        height,
        frame_count,
        palette_rgb,
        DelaySource::PerFrame(delays),
        loop_count,
        deltas != 0,
        alpha_threshold,
        false,
        quantization,
        RgbaPaletteMode::Global,
    );
    let Ok(mut bytes) = result else {
        return 0;
    };

    let byte_len = bytes.len();
    let byte_capacity = bytes.capacity();
    let bytes_ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    encoded.write(NativeEncodedGif {
        bytes: bytes_ptr,
        byte_len,
        byte_capacity,
    });
    1
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
/// Encodes indexed frames using the fast native profile.
///
/// # Safety
///
/// Every input pointer must reference the number of readable elements given by
/// its corresponding length, and `encoded` must be writable.
pub unsafe extern "C" fn wtfgif_encode_indexed_fast(
    index_stream: *const u8,
    index_len: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: *const u32,
    palette_len: usize,
    delays: *const u16,
    delay_count: usize,
    loop_count: i32,
    deltas: i32,
    encoded: *mut NativeEncodedGif,
) -> i32 {
    if index_stream.is_null() || encoded.is_null() || palette_rgb.is_null() || delays.is_null() {
        return 0;
    }
    let index_stream = std::slice::from_raw_parts(index_stream, index_len);
    let palette_rgb = std::slice::from_raw_parts(palette_rgb, palette_len);
    let delays = std::slice::from_raw_parts(delays, delay_count);
    let delay_source = DelaySource::PerFrame(delays);
    let result = if deltas != 0 {
        encode_indexed_literal_delta_gif_inner(
            index_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delay_source,
            loop_count,
        )
    } else {
        encode_indexed_literal_gif_inner(
            index_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delay_source,
            loop_count,
            None,
        )
    };
    let Ok(mut bytes) = result else {
        return 0;
    };
    let byte_len = bytes.len();
    let byte_capacity = bytes.capacity();
    let bytes_ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    encoded.write(NativeEncodedGif {
        bytes: bytes_ptr,
        byte_len,
        byte_capacity,
    });
    1
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
/// Encodes indexed frames using the balanced native profile.
///
/// # Safety
///
/// Every input pointer must reference the number of readable elements given by
/// its corresponding length, and `encoded` must be writable.
pub unsafe extern "C" fn wtfgif_encode_indexed_balanced(
    index_stream: *const u8,
    index_len: usize,
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: *const u32,
    palette_len: usize,
    delays: *const u16,
    delay_count: usize,
    loop_count: i32,
    deltas: i32,
    encoded: *mut NativeEncodedGif,
) -> i32 {
    if index_stream.is_null() || encoded.is_null() || palette_rgb.is_null() || delays.is_null() {
        return 0;
    }
    let index_stream = std::slice::from_raw_parts(index_stream, index_len);
    let palette_rgb = std::slice::from_raw_parts(palette_rgb, palette_len);
    let delays = std::slice::from_raw_parts(delays, delay_count);
    let delay_source = DelaySource::PerFrame(delays);
    let result = if deltas != 0 {
        encode_indexed_delta_gif_inner(
            index_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delay_source,
            loop_count,
        )
    } else {
        encode_indexed_gif_inner(
            index_stream,
            width,
            height,
            frame_count,
            palette_rgb,
            delay_source,
            loop_count,
            None,
        )
    };
    let Ok(mut bytes) = result else {
        return 0;
    };
    let byte_len = bytes.len();
    let byte_capacity = bytes.capacity();
    let bytes_ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    encoded.write(NativeEncodedGif {
        bytes: bytes_ptr,
        byte_len,
        byte_capacity,
    });
    1
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
/// Reencodes a GIF into a Rust-owned byte allocation.
///
/// # Safety
///
/// `data` must reference `data_len` readable bytes and `encoded` must be
/// writable. Release the returned bytes with `wtfgif_free_bytes`.
pub unsafe extern "C" fn wtfgif_reencode_gif_fast(
    data: *const u8,
    data_len: usize,
    encoded: *mut NativeEncodedGif,
) -> i32 {
    if data.is_null() || encoded.is_null() {
        return 0;
    }
    let data = std::slice::from_raw_parts(data, data_len);
    let result = (|| {
        let metadata = parse_metadata(data)?;
        let loop_count = metadata.loop_count.map(i32::from).unwrap_or(-1);
        reencode_gif_literal_parallel_native(data, &metadata, loop_count)
    })();
    let Ok(bytes) = result else {
        return 0;
    };

    let mut bytes = bytes;
    let byte_len = bytes.len();
    let byte_capacity = bytes.capacity();
    let bytes_ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    encoded.write(NativeEncodedGif {
        bytes: bytes_ptr,
        byte_len,
        byte_capacity,
    });
    1
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
/// Reencodes a GIF into memory supplied by the host allocator.
///
/// # Safety
///
/// `data` must reference `data_len` readable bytes, `encoded` must be writable,
/// and `allocate` must return a writable allocation of at least the requested
/// size that remains valid after this call returns.
pub unsafe extern "C" fn wtfgif_reencode_gif_fast_host(
    data: *const u8,
    data_len: usize,
    allocate: NativeRgbaAllocator,
    allocate_context: *mut std::ffi::c_void,
    encoded: *mut NativeEncodedGif,
) -> i32 {
    if data.is_null() || encoded.is_null() {
        return 0;
    }
    let data_slice = std::slice::from_raw_parts(data, data_len);
    let Ok(metadata) = parse_metadata(data_slice) else {
        return 0;
    };
    let loop_count = metadata.loop_count.map(i32::from).unwrap_or(-1);
    let Ok(bytes) = reencode_gif_literal_parallel_native(data_slice, &metadata, loop_count) else {
        return 0;
    };
    let byte_len = bytes.len();
    let destination = allocate(allocate_context, byte_len);
    if destination.is_null() {
        return 0;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, byte_len);
    encoded.write(NativeEncodedGif {
        bytes: destination,
        byte_len,
        byte_capacity: 0,
    });
    2
}

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
/// Releases bytes returned by a Rust-owned native encoder.
///
/// # Safety
///
/// The pointer, length, and capacity must be the exact triple returned by a
/// successful native encoder and must not have been released previously.
pub unsafe extern "C" fn wtfgif_free_bytes(bytes: *mut u8, byte_len: usize, byte_capacity: usize) {
    if bytes.is_null() {
        return;
    }
    drop(Vec::from_raw_parts(bytes, byte_len, byte_capacity));
}
