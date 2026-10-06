//! GIF stream parsing, structural validation, and metadata JSON.

use super::*;

pub(crate) fn parse_metadata(data: &[u8]) -> Result<GifMetadata, String> {
    if data.len() < 13 {
        return Err("GIF data is too short for a header".to_string());
    }

    let version = match &data[0..6] {
        b"GIF87a" => "GIF87a",
        b"GIF89a" => "GIF89a",
        _ => return Err("Invalid GIF signature".to_string()),
    };

    let width = read_u16(data, 6, "logical screen width")?;
    let height = read_u16(data, 8, "logical screen height")?;
    let packed = data[10];
    let has_global_palette = (packed & 0x80) != 0;
    let global_palette_size = if has_global_palette {
        2usize << usize::from(packed & 0x07)
    } else {
        0
    };
    let global_palette_offset = has_global_palette.then_some(13);
    let mut offset = 13usize;

    if has_global_palette {
        offset = checked_add(
            offset,
            global_palette_size * 3,
            data.len(),
            "global color table",
        )?;
    }

    let mut frames = Vec::new();
    let mut graphic_control = GraphicControl::default();
    let mut loop_count = None;

    while offset < data.len() {
        let byte = data[offset];
        offset += 1;

        match byte {
            0x2c => {
                let (frame, next_offset) = parse_image_descriptor(
                    data,
                    offset,
                    global_palette_offset,
                    global_palette_size,
                    graphic_control,
                )?;
                validate_frame_bounds(&frame, width, height)?;
                frames.push(frame);
                offset = next_offset;
                graphic_control = GraphicControl::default();
            }
            0x21 => {
                if offset >= data.len() {
                    return Err("Truncated extension block".to_string());
                }
                let label = data[offset];
                offset += 1;

                if label == 0xf9 {
                    let (gce, next_offset) = parse_graphic_control(data, offset)?;
                    graphic_control = gce;
                    offset = next_offset;
                } else {
                    if label == 0xff {
                        loop_count = read_loop_count_extension(data, offset).or(loop_count);
                    }
                    offset = skip_sub_blocks(data, offset, "extension data")?;
                }
            }
            0x3b => break,
            _ => {
                return Err(format!(
                    "Unexpected GIF block byte 0x{byte:02x} at offset {}",
                    offset - 1
                ));
            }
        }
    }

    Ok(GifMetadata {
        version,
        width,
        height,
        global_palette_offset,
        global_palette_size,
        loop_count,
        frames,
    })
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn validate_gif_structure_no_alloc(data: &[u8]) -> Result<(), String> {
    if data.len() < 13 {
        return Err("GIF data is too short for a header".to_string());
    }
    if !matches!(&data[0..6], b"GIF87a" | b"GIF89a") {
        return Err("Invalid GIF signature".to_string());
    }

    let width = read_u16(data, 6, "logical screen width")?;
    let height = read_u16(data, 8, "logical screen height")?;
    let packed = data[10];
    let has_global_palette = (packed & 0x80) != 0;
    let global_palette_size = if has_global_palette {
        2usize << usize::from(packed & 0x07)
    } else {
        0
    };
    let global_palette_offset = has_global_palette.then_some(13);
    let mut offset = 13usize;
    if has_global_palette {
        offset = checked_add(
            offset,
            global_palette_size * 3,
            data.len(),
            "global color table",
        )?;
    }

    let mut graphic_control = GraphicControl::default();
    while offset < data.len() {
        let byte = data[offset];
        offset += 1;
        match byte {
            0x2c => {
                let (frame, next_offset) = parse_image_descriptor(
                    data,
                    offset,
                    global_palette_offset,
                    global_palette_size,
                    graphic_control,
                )?;
                validate_frame_bounds(&frame, width, height)?;
                offset = next_offset;
                graphic_control = GraphicControl::default();
            }
            0x21 => {
                if offset >= data.len() {
                    return Err("Truncated extension block".to_string());
                }
                let label = data[offset];
                offset += 1;
                if label == 0xf9 {
                    let (gce, next_offset) = parse_graphic_control(data, offset)?;
                    graphic_control = gce;
                    offset = next_offset;
                } else {
                    offset = skip_sub_blocks(data, offset, "extension data")?;
                }
            }
            0x3b => return Ok(()),
            _ => {
                return Err(format!(
                    "Unexpected GIF block byte 0x{byte:02x} at offset {}",
                    offset - 1
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn read_loop_count_extension(data: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(16)?;
    if end > data.len()
        || data[offset] != 11
        || (&data[offset + 1..offset + 12] != b"NETSCAPE2.0"
            && &data[offset + 1..offset + 12] != b"ANIMEXTS1.0")
        || data[offset + 12] != 3
        || data[offset + 13] != 1
    {
        return None;
    }
    Some(u16::from_le_bytes([data[offset + 14], data[offset + 15]]))
}

pub(crate) fn parse_graphic_control(
    data: &[u8],
    offset: usize,
) -> Result<(GraphicControl, usize), String> {
    let block_end = checked_add(offset, 6, data.len(), "graphic control extension")?;
    if data[offset] != 4 {
        return Err(format!(
            "Invalid graphic control extension length {} at offset {offset}",
            data[offset]
        ));
    }
    if data[offset + 5] != 0 {
        return Err(format!(
            "Graphic control extension missing terminator at offset {}",
            offset + 5
        ));
    }

    let packed = data[offset + 1];
    let delay = read_u16(data, offset + 2, "graphic control delay")?;
    let transparent_index = ((packed & 0x01) != 0).then_some(data[offset + 4]);

    Ok((
        GraphicControl {
            delay,
            disposal: (packed >> 2) & 0x07,
            transparent_index,
        },
        block_end,
    ))
}

pub(crate) fn parse_image_descriptor(
    data: &[u8],
    offset: usize,
    global_palette_offset: Option<usize>,
    global_palette_size: usize,
    graphic_control: GraphicControl,
) -> Result<(FrameMetadata, usize), String> {
    let descriptor_end = checked_add(offset, 9, data.len(), "image descriptor")?;
    let x = read_u16(data, offset, "image x")?;
    let y = read_u16(data, offset + 2, "image y")?;
    let width = read_u16(data, offset + 4, "image width")?;
    let height = read_u16(data, offset + 6, "image height")?;
    let packed = data[offset + 8];
    let has_local_palette = (packed & 0x80) != 0;
    let interlaced = (packed & 0x40) != 0;

    let mut data_offset = descriptor_end;
    let (palette_offset, palette_size) = if has_local_palette {
        let palette_size = 2usize << usize::from(packed & 0x07);
        let palette_offset = data_offset;
        data_offset = checked_add(
            data_offset,
            palette_size * 3,
            data.len(),
            "local color table",
        )?;
        (palette_offset, palette_size)
    } else {
        (global_palette_offset.unwrap_or(0), global_palette_size)
    };

    if data_offset >= data.len() {
        return Err("Image data is missing an LZW minimum code size".to_string());
    }
    let min_code_size = data[data_offset];
    let next_offset = skip_sub_blocks(data, data_offset + 1, "image data")?;
    let data_length = next_offset - data_offset;

    Ok((
        FrameMetadata {
            x,
            y,
            width,
            height,
            has_local_palette,
            palette_offset,
            palette_size,
            data_offset,
            data_length,
            transparent_index: graphic_control.transparent_index,
            interlaced,
            delay: graphic_control.delay,
            disposal: graphic_control.disposal,
            min_code_size,
        },
        next_offset,
    ))
}

pub(crate) fn validate_frame_bounds(
    frame: &FrameMetadata,
    canvas_width: u16,
    canvas_height: u16,
) -> Result<(), String> {
    let frame_right = u32::from(frame.x) + u32::from(frame.width);
    let frame_bottom = u32::from(frame.y) + u32::from(frame.height);
    if frame_right > u32::from(canvas_width) || frame_bottom > u32::from(canvas_height) {
        return Err("Image frame exceeds the logical screen bounds".to_string());
    }
    Ok(())
}

pub(crate) fn skip_sub_blocks(
    data: &[u8],
    mut offset: usize,
    context: &str,
) -> Result<usize, String> {
    loop {
        if offset >= data.len() {
            return Err(format!("Truncated {context}"));
        }
        let length = usize::from(data[offset]);
        offset += 1;
        if length == 0 {
            return Ok(offset);
        }
        offset = checked_add(offset, length, data.len(), context)?;
    }
}

pub(crate) fn read_u16(data: &[u8], offset: usize, context: &str) -> Result<u16, String> {
    let end = checked_add(offset, 2, data.len(), context)?;
    Ok(u16::from_le_bytes([data[offset], data[end - 1]]))
}

pub(crate) fn checked_add(
    offset: usize,
    length: usize,
    data_len: usize,
    context: &str,
) -> Result<usize, String> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| format!("Offset overflow while reading {context}"))?;
    if end > data_len {
        return Err(format!("Truncated {context}"));
    }
    Ok(end)
}

#[cfg(not(feature = "encode-only"))]
impl GifMetadata {
    pub(crate) fn to_json(&self) -> String {
        let mut json = String::new();
        json.push('{');
        push_json_field_str(&mut json, "version", self.version);
        json.push(',');
        push_json_field_u16(&mut json, "width", self.width);
        json.push(',');
        push_json_field_u16(&mut json, "height", self.height);
        json.push(',');
        push_json_field_usize_option(
            &mut json,
            "global_palette_offset",
            self.global_palette_offset,
        );
        json.push(',');
        push_json_field_usize(&mut json, "global_palette_size", self.global_palette_size);
        json.push_str(",\"frame_count\":");
        json.push_str(&self.frames.len().to_string());
        json.push_str(",\"frames\":[");
        for (index, frame) in self.frames.iter().enumerate() {
            if index > 0 {
                json.push(',');
            }
            frame.push_json(&mut json);
        }
        json.push_str("]}");
        json
    }
}

#[cfg(not(feature = "encode-only"))]
impl FrameMetadata {
    pub(crate) fn push_json(&self, json: &mut String) {
        json.push('{');
        push_json_field_u16(json, "x", self.x);
        json.push(',');
        push_json_field_u16(json, "y", self.y);
        json.push(',');
        push_json_field_u16(json, "width", self.width);
        json.push(',');
        push_json_field_u16(json, "height", self.height);
        json.push(',');
        push_json_field_bool(json, "has_local_palette", self.has_local_palette);
        json.push(',');
        push_json_field_usize(json, "palette_offset", self.palette_offset);
        json.push(',');
        push_json_field_usize(json, "palette_size", self.palette_size);
        json.push(',');
        push_json_field_usize(json, "data_offset", self.data_offset);
        json.push(',');
        push_json_field_usize(json, "data_length", self.data_length);
        json.push(',');
        push_json_field_u8_option(json, "transparent_index", self.transparent_index);
        json.push(',');
        push_json_field_bool(json, "interlaced", self.interlaced);
        json.push(',');
        push_json_field_u16(json, "delay", self.delay);
        json.push(',');
        push_json_field_u8(json, "disposal", self.disposal);
        json.push(',');
        push_json_field_u8(json, "min_code_size", self.min_code_size);
        json.push('}');
    }
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn push_json_field_str(json: &mut String, key: &str, value: &str) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":\"");
    json.push_str(value);
    json.push('"');
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn push_json_field_bool(json: &mut String, key: &str, value: bool) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":");
    json.push_str(if value { "true" } else { "false" });
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn push_json_field_u8(json: &mut String, key: &str, value: u8) {
    push_json_field_number(json, key, usize::from(value));
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn push_json_field_u16(json: &mut String, key: &str, value: u16) {
    push_json_field_number(json, key, usize::from(value));
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn push_json_field_usize(json: &mut String, key: &str, value: usize) {
    push_json_field_number(json, key, value);
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn push_json_field_u8_option(json: &mut String, key: &str, value: Option<u8>) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":");
    match value {
        Some(value) => json.push_str(&value.to_string()),
        None => json.push_str("null"),
    }
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn push_json_field_usize_option(json: &mut String, key: &str, value: Option<usize>) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":");
    match value {
        Some(value) => json.push_str(&value.to_string()),
        None => json.push_str("null"),
    }
}

#[cfg(not(feature = "encode-only"))]
pub(crate) fn push_json_field_number(json: &mut String, key: &str, value: usize) {
    json.push('"');
    json.push_str(key);
    json.push_str("\":");
    json.push_str(&value.to_string());
}
