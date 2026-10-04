use super::*;

#[test]
fn indexed_encoder_produces_a_complete_gif() {
    let encoded = encode_indexed_literal_gif(&[0, 1, 1, 0], 2, 2, 1, &[0x000000, 0xffffff], 3, 0)
        .expect("indexed encode");
    assert!(encoded.starts_with(b"GIF89a"));
    assert_eq!(encoded.last(), Some(&0x3b));
}

#[test]
fn rgba_encoder_produces_a_complete_gif() {
    let encoded = encode_rgba_literal_gif(
        &[0, 0, 0, 255, 255, 255, 255, 255],
        2,
        1,
        1,
        &[0x000000, 0xffffff],
        3,
        0,
    )
    .expect("RGBA encode");
    assert!(encoded.starts_with(b"GIF89a"));
    assert_eq!(encoded.last(), Some(&0x3b));
}

#[test]
fn nine_bit_literal_expansion_matches_reference_packing() {
    let mut packed = 0x0123_4567u32;
    for _ in 0..10_000 {
        packed ^= packed << 13;
        packed ^= packed >> 17;
        packed ^= packed << 5;

        let mut reference = 0u64;
        for lane in 0..4 {
            reference |= u64::from((packed >> (lane * 8)) as u8) << (lane * 9);
        }
        assert_eq!(expand_four_literal_codes_to_nine_bits(packed), reference);
    }
}

#[test]
fn palette_kd_nodes_keep_the_profiled_layout() {
    assert_eq!(std::mem::size_of::<PaletteKdNode>(), 16);
    assert_eq!(std::mem::align_of::<PaletteKdNode>(), 16);
}
