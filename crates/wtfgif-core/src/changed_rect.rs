//! Bounding rectangles of the pixels that change between frames, shared by
//! the encoders and the delta decoder.

#[derive(Clone, Copy)]
pub(crate) struct ChangedRectU32 {
    pub(crate) x: usize,
    pub(crate) y: usize,
    pub(crate) width: usize,
    pub(crate) height: usize,
}

#[inline(always)]
pub(crate) fn find_changed_rect_u8(
    previous: &[u8],
    current: &[u8],
    width: usize,
    height: usize,
) -> Option<ChangedRectU32> {
    let mut top = height;
    let mut bottom = 0usize;
    let mut left = width;
    let mut right = 0usize;
    let previous_pointer = previous.as_ptr();
    let current_pointer = current.as_ptr();
    for y in 0..height {
        let row = y * width;
        let previous_row = &previous[row..row + width];
        let current_row = &current[row..row + width];
        if previous_row == current_row {
            continue;
        }
        let mut row_left = width;
        let mut row_right = 0usize;
        let mut x = 0usize;
        while x + 8 <= width {
            let previous_word =
                unsafe { std::ptr::read_unaligned(previous_pointer.add(row + x).cast::<u64>()) };
            let current_word =
                unsafe { std::ptr::read_unaligned(current_pointer.add(row + x).cast::<u64>()) };
            if previous_word != current_word {
                for offset in 0..8 {
                    if previous[row + x + offset] != current[row + x + offset] {
                        let changed = x + offset;
                        row_left = row_left.min(changed);
                        row_right = row_right.max(changed);
                    }
                }
            }
            x += 8;
        }
        while x < width {
            if previous[row + x] != current[row + x] {
                row_left = row_left.min(x);
                row_right = row_right.max(x);
            }
            x += 1;
        }
        if row_left < width {
            if top == height {
                top = y;
            }
            bottom = y;
            left = left.min(row_left);
            right = right.max(row_right);
        }
    }

    if top == height {
        return None;
    }

    Some(ChangedRectU32 {
        x: left,
        y: top,
        width: right - left + 1,
        height: bottom - top + 1,
    })
}

#[inline(always)]
pub(crate) fn find_changed_rect_rgba_bytes(
    previous: &[u8],
    current: &[u8],
    width: usize,
    height: usize,
) -> Option<ChangedRectU32> {
    let row_bytes = width * 4;
    let mut top = height;
    let mut bottom = 0usize;
    let mut left = width;
    let mut right = 0usize;
    for y in 0..height {
        let row = y * row_bytes;
        let previous_row = &previous[row..row + row_bytes];
        let current_row = &current[row..row + row_bytes];
        if previous_row == current_row {
            continue;
        }

        let mut row_left = width;
        let mut row_right = 0usize;
        let previous_pointer = previous_row.as_ptr();
        let current_pointer = current_row.as_ptr();
        let pair_end = width & !1;
        let mut x = 0usize;
        while x < pair_end {
            let offset = x * 4;
            let difference = unsafe {
                u64::from_le(std::ptr::read_unaligned(
                    previous_pointer.add(offset) as *const u64
                )) ^ u64::from_le(std::ptr::read_unaligned(
                    current_pointer.add(offset) as *const u64
                ))
            };
            if difference != 0 {
                if difference & 0xffff_ffff != 0 {
                    row_left = row_left.min(x);
                    row_right = row_right.max(x);
                }
                if difference >> 32 != 0 {
                    row_left = row_left.min(x + 1);
                    row_right = row_right.max(x + 1);
                }
            }
            x += 2;
        }
        if x < width {
            let offset = x * 4;
            let difference = unsafe {
                u32::from_le(std::ptr::read_unaligned(
                    previous_pointer.add(offset) as *const u32
                )) ^ u32::from_le(std::ptr::read_unaligned(
                    current_pointer.add(offset) as *const u32
                ))
            };
            if difference != 0 {
                row_left = row_left.min(x);
                row_right = row_right.max(x);
            }
        }
        if row_left < width {
            if top == height {
                top = y;
            }
            bottom = y;
            left = left.min(row_left);
            right = right.max(row_right);
        }
    }

    if top == height {
        return None;
    }

    Some(ChangedRectU32 {
        x: left,
        y: top,
        width: right - left + 1,
        height: bottom - top + 1,
    })
}
