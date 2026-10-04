//! Compact GIF assembly for the public quality encoder.
//!
//! Both techniques here are lossless: every decoded, composited frame is the
//! same as a literal full-frame encoding of the same palette indices.
//!
//! * Run-aware LZW codes. The stream keeps the literal writer's single code
//!   width and clear interval, but a repeated palette index is emitted through
//!   codes the decoder has already built for longer runs of that index. Each
//!   new run length is created through LZW's "code equals the next entry"
//!   rule, so a fresh run of `k * (k + 1) / 2` pixels needs only `k` codes.
//!   Every code still covers at least one pixel and the width never grows, so
//!   the stream is never longer than the literal stream. Flat images with few
//!   colors may use a wider code than their palette needs, which leaves room
//!   for more run codes per clear; a frame falls back to the palette's own
//!   width if the wider stream would be longer than a literal one.
//! * Frame differencing. A later frame is cropped to the rectangle that changed
//!   since the previous frame, and unchanged pixels inside that rectangle use
//!   the transparent index, which turns static areas into long runs. Frames
//!   that turn an opaque pixel transparent cannot be expressed this way, so
//!   they fall back to a full frame after a restore-to-background disposal.

use super::*;

/// Longest run, in pixels, that a single dictionary code may represent. The
/// fixed code width already bounds chains to a few hundred codes per clear.
const RUN_LENGTH_LIMIT: usize = 255;
const RUN_CODE_STRIDE: usize = RUN_LENGTH_LIMIT + 1;
const RUN_CODE_TABLE_LEN: usize = 256 * RUN_CODE_STRIDE;
const BYTE_LANES: u64 = 0x0101_0101_0101_0101;
// The grouped run detector in `run_lzw_codes` tests three equal pairs.
const _: () = assert!(MIN_CODED_RUN == 4);

pub(crate) struct CompactScratch {
    /// `run_codes[value * RUN_CODE_STRIDE + length]` is the dictionary code
    /// for `length` copies of `value`. Entries above the per-value maximum
    /// tracked by the writer are stale and never read.
    run_codes: Vec<u16>,
    /// Pixels of the current differenced rectangle.
    pixels: Vec<u8>,
    plans: Vec<FramePlan>,
}

reusable_cells! {
    static REUSABLE_COMPACT_SCRATCH: CompactScratch = CompactScratch {
            run_codes: Vec::new(),
            pixels: Vec::new(),
            plans: Vec::new(),
        };
}

/// An inclusive pixel rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
}

impl Rect {
    fn union(self, other: Rect) -> Rect {
        Rect {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    fn width(&self) -> usize {
        self.right - self.left + 1
    }

    fn height(&self) -> usize {
        self.bottom - self.top + 1
    }
}

/// Grow an optional bounding box to include columns `first..=last` of `row`.
fn include(bounds: &mut Option<Rect>, row: usize, first: usize, last: usize) {
    let span = Rect {
        left: first,
        top: row,
        right: last,
        bottom: row,
    };
    *bounds = Some(bounds.map_or(span, |bounds| bounds.union(span)));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FramePlan {
    rect: Rect,
    /// Pixels the canvas already shows may be left to it.
    delta: bool,
    disposal: u8,
    /// The previous frame's rectangle, which its restore-to-background
    /// disposal clears to transparent before this frame is drawn.
    cleared: Option<Rect>,
}

impl FramePlan {
    fn full(width: usize, height: usize) -> Self {
        Self {
            rect: Rect {
                left: 0,
                top: 0,
                right: width - 1,
                bottom: height - 1,
            },
            delta: false,
            disposal: 0,
            cleared: None,
        }
    }

    fn covers(&self, width: usize, height: usize) -> bool {
        self.rect.left == 0
            && self.rect.top == 0
            && self.rect.width() == width
            && self.rect.height() == height
    }
}

/// Encode already-indexed frames as one global-palette GIF.
///
/// `independent_frames` keeps the historical layout, in which every frame is a
/// full-canvas image that can be decoded or reordered without its neighbors.
#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_indexed_gif_compact(
    mut output: Vec<u8>,
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    palette_rgb: &[u32],
    delays: DelaySource<'_>,
    loop_count: i32,
    transparent_index: Option<u8>,
    independent_frames: bool,
) -> Result<Vec<u8>, String> {
    // Explicit Wasm initialization compiles this path with an empty sentinel.
    if frame_count == 0 && index_stream.is_empty() && palette_rgb.is_empty() {
        output.clear();
        return Ok(output);
    }
    if width == 0 || height == 0 {
        return Err("Width/Height invalid".to_string());
    }
    if frame_count == 0 {
        return Err("Frame count must be greater than zero".to_string());
    }
    if loop_count < -1 || loop_count > i32::from(u16::MAX) {
        return Err("Loop count invalid".to_string());
    }
    delays.validate(frame_count)?;
    let color_count = checked_palette_color_count(palette_rgb.len())?;
    let frame_len = usize::from(width) * usize::from(height);
    let expected_len = frame_len
        .checked_mul(frame_count)
        .ok_or_else(|| "Frame stream overflow".to_string())?;
    if index_stream.len() != expected_len {
        return Err("Indexed frame stream length does not match dimensions".to_string());
    }
    if transparent_index.is_some_and(|index| usize::from(index) >= color_count) {
        return Err("Transparent index is outside the palette".to_string());
    }
    let minimum_code_size = (log2_pow2(color_count) as u8).max(2);

    // Reserve the literal-stream bound at the widest code size once, so frame
    // writers never reallocate and copy the GIF assembled so far.
    let image_bound = literal_lzw_block_size(frame_len, 8)? + 8;
    let capacity = (8 + 10 + image_bound)
        .checked_mul(frame_count)
        .and_then(|frames| frames.checked_add(13 + color_count * 3 + 19 + 1 + 8))
        .ok_or_else(|| "Encoded GIF size overflow".to_string())?;
    output.clear();
    output.reserve(capacity);
    write_indexed_gif_header(&mut output, width, height, palette_rgb, color_count);
    write_loop_extension(&mut output, loop_count);

    REUSABLE_COMPACT_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let CompactScratch {
            run_codes,
            pixels,
            plans,
        } = &mut *scratch;
        if run_codes.len() < RUN_CODE_TABLE_LEN {
            run_codes.resize(RUN_CODE_TABLE_LEN, 0);
        }
        plan_frames(
            plans,
            index_stream,
            width,
            height,
            frame_count,
            transparent_index,
            independent_frames,
        );
        // A palette with an unused slot can mark unchanged pixels without a
        // larger color table, even when no source pixel is transparent.
        let fill_index = transparent_index
            .or_else(|| (palette_rgb.len() < color_count).then_some(palette_rgb.len() as u8));
        let mut previous: &[u8] = &[];
        for (frame_index, plan) in plans.iter().enumerate() {
            let frame = &index_stream[frame_index * frame_len..(frame_index + 1) * frame_len];
            let frame_transparent = if plan.delta {
                fill_index
            } else {
                transparent_index
            };
            write_indexed_gif_frame_header(
                &mut output,
                plan.rect.left as u16,
                plan.rect.top as u16,
                plan.rect.width() as u16,
                plan.rect.height() as u16,
                delays.get(frame_index),
                frame_transparent,
                plan.disposal,
            );
            // Marking unchanged pixels only pays when the rectangle leaves a
            // large static area; otherwise write the frame's own pixels.
            let fills = plan.delta
                && fill_index.is_some()
                && plan.rect.width() * plan.rect.height() * 4 <= frame_len * 3;
            let image: &[u8] = if fills {
                fill_delta_pixels(
                    previous,
                    frame,
                    usize::from(width),
                    plan,
                    fill_index,
                    transparent_index,
                    pixels,
                );
                pixels.as_slice()
            } else if plan.covers(usize::from(width), usize::from(height)) {
                frame
            } else {
                fill_delta_pixels(
                    previous,
                    frame,
                    usize::from(width),
                    plan,
                    None,
                    transparent_index,
                    pixels,
                );
                pixels.as_slice()
            };
            let code_size = choose_minimum_code_size(image, minimum_code_size, run_codes);
            let image_start = output.len();
            write_run_lzw_image(&mut output, image, code_size, run_codes);
            // The wider-code choice is a heuristic; never let it lose to the
            // literal stream at the palette's own code size.
            if code_size != minimum_code_size
                && output.len() - image_start
                    > literal_lzw_block_size(image.len(), minimum_code_size).unwrap_or(usize::MAX)
            {
                output.truncate(image_start);
                write_run_lzw_image(&mut output, image, minimum_code_size, run_codes);
            }
            previous = frame;
        }
    });
    output.push(0x3b);
    Ok(output)
}

fn plan_frames(
    plans: &mut Vec<FramePlan>,
    index_stream: &[u8],
    width: u16,
    height: u16,
    frame_count: usize,
    transparent_index: Option<u8>,
    independent_frames: bool,
) {
    let (width, height) = (usize::from(width), usize::from(height));
    let frame_len = width * height;
    let frame = |index: usize| &index_stream[index * frame_len..(index + 1) * frame_len];
    plans.clear();
    if frame_count == 0 {
        return;
    }
    if independent_frames || frame_count == 1 {
        let disposal = if transparent_index.is_some() { 2 } else { 0 };
        plans.extend((0..frame_count).map(|_| FramePlan {
            disposal,
            ..FramePlan::full(width, height)
        }));
        return;
    }

    plans.push(FramePlan::full(width, height));
    for frame_index in 1..frame_count {
        let (before, after) = (frame(frame_index - 1), frame(frame_index));
        // Behind a previous frame that already covers most of the canvas, clear
        // the whole canvas on the first pixel that turns transparent instead of
        // measuring exactly where they are.
        let previous_rect = plans[frame_index - 1].rect;
        let clears = transparent_index.and_then(|index| {
            if previous_rect.width() * previous_rect.height() * 4 >= frame_len * 3 {
                any_clears(before, after, index).then_some(FramePlan::full(width, height).rect)
            } else {
                clears_rect(before, after, width, index)
            }
        });
        let (rect, cleared) = match (clears, transparent_index) {
            (Some(clears), Some(index)) => {
                // Pixels that turn transparent are removed by restoring the
                // previous frame's area, grown to cover them, to background.
                // This frame then redraws its opaque pixels inside that area.
                let previous = &mut plans[frame_index - 1];
                previous.disposal = 2;
                previous.rect = previous.rect.union(clears);
                let cleared = previous.rect;
                (
                    changed_rect_after_clear(before, after, width, index, cleared),
                    Some(cleared),
                )
            }
            _ => {
                plans[frame_index - 1].disposal = 1;
                (changed_rect(before, after, width), None)
            }
        };
        plans.push(FramePlan {
            // An unchanged frame still needs an image: one kept pixel.
            rect: rect.unwrap_or(Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            }),
            delta: true,
            disposal: 1,
            cleared,
        });
    }

    // Decoders that keep the canvas when an animation repeats draw frame 0
    // over the last frame, so the last frame clears what frame 0 does not show.
    let last = frame_count - 1;
    if let Some(clears) =
        transparent_index.and_then(|index| clears_rect(frame(last), frame(0), width, index))
    {
        let plan = &mut plans[last];
        plan.disposal = 2;
        plan.rect = plan.rect.union(clears);
    }
}

/// Bytes equal to zero become `0x80` in their lane; every other lane is zero.
#[inline(always)]
fn zero_byte_lanes(value: u64) -> u64 {
    const LOW: u64 = 0x7f7f_7f7f_7f7f_7f7f;
    !(((value & LOW).wrapping_add(LOW)) | value | LOW)
}

#[inline(always)]
unsafe fn load_u64(pointer: *const u8) -> u64 {
    u64::from_le(std::ptr::read_unaligned(pointer.cast::<u64>()))
}

/// Bounds of the pixels that change between two frames.
fn changed_rect(before: &[u8], after: &[u8], width: usize) -> Option<Rect> {
    let mut bounds = None;
    for (row, (before, after)) in before
        .chunks_exact(width)
        .zip(after.chunks_exact(width))
        .enumerate()
    {
        if let Some((first, last)) = changed_span(before, after) {
            include(&mut bounds, row, first, last);
        }
    }
    bounds
}

/// Bounds of the pixels a frame must draw after the previous frame's
/// `cleared` rectangle has been restored to transparent.
fn changed_rect_after_clear(
    before: &[u8],
    after: &[u8],
    width: usize,
    transparent: u8,
    cleared: Rect,
) -> Option<Rect> {
    let mut bounds = None;
    for (row, (before, after)) in before
        .chunks_exact(width)
        .zip(after.chunks_exact(width))
        .enumerate()
    {
        if row < cleared.top || row > cleared.bottom {
            if let Some((first, last)) = changed_span(before, after) {
                include(&mut bounds, row, first, last);
            }
            continue;
        }
        let (left, right) = (cleared.left, cleared.right + 1);
        let spans = [
            changed_span(&before[..left], &after[..left]),
            opaque_span(&after[left..right], transparent).map(|(a, b)| (a + left, b + left)),
            changed_span(&before[right..], &after[right..]).map(|(a, b)| (a + right, b + right)),
        ];
        for (first, last) in spans.into_iter().flatten() {
            include(&mut bounds, row, first, last);
        }
    }
    bounds
}

/// Bounds of the pixels that are opaque in `before` and transparent in
/// `after`. A frame cannot leave those to the canvas.
fn clears_rect(before: &[u8], after: &[u8], width: usize, transparent: u8) -> Option<Rect> {
    let lanes = u64::from(transparent) * BYTE_LANES;
    let mut bounds = None;
    for (row, (before, after)) in before
        .chunks_exact(width)
        .zip(after.chunks_exact(width))
        .enumerate()
    {
        let mut first = usize::MAX;
        let mut last = 0usize;
        let mut column = 0usize;
        while column + 8 <= width {
            let (old, new) = unsafe {
                (
                    load_u64(before.as_ptr().add(column)),
                    load_u64(after.as_ptr().add(column)),
                )
            };
            let clears = zero_byte_lanes(new ^ lanes) & !zero_byte_lanes(old ^ lanes);
            if clears != 0 {
                first = first.min(column + clears.trailing_zeros() as usize / 8);
                last = column + 7 - clears.leading_zeros() as usize / 8;
            }
            column += 8;
        }
        while column < width {
            if after[column] == transparent && before[column] != transparent {
                first = first.min(column);
                last = column;
            }
            column += 1;
        }
        if first != usize::MAX {
            include(&mut bounds, row, first, last);
        }
    }
    bounds
}

/// Whether any pixel is opaque in `before` and transparent in `after`.
fn any_clears(before: &[u8], after: &[u8], transparent: u8) -> bool {
    let lanes = u64::from(transparent) * BYTE_LANES;
    let mut position = 0usize;
    while position + 8 <= after.len() {
        let (old, new) = unsafe {
            (
                load_u64(before.as_ptr().add(position)),
                load_u64(after.as_ptr().add(position)),
            )
        };
        if zero_byte_lanes(new ^ lanes) & !zero_byte_lanes(old ^ lanes) != 0 {
            return true;
        }
        position += 8;
    }
    (position..after.len()).any(|index| after[index] == transparent && before[index] != transparent)
}

/// First and last differing columns of a row.
fn changed_span(before: &[u8], after: &[u8]) -> Option<(usize, usize)> {
    let width = before.len();
    let mut first = 0usize;
    while first + 8 <= width {
        let difference =
            unsafe { load_u64(before.as_ptr().add(first)) ^ load_u64(after.as_ptr().add(first)) };
        if difference != 0 {
            first += difference.trailing_zeros() as usize / 8;
            break;
        }
        first += 8;
    }
    while first < width && before[first] == after[first] {
        first += 1;
    }
    if first == width {
        return None;
    }
    let mut last = width;
    while last >= first + 8 {
        let difference = unsafe {
            load_u64(before.as_ptr().add(last - 8)) ^ load_u64(after.as_ptr().add(last - 8))
        };
        if difference != 0 {
            return Some((first, last - 1 - difference.leading_zeros() as usize / 8));
        }
        last -= 8;
    }
    while before[last - 1] == after[last - 1] {
        last -= 1;
    }
    Some((first, last - 1))
}

/// First and last pixels of a row that are not `transparent`.
fn opaque_span(pixels: &[u8], transparent: u8) -> Option<(usize, usize)> {
    let lanes = u64::from(transparent) * BYTE_LANES;
    let width = pixels.len();
    let mut first = 0usize;
    while first + 8 <= width {
        let opaque = unsafe { load_u64(pixels.as_ptr().add(first)) } ^ lanes;
        if opaque != 0 {
            first += opaque.trailing_zeros() as usize / 8;
            break;
        }
        first += 8;
    }
    while first < width && pixels[first] == transparent {
        first += 1;
    }
    if first == width {
        return None;
    }
    let mut last = width;
    while last >= first + 8 {
        let opaque = unsafe { load_u64(pixels.as_ptr().add(last - 8)) } ^ lanes;
        if opaque != 0 {
            return Some((first, last - 1 - opaque.leading_zeros() as usize / 8));
        }
        last -= 8;
    }
    while pixels[last - 1] == transparent {
        last -= 1;
    }
    Some((first, last - 1))
}

/// Copy the plan's rectangle of `current` into `pixels`. With a fill index,
/// a pixel the canvas already shows becomes that index unless keeping its own
/// value continues the run that is already being emitted. The canvas shows
/// the previous frame, except inside a cleared rectangle, where it is
/// transparent.
fn fill_delta_pixels(
    previous: &[u8],
    current: &[u8],
    width: usize,
    plan: &FramePlan,
    fill_index: Option<u8>,
    transparent_index: Option<u8>,
    pixels: &mut Vec<u8>,
) {
    let rect = plan.rect;
    pixels.clear();
    pixels.reserve(rect.width() * rect.height());
    let Some(fill) = fill_index else {
        for row in rect.top..=rect.bottom {
            let start = row * width;
            pixels.extend_from_slice(&current[start + rect.left..=start + rect.right]);
        }
        return;
    };
    debug_assert_eq!(previous.len(), current.len());
    resize_output_uninitialized(pixels, rect.width() * rect.height());
    let mut writer = FillWriter {
        output: pixels.as_mut_ptr(),
        position: 0,
        last: fill,
        fill,
    };
    for row in rect.top..=rect.bottom {
        let start = row * width;
        let before = &previous[start..start + width];
        let after = &current[start..start + width];
        match (plan.cleared, transparent_index) {
            (Some(cleared), Some(transparent)) if (cleared.top..=cleared.bottom).contains(&row) => {
                // Clamp the cleared columns to this frame's rectangle.
                let inner_left = cleared.left.clamp(rect.left, rect.right + 1);
                let inner_right = (cleared.right + 1).clamp(rect.left, rect.right + 1);
                writer.segment(
                    Some(&before[rect.left..inner_left]),
                    &after[rect.left..inner_left],
                    transparent,
                );
                writer.segment(None, &after[inner_left..inner_right], transparent);
                writer.segment(
                    Some(&before[inner_right..=rect.right]),
                    &after[inner_right..=rect.right],
                    transparent,
                );
            }
            _ => writer.segment(
                Some(&before[rect.left..=rect.right]),
                &after[rect.left..=rect.right],
                0,
            ),
        }
    }
    debug_assert_eq!(writer.position, pixels.len());
}

struct FillWriter {
    output: *mut u8,
    position: usize,
    last: u8,
    fill: u8,
}

impl FillWriter {
    /// Append `after`, treating each pixel equal to the canvas as unchanged.
    /// The canvas is `before`, or all `transparent` when `before` is `None`.
    fn segment(&mut self, before: Option<&[u8]>, after: &[u8], transparent: u8) {
        if before.is_none() {
            // Over a cleared canvas, an unchanged pixel is transparent, and the
            // fill index is the transparent index, so every pixel keeps its
            // own value.
            debug_assert_eq!(transparent, self.fill);
            unsafe {
                std::ptr::copy_nonoverlapping(
                    after.as_ptr(),
                    self.output.add(self.position),
                    after.len(),
                )
            };
            self.position += after.len();
            self.last = after.last().copied().unwrap_or(self.last);
            return;
        }
        let canvas_lanes = u64::from(transparent) * BYTE_LANES;
        let fill_lanes = u64::from(self.fill) * BYTE_LANES;
        let mut column = 0usize;
        while column < after.len() {
            let mut scalar_end = column + 1;
            if column + 8 <= after.len() {
                let (old, new) = unsafe {
                    (
                        before.map_or(canvas_lanes, |before| load_u64(before.as_ptr().add(column))),
                        load_u64(after.as_ptr().add(column)),
                    )
                };
                let unchanged = zero_byte_lanes(old ^ new);
                let lanes = if unchanged == 0 {
                    Some(new)
                } else if unchanged == BYTE_LANES << 7 {
                    if self.last == self.fill {
                        Some(fill_lanes)
                    } else if new == u64::from(self.last) * BYTE_LANES {
                        Some(new)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(lanes) = lanes {
                    unsafe {
                        std::ptr::write_unaligned(
                            self.output.add(self.position).cast::<u64>(),
                            lanes.to_le(),
                        )
                    };
                    self.last = (lanes >> 56) as u8;
                    self.position += 8;
                    column += 8;
                    continue;
                }
                scalar_end = column + 8;
            }
            while column < scalar_end {
                let new = after[column];
                let old = before.map_or(transparent, |before| before[column]);
                let value = if new != old || new == self.last {
                    new
                } else {
                    self.fill
                };
                unsafe { self.output.add(self.position).write(value) };
                self.last = value;
                self.position += 1;
                column += 1;
            }
        }
    }
}

/// Choose the LZW minimum code size for one image. A wider code than the
/// palette needs gives each clear interval room for more and longer run codes,
/// which pays off on flat images with few colors.
fn choose_minimum_code_size(pixels: &[u8], palette_code_size: u8, run_codes: &mut [u16]) -> u8 {
    if palette_code_size >= 8 || pixels.len() < 2 {
        return palette_code_size;
    }
    let equal = count_equal_neighbors(pixels);
    // Runs average fewer than two pixels: wider codes would only add bits.
    if equal * 2 < pixels.len() {
        return palette_code_size;
    }
    // On flat corpus images the best wider size was 7 bits below a 128-color
    // palette and 8 bits at 128 colors; intermediate sizes never won.
    let wide = if palette_code_size < 7 { 7 } else { 8 };
    // Runs average at least four pixels: the wider code always won there.
    if equal * 4 >= pixels.len() * 3 {
        return wide;
    }
    let narrow_bits = count_run_lzw_codes(pixels, palette_code_size, run_codes)
        * (usize::from(palette_code_size) + 1);
    let wide_bits = count_run_lzw_codes(pixels, wide, run_codes) * (usize::from(wide) + 1);
    if wide_bits < narrow_bits {
        wide
    } else {
        palette_code_size
    }
}

/// Number of pixels equal to the pixel before them.
fn count_equal_neighbors(pixels: &[u8]) -> usize {
    let pointer = pixels.as_ptr();
    let mut equal = 0usize;
    let mut position = 0usize;
    while position + 9 <= pixels.len() {
        let (group, following) = unsafe {
            (
                load_u64(pointer.add(position)),
                load_u64(pointer.add(position + 1)),
            )
        };
        equal += zero_byte_lanes(group ^ following).count_ones() as usize;
        position += 8;
    }
    while position + 1 < pixels.len() {
        equal += usize::from(pixels[position] == pixels[position + 1]);
        position += 1;
    }
    equal
}

trait RunLzwSink {
    fn code(&mut self, code: u32);
    /// All eight bytes of `packed`, least significant first, as literals.
    fn eight_literals(&mut self, packed: u64);
    /// The first `count` (1..=8) bytes of `packed`, least significant first,
    /// as literal codes.
    fn literals(&mut self, packed: u64, count: usize);
}

struct CodeCounter {
    codes: usize,
}

impl RunLzwSink for CodeCounter {
    #[inline(always)]
    fn code(&mut self, _code: u32) {
        self.codes += 1;
    }

    #[inline(always)]
    fn eight_literals(&mut self, _packed: u64) {
        self.codes += 8;
    }

    #[inline(always)]
    fn literals(&mut self, _packed: u64, count: usize) {
        self.codes += count;
    }
}

struct CodeWriter {
    output: *mut u8,
    position: usize,
    bits: u64,
    bit_count: u32,
    code_size: u32,
}

impl CodeWriter {
    /// Store all pending bits and advance past each complete byte. The caller
    /// guarantees eight writable bytes at the current position.
    #[inline(always)]
    fn flush(&mut self) {
        unsafe {
            std::ptr::write_unaligned(
                self.output.add(self.position).cast::<u64>(),
                self.bits.to_le(),
            )
        };
        let bytes = self.bit_count >> 3;
        self.position += bytes as usize;
        self.bits >>= bytes * 8;
        self.bit_count &= 7;
    }
}

impl RunLzwSink for CodeWriter {
    #[inline(always)]
    fn code(&mut self, code: u32) {
        self.bits |= u64::from(code) << self.bit_count;
        self.bit_count += self.code_size;
        self.flush();
    }

    /// Eight 8- or 9-bit literals fill exactly 8 or 9 bytes, so they are
    /// written whole at any bit offset instead of code by code.
    #[inline(always)]
    fn eight_literals(&mut self, packed: u64) {
        let offset = self.bit_count;
        let (low, high, bytes) = match self.code_size {
            9 => {
                let first = expand_four_literal_codes_to_nine_bits(packed as u32);
                let second = expand_four_literal_codes_to_nine_bits((packed >> 32) as u32);
                (first | (second << 36), second >> 28, 9)
            }
            8 => (packed, 0, 8),
            _ => return self.literals(packed, 8),
        };
        // Bits shifted out of the first word; `>> 1 >> (63 - offset)` is a
        // shift by `64 - offset` that stays defined when the offset is zero.
        let carry = ((low >> 1) >> (63 - offset)) | (high << offset);
        unsafe {
            let output = self.output.add(self.position);
            std::ptr::write_unaligned(output.cast::<u64>(), (self.bits | (low << offset)).to_le());
            std::ptr::write_unaligned(output.add(8).cast::<u64>(), carry.to_le());
        }
        self.position += bytes;
        self.bits = if bytes == 9 { carry >> 8 } else { carry };
    }

    #[inline(always)]
    fn literals(&mut self, packed: u64, count: usize) {
        debug_assert!((1..=8).contains(&count));
        if self.code_size == 9 {
            let low = expand_four_literal_codes_to_nine_bits(packed as u32);
            let first = count.min(4) as u32 * 9;
            self.bits |= (low & ((1u64 << first) - 1)) << self.bit_count;
            self.bit_count += first;
            self.flush();
            if count > 4 {
                let high = expand_four_literal_codes_to_nine_bits((packed >> 32) as u32);
                let second = (count - 4) as u32 * 9;
                self.bits |= (high & ((1u64 << second) - 1)) << self.bit_count;
                self.bit_count += second;
                self.flush();
            }
        } else {
            for lane in 0..count {
                self.bits |= ((packed >> (lane * 8)) & 0xff) << self.bit_count;
                self.bit_count += self.code_size;
                if lane == 3 {
                    self.flush();
                }
            }
            self.flush();
        }
    }
}

fn count_run_lzw_codes(pixels: &[u8], minimum_code_size: u8, run_codes: &mut [u16]) -> usize {
    let mut counter = CodeCounter { codes: 0 };
    run_lzw_codes(pixels, minimum_code_size, run_codes, &mut counter);
    counter.codes
}

/// Append one complete image-data block: minimum code size, sub-blocks, and
/// the block terminator.
pub(crate) fn write_run_lzw_image(
    output: &mut Vec<u8>,
    pixels: &[u8],
    minimum_code_size: u8,
    run_codes: &mut [u16],
) {
    debug_assert!((2..=8).contains(&minimum_code_size));
    debug_assert!(run_codes.len() >= RUN_CODE_TABLE_LEN);
    output.push(minimum_code_size);
    let start = output.len();
    let code_size = u32::from(minimum_code_size) + 1;
    // No stream needs more codes than the literal stream: one per pixel, one
    // clear per full interval, and the end code.
    let interval = (1usize << minimum_code_size) - 2;
    let max_codes = pixels.len() + pixels.len().div_ceil(interval) + 2;
    let max_bytes = (max_codes * code_size as usize).div_ceil(8);
    let max_framed = max_bytes + max_bytes.div_ceil(255) + 1;
    output.reserve(max_framed.max(max_bytes + 8) + 8);
    let mut writer = CodeWriter {
        output: output.as_mut_ptr(),
        position: start,
        bits: 0,
        bit_count: 0,
        code_size,
    };
    run_lzw_codes(pixels, minimum_code_size, run_codes, &mut writer);
    if writer.bit_count > 0 {
        writer.position += 1;
    }
    let raw_length = writer.position - start;
    let block_count = raw_length.div_ceil(255);
    let final_length = start + raw_length + block_count + 1;
    debug_assert!(final_length <= output.capacity());
    // SAFETY: the reserved capacity holds the raw stream, which is moved
    // backwards into 255-byte sub-blocks before any byte is read.
    unsafe { output.set_len(final_length) };
    for block in (0..block_count).rev() {
        let source = start + block * 255;
        let length = (raw_length - block * 255).min(255);
        let destination = start + block * 256;
        output.copy_within(source..source + length, destination + 1);
        output[destination] = length as u8;
    }
    output[final_length - 1] = 0;
}

#[inline(always)]
fn run_end(pixels: &[u8], start: usize, value: u8) -> usize {
    let length = pixels.len();
    let pointer = pixels.as_ptr();
    let lanes = u64::from(value) * BYTE_LANES;
    let mut position = start + 1;
    while position + 8 <= length {
        let difference = unsafe { load_u64(pointer.add(position)) } ^ lanes;
        if difference != 0 {
            return position + difference.trailing_zeros() as usize / 8;
        }
        position += 8;
    }
    while position < length && pixels[position] == value {
        position += 1;
    }
    position
}

/// Shortest run the grouped fast path hands to the run-code machinery.
/// Shorter runs are emitted as literals together with their neighbors. On the
/// corpus this kept most pixels on the bulk literal path: coding runs of two
/// made MakeEmoji 16% smaller but its emission about 2.4 times slower.
const MIN_CODED_RUN: usize = 4;

/// Emit the fixed-width code stream for one image. The decoder adds the entry
/// `previous string + first byte of the current string` for every code except
/// the first after a clear, so the writer mirrors that bookkeeping exactly and
/// clears just before the entry count would widen the code.
#[inline(always)]
fn run_lzw_codes<S: RunLzwSink>(
    pixels: &[u8],
    minimum_code_size: u8,
    run_codes: &mut [u16],
    sink: &mut S,
) {
    let clear = 1u32 << minimum_code_size;
    let first_entry = clear + 2;
    let entry_limit = (1u32 << (minimum_code_size + 1)) - 1;
    let mut longest_run = [1u8; 256];
    let mut next_entry = first_entry;
    let mut fresh = true;
    let length = pixels.len();
    let pointer = pixels.as_ptr();
    sink.code(clear);
    let mut position = 0usize;
    while position < length {
        // Emit the next eight pixels as literals up to the first run of at
        // least MIN_CODED_RUN equal pixels. Literal codes are always valid; the
        // entries they create are simply not tracked for reuse. A run that
        // starts in the last lanes is found from the next group instead.
        let mut end = 0usize;
        if !fresh && position + 9 <= length {
            let (group, following) = unsafe {
                (
                    load_u64(pointer.add(position)),
                    load_u64(pointer.add(position + 1)),
                )
            };
            let equal = zero_byte_lanes(group ^ following);
            // Lanes where MIN_CODED_RUN equal pixels begin: three equal pairs.
            let starts = equal & (equal >> 8) & (equal >> 16);
            if starts == 0 {
                // Keep the common all-singleton step a fixed stride so the
                // next group's loads do not wait on this group's bit math.
                if next_entry + 8 <= entry_limit {
                    sink.eight_literals(group);
                    next_entry += 8;
                    position += 8;
                    continue;
                }
            } else {
                let literals = (starts.trailing_zeros() / 8) as usize;
                if next_entry + literals as u32 <= entry_limit {
                    if literals > 0 {
                        sink.literals(group, literals);
                        next_entry += literals as u32;
                    }
                    // The run ends at the next unequal pair in this group, or
                    // continues past it.
                    let differ = !equal & !(u64::MAX >> (63 - literals * 8)) & (BYTE_LANES << 7);
                    end = if differ == 0 {
                        run_end(pixels, position + 8, group.to_le_bytes()[7])
                    } else {
                        position + differ.trailing_zeros() as usize / 8 + 1
                    };
                    position += literals;
                }
            }
        }
        let value = unsafe { *pointer.add(position) };
        if end <= position {
            end = run_end(pixels, position, value);
        }
        let mut remaining = end - position;
        position = end;
        let row = usize::from(value) * RUN_CODE_STRIDE;
        // Pixels covered by the previous code when it belongs to this run.
        let mut previous_length = 0usize;
        while remaining > 0 {
            if next_entry == entry_limit {
                sink.code(clear);
                next_entry = first_entry;
                fresh = true;
                longest_run = [1u8; 256];
                previous_length = 0;
            }
            let mut longest = usize::from(longest_run[usize::from(value)]);
            if !fresh && previous_length == longest && longest < RUN_LENGTH_LIMIT {
                // This code makes the decoder add `value` repeated
                // `longest + 1` times as entry `next_entry`.
                longest += 1;
                run_codes[row + longest] = next_entry as u16;
                longest_run[usize::from(value)] = longest as u8;
            }
            let run = remaining.min(longest);
            sink.code(if run == 1 {
                u32::from(value)
            } else {
                u32::from(run_codes[row + run])
            });
            if fresh {
                fresh = false;
            } else {
                next_entry += 1;
            }
            remaining -= run;
            previous_length = run;
        }
    }
    sink.code(clear + 1);
}

/// Compile the compact writer during explicit Wasm initialization without
/// touching image-derived state.
pub(crate) fn prepare_compact_code() -> usize {
    let mut run_codes = [0u16; 0];
    let empty = std::hint::black_box(&[][..]);
    let mut plans = Vec::new();
    plan_frames(&mut plans, empty, 1, 1, 0, None, false);
    let count = count_run_lzw_codes(empty, 8, &mut run_codes);
    let encoded = encode_indexed_gif_compact(
        Vec::new(),
        empty,
        1,
        1,
        0,
        &[],
        DelaySource::Constant(0),
        0,
        None,
        false,
    );
    count ^ usize::from(encoded.is_ok()) ^ plans.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_image(encoded: &[u8], pixel_count: usize) -> Vec<u8> {
        let minimum_code_size = encoded[0];
        let mut data = Vec::new();
        collect_image_data_into(encoded, 0, &mut data).unwrap();
        let mut output = vec![0u8; pixel_count];
        lzw_decode_to_indices_direct(minimum_code_size, &data, &mut output).unwrap();
        output
    }

    fn literal_length(pixels: usize, minimum_code_size: u8) -> usize {
        literal_lzw_block_size(pixels, minimum_code_size).unwrap()
    }

    fn table() -> Vec<u16> {
        vec![0; RUN_CODE_TABLE_LEN]
    }

    fn round_trip(pixels: &[u8], minimum_code_size: u8) -> usize {
        let mut encoded = Vec::new();
        write_run_lzw_image(&mut encoded, pixels, minimum_code_size, &mut table());
        assert_eq!(
            decode_image(&encoded, pixels.len()),
            pixels,
            "code size {minimum_code_size}"
        );
        let expected_codes = count_run_lzw_codes(pixels, minimum_code_size, &mut table());
        let raw = (expected_codes * (usize::from(minimum_code_size) + 1)).div_ceil(8);
        assert_eq!(encoded.len(), 1 + raw + raw.div_ceil(255) + 1);
        assert!(encoded.len() <= literal_length(pixels.len(), minimum_code_size));
        encoded.len()
    }

    fn xorshift(state: &mut u32) -> u32 {
        *state ^= *state << 13;
        *state ^= *state >> 17;
        *state ^= *state << 5;
        *state
    }

    #[test]
    fn run_codes_round_trip_every_code_size_and_run_shape() {
        let mut state = 0x9e37_79b9u32;
        for minimum_code_size in 2..=8u8 {
            let colors = 1u32 << minimum_code_size;
            for case in 0..40 {
                let length = 1 + (xorshift(&mut state) % 5000) as usize;
                let mean_run = 1 + case % 9 * 7;
                let mut pixels = Vec::with_capacity(length);
                while pixels.len() < length {
                    let value = (xorshift(&mut state) % colors) as u8;
                    let run = 1 + (xorshift(&mut state) as usize % (mean_run * 2));
                    pixels.extend(std::iter::repeat_n(value, run.min(length - pixels.len())));
                }
                round_trip(&pixels, minimum_code_size);
            }
            round_trip(&[0], minimum_code_size);
            round_trip(&vec![1; 70_000], minimum_code_size);
            let alternating: Vec<u8> = (0..4097).map(|index| (index & 1) as u8).collect();
            round_trip(&alternating, minimum_code_size);
        }
    }

    #[test]
    fn run_codes_shrink_flat_images_and_never_grow_noise() {
        let flat = vec![3u8; 128 * 128];
        assert!(round_trip(&flat, 8) * 50 < literal_length(flat.len(), 8));
        let mut state = 7u32;
        let noise: Vec<u8> = (0..128 * 128).map(|_| xorshift(&mut state) as u8).collect();
        assert!(round_trip(&noise, 8) <= literal_length(noise.len(), 8));
    }

    #[test]
    fn wider_codes_are_chosen_only_when_they_shrink_the_stream() {
        let mut flat = vec![0u8; 64 * 64];
        flat[100] = 1;
        let chosen = choose_minimum_code_size(&flat, 2, &mut table());
        assert!(chosen > 2);
        let bits =
            |size: u8| count_run_lzw_codes(&flat, size, &mut table()) * (usize::from(size) + 1);
        assert!(bits(chosen) < bits(2));
        let mut state = 11u32;
        let noise: Vec<u8> = (0..4096)
            .map(|_| (xorshift(&mut state) & 3) as u8)
            .collect();
        assert_eq!(choose_minimum_code_size(&noise, 2, &mut table()), 2);
    }

    #[cfg(not(feature = "encode-only"))]
    fn composite(encoded: &[u8]) -> Vec<u32> {
        let metadata = parse_metadata(encoded).unwrap();
        let requested = vec![1u8; metadata.frames.len()];
        prepare_composited_frames_inner(encoded, &metadata, &requested, PixelFormat::Rgba).unwrap()
    }

    #[cfg(not(feature = "encode-only"))]
    fn expected_frames(indices: &[u8], palette: &[u32], transparent: Option<u8>) -> Vec<u32> {
        indices
            .iter()
            .map(|&index| {
                if Some(index) == transparent {
                    0
                } else {
                    let rgb = palette[usize::from(index)];
                    0xff00_0000 | ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff)
                }
            })
            .collect()
    }

    #[cfg(not(feature = "encode-only"))]
    fn check_animation(
        indices: &[u8],
        width: u16,
        height: u16,
        frame_count: usize,
        palette: &[u32],
        transparent: Option<u8>,
    ) -> (usize, usize) {
        let expected = expected_frames(indices, palette, transparent);
        let mut lengths = [0usize; 2];
        for (slot, independent) in [false, true].into_iter().enumerate() {
            let encoded = encode_indexed_gif_compact(
                Vec::new(),
                indices,
                width,
                height,
                frame_count,
                palette,
                DelaySource::Constant(4),
                0,
                transparent,
                independent,
            )
            .unwrap();
            assert_eq!(composite(&encoded), expected, "independent={independent}");
            let literal = encode_indexed_literal_gif_inner(
                indices,
                width,
                height,
                frame_count,
                palette,
                DelaySource::Constant(4),
                0,
                transparent,
            )
            .unwrap();
            assert_eq!(composite(&literal), expected);
            if independent {
                let metadata = parse_metadata(&encoded).unwrap();
                for frame in &metadata.frames {
                    assert_eq!((frame.x, frame.y), (0, 0));
                    assert_eq!((frame.width, frame.height), (width, height));
                }
            }
            lengths[slot] = encoded.len();
        }
        (lengths[0], lengths[1])
    }

    #[cfg(not(feature = "encode-only"))]
    #[test]
    fn differenced_frames_composite_exactly() {
        let palette: Vec<u32> = (0..7u32).map(|index| index * 0x0024_1a0f).collect();
        let (width, height, frame_count) = (37u16, 23u16, 9usize);
        let frame_len = usize::from(width) * usize::from(height);
        let mut state = 99u32;
        for transparent in [None, Some(6u8)] {
            for trial in 0..200 {
                let mut indices = vec![0u8; frame_len * frame_count];
                let background = (xorshift(&mut state) % 6) as u8;
                indices[..frame_len].fill(background);
                for frame in 0..frame_count {
                    if frame > 0 {
                        indices.copy_within(
                            (frame - 1) * frame_len..frame * frame_len,
                            frame * frame_len,
                        );
                    }
                    // Move a few rectangles, sometimes revealing transparency.
                    for _ in 0..(xorshift(&mut state) % 4) {
                        let x = xorshift(&mut state) as usize % usize::from(width);
                        let y = xorshift(&mut state) as usize % usize::from(height);
                        let w = 1 + xorshift(&mut state) as usize % 12;
                        let h = 1 + xorshift(&mut state) as usize % 8;
                        let mut value = (xorshift(&mut state) % 7) as u8;
                        if transparent.is_none() && value == 6 {
                            value = 5;
                        }
                        for row in y..(y + h).min(usize::from(height)) {
                            for column in x..(x + w).min(usize::from(width)) {
                                indices[frame * frame_len + row * usize::from(width) + column] =
                                    value;
                            }
                        }
                    }
                    if trial % 5 == 0 && frame == 4 {
                        let start = frame * frame_len;
                        indices[start..start + frame_len].fill(transparent.unwrap_or(2));
                    }
                }
                check_animation(&indices, width, height, frame_count, &palette, transparent);
            }
        }
    }

    #[cfg(not(feature = "encode-only"))]
    #[test]
    fn sprites_moving_over_transparency_clear_only_their_area() {
        let palette: Vec<u32> = (0..4u32).map(|index| index * 0x0050_3010).collect();
        let transparent = Some(3u8);
        let (width, height, frame_count) = (96u16, 64u16, 10usize);
        let frame_len = usize::from(width) * usize::from(height);
        let mut indices = vec![3u8; frame_len * frame_count];
        for frame in 0..frame_count {
            for row in 20..36 {
                for column in frame * 6..frame * 6 + 16 {
                    indices[frame * frame_len + row * usize::from(width) + column] =
                        ((row + column) % 3) as u8;
                }
            }
        }
        let (differenced, independent) =
            check_animation(&indices, width, height, frame_count, &palette, transparent);
        assert!(
            differenced * 2 < independent,
            "{differenced} vs {independent}"
        );
        let encoded = encode_indexed_gif_compact(
            Vec::new(),
            &indices,
            width,
            height,
            frame_count,
            &palette,
            DelaySource::Constant(4),
            0,
            transparent,
            false,
        )
        .unwrap();
        // Later frames stay near the sprite instead of covering the canvas.
        let metadata = parse_metadata(&encoded).unwrap();
        for frame in &metadata.frames[1..] {
            assert!(frame.width < width / 2, "{} px wide", frame.width);
        }
    }

    #[cfg(not(feature = "encode-only"))]
    #[test]
    fn differencing_shrinks_static_backgrounds() {
        let palette: Vec<u32> = (0..4u32).map(|index| index * 0x0040_3020).collect();
        let (width, height, frame_count) = (128u16, 128u16, 12usize);
        let frame_len = usize::from(width) * usize::from(height);
        let mut indices = vec![0u8; frame_len * frame_count];
        for frame in 0..frame_count {
            for row in 0..usize::from(height) {
                for column in 0..usize::from(width) {
                    let moving =
                        column >= frame * 8 && column < frame * 8 + 12 && (56..68).contains(&row);
                    let grid = column % 16 == 0 || row % 16 == 0;
                    indices[frame * frame_len + row * usize::from(width) + column] = if moving {
                        2
                    } else if grid {
                        1
                    } else {
                        0
                    };
                }
            }
        }
        let (differenced, independent) =
            check_animation(&indices, width, height, frame_count, &palette, None);
        assert!(
            differenced * 3 < independent,
            "{differenced} vs {independent}"
        );
    }
}
