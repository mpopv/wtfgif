use super::*;

const ONE_PIXEL_TRANSPARENT_GIF: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, // GIF89a
    0x01, 0x00, 0x01, 0x00, // 1x1 logical screen
    0x80, 0x00, 0x00, // global table: 2 colors
    0x00, 0x00, 0x00, // black
    0xff, 0xff, 0xff, // white
    0x21, 0xf9, 0x04, 0x09, 0x05, 0x00, 0x01, 0x00, // GCE
    0x2c, 0x00, 0x00, 0x00, 0x00, // image separator + x/y
    0x01, 0x00, 0x01, 0x00, 0x00, // 1x1, no local table
    0x02, 0x02, 0x4c, 0x01, 0x00, // image data
    0x3b, // trailer
];

#[test]
fn parses_screen_and_frame_metadata() {
    let metadata = parse_metadata(ONE_PIXEL_TRANSPARENT_GIF).unwrap();

    assert_eq!(metadata.version, "GIF89a");
    assert_eq!(metadata.width, 1);
    assert_eq!(metadata.height, 1);
    assert_eq!(metadata.global_palette_offset, Some(13));
    assert_eq!(metadata.global_palette_size, 2);
    assert_eq!(metadata.frames.len(), 1);

    let frame = &metadata.frames[0];
    assert_eq!(frame.x, 0);
    assert_eq!(frame.y, 0);
    assert_eq!(frame.width, 1);
    assert_eq!(frame.height, 1);
    assert!(!frame.has_local_palette);
    assert_eq!(frame.palette_offset, 13);
    assert_eq!(frame.palette_size, 2);
    assert_eq!(frame.data_offset, 37);
    assert_eq!(frame.data_length, 5);
    assert_eq!(frame.transparent_index, Some(1));
    assert_eq!(frame.delay, 5);
    assert_eq!(frame.disposal, 2);
    assert_eq!(frame.min_code_size, 2);
}

#[test]
fn rejects_invalid_signatures() {
    let error = parse_metadata(b"not a gif.....").unwrap_err();
    assert!(error.contains("Invalid GIF signature"));
}

#[test]
fn validates_small_gifs_without_allocating_metadata() {
    validate_gif_structure_no_alloc(ONE_PIXEL_TRANSPARENT_GIF).unwrap();
    let error = validate_gif_structure_no_alloc(b"not a gif.....").unwrap_err();
    assert!(error.contains("Invalid GIF signature"));
}

#[test]
fn rejects_frames_that_exceed_the_logical_screen() {
    let mut gif = ONE_PIXEL_TRANSPARENT_GIF.to_vec();
    gif[32] = 2;

    let error = parse_metadata(&gif).unwrap_err();
    assert!(error.contains("exceeds the logical screen bounds"));

    let error = validate_gif_structure_no_alloc(&gif).unwrap_err();
    assert!(error.contains("exceeds the logical screen bounds"));
}

#[cfg(not(feature = "encode-only"))]
#[test]
fn lossless_remux_can_return_a_valid_small_gif_unchanged() {
    let remuxed = remux_gif_pixel_perfect(ONE_PIXEL_TRANSPARENT_GIF).unwrap();
    assert_eq!(remuxed, ONE_PIXEL_TRANSPARENT_GIF);
}

#[test]
fn serializes_metadata_json() {
    let json = parse_metadata(ONE_PIXEL_TRANSPARENT_GIF).unwrap().to_json();

    assert!(json.contains("\"width\":1"));
    assert!(json.contains("\"frame_count\":1"));
    assert!(json.contains("\"transparent_index\":1"));
}

#[test]
fn decodes_frame_indices() {
    let metadata = parse_metadata(ONE_PIXEL_TRANSPARENT_GIF).unwrap();
    let indices =
        decode_frame_indices_inner(ONE_PIXEL_TRANSPARENT_GIF, &metadata.frames[0]).unwrap();

    assert_eq!(indices, vec![1]);
}

#[test]
fn decodes_transparent_frame_to_zero_rgba() {
    let metadata = parse_metadata(ONE_PIXEL_TRANSPARENT_GIF).unwrap();
    let pixels =
        decode_frame_pixels_inner(ONE_PIXEL_TRANSPARENT_GIF, &metadata, 0, PixelFormat::Rgba)
            .unwrap();

    assert_eq!(pixels, vec![0, 0, 0, 0]);
}

#[test]
fn decodes_opaque_frame_to_rgba_and_bgra() {
    let mut gif = ONE_PIXEL_TRANSPARENT_GIF.to_vec();
    gif[16] = 0xff;
    gif[17] = 0x00;
    gif[18] = 0x00;
    gif[22] = 0x08;
    let metadata = parse_metadata(&gif).unwrap();

    let rgba = decode_frame_pixels_inner(&gif, &metadata, 0, PixelFormat::Rgba).unwrap();
    let bgra = decode_frame_pixels_inner(&gif, &metadata, 0, PixelFormat::Bgra).unwrap();

    assert_eq!(rgba, vec![255, 0, 0, 255]);
    assert_eq!(bgra, vec![0, 0, 255, 255]);
}

#[test]
fn prepares_composited_rgba_and_bgra_frames() {
    let mut gif = ONE_PIXEL_TRANSPARENT_GIF.to_vec();
    gif[16] = 0xff;
    gif[17] = 0x00;
    gif[18] = 0x00;
    gif[22] = 0x08;
    let metadata = parse_metadata(&gif).unwrap();

    let rgba = prepare_composited_frames_inner(&gif, &metadata, &[1], PixelFormat::Rgba).unwrap();
    let bgra = prepare_composited_frames_inner(&gif, &metadata, &[1], PixelFormat::Bgra).unwrap();

    assert_eq!(rgba, vec![0xff0000ff]);
    assert_eq!(bgra, vec![0xffff0000]);
}

#[test]
fn direct_full_opaque_compositing_matches_per_frame_decode() {
    let palette = [0x000000, 0xff0000];
    let frames = [0, 1, 1, 0];
    let gif = encode_indexed_literal_gif_inner(
        &frames,
        2,
        1,
        2,
        &palette,
        DelaySource::Constant(0),
        -1,
        None,
    )
    .unwrap();
    let metadata = parse_metadata(&gif).unwrap();
    assert!(all_frames_full_opaque(&metadata));

    let direct =
        prepare_composited_frames_selected(&gif, &metadata, None, PixelFormat::Rgba).unwrap();
    let mut expected = Vec::new();
    for frame_index in 0..metadata.frames.len() {
        let pixels =
            decode_frame_pixels_inner(&gif, &metadata, frame_index, PixelFormat::Rgba).unwrap();
        expected.extend(
            pixels
                .chunks_exact(4)
                .map(|pixel| u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]])),
        );
    }
    assert_eq!(direct, expected);
}

#[test]
fn prepares_composited_delta_stream() {
    let mut gif = ONE_PIXEL_TRANSPARENT_GIF.to_vec();
    gif[16] = 0xff;
    gif[17] = 0x00;
    gif[18] = 0x00;
    gif[22] = 0x08;
    let metadata = parse_metadata(&gif).unwrap();

    let stream =
        prepare_composited_delta_frames_inner(&gif, &metadata, &[1], PixelFormat::Rgba).unwrap();

    assert_eq!(stream[0], COMPOSITED_DELTA_MAGIC);
    assert_eq!(stream[1], COMPOSITED_DELTA_VERSION);
    assert_eq!(stream[2], 1);
    assert_eq!(stream[3], 1);
    assert_eq!(stream[4], 0);
    assert_eq!(stream[5], 13);
    assert_eq!(stream[6], 1);
    assert_eq!(stream[11], 14);
    assert_eq!(stream[12], 0);
    assert_eq!(stream[13], 0xff0000ff);
}

#[test]
fn encodes_indexed_lzw_code_stream() {
    let encoded_length = encode_indexed_literal_lzw_scratch_inner(&[1], 2, 2).unwrap();
    assert!(encoded_length > 0);

    assert!(encode_indexed_literal_lzw_scratch_inner(&[2], 2, 2)
        .unwrap_err()
        .contains("Pixel index out of range"));
}

#[test]
fn encodes_indexed_gif_frames() {
    let palette = [0x000000, 0xff0000, 0x00ff00];
    let encoded = encode_indexed_literal_gif_inner(
        &[1, 1, 1, 1, 2, 0, 0, 2],
        2,
        2,
        2,
        &palette,
        DelaySource::Constant(5),
        0,
        None,
    )
    .unwrap();
    let metadata = parse_metadata(&encoded).unwrap();

    assert_eq!(metadata.width, 2);
    assert_eq!(metadata.height, 2);
    assert_eq!(metadata.global_palette_size, 4);
    assert_eq!(metadata.frames.len(), 2);
    assert_eq!(metadata.frames[0].delay, 5);
    assert_eq!(metadata.frames[1].delay, 5);
    assert_eq!(
        decode_frame_indices_inner(&encoded, &metadata.frames[0]).unwrap(),
        vec![1, 1, 1, 1]
    );
    assert_eq!(
        decode_frame_indices_inner(&encoded, &metadata.frames[1]).unwrap(),
        vec![2, 0, 0, 2]
    );
}

#[test]
fn encodes_full_256_color_literal_stream() {
    let palette: Vec<u32> = (0..256u32)
        .map(|value| (value << 16) | (value << 8) | value)
        .collect();
    let indices: Vec<u8> = (0..4096u32).map(|value| (value & 0xff) as u8).collect();
    let encoded = encode_indexed_literal_gif_inner(
        &indices,
        64,
        64,
        1,
        &palette,
        DelaySource::Constant(0),
        -1,
        None,
    )
    .unwrap();
    let metadata = parse_metadata(&encoded).unwrap();
    assert_eq!(
        decode_frame_indices_inner(&encoded, &metadata.frames[0]).unwrap(),
        indices
    );
}

#[test]
fn nine_bit_literal_packer_matches_reference_bitstream() {
    for length in [1usize, 2, 6, 7, 8, 14, 253, 254, 255, 508, 1024, 4096] {
        let indices: Vec<u8> = (0..length)
            .map(|index| ((index * 73 + index / 11) & 0xff) as u8)
            .collect();
        let mut optimized = Vec::new();
        encode_nine_bit_literal_codes(&mut optimized, &indices).unwrap();

        let mut reference = Vec::new();
        let mut bits = 0u64;
        let mut bit_count = 0usize;
        for literals in indices.chunks(254) {
            emit_raw_lzw_code(&mut reference, &mut bits, &mut bit_count, 9, 256);
            for &pixel in literals {
                emit_raw_lzw_code(
                    &mut reference,
                    &mut bits,
                    &mut bit_count,
                    9,
                    usize::from(pixel),
                );
            }
        }
        emit_raw_lzw_code(&mut reference, &mut bits, &mut bit_count, 9, 257);
        while bit_count > 0 {
            reference.push(bits as u8);
            bits >>= 8;
            bit_count = bit_count.saturating_sub(8);
        }

        assert_eq!(optimized, reference, "length {length}");
    }
}

#[test]
fn nine_bit_literal_direct_subblocks_match_buffered_writer() {
    for length in [1usize, 254, 255, 508, 4096] {
        let indices: Vec<u8> = (0..length)
            .map(|index| ((index * 73 + index / 11) & 0xff) as u8)
            .collect();
        let mut direct = Vec::new();
        encode_indexed_literal_lzw_direct_to(&mut direct, &indices, 8, 256).unwrap();

        let mut buffered = Vec::new();
        let mut compressed = Vec::new();
        encode_indexed_literal_lzw_to(&mut buffered, &indices, 8, 256, &mut compressed).unwrap();

        assert_eq!(direct, buffered, "length {length}");
    }
}

#[test]
fn four_bit_literal_direct_subblocks_match_buffered_writer() {
    for length in [1usize, 6, 7, 11, 12, 13, 255, 508, 4096] {
        let indices: Vec<u8> = (0..length)
            .map(|index| ((index * 5 + index / 3) & 7) as u8)
            .collect();
        let mut direct = Vec::new();
        encode_indexed_literal_lzw_direct_to(&mut direct, &indices, 3, 8).unwrap();

        let mut buffered = Vec::new();
        let mut compressed = Vec::new();
        encode_indexed_literal_lzw_to(&mut buffered, &indices, 3, 8, &mut compressed).unwrap();

        assert_eq!(direct, buffered, "length {length}");
    }
}

#[test]
fn seven_bit_literal_direct_subblocks_match_buffered_writer() {
    for length in [1usize, 62, 63, 255, 508, 4096] {
        let indices: Vec<u8> = (0..length)
            .map(|index| ((index * 73 + index / 11) & 63) as u8)
            .collect();
        let mut direct = Vec::new();
        encode_indexed_literal_lzw_direct_to(&mut direct, &indices, 6, 64).unwrap();

        let mut buffered = Vec::new();
        let mut compressed = Vec::new();
        encode_indexed_literal_lzw_to(&mut buffered, &indices, 6, 64, &mut compressed).unwrap();

        assert_eq!(direct, buffered, "length {length}");
    }
}

#[test]
fn six_bit_literal_direct_subblocks_match_buffered_writer() {
    for length in [1usize, 30, 31, 255, 508, 4096] {
        let indices: Vec<u8> = (0..length)
            .map(|index| ((index * 73 + index / 11) & 31) as u8)
            .collect();
        let mut direct = Vec::new();
        encode_indexed_literal_lzw_direct_to(&mut direct, &indices, 5, 32).unwrap();

        let mut buffered = Vec::new();
        let mut compressed = Vec::new();
        encode_indexed_literal_lzw_to(&mut buffered, &indices, 5, 32, &mut compressed).unwrap();

        assert_eq!(direct, buffered, "length {length}");
    }
}

#[test]
fn eight_bit_literal_direct_subblocks_match_buffered_writer() {
    for length in [1usize, 126, 127, 255, 508, 4096] {
        let indices: Vec<u8> = (0..length)
            .map(|index| ((index * 73 + index / 11) & 1) as u8)
            .collect();
        let mut direct = Vec::new();
        encode_indexed_literal_lzw_direct_to(&mut direct, &indices, 7, 2).unwrap();

        let mut buffered = Vec::new();
        let mut compressed = Vec::new();
        encode_indexed_literal_lzw_to(&mut buffered, &indices, 7, 2, &mut compressed).unwrap();

        assert_eq!(direct, buffered, "length {length}");
    }
}

#[test]
fn nine_bit_literal_copy_decoder_matches_indices() {
    let indices: Vec<u8> = (0..4096u32)
        .map(|index| ((index * 73 + index / 11) & 0xff) as u8)
        .collect();
    let mut image_data = Vec::new();
    encode_nine_bit_literal_codes(&mut image_data, &indices).unwrap();
    let mut output = vec![0u8; indices.len()];
    let mut scratch = LzwStackScratch::default();

    lzw_decode_to_indices_copy_with_scratch(8, &image_data, &mut output, &mut scratch).unwrap();

    assert_eq!(output, indices);
}

#[test]
fn direct_lzw_color_decoder_matches_palette_mapping() {
    let indices: Vec<u8> = (0..4096u32)
        .map(|index| ((index * 73 + index / 11) & 0xff) as u8)
        .collect();
    let palette: Vec<u32> = (0..256u32)
        .map(|index| (index << 16) | (index << 8) | index)
        .collect();
    let mut image_data = Vec::new();
    encode_nine_bit_literal_codes(&mut image_data, &indices).unwrap();
    let mut output = vec![0u32; indices.len()];
    let mut scratch = LzwStackScratch::default();

    lzw_decode_to_pixels_copy_with_scratch(8, &image_data, &palette, &mut output, &mut scratch)
        .unwrap();

    let expected: Vec<u32> = indices
        .iter()
        .map(|&index| palette[usize::from(index)])
        .collect();
    assert_eq!(output, expected);
}

#[test]
fn direct_lzw_color_decoder_matches_literal_stream() {
    let indices: Vec<u8> = (0..8192u32)
        .map(|index| ((index * 17 + (index >> 3) * 5) & 255) as u8)
        .collect();
    let palette: Vec<u32> = (0..256u32)
        .map(|index| 0xff00_0000 | (index << 16) | (index << 8) | index)
        .collect();
    let mut image_data = Vec::new();
    encode_indexed_literal_lzw_direct_to(&mut image_data, &indices, 8, 256).unwrap();
    let mut payload = Vec::new();
    collect_image_data_into(&image_data, 0, &mut payload).unwrap();

    let mut decoded_indices = vec![0u8; indices.len()];
    let mut decoded_colors = vec![0u32; indices.len()];
    let mut scratch = LzwStackScratch::default();
    lzw_decode_to_indices_copy_with_scratch(8, &payload, &mut decoded_indices, &mut scratch)
        .unwrap();
    lzw_decode_to_pixels_copy_with_scratch(
        8,
        &payload,
        &palette,
        &mut decoded_colors,
        &mut scratch,
    )
    .unwrap();

    assert_eq!(decoded_indices, indices);
    for (index, &color) in decoded_indices.iter().zip(&decoded_colors) {
        assert_eq!(color, palette[usize::from(*index)]);
    }
}

#[test]
fn encodes_indexed_delta_gif_frames() {
    let palette = [0x000000, 0xff0000, 0x00ff00];
    let encoded = encode_indexed_literal_delta_gif_inner(
        &[
            1, 1, 1, 1, //
            1, 2, 1, 1, //
            1, 2, 1, 1,
        ],
        2,
        2,
        3,
        &palette,
        DelaySource::Constant(4),
        0,
        None,
    )
    .unwrap();
    let metadata = parse_metadata(&encoded).unwrap();

    assert_eq!(metadata.frames.len(), 3);
    assert_eq!(metadata.frames[0].width, 2);
    assert_eq!(metadata.frames[0].height, 2);
    assert_eq!(metadata.frames[1].x, 1);
    assert_eq!(metadata.frames[1].y, 0);
    assert_eq!(metadata.frames[1].width, 1);
    assert_eq!(metadata.frames[1].height, 1);
    assert_eq!(metadata.frames[2].x, 0);
    assert_eq!(metadata.frames[2].y, 0);
    assert_eq!(metadata.frames[2].width, 1);
    assert_eq!(metadata.frames[2].height, 1);
    assert_eq!(
        decode_frame_indices_inner(&encoded, &metadata.frames[1]).unwrap(),
        vec![2]
    );
}

#[test]
fn encodes_rgba_gif_frames_with_generated_exact_palette() {
    let encoded = encode_rgba_gif_inner(
        &[
            255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 255, 0, 255, 0, 0,
            255, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ],
        2,
        2,
        2,
        &[],
        DelaySource::Constant(6),
        0,
        false,
        TRANSPARENT_ALPHA_THRESHOLD,
    )
    .unwrap();
    let metadata = parse_metadata(&encoded).unwrap();

    assert_eq!(metadata.global_palette_size, 4);
    assert_eq!(metadata.frames.len(), 2);
    assert_eq!(metadata.frames[0].delay, 6);
    assert_eq!(
        decode_frame_indices_inner(&encoded, &metadata.frames[0]).unwrap(),
        vec![0, 0, 1, 2]
    );
    assert_eq!(
        decode_frame_indices_inner(&encoded, &metadata.frames[1]).unwrap(),
        vec![1, 2, 0, 0]
    );
}

#[test]
fn quality_exact_direct_buffer_preserves_runs_and_transparency() {
    let pixel_count = 8_193usize;
    let colors = [[0xf0, 0x20, 0x10], [0x10, 0xe0, 0x30], [0x20, 0x40, 0xf0]];
    let mut rgba = Vec::with_capacity(pixel_count * 4);
    let mut expected = Vec::with_capacity(pixel_count);
    for pixel in 0..pixel_count {
        let color_index = (pixel / 137) % colors.len();
        let transparent = pixel % 997 == 996;
        rgba.extend_from_slice(&[
            colors[color_index][0],
            colors[color_index][1],
            colors[color_index][2],
            if transparent { 0 } else { 255 },
        ]);
        expected.push(if transparent { 3 } else { color_index as u8 });
    }

    for result in [
        index_rgba_frames_quality_low_res_exact::<false>(&rgba, TRANSPARENT_ALPHA_THRESHOLD, 0),
        index_rgba_frames_quality_low_res_exact::<true>(&rgba, TRANSPARENT_ALPHA_THRESHOLD, 0),
    ] {
        let QualityIndexResult::Exact((palette, indexed, transparent_index)) = result else {
            panic!("three opaque colors plus transparency must remain exact");
        };
        assert_eq!(palette, vec![0xf02010, 0x10e030, 0x2040f0, 0]);
        assert_eq!(transparent_index, Some(3));
        assert_eq!(indexed, expected);
        recycle_quality_palette(palette);
        recycle_quantized_indexed(indexed);
    }
}

#[test]
fn rgba_run_end_matches_scalar_boundaries() {
    const RUN_PIXEL: u32 = 0xff30_2010;
    const OTHER_PIXEL: u32 = 0xff60_5040;
    for prefix in 0..5usize {
        for run_length in 1..20usize {
            let mut pixels = vec![OTHER_PIXEL; prefix];
            pixels.extend(std::iter::repeat_n(RUN_PIXEL, run_length));
            pixels.push(OTHER_PIXEL);
            let mut rgba = Vec::with_capacity(pixels.len() * 4);
            for pixel in pixels {
                rgba.extend_from_slice(&pixel.to_le_bytes());
            }
            assert_eq!(
                rgba_run_end(rgba.as_ptr(), prefix, rgba.len() / 4, RUN_PIXEL),
                prefix + run_length,
                "prefix {prefix}, run length {run_length}",
            );
        }
    }

    let rgba = RUN_PIXEL.to_le_bytes().repeat(17);
    assert_eq!(rgba_run_end(rgba.as_ptr(), 0, 17, RUN_PIXEL), 17);
}

#[test]
fn exact_row_reuse_matches_full_run_scan() {
    const WIDTH: usize = 17;
    let mut first_row = Vec::with_capacity(WIDTH * 4);
    for x in 0..WIDTH {
        let color = if x < 8 {
            [0x10, 0x20, 0x30, 255]
        } else {
            [0x90, 0x80, 0x70, 255]
        };
        first_row.extend_from_slice(&color);
    }
    first_row[5 * 4 + 3] = 0;
    let mut rgba = first_row.repeat(3);
    rgba[(WIDTH * 2 + 12) * 4..(WIDTH * 2 + 12) * 4 + 4].copy_from_slice(&[0x40, 0x50, 0x60, 255]);

    let QualityIndexResult::Exact(without_reuse) =
        index_rgba_frames_quality_low_res_exact::<true>(&rgba, TRANSPARENT_ALPHA_THRESHOLD, 0)
    else {
        panic!("test image must remain exact");
    };
    let QualityIndexResult::Exact(with_reuse) =
        index_rgba_frames_quality_low_res_exact::<true>(&rgba, TRANSPARENT_ALPHA_THRESHOLD, WIDTH)
    else {
        panic!("test image must remain exact with row reuse");
    };
    assert_eq!(with_reuse.0, without_reuse.0);
    assert_eq!(with_reuse.1, without_reuse.1);
    assert_eq!(with_reuse.2, without_reuse.2);
    recycle_quality_palette(without_reuse.0);
    recycle_quantized_indexed(without_reuse.1);
    recycle_quality_palette(with_reuse.0);
    recycle_quantized_indexed(with_reuse.1);
}

#[test]
fn uniform_run_hint_is_conservative_and_keeps_the_small_palette_route_exact() {
    let mut rgba = [12, 16, 32, 255].repeat(40_000);
    assert!(quality_low_res_has_uniform_sampled_runs(&rgba));
    let hints = quality_low_res_hints(&rgba, TRANSPARENT_ALPHA_THRESHOLD);
    assert!(hints.likely_exact);
    assert!(hints.likely_small_palette);
    assert!(hints.prefers_run_coalescing);

    rgba[8_191 * 4] = 13;
    assert!(!quality_low_res_has_uniform_sampled_runs(&rgba));
}

#[test]
fn adaptive_histogram_matches_the_general_pair_scan() {
    // One matching leading pair and a varied later probe selects the dedicated
    // split-bin scanner; the other cases retain the general scanner.
    for matching_probe_pairs in [0usize, 1, 7, 8] {
        let mut rgba: Vec<u8> = (0..4_096u32)
            .flat_map(|pixel| {
                [
                    pixel.wrapping_mul(17) as u8,
                    pixel.wrapping_mul(31) as u8,
                    pixel.wrapping_mul(47) as u8,
                    255,
                ]
            })
            .collect();
        for pair in 0..matching_probe_pairs {
            let first = pair * 8;
            rgba.copy_within(first..first + 4, first + 4);
        }
        rgba[100 * 4 + 3] = 17;
        let mut expected = vec![RgbHistogramBin32::default(); 1 << 12];
        let mut actual = vec![RgbHistogramBin32::default(); 1 << 12];
        let expected_alpha = accumulate_quality_histogram_u32_bits_remaining_opaque::<4, true>(
            &mut expected,
            &rgba,
            0,
        );
        let actual_alpha =
            accumulate_quality_histogram_u32_bits_opaque_adaptive::<true>(&mut actual, &rgba);
        assert_eq!(actual_alpha, expected_alpha);
        assert!(!actual_alpha);
        for (expected, actual) in expected.iter().zip(&actual) {
            assert_eq!(
                (expected.count, expected.red, expected.green, expected.blue),
                (actual.count, actual.red, actual.green, actual.blue)
            );
        }
    }
}

#[test]
fn transparent_long_sample_can_prove_quantization_without_an_occupancy_scan() {
    let pixel_count = 40_000usize;
    let mut rgba = [0, 0, 0, 0].repeat(pixel_count);
    let sample_step = 8_191 % pixel_count;
    let mut sample_pixel = 0usize;
    let mut color = 0u16;
    for sample_index in 0..1_030 {
        if sample_index % 4 == 1 && color < 256 {
            rgba[sample_pixel * 4..sample_pixel * 4 + 4].copy_from_slice(&[
                color as u8,
                (color >> 8) as u8,
                127,
                255,
            ]);
            color += 1;
        }
        sample_pixel = (sample_pixel + sample_step) % pixel_count;
    }
    assert_eq!(color, 256);

    let hints = quality_low_res_hints(&rgba, TRANSPARENT_ALPHA_THRESHOLD);
    assert!(hints.exact_impossible);
    assert!(!hints.sampled_alpha_255);
}

#[test]
fn likely_exact_delta_probe_falls_back_without_changing_quantization() {
    let pixel_count = 8_192usize;
    let mut rgba = Vec::with_capacity(pixel_count * 4);
    for pixel in 0..pixel_count {
        rgba.extend_from_slice(&[pixel as u8, (pixel >> 8) as u8, (pixel >> 16) as u8, 255]);
    }
    let mut sample_pixel = 0usize;
    for _ in 0..256 {
        rgba[sample_pixel * 4..sample_pixel * 4 + 4].copy_from_slice(&[0, 0, 0, 255]);
        sample_pixel = (sample_pixel + 8_191) % pixel_count;
    }

    assert!(quality_low_res_hints(&rgba, TRANSPARENT_ALPHA_THRESHOLD).likely_exact);
    assert!(quality_low_res_exact_is_impossible(
        &rgba,
        TRANSPARENT_ALPHA_THRESHOLD,
    ));

    let expected = match index_rgba_frames_quality_low_res(&rgba, TRANSPARENT_ALPHA_THRESHOLD) {
        QualityIndexResult::Exact(_) => panic!("fixture must overflow the exact palette"),
        QualityIndexResult::Quantized(plan) => {
            plan.into_indexed(&rgba, TRANSPARENT_ALPHA_THRESHOLD)
        }
    };
    let actual = index_rgba_frames_quality_low_res_delta(&rgba, TRANSPARENT_ALPHA_THRESHOLD);
    assert_eq!(actual, expected);
    recycle_quality_palette(expected.0);
    recycle_quantized_indexed(expected.1);
    recycle_quality_palette(actual.0);
    recycle_quantized_indexed(actual.1);
}

#[test]
fn transparent_rgba_frames_restore_the_canvas() {
    let encoded = encode_rgba_gif_advanced_inner(
        &[
            255, 0, 0, 255, 0, 0, 0, 0, // red, transparent
            0, 0, 0, 0, 0, 0, 255, 255, // transparent, blue
        ],
        2,
        1,
        2,
        &[],
        DelaySource::Constant(6),
        0,
        false,
        TRANSPARENT_ALPHA_THRESHOLD,
        RgbaQuantization::Fast,
        RgbaPaletteMode::Global,
    )
    .unwrap();
    let metadata = parse_metadata(&encoded).unwrap();

    assert!(metadata.frames.iter().all(|frame| frame.disposal == 2));
}

#[test]
fn encodes_rgba_delta_gif_frames_with_provided_palette() {
    let palette = [0x000000, 0xff0000, 0x00ff00];
    let encoded = encode_rgba_gif_inner(
        &[
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 0, 255,
            0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ],
        2,
        2,
        2,
        &palette,
        DelaySource::Constant(4),
        0,
        true,
        TRANSPARENT_ALPHA_THRESHOLD,
    )
    .unwrap();
    let metadata = parse_metadata(&encoded).unwrap();

    assert_eq!(metadata.frames.len(), 2);
    assert_eq!(metadata.frames[1].x, 1);
    assert_eq!(metadata.frames[1].y, 0);
    assert_eq!(metadata.frames[1].width, 1);
    assert_eq!(metadata.frames[1].height, 1);
    assert_eq!(
        decode_frame_indices_inner(&encoded, &metadata.frames[1]).unwrap(),
        vec![2]
    );
}

#[test]
fn quantizes_rgba_gif_frames_when_exact_palette_overflows() {
    let mut rgba = Vec::new();
    for value in 0..257u16 {
        rgba.push((value & 0xff) as u8);
        rgba.push(((value * 3) & 0xff) as u8);
        rgba.push(((value * 7) & 0xff) as u8);
        rgba.push(255);
    }

    let encoded = encode_rgba_gif_advanced_inner(
        &rgba,
        257,
        1,
        1,
        &[],
        DelaySource::Constant(0),
        -1,
        false,
        TRANSPARENT_ALPHA_THRESHOLD,
        RgbaQuantization::Fast,
        RgbaPaletteMode::Global,
    )
    .unwrap();
    let metadata = parse_metadata(&encoded).unwrap();

    assert_eq!(metadata.global_palette_size, 256);
    assert_eq!(metadata.frames.len(), 1);
    assert_eq!(
        decode_frame_indices_inner(&encoded, &metadata.frames[0])
            .unwrap()
            .len(),
        257
    );
}

#[test]
fn advanced_encoder_quantizes_arbitrary_rgba() {
    let mut rgba = Vec::new();
    for value in 0..1024u16 {
        rgba.extend_from_slice(&[
            (value & 0xff) as u8,
            ((value * 5) & 0xff) as u8,
            ((value * 11) & 0xff) as u8,
            255,
        ]);
    }

    let encoded = encode_rgba_gif_advanced_inner(
        &rgba,
        32,
        32,
        1,
        &[],
        DelaySource::Constant(3),
        0,
        false,
        TRANSPARENT_ALPHA_THRESHOLD,
        RgbaQuantization::Fast,
        RgbaPaletteMode::Global,
    )
    .unwrap();
    let metadata = parse_metadata(&encoded).unwrap();

    assert_eq!(metadata.global_palette_size, 256);
    assert_eq!(metadata.frames.len(), 1);
    assert_eq!(metadata.frames[0].delay, 3);
    assert_eq!(
        decode_frame_indices_inner(&encoded, &metadata.frames[0])
            .unwrap()
            .len(),
        1024
    );
}

#[test]
fn median_cut_split_carries_exact_child_statistics() {
    let colors = vec![
        QuantizedColor {
            histogram_index: 0,
            count: 1,
            red: 0,
            green: 10,
            blue: 20,
        },
        QuantizedColor {
            histogram_index: 1,
            count: 1,
            red: 10,
            green: 20,
            blue: 30,
        },
        QuantizedColor {
            histogram_index: 2,
            count: 100,
            red: 20,
            green: 30,
            blue: 40,
        },
    ];
    let color_box = QuantizedColorBox::new(colors);
    let Some((left, right)) = color_box.split().ok() else {
        panic!("expected a splittable color box");
    };
    let expected_left = QuantizedColorBox::new(left.colors.clone());
    let expected_right = QuantizedColorBox::new(right.colors.clone());

    assert_eq!(left.weight, expected_left.weight);
    assert_eq!(left.score, expected_left.score);
    assert_eq!(left.red_range, expected_left.red_range);
    assert_eq!(left.green_range, expected_left.green_range);
    assert_eq!(left.blue_range, expected_left.blue_range);
    assert_eq!(right.weight, expected_right.weight);
    assert_eq!(right.score, expected_right.score);
    assert_eq!(right.red_range, expected_right.red_range);
    assert_eq!(right.green_range, expected_right.green_range);
    assert_eq!(right.blue_range, expected_right.blue_range);
}

#[test]
fn weighted_median_partition_keeps_axis_order() {
    let colors = vec![
        QuantizedColor {
            histogram_index: 0,
            count: 1,
            red: 240,
            green: 8,
            blue: 200,
        },
        QuantizedColor {
            histogram_index: 1,
            count: 7,
            red: 12,
            green: 220,
            blue: 30,
        },
        QuantizedColor {
            histogram_index: 2,
            count: 3,
            red: 140,
            green: 70,
            blue: 180,
        },
        QuantizedColor {
            histogram_index: 3,
            count: 11,
            red: 60,
            green: 180,
            blue: 90,
        },
        QuantizedColor {
            histogram_index: 4,
            count: 5,
            red: 190,
            green: 45,
            blue: 15,
        },
        QuantizedColor {
            histogram_index: 5,
            count: 2,
            red: 35,
            green: 130,
            blue: 240,
        },
    ];
    let midpoint = colors
        .iter()
        .map(|color| color.count)
        .sum::<u64>()
        .div_ceil(2);
    for axis in 0..3 {
        let mut partitioned = colors.clone();
        let split = weighted_axis_split_index(&mut partitioned, axis, midpoint);
        assert!(split > 0 && split < partitioned.len());
        let left_max = partitioned[..split]
            .iter()
            .map(|color| quantized_color_axis(color, axis))
            .max()
            .unwrap();
        let right_min = partitioned[split..]
            .iter()
            .map(|color| quantized_color_axis(color, axis))
            .min()
            .unwrap();
        assert!(left_max <= right_min);
    }
}

#[test]
fn arena_median_cut_preserves_vector_partition_membership() {
    let colors = (0..5_000usize)
        .map(|histogram_index| QuantizedColor {
            histogram_index: histogram_index as u16,
            count: ((histogram_index * 17) % 11 + 1) as u64,
            red: ((histogram_index * 37) & 255) as u8,
            green: ((histogram_index * 61 + 13) & 255) as u8,
            blue: ((histogram_index * 97 + 29) & 255) as u8,
        })
        .collect::<Vec<_>>();

    let mut arena_colors = colors.clone();
    let mut arena_boxes = vec![QuantizedColorArenaBox::new(
        0,
        arena_colors.len(),
        &arena_colors,
    )];
    while arena_boxes.len() < 256 {
        let Some((split_index, _)) = arena_boxes
            .iter()
            .enumerate()
            .filter(|(_, color_box)| color_box.len() > 1)
            .max_by_key(|(_, color_box)| color_box.score)
        else {
            break;
        };
        let mut color_box = arena_boxes.swap_remove(split_index);
        let Some(right) = color_box.split(&mut arena_colors) else {
            arena_boxes.push(color_box);
            break;
        };
        arena_boxes.push(color_box);
        arena_boxes.push(right);
    }
    let mut arena_groups = arena_boxes
        .iter()
        .map(|color_box| {
            let mut group = arena_colors[color_box.start..color_box.end]
                .iter()
                .map(|color| color.histogram_index)
                .collect::<Vec<_>>();
            group.sort_unstable();
            group
        })
        .collect::<Vec<_>>();
    arena_groups.sort_unstable();

    let mut vector_boxes = vec![QuantizedColorBox::new(colors)];
    while vector_boxes.len() < 256 {
        let Some((split_index, _)) = vector_boxes
            .iter()
            .enumerate()
            .filter(|(_, color_box)| color_box.colors.len() > 1)
            .max_by_key(|(_, color_box)| color_box.score)
        else {
            break;
        };
        let color_box = vector_boxes.swap_remove(split_index);
        match color_box.split() {
            Ok((left, right)) => {
                vector_boxes.push(left);
                vector_boxes.push(right);
            }
            Err(unsplit) => {
                vector_boxes.push(unsplit);
                break;
            }
        }
    }
    let mut vector_groups = vector_boxes
        .iter()
        .map(|color_box| {
            let mut group = color_box
                .colors
                .iter()
                .map(|color| color.histogram_index)
                .collect::<Vec<_>>();
            group.sort_unstable();
            group
        })
        .collect::<Vec<_>>();
    vector_groups.sort_unstable();

    assert_eq!(arena_groups, vector_groups);
}

#[test]
fn quality_palette_lookup_matches_linear_nearest_color() {
    let palette = [
        0x000000, 0xff0000, 0x00ff00, 0x0000ff, 0xffff00, 0xff00ff, 0x00ffff, 0xffffff, 0x402010,
        0x804020, 0x204080, 0xc08040,
    ];
    let tree = PaletteKdTree::new(&palette);
    for red in (0..=255).step_by(17) {
        for green in (0..=255).step_by(19) {
            for blue in (0..=255).step_by(23) {
                assert_eq!(
                    tree.nearest(red, green, blue),
                    nearest_palette_index(red, green, blue, &palette),
                );
            }
        }
    }
}

#[test]
fn hinted_palette_lookup_matches_unhinted_exact_search() {
    let palette = (0..256u32)
        .map(|index| {
            let red = (index * 73 + 19) & 255;
            let green = (index * 151 + 7) & 255;
            let blue = (index * 199 + 43) & 255;
            (red << 16) | (green << 8) | blue
        })
        .collect::<Vec<_>>();
    let tree = PaletteKdTree::new(&palette);
    for red in (0..=255).step_by(11) {
        for green in (0..=255).step_by(13) {
            for blue in (0..=255).step_by(17) {
                let expected = tree.nearest(red, green, blue);
                for hint in (0..palette.len()).step_by(19) {
                    let hinted = Some((hint as u8, palette[hint]));
                    assert_eq!(
                        tree.nearest_with_hint(red, green, blue, hinted),
                        expected,
                        "color {red},{green},{blue} hint {hint}",
                    );
                    assert_eq!(
                        tree.nearest_with_hint_split(red, green, blue, hinted),
                        expected,
                        "split color {red},{green},{blue} hint {hint}",
                    );
                }
            }
        }
    }
}

#[test]
fn recolored_palette_tree_matches_rebuilt_tree() {
    let initial = (0..256u32)
        .map(|index| {
            let red = (index * 73 + 19) & 255;
            let green = (index * 151 + 7) & 255;
            let blue = (index * 199 + 43) & 255;
            (red << 16) | (green << 8) | blue
        })
        .collect::<Vec<_>>();
    let updated = initial
        .iter()
        .enumerate()
        .map(|(index, &color)| {
            let red = ((color >> 16) as u8).wrapping_add((index * 11) as u8);
            let green = ((color >> 8) as u8).wrapping_sub((index * 7) as u8);
            let blue = (color as u8).wrapping_add((index * 5) as u8);
            (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
        })
        .collect::<Vec<_>>();
    let mut recolored = PaletteKdTree::new(&initial);
    recolored.recolor(&updated);
    let rebuilt = PaletteKdTree::new(&updated);
    for red in (0..=255).step_by(11) {
        for green in (0..=255).step_by(13) {
            for blue in (0..=255).step_by(17) {
                assert_eq!(
                    recolored.nearest(red, green, blue),
                    rebuilt.nearest(red, green, blue),
                    "color {red},{green},{blue}",
                );
            }
        }
    }
}

#[test]
fn coarse_nearest_table_matches_exact_search() {
    let palettes = [
        vec![
            0x000000, 0xff0000, 0x00ff00, 0x0000ff, 0xffff00, 0xff00ff, 0x00ffff, 0xffffff,
            0x402010, 0x804020, 0x204080, 0xc08040,
        ],
        (0..256u32)
            .map(|index| {
                let red = (index * 73 + 19) & 255;
                let green = (index * 151 + 7) & 255;
                let blue = (index * 199 + 43) & 255;
                (red << 16) | (green << 8) | blue
            })
            .collect::<Vec<_>>(),
        vec![0x000000, 0xff0000, 0x000000, 0x00ff00, 0x0000ff],
    ];
    for palette in palettes {
        let tree = PaletteKdTree::new(&palette);
        let table = tree.coarse_nearest_table(&palette);
        let mut requested = [false; 1 << 12];
        for red in 0..16usize {
            for green in 0..16usize {
                for blue in 0..16usize {
                    let index = (red << 8) | (green << 4) | blue;
                    requested[index] = (red + green * 3 + blue * 5) % 7 < 3;
                }
            }
        }
        let sparse_table = tree.coarse_nearest_table_for_cells(
            &palette,
            &requested,
            &[0; 1 << 12],
            requested.iter().filter(|requested| **requested).count(),
        );
        for red in 0..16u8 {
            for green in 0..16u8 {
                for blue in 0..16u8 {
                    let index =
                        (usize::from(red) << 8) | (usize::from(green) << 4) | usize::from(blue);
                    if requested[index] {
                        assert_eq!(
                            sparse_table[index], table[index],
                            "sparse coarse cell {red},{green},{blue}",
                        );
                    }
                    assert_eq!(
                        table[index],
                        tree.nearest((red << 4) | 8, (green << 4) | 8, (blue << 4) | 8),
                        "coarse cell {red},{green},{blue}",
                    );
                }
            }
        }
    }
}

#[test]
fn quality_quantization_improves_rgb_error_over_fast_quantization() {
    let mut rgba = Vec::new();
    for y in 0..64u16 {
        for x in 0..64u16 {
            rgba.extend_from_slice(&[
                (x * 255 / 63) as u8,
                (y * 255 / 63) as u8,
                ((x * 3 + y * 5) & 0xff) as u8,
                255,
            ]);
        }
    }

    let (fast_palette, fast_indices, _) = index_rgba_frames_with_quantization(
        &rgba,
        &[],
        TRANSPARENT_ALPHA_THRESHOLD,
        RgbaQuantization::Fast,
    )
    .unwrap();
    let (quality_palette, quality_indices, _) = index_rgba_frames_with_quantization(
        &rgba,
        &[],
        TRANSPARENT_ALPHA_THRESHOLD,
        RgbaQuantization::Quality,
    )
    .unwrap();
    let error = |palette: &[u32], indices: &[u8]| -> u64 {
        rgba.chunks_exact(4)
            .zip(indices)
            .map(|(pixel, index)| {
                let color = palette[usize::from(*index)];
                let red = ((color >> 16) & 0xff) as i32;
                let green = ((color >> 8) & 0xff) as i32;
                let blue = (color & 0xff) as i32;
                let dr = i32::from(pixel[0]) - red;
                let dg = i32::from(pixel[1]) - green;
                let db = i32::from(pixel[2]) - blue;
                (dr * dr + dg * dg + db * db) as u64
            })
            .sum()
    };

    assert!(error(&quality_palette, &quality_indices) < error(&fast_palette, &fast_indices));
}

#[test]
fn wu_dense_palette_improves_weighted_error_without_approximate_mapping() {
    let mut colors = Vec::with_capacity(1 << 12);
    for red in 0..16u16 {
        for green in 0..16u16 {
            for blue in 0..16u16 {
                let histogram_index = (red << 8) | (green << 4) | blue;
                let count = 1 + ((red * 73 + green * 151 + blue * 199) ^ (red * green * 17)) % 257;
                colors.push(QuantizedColor {
                    count: QuantizedColorCount::from(count),
                    histogram_index,
                    red: ((red << 4) | 8) as u8,
                    green: ((green << 4) | 8) as u8,
                    blue: ((blue << 4) | 8) as u8,
                });
            }
        }
    }
    let weighted_error = |palette: &[u32], mapping: &[u8], colors: &[QuantizedColor]| -> u64 {
        colors
            .iter()
            .map(|color| {
                let representative =
                    palette[usize::from(mapping[usize::from(color.histogram_index)])];
                let red = ((representative >> 16) & 255) as i32;
                let green = ((representative >> 8) & 255) as i32;
                let blue = (representative & 255) as i32;
                let dr = i32::from(color.red) - red;
                let dg = i32::from(color.green) - green;
                let db = i32::from(color.blue) - blue;
                (dr * dr + dg * dg + db * db) as u64 * quantized_color_count_u64(color.count)
            })
            .sum()
    };

    let (wu_palette, wu_mapping) =
        build_quality_wu_palette(false, colors.clone(), 1 << 12, 256, Vec::new());
    let wu_error = weighted_error(&wu_palette, &wu_mapping, &colors);
    recycle_quality_histogram_to_palette(wu_mapping);
    recycle_quality_palette(wu_palette);

    let (median_palette, median_mapping) =
        build_quality_median_cut_palette(false, colors.clone(), 4, 1 << 12, 256, Vec::new());
    let median_error = weighted_error(&median_palette, &median_mapping, &colors);
    recycle_quality_histogram_to_palette(median_mapping);

    assert!(
        wu_error < median_error,
        "{wu_error} should beat {median_error}"
    );
}

#[test]
fn flat_grid_palette_improves_uniform_color_volume_error() {
    let mut colors = Vec::with_capacity(1 << 12);
    for red in 0..16u16 {
        for green in 0..16u16 {
            for blue in 0..16u16 {
                colors.push(QuantizedColor {
                    count: QuantizedColorCount::from(1u16),
                    histogram_index: (red << 8) | (green << 4) | blue,
                    red: ((red << 4) | 8) as u8,
                    green: ((green << 4) | 8) as u8,
                    blue: ((blue << 4) | 8) as u8,
                });
            }
        }
    }
    let weighted_error =
        |palette: &[u32], mapping: &[u8], source_colors: &[QuantizedColor]| -> u64 {
            source_colors
                .iter()
                .map(|color| {
                    let representative =
                        palette[usize::from(mapping[usize::from(color.histogram_index)])];
                    let red = ((representative >> 16) & 255) as i32;
                    let green = ((representative >> 8) & 255) as i32;
                    let blue = (representative & 255) as i32;
                    let dr = i32::from(color.red) - red;
                    let dg = i32::from(color.green) - green;
                    let db = i32::from(color.blue) - blue;
                    (dr * dr + dg * dg + db * db) as u64
                })
                .sum()
        };

    let (grid_palette, grid_mapping) =
        build_quality_flat_grid_palette(colors.clone(), 1 << 12, 256, Vec::new());
    assert_eq!(grid_palette.len(), 256);
    assert_eq!(grid_mapping.len(), 1 << 12);
    assert!(grid_mapping
        .iter()
        .all(|&index| usize::from(index) < grid_palette.len()));
    let grid_error = weighted_error(&grid_palette, &grid_mapping, &colors);

    let equally_weighted_colors = colors
        .iter()
        .map(|color| QuantizedColor {
            count: QuantizedColorCount::from(37u16),
            ..*color
        })
        .collect();
    let (equally_weighted_palette, equally_weighted_mapping) =
        build_quality_flat_grid_palette(equally_weighted_colors, 1 << 12, 256, Vec::new());
    assert_eq!(equally_weighted_palette, grid_palette);
    assert_eq!(equally_weighted_mapping, grid_mapping);
    recycle_quality_histogram_to_palette(equally_weighted_mapping);
    recycle_quality_palette(equally_weighted_palette);

    let (wu_palette, wu_mapping) =
        build_quality_wu_palette(false, colors.clone(), 1 << 12, 256, Vec::new());
    let wu_error = weighted_error(&wu_palette, &wu_mapping, &colors);
    assert!(grid_error < wu_error, "{grid_error} should beat {wu_error}");

    recycle_quality_histogram_to_palette(grid_mapping);
    recycle_quality_palette(grid_palette);
    recycle_quality_histogram_to_palette(wu_mapping);
    recycle_quality_palette(wu_palette);
}

#[test]
fn direct_cell_plan_preserves_every_coarse_representative() {
    let colors: Vec<_> = (0..240u16)
        .map(|histogram_index| QuantizedColor {
            count: QuantizedColorCount::from(histogram_index + 1),
            histogram_index,
            red: ((histogram_index >> 8) as u8) * 16 + 8,
            green: (((histogram_index >> 4) & 15) as u8) * 16 + 8,
            blue: ((histogram_index & 15) as u8) * 16 + 8,
        })
        .collect();
    let expected = colors.clone();
    let plan = build_quality_direct_cell_plan(false, colors, Vec::new());

    assert_eq!(plan.palette.len(), 256);
    for color in expected {
        let index = plan.histogram_to_palette[usize::from(color.histogram_index)];
        assert_eq!(
            plan.palette[usize::from(index)],
            rgb_key(color.red, color.green, color.blue)
        );
    }

    recycle_quality_histogram_to_palette(plan.histogram_to_palette);
    recycle_quality_palette(plan.palette);
}

#[test]
fn single_merge_plan_matches_general_palette_refinement() {
    let colors: Vec<_> = (0..256u16)
        .map(|position| {
            let histogram_index = position;
            QuantizedColor {
                count: QuantizedColorCount::from(1 + (position * 41 + 7) % 251),
                histogram_index,
                red: ((histogram_index >> 8) as u8) * 16 + 8,
                green: (((histogram_index >> 4) & 15) as u8) * 16 + 8,
                blue: ((histogram_index & 15) as u8) * 16 + 8,
            }
        })
        .collect();
    let expected =
        build_quality_index_plan_from_colors::<true, 4>(true, colors.clone(), Vec::new());
    let actual = build_quality_single_merge_plan(true, colors.clone(), Vec::new());

    let weighted_error = |plan: &QualityIndexPlan| -> u64 {
        colors
            .iter()
            .map(|color| {
                let cell = usize::from(color.histogram_index);
                let representative = plan.palette[usize::from(plan.histogram_to_palette[cell])];
                let dr = i32::from(color.red) - ((representative >> 16) & 255) as i32;
                let dg = i32::from(color.green) - ((representative >> 8) & 255) as i32;
                let db = i32::from(color.blue) - (representative & 255) as i32;
                (dr * dr + dg * dg + db * db) as u64 * quantized_color_count_u64(color.count)
            })
            .sum()
    };
    let expected_error = weighted_error(&expected);
    let actual_error = weighted_error(&actual);
    assert!(
        actual_error <= expected_error,
        "{actual_error} should not exceed {expected_error}"
    );

    recycle_quality_histogram_to_palette(expected.histogram_to_palette);
    recycle_quality_palette(expected.palette);
    recycle_quality_histogram_to_palette(actual.histogram_to_palette);
    recycle_quality_palette(actual.palette);
}

#[test]
fn quality_precision_guard_keeps_smooth_ramps_on_the_fine_histogram() {
    let mut gradient = Vec::new();
    let mut noisy = Vec::new();
    for y in 0..64u16 {
        for x in 0..64u16 {
            gradient.extend_from_slice(&[
                (x * 255 / 63) as u8,
                (y * 255 / 63) as u8,
                ((x + y) * 127 / 126) as u8,
                255,
            ]);
            noisy.extend_from_slice(&[
                ((x * 73 + y * 151) & 255) as u8,
                ((x * 193 + y * 47) & 255) as u8,
                ((x * 11 + y * 223) & 255) as u8,
                255,
            ]);
        }
    }

    assert!(quality_prefers_high_precision_histogram(
        &gradient,
        TRANSPARENT_ALPHA_THRESHOLD,
    ));
    assert!(!quality_prefers_high_precision_histogram(
        &noisy,
        TRANSPARENT_ALPHA_THRESHOLD,
    ));
}

#[test]
fn quality_encoding_preserves_indexed_pixels() {
    let width = 64u16;
    let height = 64u16;
    let frame_count = 3usize;
    let mut rgba = Vec::with_capacity(usize::from(width) * usize::from(height) * frame_count * 4);
    for frame in 0..frame_count as u16 {
        for y in 0..height {
            for x in 0..width {
                let moving = x >= frame * 9 && x < frame * 9 + 20;
                rgba.extend_from_slice(&[
                    ((x * 73 + y * 151 + u16::from(moving) * 40) & 255) as u8,
                    ((x * 193 + y * 47) & 255) as u8,
                    ((x * 11 + y * 223) & 255) as u8,
                    if (x + y) % 5 == 0 && !moving { 0 } else { 255 },
                ]);
            }
        }
    }
    let delays = [10u16, 20, 30];
    let (palette, indexed, transparent_index) =
        index_rgba_frames_quality(&rgba, TRANSPARENT_ALPHA_THRESHOLD);
    let literal = encode_indexed_literal_gif_inner_with_output(
        Vec::new(),
        &indexed,
        width,
        height,
        frame_count,
        &palette,
        DelaySource::PerFrame(&delays),
        0,
        transparent_index,
    )
    .unwrap();
    recycle_quantized_indexed(indexed);
    let literal_metadata = parse_metadata(&literal).unwrap();
    let requested = vec![1u8; frame_count];
    let expected =
        prepare_composited_frames_inner(&literal, &literal_metadata, &requested, PixelFormat::Rgba)
            .unwrap();

    for independent_frames in [false, true] {
        let encoded = encode_rgba_quality_gif_inner_with_output(
            &rgba,
            width,
            height,
            frame_count,
            DelaySource::PerFrame(&delays),
            0,
            TRANSPARENT_ALPHA_THRESHOLD,
            independent_frames,
            Vec::new(),
        )
        .unwrap();
        let metadata = parse_metadata(&encoded).unwrap();
        assert_eq!(
            metadata
                .frames
                .iter()
                .map(|frame| frame.delay)
                .collect::<Vec<_>>(),
            delays
        );
        assert_eq!(
            prepare_composited_frames_inner(&encoded, &metadata, &requested, PixelFormat::Rgba)
                .unwrap(),
            expected,
            "independent_frames={independent_frames}"
        );
        assert!(
            encoded.len() <= literal.len(),
            "independent_frames={independent_frames} compact={} literal={}",
            encoded.len(),
            literal.len()
        );
    }
}

#[test]
fn opaque_quality_scan_is_identical_for_zero_alpha_threshold() {
    let mut rgba = Vec::new();
    for y in 0..48u16 {
        for x in 0..48u16 {
            rgba.extend_from_slice(&[
                ((x * 13 + y * 7) & 255) as u8,
                ((x * 5 + y * 17) & 255) as u8,
                ((x * 19 + y * 3) & 255) as u8,
                255,
            ]);
        }
    }

    let thresholded = index_rgba_frames_quality(&rgba, TRANSPARENT_ALPHA_THRESHOLD);
    let opaque = index_rgba_frames_quality(&rgba, 0);
    assert_eq!(thresholded, opaque);
}

#[test]
fn low_res_opaque_probe_recovers_unsampled_transparency() {
    let pixel_count = 8_192usize;
    let mut rgba = Vec::with_capacity(pixel_count * 4);
    for pixel in 0..pixel_count {
        rgba.extend_from_slice(&[
            (pixel * 73) as u8,
            (pixel * 151 + 17) as u8,
            (pixel * 211 + 29) as u8,
            255,
        ]);
    }
    rgba[7] = 0;
    assert!(rgba_stream_samples_alpha_255(&rgba));

    let mut histogram = take_quality_histogram_u32(1 << 12);
    let has_transparent = accumulate_quality_histogram_u32_bits_remaining_mixed::<4>(
        &mut histogram,
        &rgba,
        0,
        TRANSPARENT_ALPHA_THRESHOLD,
    );
    let expected =
        finish_quality_low_res_quantized(histogram, has_transparent, take_quality_palette(256))
            .into_indexed(&rgba, TRANSPARENT_ALPHA_THRESHOLD);
    let actual = index_rgba_frames_quality_low_res_quantized_alpha(
        &rgba,
        TRANSPARENT_ALPHA_THRESHOLD,
        take_quality_palette(256),
    )
    .into_indexed(&rgba, TRANSPARENT_ALPHA_THRESHOLD);

    assert_eq!(actual, expected);
    recycle_quality_palette(expected.0);
    recycle_quantized_indexed(expected.1);
    recycle_quality_palette(actual.0);
    recycle_quantized_indexed(actual.1);
}

#[test]
fn opaque_quality_histogram_fast_scan_matches_alpha_checked_scan() {
    let mut rgba = Vec::new();
    for y in 0..64u16 {
        for x in 0..64u16 {
            rgba.extend_from_slice(&[
                ((x * 13 + y * 7) & 255) as u8,
                ((x * 5 + y * 17) & 255) as u8,
                ((x * 19 + y * 3) & 255) as u8,
                255,
            ]);
        }
    }

    let materialize = |result| match result {
        QualityIndexResult::Exact(indexed) => indexed,
        QualityIndexResult::Quantized(plan) => plan.into_indexed(&rgba, 179),
    };
    let fast = materialize(index_rgba_frames_quality_u32::<4>(&rgba, 179, true));
    let checked = materialize(index_rgba_frames_quality_u32::<4>(&rgba, 179, false));
    assert_eq!(fast, checked);
}

#[test]
fn local_palette_mode_preserves_independent_exact_frame_colors() {
    let mut rgba = Vec::new();
    for frame in 0..2u16 {
        for value in 0..256u16 {
            rgba.extend_from_slice(&[
                value as u8,
                ((value * 3 + frame * 17) & 0xff) as u8,
                ((value * 7 + frame * 29) & 0xff) as u8,
                255,
            ]);
        }
    }

    let encoded = encode_rgba_gif_advanced_inner(
        &rgba,
        16,
        16,
        2,
        &[],
        DelaySource::PerFrame(&[4, 9]),
        0,
        false,
        TRANSPARENT_ALPHA_THRESHOLD,
        RgbaQuantization::Exact,
        RgbaPaletteMode::Local,
    )
    .unwrap();
    let metadata = parse_metadata(&encoded).unwrap();

    assert_eq!(metadata.global_palette_size, 0);
    assert_eq!(metadata.frames.len(), 2);
    assert!(metadata.frames.iter().all(|frame| frame.has_local_palette));
    assert_eq!(metadata.frames[0].delay, 4);
    assert_eq!(metadata.frames[1].delay, 9);
    assert!(metadata.frames.iter().all(|frame| frame.disposal == 2));
    for frame_index in 0..metadata.frames.len() {
        let decoded =
            decode_frame_pixels_inner(&encoded, &metadata, frame_index, PixelFormat::Rgba).unwrap();
        let expected_start = frame_index * 16 * 16 * 4;
        assert_eq!(decoded, rgba[expected_start..expected_start + 16 * 16 * 4]);
    }
}
#[test]
fn packed_quality_histogram_indices_match_channel_indices() {
    for red in (0..=255u16).step_by(17) {
        for green in (0..=255u16).step_by(17) {
            for blue in (0..=255u16).step_by(17) {
                let packed = u32::from(red as u8)
                    | (u32::from(green as u8) << 8)
                    | (u32::from(blue as u8) << 16)
                    | 0xaa00_0000;
                assert_eq!(
                    quality_histogram_index_packed::<4>(packed),
                    quality_histogram_index_bits_const::<4>(red as u8, green as u8, blue as u8,)
                );
                assert_eq!(
                    quality_histogram_index_packed::<5>(packed),
                    quality_histogram_index_bits_const::<5>(red as u8, green as u8, blue as u8,)
                );
            }
        }
    }
}

#[test]
fn reciprocal_quality_averages_match_integer_division() {
    let verify = |sum: u32, count: u32| {
        let reciprocal = (1u64 << 32) / u64::from(count);
        assert_eq!(
            rounded_histogram_average_u32_reciprocal(sum, count, reciprocal),
            ((sum + count / 2) / count) as u8,
            "sum={sum}, count={count}",
        );
    };
    for count in [1u32, 2, 3, 7, 255, 256, 257, 65_535, 1_000_000] {
        for sum in [
            0,
            count / 2,
            count.saturating_sub(1),
            count,
            count * 127,
            count * 255,
        ] {
            verify(sum, count);
        }
    }
    let mut state = 0x9e37_79b9u32;
    for _ in 0..250_000 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let count = state % 1_000_000 + 1;
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let sum = state % (count * 255 + 1);
        verify(sum, count);
    }

    let verify_weighted = |sum: u32, count: u32| {
        let reciprocal = (1u64 << 31) / u64::from(count);
        let half = count / 2;
        let expected = match sum.checked_add(half) {
            Some(adjusted) => (adjusted / count) as u8,
            None => ((u64::from(sum) + u64::from(half)) / u64::from(count)) as u8,
        };
        assert_eq!(
            rounded_weighted_average_u32_reciprocal(sum, count, reciprocal),
            expected,
            "weighted sum={sum}, count={count}",
        );
    };
    for count in [
        1u32,
        2,
        3,
        255,
        256,
        65_535,
        1_000_000,
        QUALITY_U32_PIXEL_LIMIT as u32,
    ] {
        let maximum_sum = (u64::from(count) * 255).min(u64::from(u32::MAX)) as u32;
        for sum in [0, count / 2, count, maximum_sum] {
            verify_weighted(sum, count);
        }
    }
    let mut wide_state = 0xd1b5_4a32_d192_ed03u64;
    for _ in 0..250_000 {
        wide_state = wide_state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let count = (wide_state % QUALITY_U32_PIXEL_LIMIT as u64 + 1) as u32;
        let maximum_sum = (u64::from(count) * 255).min(u64::from(u32::MAX));
        wide_state = wide_state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let sum = (wide_state % (maximum_sum + 1)) as u32;
        verify_weighted(sum, count);
    }
}

#[test]
fn temporal_quality_histogram_matches_the_per_pixel_scan() {
    let mut state = 0x1234_5678u32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    for trial in 0..40 {
        let width = 8 + (next() as usize % 60);
        let height = 1 + (next() as usize % 40);
        let frame_count = 2 + (next() as usize % 8);
        let frame_len = width * height;
        let mut rgba = Vec::with_capacity(frame_len * frame_count * 4);
        for _ in 0..frame_len {
            let alpha = if next() % 9 == 0 { 0 } else { 255 };
            rgba.extend_from_slice(&[next() as u8, next() as u8, next() as u8, alpha]);
        }
        for frame in 1..frame_count {
            let previous = (frame - 1) * frame_len * 4;
            rgba.extend_from_within(previous..previous + frame_len * 4);
            // Change a few spans, sometimes none, so groups both hold and change.
            for _ in 0..(next() % 4) {
                let start = next() as usize % frame_len;
                let end = (start + 1 + next() as usize % 40).min(frame_len);
                for pixel in start..end {
                    let offset = (frame * frame_len + pixel) * 4;
                    rgba[offset..offset + 4].copy_from_slice(&[
                        next() as u8,
                        next() as u8,
                        next() as u8,
                        if trial % 3 == 0 { next() as u8 } else { 255 },
                    ]);
                }
            }
        }
        let threshold = if trial % 4 == 0 { 0 } else { 128 };
        let expected = if threshold == 0 {
            index_rgba_frames_quality_low_res_quantized_opaque(&rgba, take_quality_palette(256))
        } else {
            index_rgba_frames_quality_low_res_quantized_mixed(
                &rgba,
                threshold,
                take_quality_palette(256),
            )
        };
        let actual = index_rgba_frames_quality_low_res_quantized_temporal(
            &rgba,
            frame_len,
            threshold,
            take_quality_palette(256),
        );
        assert_eq!(actual.palette, expected.palette, "trial {trial}");
        assert_eq!(
            actual.histogram_to_palette, expected.histogram_to_palette,
            "trial {trial}"
        );
        assert_eq!(
            actual.transparent_index, expected.transparent_index,
            "trial {trial}"
        );
    }
}

#[test]
fn quality_frame_reuse_matches_a_full_mapping() {
    let (width, height) = (48usize, 20usize);
    let frame_len = width * height;
    let mut state = 0x0bad_5eedu32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let mut first = Vec::with_capacity(frame_len * 4);
    for _ in 0..frame_len {
        let alpha = if next() % 11 == 0 { 0 } else { 255 };
        first.extend_from_slice(&[next() as u8, next() as u8, next() as u8, alpha]);
    }
    let mut second = first.clone();
    for pixel in (0..frame_len).step_by(37) {
        second[pixel * 4..pixel * 4 + 4].copy_from_slice(&[next() as u8, 9, 200, 255]);
    }
    let mut rgba = first.clone();
    rgba.extend_from_slice(&second);
    let plan = match index_rgba_frames_quality_result(&rgba, TRANSPARENT_ALPHA_THRESHOLD) {
        QualityIndexResult::Quantized(plan) => plan,
        QualityIndexResult::Exact(_) => panic!("fixture should need quantization"),
    };
    assert_eq!(plan.mapping_bits, 4);
    let transparent = plan
        .transparent_index
        .expect("fixture has transparent pixels");
    let table = &plan.histogram_to_palette;
    let mut previous = vec![0u8; frame_len];
    map_quality_frame::<4, true>(
        &first,
        None,
        TRANSPARENT_ALPHA_THRESHOLD,
        transparent,
        table,
        &mut previous,
    );
    let mut reused = vec![0u8; frame_len];
    map_quality_frame::<4, true>(
        &second,
        Some((&first, &previous)),
        TRANSPARENT_ALPHA_THRESHOLD,
        transparent,
        table,
        &mut reused,
    );
    let mut mapped = vec![0u8; frame_len];
    map_quality_pixels_grouped::<4, true>(
        &second,
        TRANSPARENT_ALPHA_THRESHOLD,
        transparent,
        table,
        &mut mapped,
    );
    assert_eq!(reused, mapped);
}
