//! Median-cut palette construction over quantized histogram colors.

use super::*;

#[derive(Clone, Copy, Default)]
pub(crate) struct QuantizedColor {
    // A Wasm input lives in a 32-bit address space, so its total pixel count
    // (and therefore every histogram-bin count) fits in u32. Keep the native
    // build wide for callers that can supply slices larger than Wasm memory.
    pub(crate) count: QuantizedColorCount,
    // The quality histograms are at most 5 bits per channel (32,768 bins),
    // so a u16 is sufficient. Keeping the hot fields compact reduces the
    // color-arena footprint on wasm32 versus usize + u64 + padding.
    pub(crate) histogram_index: u16,
    pub(crate) red: u8,
    pub(crate) green: u8,
    pub(crate) blue: u8,
}

pub(crate) type QuantizedColorStats = (QuantizedColorCount, u8, u8, u8);

pub(crate) type QuantizedColorSplit = (usize, QuantizedColorStats, QuantizedColorStats);

#[cfg(all(test, not(feature = "encode-only")))]
pub(crate) struct QuantizedColorBox {
    pub(crate) colors: Vec<QuantizedColor>,
    pub(crate) weight: u64,
    pub(crate) score: u64,
    pub(crate) red_range: u8,
    pub(crate) green_range: u8,
    pub(crate) blue_range: u8,
}

#[cfg(all(test, not(feature = "encode-only")))]
impl QuantizedColorBox {
    pub(crate) fn new(colors: Vec<QuantizedColor>) -> Self {
        let (weight, red_range, green_range, blue_range) = Self::stats(&colors);
        Self::from_stats(colors, weight, red_range, green_range, blue_range)
    }

    pub(crate) fn from_stats(
        colors: Vec<QuantizedColor>,
        weight: u64,
        red_range: u8,
        green_range: u8,
        blue_range: u8,
    ) -> Self {
        let range = u64::from(red_range.max(green_range).max(blue_range));
        Self {
            colors,
            weight,
            score: weight * range * range,
            red_range,
            green_range,
            blue_range,
        }
    }

    pub(crate) fn stats(colors: &[QuantizedColor]) -> (u64, u8, u8, u8) {
        quantized_color_stats(colors)
    }

    pub(crate) fn split(mut self) -> Result<(Self, Self), Self> {
        if self.colors.len() < 2 {
            return Err(self);
        }
        let axis = if self.red_range >= self.green_range && self.red_range >= self.blue_range {
            0
        } else if self.green_range >= self.blue_range {
            1
        } else {
            2
        };
        let midpoint = self.weight.div_ceil(2);
        let split_index = weighted_axis_split_index(&mut self.colors, axis, midpoint);
        let (left_stats, right_stats) = quantized_color_split_stats(&self.colors, split_index);
        let right = self.colors.split_off(split_index);
        let left = Self::from_stats(
            self.colors,
            left_stats.0,
            left_stats.1,
            left_stats.2,
            left_stats.3,
        );
        let right = Self::from_stats(
            right,
            right_stats.0,
            right_stats.1,
            right_stats.2,
            right_stats.3,
        );
        Ok((left, right))
    }
}

#[inline(always)]
pub(crate) fn quantized_color_stats(colors: &[QuantizedColor]) -> QuantizedColorStats {
    let mut weight = QuantizedColorCount::default();
    let mut min_red = u8::MAX;
    let mut min_green = u8::MAX;
    let mut min_blue = u8::MAX;
    let mut max_red = 0;
    let mut max_green = 0;
    let mut max_blue = 0;
    for color in colors {
        weight += color.count;
        min_red = min_red.min(color.red);
        min_green = min_green.min(color.green);
        min_blue = min_blue.min(color.blue);
        max_red = max_red.max(color.red);
        max_green = max_green.max(color.green);
        max_blue = max_blue.max(color.blue);
    }
    (
        weight,
        max_red - min_red,
        max_green - min_green,
        max_blue - min_blue,
    )
}

#[inline(always)]
#[cfg(all(test, not(feature = "encode-only")))]
pub(crate) fn quantized_color_split_stats(
    colors: &[QuantizedColor],
    split: usize,
) -> (QuantizedColorStats, QuantizedColorStats) {
    debug_assert!(split > 0 && split < colors.len());
    let mut left_weight = QuantizedColorCount::default();
    let mut left_min_red = u8::MAX;
    let mut left_min_green = u8::MAX;
    let mut left_min_blue = u8::MAX;
    let mut left_max_red = 0;
    let mut left_max_green = 0;
    let mut left_max_blue = 0;
    let mut right_weight = QuantizedColorCount::default();
    let mut right_min_red = u8::MAX;
    let mut right_min_green = u8::MAX;
    let mut right_min_blue = u8::MAX;
    let mut right_max_red = 0;
    let mut right_max_green = 0;
    let mut right_max_blue = 0;
    for (index, color) in colors.iter().enumerate() {
        if index < split {
            left_weight += color.count;
            left_min_red = left_min_red.min(color.red);
            left_min_green = left_min_green.min(color.green);
            left_min_blue = left_min_blue.min(color.blue);
            left_max_red = left_max_red.max(color.red);
            left_max_green = left_max_green.max(color.green);
            left_max_blue = left_max_blue.max(color.blue);
        } else {
            right_weight += color.count;
            right_min_red = right_min_red.min(color.red);
            right_min_green = right_min_green.min(color.green);
            right_min_blue = right_min_blue.min(color.blue);
            right_max_red = right_max_red.max(color.red);
            right_max_green = right_max_green.max(color.green);
            right_max_blue = right_max_blue.max(color.blue);
        }
    }
    (
        (
            left_weight,
            left_max_red - left_min_red,
            left_max_green - left_min_green,
            left_max_blue - left_min_blue,
        ),
        (
            right_weight,
            right_max_red - right_min_red,
            right_max_green - right_min_green,
            right_max_blue - right_min_blue,
        ),
    )
}

pub(crate) struct QuantizedColorArenaBox {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) weight: QuantizedColorCount,
    pub(crate) score: u64,
    pub(crate) representative: u32,
    pub(crate) red_range: u8,
    pub(crate) green_range: u8,
    pub(crate) blue_range: u8,
}

impl QuantizedColorArenaBox {
    pub(crate) fn new(start: usize, end: usize, colors: &[QuantizedColor]) -> Self {
        let (weight, red_range, green_range, blue_range) =
            quantized_color_stats(&colors[start..end]);
        Self::from_stats(start, end, weight, red_range, green_range, blue_range)
    }

    pub(crate) fn from_stats(
        start: usize,
        end: usize,
        weight: QuantizedColorCount,
        red_range: u8,
        green_range: u8,
        blue_range: u8,
    ) -> Self {
        let range = u64::from(red_range.max(green_range).max(blue_range));
        Self {
            start,
            end,
            weight,
            score: quantized_color_count_u64(weight) * range * range,
            representative: 0,
            red_range,
            green_range,
            blue_range,
        }
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.end - self.start
    }

    pub(crate) fn split(&mut self, colors: &mut [QuantizedColor]) -> Option<Self> {
        if self.len() < 2 {
            return None;
        }
        let axis = if self.red_range >= self.green_range && self.red_range >= self.blue_range {
            0
        } else if self.green_range >= self.blue_range {
            1
        } else {
            2
        };
        let midpoint = self.weight.div_ceil(2);
        let (relative_split, left_stats, right_stats) =
            weighted_axis_split_index_with_stats(&mut colors[self.start..self.end], axis, midpoint);
        let split = self.start + relative_split;
        let right = Self::from_stats(
            split,
            self.end,
            right_stats.0,
            right_stats.1,
            right_stats.2,
            right_stats.3,
        );
        self.end = split;
        self.weight = left_stats.0;
        self.red_range = left_stats.1;
        self.green_range = left_stats.2;
        self.blue_range = left_stats.3;
        let range = u64::from(left_stats.1.max(left_stats.2).max(left_stats.3));
        self.score = quantized_color_count_u64(left_stats.0) * range * range;
        Some(right)
    }

    pub(crate) fn calculate_representative(&mut self, colors: &[QuantizedColor]) {
        // A GIF input that fits in Wasm memory has fewer than 2^32 pixels, so
        // each channel's weighted sum (255 * pixel_count) fits comfortably in
        // u64. Keeping this hot representative pass in native-width integer
        // arithmetic avoids the software u128 multiply/divide sequence.
        let mut red = 0u64;
        let mut green = 0u64;
        let mut blue = 0u64;
        let mut count = 0u64;
        for color in &colors[self.start..self.end] {
            let weight = quantized_color_count_u64(color.count);
            red += u64::from(color.red) * weight;
            green += u64::from(color.green) * weight;
            blue += u64::from(color.blue) * weight;
            count += weight;
        }
        if count == 0 {
            self.representative = 0;
            return;
        }
        let red = ((red + count / 2) / count) as u32;
        let green = ((green + count / 2) / count) as u32;
        let blue = ((blue + count / 2) / count) as u32;
        self.representative = (red << 16) | (green << 8) | blue;
    }
}

#[inline(always)]
#[cfg(all(test, not(feature = "encode-only")))]
pub(crate) fn quantized_color_axis(color: &QuantizedColor, axis: u8) -> u8 {
    match axis {
        0 => color.red,
        1 => color.green,
        _ => color.blue,
    }
}

#[inline(always)]
pub(crate) fn quantized_color_axis_const<const AXIS: usize>(color: &QuantizedColor) -> u8 {
    match AXIS {
        0 => color.red,
        1 => color.green,
        _ => color.blue,
    }
}

/// Partition a color box around its weighted median on one color axis.
///
/// Median-cut only needs every color on the left of the split to be no larger
/// on the chosen axis than every color on the right. The channel values are
/// bytes, so a 256-bin weight pass plus a three-way in-place partition avoids
/// repeated comparison-based selection work on dense histograms.
#[inline(always)]
#[cfg(all(test, not(feature = "encode-only")))]
pub(crate) fn weighted_axis_split_index(
    colors: &mut [QuantizedColor],
    axis: u8,
    midpoint: u64,
) -> usize {
    weighted_axis_split_index_with_stats(colors, axis, midpoint).0
}

#[inline(always)]
pub(crate) fn weighted_axis_split_index_with_stats(
    colors: &mut [QuantizedColor],
    axis: u8,
    midpoint: QuantizedColorCount,
) -> QuantizedColorSplit {
    match axis {
        0 => weighted_axis_split_index_with_stats_const::<0>(colors, midpoint),
        1 => weighted_axis_split_index_with_stats_const::<1>(colors, midpoint),
        _ => weighted_axis_split_index_with_stats_const::<2>(colors, midpoint),
    }
}

#[inline(always)]
pub(crate) fn weighted_axis_split_index_with_stats_const<const AXIS: usize>(
    colors: &mut [QuantizedColor],
    midpoint: QuantizedColorCount,
) -> QuantizedColorSplit {
    let total_len = colors.len();
    if total_len <= 1 {
        let stats = quantized_color_stats(colors);
        return (1.min(total_len), stats, (0, u8::MAX, u8::MAX, u8::MAX));
    }
    // Median-cut splits repeatedly rebuild this byte-axis histogram. Keep the
    // sparse weight slots uninitialized and track which byte values were seen;
    // zeroing 256 wide weight slots for every split is otherwise pure overhead.
    let mut weights = [std::mem::MaybeUninit::<QuantizedColorCount>::uninit(); 256];
    let mut seen = [0u64; 4];
    for color in colors.iter() {
        let value = usize::from(quantized_color_axis_const::<AXIS>(color));
        let word = value >> 6;
        let bit = 1u64 << (value & 63);
        if seen[word] & bit == 0 {
            seen[word] |= bit;
            weights[value].write(color.count);
        } else {
            unsafe {
                *weights[value].assume_init_mut() += color.count;
            }
        }
    }
    let target = midpoint.max(1);
    let mut below_weight = QuantizedColorCount::default();
    let mut split_axis = 0usize;
    for (value, weight_slot) in weights.iter().enumerate() {
        let word = value >> 6;
        let bit = 1u64 << (value & 63);
        if seen[word] & bit == 0 {
            continue;
        }
        let weight = unsafe { weight_slot.assume_init() };
        if target <= below_weight + weight {
            split_axis = value;
            break;
        }
        below_weight += weight;
    }

    // Three-way partition: [0, less_end) is below the split byte,
    // [less_end, equal_end) is equal, and [equal_end, len) is above it.
    // Unlike a comparison-based selection this touches each color once after
    // the weight pass and makes the equal-bin walk deterministic.
    let mut less_end = 0usize;
    let mut scan = 0usize;
    let mut greater_start = total_len;
    let mut less_weight = QuantizedColorCount::default();
    let mut less_min_red = u8::MAX;
    let mut less_min_green = u8::MAX;
    let mut less_min_blue = u8::MAX;
    let mut less_max_red = 0u8;
    let mut less_max_green = 0u8;
    let mut less_max_blue = 0u8;
    let mut greater_weight = QuantizedColorCount::default();
    let mut greater_min_red = u8::MAX;
    let mut greater_min_green = u8::MAX;
    let mut greater_min_blue = u8::MAX;
    let mut greater_max_red = 0u8;
    let mut greater_max_green = 0u8;
    let mut greater_max_blue = 0u8;
    while scan < greater_start {
        let color = colors[scan];
        let value = usize::from(quantized_color_axis_const::<AXIS>(&color));
        match value.cmp(&split_axis) {
            std::cmp::Ordering::Less => {
                colors.swap(scan, less_end);
                less_end += 1;
                scan += 1;
                less_weight += color.count;
                less_min_red = less_min_red.min(color.red);
                less_min_green = less_min_green.min(color.green);
                less_min_blue = less_min_blue.min(color.blue);
                less_max_red = less_max_red.max(color.red);
                less_max_green = less_max_green.max(color.green);
                less_max_blue = less_max_blue.max(color.blue);
            }
            std::cmp::Ordering::Greater => {
                greater_start -= 1;
                colors.swap(scan, greater_start);
                greater_weight += color.count;
                greater_min_red = greater_min_red.min(color.red);
                greater_min_green = greater_min_green.min(color.green);
                greater_min_blue = greater_min_blue.min(color.blue);
                greater_max_red = greater_max_red.max(color.red);
                greater_max_green = greater_max_green.max(color.green);
                greater_max_blue = greater_max_blue.max(color.blue);
            }
            std::cmp::Ordering::Equal => {
                scan += 1;
            }
        }
    }
    let target_in_equal = target - below_weight;
    let mut split = less_end;
    let mut equal_weight = QuantizedColorCount::default();
    let mut left_equal_weight = QuantizedColorCount::default();
    let mut left_equal_min_red = u8::MAX;
    let mut left_equal_min_green = u8::MAX;
    let mut left_equal_min_blue = u8::MAX;
    let mut left_equal_max_red = 0u8;
    let mut left_equal_max_green = 0u8;
    let mut left_equal_max_blue = 0u8;
    let mut right_equal_weight = QuantizedColorCount::default();
    let mut right_equal_min_red = u8::MAX;
    let mut right_equal_min_green = u8::MAX;
    let mut right_equal_min_blue = u8::MAX;
    let mut right_equal_max_red = 0u8;
    let mut right_equal_max_green = 0u8;
    let mut right_equal_max_blue = 0u8;
    // Accumulate the equal-axis statistics while finding the weighted split.
    // The previous two-pass form walked this potentially large middle region
    // once to find the boundary and again to rebuild both child bounds.
    let split_limit = greater_start.min(total_len - 1);
    while split < split_limit && equal_weight < target_in_equal {
        let color = colors[split];
        equal_weight += color.count;
        left_equal_weight += color.count;
        left_equal_min_red = left_equal_min_red.min(color.red);
        left_equal_min_green = left_equal_min_green.min(color.green);
        left_equal_min_blue = left_equal_min_blue.min(color.blue);
        left_equal_max_red = left_equal_max_red.max(color.red);
        left_equal_max_green = left_equal_max_green.max(color.green);
        left_equal_max_blue = left_equal_max_blue.max(color.blue);
        split += 1;
    }
    for color in &colors[split..greater_start] {
        right_equal_weight += color.count;
        right_equal_min_red = right_equal_min_red.min(color.red);
        right_equal_min_green = right_equal_min_green.min(color.green);
        right_equal_min_blue = right_equal_min_blue.min(color.blue);
        right_equal_max_red = right_equal_max_red.max(color.red);
        right_equal_max_green = right_equal_max_green.max(color.green);
        right_equal_max_blue = right_equal_max_blue.max(color.blue);
    }
    let left_min_red = less_min_red.min(left_equal_min_red);
    let left_min_green = less_min_green.min(left_equal_min_green);
    let left_min_blue = less_min_blue.min(left_equal_min_blue);
    let left_max_red = less_max_red.max(left_equal_max_red);
    let left_max_green = less_max_green.max(left_equal_max_green);
    let left_max_blue = less_max_blue.max(left_equal_max_blue);
    let right_min_red = right_equal_min_red.min(greater_min_red);
    let right_min_green = right_equal_min_green.min(greater_min_green);
    let right_min_blue = right_equal_min_blue.min(greater_min_blue);
    let right_max_red = right_equal_max_red.max(greater_max_red);
    let right_max_green = right_equal_max_green.max(greater_max_green);
    let right_max_blue = right_equal_max_blue.max(greater_max_blue);
    (
        split,
        (
            less_weight + left_equal_weight,
            left_max_red - left_min_red,
            left_max_green - left_min_green,
            left_max_blue - left_min_blue,
        ),
        (
            right_equal_weight + greater_weight,
            right_max_red - right_min_red,
            right_max_green - right_min_green,
            right_max_blue - right_min_blue,
        ),
    )
}

#[inline(always)]
pub(crate) fn quality_histogram_index_bits_const<const BITS: usize>(
    red: u8,
    green: u8,
    blue: u8,
) -> usize {
    (usize::from(red >> (8 - BITS)) << (BITS * 2))
        | (usize::from(green >> (8 - BITS)) << BITS)
        | usize::from(blue >> (8 - BITS))
}

pub(crate) fn quality_colors_from_histogram_u32<const SAFE_SUMS: bool>(
    histogram: &mut [RgbHistogramBin32],
) -> Vec<QuantizedColor> {
    let mut colors = take_quality_colors();
    if colors.capacity() < histogram.len() {
        colors.reserve(histogram.len() - colors.capacity());
    }
    for (histogram_index, bin_slot) in histogram.iter_mut().enumerate() {
        let bin = *bin_slot;
        if bin.count == 0 {
            continue;
        }
        let reciprocal = if SAFE_SUMS {
            (1u64 << 32) / u64::from(bin.count)
        } else {
            0
        };
        let red = if SAFE_SUMS {
            rounded_histogram_average_u32_reciprocal(bin.red, bin.count, reciprocal)
        } else {
            rounded_histogram_average_u32(bin.red, bin.count)
        };
        let green = if SAFE_SUMS {
            rounded_histogram_average_u32_reciprocal(bin.green, bin.count, reciprocal)
        } else {
            rounded_histogram_average_u32(bin.green, bin.count)
        };
        let blue = if SAFE_SUMS {
            rounded_histogram_average_u32_reciprocal(bin.blue, bin.count, reciprocal)
        } else {
            rounded_histogram_average_u32(bin.blue, bin.count)
        };
        colors.push(QuantizedColor {
            count: QuantizedColorCount::from(bin.count),
            histogram_index: histogram_index as u16,
            red,
            green,
            blue,
        });
        *bin_slot = RgbHistogramBin32::default();
    }
    REUSABLE_QUALITY_HISTOGRAM_U32_CLEAN.with(|clean| {
        clean.replace(true);
    });
    colors
}

#[inline(always)]
pub(crate) fn rounded_histogram_average_u32_reciprocal(
    sum: u32,
    count: u32,
    reciprocal: u64,
) -> u8 {
    let divisor = u64::from(count);
    let numerator = u64::from(sum + count / 2);
    let estimate = (numerator * reciprocal) >> 32;
    (estimate + u64::from(numerator - estimate * divisor >= divisor)) as u8
}

#[inline(always)]
pub(crate) fn rounded_histogram_average_u32(sum: u32, count: u32) -> u8 {
    let half = count / 2;
    match sum.checked_add(half) {
        Some(adjusted) => (adjusted / count) as u8,
        None => ((u64::from(sum) + u64::from(half)) / u64::from(count)) as u8,
    }
}

#[inline(always)]
pub(crate) fn rounded_weighted_average_u32_reciprocal(sum: u32, count: u32, reciprocal: u64) -> u8 {
    let divisor = u64::from(count);
    let numerator = u64::from(sum) + divisor / 2;
    let mut estimate = (numerator * reciprocal) >> 31;
    let mut remainder = numerator - estimate * divisor;
    if remainder >= divisor {
        estimate += 1;
        remainder -= divisor;
    }
    if remainder >= divisor {
        estimate += 1;
    }
    estimate as u8
}

pub(crate) fn quality_colors_from_histogram_u64(
    histogram: &mut [RgbHistogramBin],
) -> Vec<QuantizedColor> {
    let mut colors = take_quality_colors();
    if colors.capacity() < histogram.len() {
        colors.reserve(histogram.len() - colors.capacity());
    }
    for (histogram_index, bin_slot) in histogram.iter_mut().enumerate() {
        let bin = *bin_slot;
        if bin.count == 0 {
            continue;
        }
        colors.push(QuantizedColor {
            count: QuantizedColorCount::try_from(bin.count)
                .expect("quality histogram count exceeds the Wasm addressable pixel limit"),
            histogram_index: histogram_index as u16,
            red: ((bin.red + bin.count / 2) / bin.count) as u8,
            green: ((bin.green + bin.count / 2) / bin.count) as u8,
            blue: ((bin.blue + bin.count / 2) / bin.count) as u8,
        });
        *bin_slot = RgbHistogramBin::default();
    }
    REUSABLE_QUALITY_HISTOGRAM_U64_CLEAN.with(|clean| {
        clean.replace(true);
    });
    colors
}

pub(crate) fn take_quality_colors() -> Vec<QuantizedColor> {
    REUSABLE_QUALITY_COLORS.with(|scratch| std::mem::take(&mut *scratch.borrow_mut()))
}

pub(crate) fn take_quality_palette(capacity: usize) -> Vec<u32> {
    REUSABLE_QUALITY_PALETTE.with(|scratch| {
        let mut palette = std::mem::take(&mut *scratch.borrow_mut());
        palette.clear();
        if palette.capacity() < capacity {
            palette.reserve(capacity - palette.capacity());
        }
        palette
    })
}

pub(crate) fn recycle_quality_palette(mut palette: Vec<u32>) {
    palette.clear();
    REUSABLE_QUALITY_PALETTE.with(|scratch| {
        *scratch.borrow_mut() = palette;
    });
}

pub(crate) fn recycle_quality_colors(mut colors: Vec<QuantizedColor>) {
    colors.clear();
    REUSABLE_QUALITY_COLORS.with(|scratch| {
        *scratch.borrow_mut() = colors;
    });
}

#[inline(never)]
pub(crate) fn build_quality_median_cut_palette(
    has_transparent_pixels: bool,
    mut colors: Vec<QuantizedColor>,
    histogram_bits: usize,
    mapping_len: usize,
    opaque_color_limit: usize,
    mut palette: Vec<u32>,
) -> (Vec<u32>, Vec<u8>) {
    let mut boxes = REUSABLE_QUANTIZED_COLOR_BOXES.with(|scratch| {
        let mut boxes = std::mem::take(&mut *scratch.borrow_mut());
        boxes.clear();
        boxes.push(QuantizedColorArenaBox::new(0, colors.len(), &colors));
        boxes
    });
    while boxes.len() < opaque_color_limit {
        let Some((split_index, _)) = boxes
            .iter()
            .enumerate()
            .filter(|(_, color_box)| color_box.len() > 1)
            .max_by_key(|(_, color_box)| color_box.score)
        else {
            break;
        };
        let mut color_box = boxes.swap_remove(split_index);
        if let Some(right) = color_box.split(&mut colors) {
            boxes.push(color_box);
            boxes.push(right);
        } else {
            boxes.push(color_box);
            break;
        }
    }
    for color_box in &mut boxes {
        color_box.calculate_representative(&colors);
    }
    boxes.sort_unstable_by_key(|color_box| color_box.representative);

    palette.clear();
    let palette_capacity = boxes.len() + usize::from(has_transparent_pixels);
    if palette.capacity() < palette_capacity {
        palette.reserve(palette_capacity - palette.capacity());
    }
    for color_box in &boxes {
        palette.push(color_box.representative);
    }
    let mut histogram_to_palette = take_quality_histogram_to_palette(mapping_len);
    let mut requested_cells = [false; 1 << 12];
    let mut requested_cell_count = 0usize;
    let mut initial_hints = [0u8; 1 << 12];
    let mut canonical_palette_indices = [0u8; 256];
    for (palette_index, &representative) in palette.iter().enumerate() {
        canonical_palette_indices[palette_index] = palette[..palette_index]
            .iter()
            .position(|&candidate| candidate == representative)
            .unwrap_or(palette_index) as u8;
    }
    for (palette_index, color_box) in boxes.iter().enumerate() {
        let candidate = canonical_palette_indices[palette_index];
        let candidate_color = palette[usize::from(candidate)];
        for color in &colors[color_box.start..color_box.end] {
            let coarse_index = (usize::from(color.red >> 4) << 8)
                | (usize::from(color.green >> 4) << 4)
                | usize::from(color.blue >> 4);
            let red = (color.red & 0xf0) | 8;
            let green = (color.green & 0xf0) | 8;
            let blue = (color.blue & 0xf0) | 8;
            let candidate_distance = palette_color_distance(candidate_color, red, green, blue);
            if !requested_cells[coarse_index] {
                requested_cells[coarse_index] = true;
                requested_cell_count += 1;
                initial_hints[coarse_index] = candidate;
                continue;
            }
            let current = initial_hints[coarse_index];
            let current_distance =
                palette_color_distance(palette[usize::from(current)], red, green, blue);
            if candidate_distance < current_distance
                || (candidate_distance == current_distance && candidate < current)
            {
                initial_hints[coarse_index] = candidate;
            }
        }
    }
    let palette_tree = PaletteKdTree::new(&palette);
    let coarse_nearest = palette_tree.coarse_nearest_table_for_cells(
        &palette,
        &requested_cells,
        &initial_hints,
        requested_cell_count,
    );
    if histogram_bits == 4 {
        for color_box in &boxes {
            for color in &colors[color_box.start..color_box.end] {
                let index = (usize::from(color.red >> 4) << 8)
                    | (usize::from(color.green >> 4) << 4)
                    | usize::from(color.blue >> 4);
                histogram_to_palette[usize::from(color.histogram_index)] = coarse_nearest[index];
            }
        }
    } else {
        for color_box in &boxes {
            for color in &colors[color_box.start..color_box.end] {
                let coarse_index = (usize::from(color.red >> 4) << 8)
                    | (usize::from(color.green >> 4) << 4)
                    | usize::from(color.blue >> 4);
                let mapping_index = if histogram_bits == 5 {
                    coarse_index
                } else {
                    usize::from(color.histogram_index)
                };
                histogram_to_palette[mapping_index] = coarse_nearest[coarse_index];
            }
        }
    }
    REUSABLE_QUANTIZED_COLOR_BOXES.with(|scratch| {
        *scratch.borrow_mut() = boxes;
    });
    recycle_quality_colors(colors);
    (palette, histogram_to_palette)
}
