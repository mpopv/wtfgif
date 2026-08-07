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
    let mut packed = 0x0123_4567_89ab_cdefu64;
    for _ in 0..10_000 {
        packed ^= packed << 13;
        packed ^= packed >> 7;
        packed ^= packed << 17;

        let mut reference = 0u128;
        for lane in 0..8 {
            reference |= u128::from((packed >> (lane * 8)) as u8) << (lane * 9);
        }
        let (low, high) = expand_eight_literal_codes_to_nine_bits(packed);
        assert_eq!(u128::from(low) | (u128::from(high) << 64), reference);
    }
}
