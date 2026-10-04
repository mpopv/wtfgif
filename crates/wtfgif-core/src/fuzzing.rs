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
        independent_frames,
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
