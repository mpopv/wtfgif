//! Wu's variance-minimizing palette construction.

use super::*;

#[derive(Clone, Copy, Default)]
pub(crate) struct WuMoment {
    pub(crate) count: u32,
    pub(crate) red: u32,
    pub(crate) green: u32,
    pub(crate) blue: u32,
    pub(crate) squared: f32,
}

impl WuMoment {
    #[inline(always)]
    pub(crate) fn subtract(self, other: Self) -> Self {
        Self {
            count: self.count - other.count,
            red: self.red - other.red,
            green: self.green - other.green,
            blue: self.blue - other.blue,
            squared: self.squared - other.squared,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct WuCube {
    pub(crate) minimum: [u8; 3],
    pub(crate) maximum: [u8; 3],
}

#[derive(Clone, Copy)]
pub(crate) struct WuCubeVariance {
    pub(crate) cube: WuCube,
    pub(crate) variance: f32,
}

pub(crate) const WU_HISTOGRAM_BITS: usize = 4;

pub(crate) const WU_HISTOGRAM_SIDE: usize = 1 << WU_HISTOGRAM_BITS;

pub(crate) const WU_HISTOGRAM_LEN: usize = 1 << (WU_HISTOGRAM_BITS * 3);

#[inline(always)]
pub(crate) fn wu_prefix_moment(
    moments: &[RgbHistogramBin32],
    squared: &[f32],
    red: i8,
    green: i8,
    blue: i8,
) -> WuMoment {
    if red < 0 || green < 0 || blue < 0 {
        WuMoment::default()
    } else {
        let index = (usize::from(red as u8) << 8)
            | (usize::from(green as u8) << 4)
            | usize::from(blue as u8);
        let moment = moments[index];
        WuMoment {
            count: moment.count,
            red: moment.red,
            green: moment.green,
            blue: moment.blue,
            squared: squared[index],
        }
    }
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn wu_moment_from_corners(
    upper: WuMoment,
    red_plane: WuMoment,
    green_plane: WuMoment,
    blue_plane: WuMoment,
    red_green: WuMoment,
    red_blue: WuMoment,
    green_blue: WuMoment,
    lower: WuMoment,
) -> WuMoment {
    macro_rules! volume_field {
        ($field:ident) => {
            ((upper.$field - red_plane.$field) - (green_plane.$field - red_green.$field))
                - ((blue_plane.$field - red_blue.$field) - (green_blue.$field - lower.$field))
        };
    }
    WuMoment {
        count: volume_field!(count),
        red: volume_field!(red),
        green: volume_field!(green),
        blue: volume_field!(blue),
        squared: upper.squared + red_green.squared + red_blue.squared + green_blue.squared
            - red_plane.squared
            - green_plane.squared
            - blue_plane.squared
            - lower.squared,
    }
}

#[inline(always)]
pub(crate) fn wu_cube_moment(
    moments: &[RgbHistogramBin32],
    squared: &[f32],
    cube: WuCube,
) -> WuMoment {
    let red_low = cube.minimum[0] as i8 - 1;
    let green_low = cube.minimum[1] as i8 - 1;
    let blue_low = cube.minimum[2] as i8 - 1;
    let red_high = cube.maximum[0] as i8 - 1;
    let green_high = cube.maximum[1] as i8 - 1;
    let blue_high = cube.maximum[2] as i8 - 1;
    wu_moment_from_corners(
        wu_prefix_moment(moments, squared, red_high, green_high, blue_high),
        wu_prefix_moment(moments, squared, red_low, green_high, blue_high),
        wu_prefix_moment(moments, squared, red_high, green_low, blue_high),
        wu_prefix_moment(moments, squared, red_high, green_high, blue_low),
        wu_prefix_moment(moments, squared, red_low, green_low, blue_high),
        wu_prefix_moment(moments, squared, red_low, green_high, blue_low),
        wu_prefix_moment(moments, squared, red_high, green_low, blue_low),
        wu_prefix_moment(moments, squared, red_low, green_low, blue_low),
    )
}

#[inline(always)]
pub(crate) fn wu_best_axis_cut<const AXIS: usize>(
    moments: &[RgbHistogramBin32],
    squared: &[f32],
    cube: WuCube,
    total: WuMoment,
) -> (f32, u8) {
    let red_low = cube.minimum[0] as i8 - 1;
    let green_low = cube.minimum[1] as i8 - 1;
    let blue_low = cube.minimum[2] as i8 - 1;
    let red_high = cube.maximum[0] as i8 - 1;
    let green_high = cube.maximum[1] as i8 - 1;
    let blue_high = cube.maximum[2] as i8 - 1;
    let fixed = if AXIS == 0 {
        [
            wu_prefix_moment(moments, squared, red_low, green_high, blue_high),
            wu_prefix_moment(moments, squared, red_low, green_low, blue_high),
            wu_prefix_moment(moments, squared, red_low, green_high, blue_low),
            wu_prefix_moment(moments, squared, red_low, green_low, blue_low),
        ]
    } else if AXIS == 1 {
        [
            wu_prefix_moment(moments, squared, red_high, green_low, blue_high),
            wu_prefix_moment(moments, squared, red_low, green_low, blue_high),
            wu_prefix_moment(moments, squared, red_high, green_low, blue_low),
            wu_prefix_moment(moments, squared, red_low, green_low, blue_low),
        ]
    } else {
        [
            wu_prefix_moment(moments, squared, red_high, green_high, blue_low),
            wu_prefix_moment(moments, squared, red_low, green_high, blue_low),
            wu_prefix_moment(moments, squared, red_high, green_low, blue_low),
            wu_prefix_moment(moments, squared, red_low, green_low, blue_low),
        ]
    };
    let mut best_score = f32::NEG_INFINITY;
    let mut best_cut = 0u8;
    for cut in (cube.minimum[AXIS] + 1)..cube.maximum[AXIS] {
        let coordinate = cut as i8 - 1;
        let moving = if AXIS == 0 {
            [
                wu_prefix_moment(moments, squared, coordinate, green_high, blue_high),
                wu_prefix_moment(moments, squared, coordinate, green_low, blue_high),
                wu_prefix_moment(moments, squared, coordinate, green_high, blue_low),
                wu_prefix_moment(moments, squared, coordinate, green_low, blue_low),
            ]
        } else if AXIS == 1 {
            [
                wu_prefix_moment(moments, squared, red_high, coordinate, blue_high),
                wu_prefix_moment(moments, squared, red_low, coordinate, blue_high),
                wu_prefix_moment(moments, squared, red_high, coordinate, blue_low),
                wu_prefix_moment(moments, squared, red_low, coordinate, blue_low),
            ]
        } else {
            [
                wu_prefix_moment(moments, squared, red_high, green_high, coordinate),
                wu_prefix_moment(moments, squared, red_low, green_high, coordinate),
                wu_prefix_moment(moments, squared, red_high, green_low, coordinate),
                wu_prefix_moment(moments, squared, red_low, green_low, coordinate),
            ]
        };
        let left_moment = if AXIS == 0 {
            wu_moment_from_corners(
                moving[0], fixed[0], moving[1], moving[2], fixed[1], fixed[2], moving[3], fixed[3],
            )
        } else if AXIS == 1 {
            wu_moment_from_corners(
                moving[0], moving[1], fixed[0], moving[2], fixed[1], moving[3], fixed[2], fixed[3],
            )
        } else {
            wu_moment_from_corners(
                moving[0], moving[1], moving[2], fixed[0], moving[3], fixed[1], fixed[2], fixed[3],
            )
        };
        if left_moment.count == 0 || left_moment.count == total.count {
            continue;
        }
        let score = wu_sum_score(left_moment) + wu_sum_score(total.subtract(left_moment));
        if score > best_score {
            best_score = score;
            best_cut = cut;
        }
    }
    (best_score, best_cut)
}

#[inline(always)]
pub(crate) fn wu_sum_score(moment: WuMoment) -> f32 {
    if moment.count == 0 {
        0.0
    } else {
        let red = moment.red as f32;
        let green = moment.green as f32;
        let blue = moment.blue as f32;
        (red * red + green * green + blue * blue) / moment.count as f32
    }
}

#[inline]
pub(crate) fn wu_cube_variance(
    moments: &[RgbHistogramBin32],
    squared: &[f32],
    cube: WuCube,
) -> f32 {
    if cube.maximum[0] - cube.minimum[0] == 1
        && cube.maximum[1] - cube.minimum[1] == 1
        && cube.maximum[2] - cube.minimum[2] == 1
    {
        return 0.0;
    }
    let moment = wu_cube_moment(moments, squared, cube);
    moment.squared - wu_sum_score(moment)
}

#[inline(never)]
pub(crate) fn split_wu_cube(
    moments: &[RgbHistogramBin32],
    squared: &[f32],
    cube: WuCube,
) -> Option<(WuCube, WuCube)> {
    let total = wu_cube_moment(moments, squared, cube);
    let mut best_score = f32::NEG_INFINITY;
    let mut best_axis = 0usize;
    let mut best_cut = 0u8;
    for axis in 0..3 {
        let (score, cut) = match axis {
            0 => wu_best_axis_cut::<0>(moments, squared, cube, total),
            1 => wu_best_axis_cut::<1>(moments, squared, cube, total),
            _ => wu_best_axis_cut::<2>(moments, squared, cube, total),
        };
        if score > best_score {
            best_score = score;
            best_axis = axis;
            best_cut = cut;
        }
    }
    if best_cut == 0 {
        return None;
    }
    let mut left = cube;
    let mut right = cube;
    left.maximum[best_axis] = best_cut;
    right.minimum[best_axis] = best_cut;
    Some((left, right))
}

#[inline(never)]
pub(crate) fn build_quality_wu_palette(
    has_transparent_pixels: bool,
    colors: Vec<QuantizedColor>,
    mapping_len: usize,
    opaque_color_limit: usize,
    mut palette: Vec<u32>,
) -> (Vec<u32>, Vec<u8>) {
    if colors.is_empty() {
        return (palette, Vec::new());
    }
    #[inline(always)]
    fn heap_entry_is_greater(cubes: &[WuCubeVariance], left: u16, right: u16) -> bool {
        let left_variance = cubes[usize::from(left)].variance;
        let right_variance = cubes[usize::from(right)].variance;
        match left_variance.total_cmp(&right_variance) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Equal => left > right,
            std::cmp::Ordering::Less => false,
        }
    }

    #[inline(always)]
    fn heap_push(
        heap: &mut [u16; 256],
        heap_len: &mut usize,
        cubes: &[WuCubeVariance],
        entry: u16,
    ) {
        let mut position = *heap_len;
        *heap_len += 1;
        while position != 0 {
            let parent = (position - 1) / 2;
            if !heap_entry_is_greater(cubes, entry, heap[parent]) {
                break;
            }
            heap[position] = heap[parent];
            position = parent;
        }
        heap[position] = entry;
    }

    #[inline(always)]
    fn heap_pop(heap: &mut [u16; 256], heap_len: &mut usize, cubes: &[WuCubeVariance]) -> u16 {
        let result = heap[0];
        *heap_len -= 1;
        if *heap_len == 0 {
            return result;
        }
        let replacement = heap[*heap_len];
        let mut position = 0usize;
        loop {
            let left = position * 2 + 1;
            if left >= *heap_len {
                break;
            }
            let right = left + 1;
            let child =
                if right < *heap_len && heap_entry_is_greater(cubes, heap[right], heap[left]) {
                    right
                } else {
                    left
                };
            if !heap_entry_is_greater(cubes, heap[child], replacement) {
                break;
            }
            heap[position] = heap[child];
            position = child;
        }
        heap[position] = replacement;
        result
    }

    debug_assert_eq!(mapping_len, WU_HISTOGRAM_LEN);
    let mut moments = take_quality_histogram_u32(WU_HISTOGRAM_LEN);
    let mut squared = vec![0.0f32; WU_HISTOGRAM_LEN];
    for color in &colors {
        let count = color.count as u32;
        let red = u32::from(color.red);
        let green = u32::from(color.green);
        let blue = u32::from(color.blue);
        moments[usize::from(color.histogram_index)] = RgbHistogramBin32 {
            count,
            red: red * count,
            green: green * count,
            blue: blue * count,
        };
        squared[usize::from(color.histogram_index)] =
            count as f32 * (red * red + green * green + blue * blue) as f32;
    }

    for red in 0..WU_HISTOGRAM_SIDE {
        let mut area = [RgbHistogramBin32::default(); WU_HISTOGRAM_SIDE];
        let mut area_squared = [0.0f32; WU_HISTOGRAM_SIDE];
        for green in 0..WU_HISTOGRAM_SIDE {
            let mut line = RgbHistogramBin32::default();
            let mut line_squared = 0.0f32;
            for blue in 0..WU_HISTOGRAM_SIDE {
                let index = (red << 8) | (green << 4) | blue;
                line.count += moments[index].count;
                line.red += moments[index].red;
                line.green += moments[index].green;
                line.blue += moments[index].blue;
                line_squared += squared[index];
                area[blue].count += line.count;
                area[blue].red += line.red;
                area[blue].green += line.green;
                area[blue].blue += line.blue;
                area_squared[blue] += line_squared;
                moments[index] = if red == 0 {
                    area[blue]
                } else {
                    let previous = moments[index - (1 << 8)];
                    RgbHistogramBin32 {
                        count: previous.count + area[blue].count,
                        red: previous.red + area[blue].red,
                        green: previous.green + area[blue].green,
                        blue: previous.blue + area[blue].blue,
                    }
                };
                squared[index] = if red == 0 {
                    area_squared[blue]
                } else {
                    squared[index - (1 << 8)] + area_squared[blue]
                };
            }
        }
    }

    let full_cube = WuCube {
        minimum: [0; 3],
        maximum: [WU_HISTOGRAM_SIDE as u8; 3],
    };
    let mut cubes = Vec::with_capacity(opaque_color_limit);
    cubes.push(WuCubeVariance {
        cube: full_cube,
        variance: f32::INFINITY,
    });
    let mut split_heap = [0u16; 256];
    let mut split_heap_len = 1usize;
    while cubes.len() < opaque_color_limit {
        let split_index = usize::from(heap_pop(&mut split_heap, &mut split_heap_len, &cubes));
        let selected = cubes[split_index];
        if selected.variance <= 0.0 {
            break;
        }
        let cube = selected.cube;
        if let Some((left, right)) = split_wu_cube(&moments, &squared, cube) {
            cubes[split_index] = WuCubeVariance {
                cube: left,
                variance: wu_cube_variance(&moments, &squared, left),
            };
            heap_push(
                &mut split_heap,
                &mut split_heap_len,
                &cubes,
                split_index as u16,
            );
            let right_index = cubes.len() as u16;
            cubes.push(WuCubeVariance {
                cube: right,
                variance: wu_cube_variance(&moments, &squared, right),
            });
            heap_push(&mut split_heap, &mut split_heap_len, &cubes, right_index);
        } else {
            cubes[split_index].variance = 0.0;
            heap_push(
                &mut split_heap,
                &mut split_heap_len,
                &cubes,
                split_index as u16,
            );
        }
    }

    palette.clear();
    let palette_capacity = cubes.len() + usize::from(has_transparent_pixels);
    if palette.capacity() < palette_capacity {
        palette.reserve(palette_capacity - palette.capacity());
    }
    let mut cube_representatives = [0u32; 256];
    for (cube_index, entry) in cubes.iter().enumerate() {
        let moment = wu_cube_moment(&moments, &squared, entry.cube);
        let representative = rgb_key(
            (moment.red / moment.count) as u8,
            (moment.green / moment.count) as u8,
            (moment.blue / moment.count) as u8,
        );
        cube_representatives[cube_index] = representative;
        palette.push(representative);
    }
    #[cfg(target_arch = "wasm32")]
    {
        let palette_len = palette.len();
        let mut scratch = [0u32; 256];
        for pass in 0..3 {
            let shift = pass * 8;
            let (source, destination): (&[u32], &mut [u32]) = if pass & 1 == 0 {
                (&palette, &mut scratch[..palette_len])
            } else {
                (&scratch[..palette_len], &mut palette)
            };
            let mut frequencies = [0u16; 256];
            for &color in source {
                frequencies[((color >> shift) & 0xff) as usize] += 1;
            }
            let mut offset = 0u16;
            for digit in 0..256 {
                let frequency = frequencies[digit];
                frequencies[digit] = offset;
                offset += frequency;
            }
            for &color in source {
                let target = &mut frequencies[((color >> shift) & 0xff) as usize];
                destination[usize::from(*target)] = color;
                *target += 1;
            }
        }
        palette.copy_from_slice(&scratch[..palette_len]);
    }
    #[cfg(not(target_arch = "wasm32"))]
    palette.sort_unstable();

    let mut canonical_cube_indices = [0u8; 256];
    for (cube_index, &representative) in cube_representatives[..cubes.len()].iter().enumerate() {
        canonical_cube_indices[cube_index] =
            palette.partition_point(|&candidate| candidate < representative) as u8;
    }

    let mut cube_lookup = [0u8; WU_HISTOGRAM_LEN];
    for (cube_index, entry) in cubes.iter().enumerate() {
        for red in entry.cube.minimum[0]..entry.cube.maximum[0] {
            for green in entry.cube.minimum[1]..entry.cube.maximum[1] {
                for blue in entry.cube.minimum[2]..entry.cube.maximum[2] {
                    cube_lookup
                        [(usize::from(red) << 8) | (usize::from(green) << 4) | usize::from(blue)] =
                        canonical_cube_indices[cube_index];
                }
            }
        }
    }
    recycle_quality_histogram_u32(moments);

    let mut requested_cells = [false; 1 << 12];
    let mut initial_hints = [0u8; 1 << 12];
    for color in &colors {
        let cell = usize::from(color.histogram_index);
        requested_cells[cell] = true;
        initial_hints[cell] = cube_lookup[cell];
    }

    let palette_tree = PaletteKdTree::new(&palette);
    let coarse_nearest = palette_tree.coarse_nearest_table_for_cells(
        &palette,
        &requested_cells,
        &initial_hints,
        colors.len(),
    );
    let mut histogram_to_palette = take_quality_histogram_to_palette(mapping_len);
    for color in &colors {
        let cell = usize::from(color.histogram_index);
        histogram_to_palette[cell] = coarse_nearest[cell];
    }
    recycle_quality_colors(colors);
    (palette, histogram_to_palette)
}
