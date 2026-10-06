use super::*;

const MAX_DECODE_PIXELS: usize = 1_048_576;
const MAX_DECODE_FRAMES: usize = 16;

pub fn fuzz_decode(data: &[u8]) {
    let Ok(metadata) = parse_metadata(data) else {
        return;
    };
    let pixels = usize::from(metadata.width) * usize::from(metadata.height);
    if pixels > MAX_DECODE_PIXELS || metadata.frames.len() > MAX_DECODE_FRAMES {
        return;
    }
    for frame in &metadata.frames {
        let _ = decode_frame_indices_inner(data, frame);
    }
    let requested = vec![1; metadata.frames.len()];
    let _ = prepare_composited_frames_inner(data, &metadata, &requested, PixelFormat::Rgba);
}

pub fn fuzz_encode(data: &[u8]) {
    if data.len() <= 4 {
        return;
    }
    let width = u16::from(data[0] % 64 + 1);
    let height = u16::from(data[1] % 64 + 1);
    let frame_count = usize::from(data[2] % 4 + 1);
    let byte_len = usize::from(width) * usize::from(height) * frame_count * 4;
    let mut rgba = vec![0; byte_len];
    for (index, byte) in rgba.iter_mut().enumerate() {
        *byte = data[4 + index % (data.len() - 4)];
    }
    // The high bit of the height byte selects the independent-frame layout.
    let independent_frames = data[1] & 0x80 != 0;
    let encoded = encode_rgba_quality_gif_inner_with_output(
        &rgba,
        width,
        height,
        frame_count,
        DelaySource::Constant(u16::from(data[3])),
        0,
        data[3],
        CompactOptions {
            independent_frames,
            smallest: false,
        },
        Vec::new(),
    )
    .expect("bounded RGBA input must encode");
    let metadata = parse_metadata(&encoded).expect("encoder output must parse");
    assert_eq!(metadata.width, width);
    assert_eq!(metadata.height, height);
    assert_eq!(metadata.frames.len(), frame_count);

    // Run codes and frame differencing are lossless: every composited frame
    // must equal the same palette indices written as literal full frames.
    let (palette, indexed, transparent_index) = index_rgba_frames_quality(&rgba, data[3]);
    let literal = encode_indexed_literal_gif_inner(
        &indexed,
        width,
        height,
        frame_count,
        &palette,
        DelaySource::Constant(u16::from(data[3])),
        0,
        transparent_index,
    )
    .expect("indexed quality frames must encode");
    let literal_metadata = parse_metadata(&literal).expect("literal output must parse");
    let requested = vec![1; frame_count];
    assert_eq!(
        prepare_composited_frames_inner(&encoded, &metadata, &requested, PixelFormat::Rgba),
        prepare_composited_frames_inner(&literal, &literal_metadata, &requested, PixelFormat::Rgba),
    );
    fuzz_decode(&encoded);
}

/// Encode a bounded arbitrary animation, decode it, and check what the
/// encoder promises about the source: shape, delays, and loop count survive;
/// alpha is exactly binary at the threshold; equal opaque colors decode to
/// equal colors; and when the opaque colors fit in the palette, every one
/// decodes exactly. Indexed frames must decode back to their indices.
pub fn fuzz_roundtrip(data: &[u8]) {
    const HEADER: usize = 6;
    if data.len() <= HEADER {
        return;
    }
    let width = u16::from(data[0] % 48 + 1);
    let height = u16::from(data[1] % 48 + 1);
    let independent_frames = data[1] & 0x80 != 0;
    let frame_count = usize::from(data[2] % 4 + 1);
    let limited_colors = data[2] & 0x04 != 0;
    let per_frame_delays = data[2] & 0x80 != 0;
    let alpha_threshold = data[3];
    let loop_count = if data[4] == 255 {
        -1
    } else {
        i32::from(data[4])
    };
    let source = &data[HEADER..];
    let byte_at = |index: usize| source[index % source.len()];

    let delay_values: Vec<u16> = (0..frame_count)
        .map(|frame| u16::from_le_bytes([byte_at(2 * frame), byte_at(2 * frame + 1)]))
        .collect();
    let delays = if per_frame_delays {
        DelaySource::PerFrame(&delay_values)
    } else {
        DelaySource::Constant(delay_values[0])
    };

    let pixel_count = usize::from(width) * usize::from(height) * frame_count;
    let mut rgba = vec![0; pixel_count * 4];
    if limited_colors {
        // Draw from a table of up to 300 colors so both the exact palette and
        // the quantizer run.
        let color_count = usize::from(data[5]) % 300 + 1;
        for (pixel, out) in rgba.chunks_exact_mut(4).enumerate() {
            let pick = usize::from(u16::from_le_bytes([
                byte_at(2 * pixel),
                byte_at(2 * pixel + 1),
            ]));
            let color = pick % color_count;
            for (channel, byte) in out.iter_mut().enumerate() {
                *byte = byte_at(4 * color + channel);
            }
        }
    } else {
        for (index, byte) in rgba.iter_mut().enumerate() {
            *byte = byte_at(index);
        }
    }

    let encoded = encode_rgba_quality_gif_inner_with_output(
        &rgba,
        width,
        height,
        frame_count,
        delays,
        loop_count,
        alpha_threshold,
        CompactOptions {
            independent_frames,
            smallest: false,
        },
        Vec::new(),
    )
    .expect("bounded RGBA input must encode");
    let metadata = parse_metadata(&encoded).expect("encoder output must parse");
    assert_eq!((metadata.width, metadata.height), (width, height));
    assert_eq!(metadata.frames.len(), frame_count);
    assert_eq!(metadata.loop_count, u16::try_from(loop_count).ok());
    for (frame, info) in metadata.frames.iter().enumerate() {
        assert_eq!(info.delay, delays.get(frame), "frame {frame} delay");
    }

    let requested = vec![1; frame_count];
    let decoded =
        prepare_composited_frames_inner(&encoded, &metadata, &requested, PixelFormat::Rgba)
            .expect("encoder output must decode");
    assert_eq!(decoded.len(), pixel_count);

    // The smallest mode changes only how images are coded.
    let smallest = encode_rgba_quality_gif_inner_with_output(
        &rgba,
        width,
        height,
        frame_count,
        delays,
        loop_count,
        alpha_threshold,
        CompactOptions {
            independent_frames,
            smallest: true,
        },
        Vec::new(),
    )
    .expect("bounded RGBA input must encode in the smallest mode");
    assert!(
        smallest.len() <= encoded.len(),
        "the smallest mode grew the GIF"
    );
    let smallest_metadata = parse_metadata(&smallest).expect("smallest output must parse");
    assert_eq!(smallest_metadata.loop_count, metadata.loop_count);
    for (frame, info) in smallest_metadata.frames.iter().enumerate() {
        assert_eq!(
            info.delay,
            delays.get(frame),
            "smallest frame {frame} delay"
        );
    }
    assert_eq!(
        prepare_composited_frames_inner(
            &smallest,
            &smallest_metadata,
            &requested,
            PixelFormat::Rgba
        )
        .expect("smallest output must decode"),
        decoded,
        "the smallest mode changed decoded pixels"
    );

    let mut decoded_color_of = std::collections::HashMap::new();
    let mut has_transparency = false;
    for (pixel, (source_pixel, &output)) in rgba.chunks_exact(4).zip(&decoded).enumerate() {
        let opaque = source_pixel[3] >= alpha_threshold;
        let decoded_alpha = output >> 24;
        assert_eq!(
            decoded_alpha,
            if opaque { 255 } else { 0 },
            "pixel {pixel} alpha"
        );
        if !opaque {
            has_transparency = true;
            continue;
        }
        let source_rgb = u32::from(source_pixel[0])
            | u32::from(source_pixel[1]) << 8
            | u32::from(source_pixel[2]) << 16;
        let decoded_rgb = output & 0x00ff_ffff;
        let first = *decoded_color_of.entry(source_rgb).or_insert(decoded_rgb);
        assert_eq!(
            first, decoded_rgb,
            "pixel {pixel}: one source color decoded two ways"
        );
    }
    let palette_capacity = if has_transparency { 255 } else { 256 };
    if decoded_color_of.len() <= palette_capacity {
        for (&source_rgb, &decoded_rgb) in &decoded_color_of {
            assert_eq!(
                source_rgb, decoded_rgb,
                "a palette-sized image must stay exact"
            );
        }
    }

    // Indexed frames, written as literal full frames and as delta frames.
    let palette_size = usize::from(data[5]) % 256 + 1;
    let palette: Vec<u32> = (0..palette_size)
        .map(|index| {
            u32::from_le_bytes([
                byte_at(3 * index),
                byte_at(3 * index + 1),
                byte_at(3 * index + 2),
                0,
            ])
        })
        .collect();
    let indices: Vec<u8> = (0..pixel_count)
        .map(|pixel| (usize::from(byte_at(pixel)) % palette_size) as u8)
        .collect();
    let literal = encode_indexed_literal_gif_inner(
        &indices,
        width,
        height,
        frame_count,
        &palette,
        delays,
        loop_count,
        None,
    )
    .expect("bounded indexed input must encode");
    let literal_metadata = parse_metadata(&literal).expect("literal output must parse");
    let frame_pixels = usize::from(width) * usize::from(height);
    for (frame, info) in literal_metadata.frames.iter().enumerate() {
        let decoded =
            decode_frame_indices_inner(&literal, info).expect("literal frame must decode");
        assert_eq!(
            decoded,
            indices[frame * frame_pixels..(frame + 1) * frame_pixels],
            "frame {frame} indices"
        );
    }
    let transparent = (palette_size < 256).then_some(palette_size as u8);
    let mut padded_palette = palette.clone();
    if transparent.is_some() {
        padded_palette.push(0);
    }
    let delta = encode_indexed_literal_delta_gif_inner(
        &indices,
        width,
        height,
        frame_count,
        &padded_palette,
        delays,
        loop_count,
        transparent,
    )
    .expect("bounded indexed input must delta-encode");
    let delta_metadata = parse_metadata(&delta).expect("delta output must parse");
    assert_eq!(
        prepare_composited_frames_inner(&delta, &delta_metadata, &requested, PixelFormat::Rgba),
        prepare_composited_frames_inner(&literal, &literal_metadata, &requested, PixelFormat::Rgba),
    );
}
