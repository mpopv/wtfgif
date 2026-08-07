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
