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
    let gif = encode_indexed_gif_inner(
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
    let encoded = encode_indexed_lzw_inner(&[1], 1, 2).unwrap();
    assert_eq!(encoded, vec![1, 1, 0x36, 0]);

    assert!(encode_indexed_lzw_inner(&[2], 1, 2)
        .unwrap_err()
        .contains("Pixel index out of range"));
}

#[test]
fn encodes_indexed_gif_frames() {
    let palette = [0x000000, 0xff0000, 0x00ff00];
    let encoded = encode_indexed_gif_inner(
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
fn encodes_full_256_color_lzw_stream() {
    let palette: Vec<u32> = (0..256u32)
        .map(|value| (value << 16) | (value << 8) | value)
        .collect();
    let indices: Vec<u8> = (0..4096u32).map(|value| (value & 0xff) as u8).collect();
    let encoded = encode_indexed_gif_inner(
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
fn direct_lzw_color_decoder_matches_dictionary_stream() {
    let indices: Vec<u8> = (0..8192u32)
        .map(|index| ((index * 17 + (index >> 3) * 5) & 255) as u8)
        .collect();
    let palette: Vec<u32> = (0..256u32)
        .map(|index| 0xff00_0000 | (index << 16) | (index << 8) | index)
        .collect();
    let mut image_data = Vec::new();
    let mut tables = LzwEncodeTables::new();
    encode_indexed_lzw_to_with_tables(&mut image_data, &indices, 8, 256, &mut tables).unwrap();
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
    let encoded = encode_indexed_delta_gif_inner(
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
        false,
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
        true,
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
        false,
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

    let encoded = encode_rgba_gif_inner(
        &rgba,
        257,
        1,
        1,
        &[],
        DelaySource::Constant(0),
        -1,
        false,
        TRANSPARENT_ALPHA_THRESHOLD,
        false,
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
fn advanced_fast_compression_quantizes_arbitrary_rgba() {
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
        true,
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
    let midpoint = (colors.iter().map(|color| color.count).sum::<u64>() + 1) / 2;
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
        let sparse_table = tree.coarse_nearest_table_for_cells(&palette, &requested, &[0; 1 << 12]);
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
fn fused_quality_literal_encoding_preserves_indexed_pixels() {
    let width = 64u16;
    let height = 64u16;
    let mut rgba = Vec::with_capacity(usize::from(width) * usize::from(height) * 4);
    for y in 0..height {
        for x in 0..width {
            rgba.extend_from_slice(&[
                ((x * 73 + y * 151) & 255) as u8,
                ((x * 193 + y * 47) & 255) as u8,
                ((x * 11 + y * 223) & 255) as u8,
                if (x + y) % 5 == 0 { 0 } else { 255 },
            ]);
        }
    }
    let delays = [10u16];
    let (palette, indexed, transparent_index) =
        index_rgba_frames_quality(&rgba, TRANSPARENT_ALPHA_THRESHOLD);
    let indexed_output = encode_indexed_literal_gif_inner_with_output(
        Vec::new(),
        &indexed,
        width,
        height,
        1,
        &palette,
        DelaySource::PerFrame(&delays),
        0,
        transparent_index,
    )
    .unwrap();
    recycle_quantized_indexed(indexed);

    let plan = match index_rgba_frames_quality_result(&rgba, TRANSPARENT_ALPHA_THRESHOLD) {
        QualityIndexResult::Quantized(plan) => plan,
        QualityIndexResult::Exact(_) => panic!("fixture should overflow the exact palette"),
    };
    let fused_output = encode_quality_index_plan_literal_gif(
        Vec::new(),
        &rgba,
        width,
        height,
        1,
        DelaySource::PerFrame(&delays),
        0,
        TRANSPARENT_ALPHA_THRESHOLD,
        plan,
    )
    .unwrap();
    let indexed_metadata = parse_metadata(&indexed_output).unwrap();
    let fused_metadata = parse_metadata(&fused_output).unwrap();
    assert_eq!(
        decode_frame_indices_inner(&fused_output, &fused_metadata.frames[0]).unwrap(),
        decode_frame_indices_inner(&indexed_output, &indexed_metadata.frames[0]).unwrap()
    );
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
        true,
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
fn quality_materialization_preserves_palette_indices() {
    let histogram_to_palette: Vec<u8> = (0..=4095u16).map(|value| value as u8).collect();
    let opaque = materialize_quality_indices(vec![0, 17, 255, 4095], None, &histogram_to_palette);
    assert_eq!(opaque, vec![0, 17, 255, 255]);

    let transparent = materialize_quality_indices(
        vec![3, u16::MAX, 7, u16::MAX],
        Some(42),
        &histogram_to_palette,
    );
    assert_eq!(transparent, vec![3, 42, 7, 42]);
    recycle_quantized_indexed(opaque);
    recycle_quantized_indexed(transparent);
}
