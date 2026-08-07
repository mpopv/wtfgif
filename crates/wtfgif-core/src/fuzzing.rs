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
    let encoded = encode_rgba_quality_gif_inner_with_output(
        &rgba,
        width,
        height,
        frame_count,
        DelaySource::Constant(u16::from(data[3])),
        0,
        data[3],
        Vec::new(),
    )
    .expect("bounded RGBA input must encode");
    let metadata = parse_metadata(&encoded).expect("encoder output must parse");
    assert_eq!(metadata.width, width);
    assert_eq!(metadata.height, height);
    assert_eq!(metadata.frames.len(), frame_count);
    fuzz_decode(&encoded);
}
