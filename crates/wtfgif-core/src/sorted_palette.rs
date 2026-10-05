//! Exact nearest-palette search that skips most of a 256-color palette.
//!
//! Colors are kept in ascending green order. A query starts where its green
//! value would be inserted and scans outward in blocks of eight. Every color
//! beyond the scanned range differs in green by at least the next unscanned
//! color's gap, so once that gap squared exceeds the best distance found, no
//! remaining color can win or tie. Candidates are compared as
//! `(squared distance << 8) | palette index`, the same packed order as the
//! full scan, so equal distances still resolve to the lowest palette index.

/// Padding on both sides lets a block straddle either end of the palette.
const PAD: usize = 8;
const SLOTS: usize = 256 + 2 * PAD;
/// Channel value for padding lanes. Its squared distance to any real color
/// exceeds every real distance, so a padding lane never wins.
const FAR: i16 = 1_000;

#[repr(C, align(16))]
pub(crate) struct SortedPaletteSearch {
    red: [i16; SLOTS],
    green: [i16; SLOTS],
    blue: [i16; SLOTS],
    index: [i32; SLOTS],
    /// Sorted position of the first color whose green is at least `g`.
    start: [u16; 256],
    len: usize,
}

impl SortedPaletteSearch {
    pub(crate) fn new() -> Self {
        Self {
            red: [FAR; SLOTS],
            green: [FAR; SLOTS],
            blue: [FAR; SLOTS],
            index: [0; SLOTS],
            start: [0; 256],
            len: 0,
        }
    }

    /// Rebuild from `palette` (`0xRRGGBB` entries, at most 256).
    pub(crate) fn rebuild(&mut self, palette: &[u32]) {
        debug_assert!(!palette.is_empty() && palette.len() <= 256);
        let mut counts = [0u16; 256];
        for &color in palette {
            counts[((color >> 8) & 0xff) as usize] += 1;
        }
        let mut position = 0u16;
        for (green, count) in counts.iter_mut().enumerate() {
            self.start[green] = position;
            position += *count;
            *count = self.start[green];
        }
        for (index, &color) in palette.iter().enumerate() {
            let green = ((color >> 8) & 0xff) as usize;
            let slot = PAD + usize::from(counts[green]);
            counts[green] += 1;
            self.red[slot] = ((color >> 16) & 0xff) as i16;
            self.green[slot] = green as i16;
            self.blue[slot] = (color & 0xff) as i16;
            self.index[slot] = index as i32;
        }
        let end = PAD + palette.len();
        self.red[end..].fill(FAR);
        self.green[end..].fill(FAR);
        self.blue[end..].fill(FAR);
        self.index[end..].fill(0);
        self.len = palette.len();
    }

    /// The palette index nearest to `(r, g, b)`, lowest index on ties.
    pub(crate) fn nearest(&self, r: u8, g: u8, b: u8) -> u8 {
        self.nearest_from(r, g, b, i32::MAX)
    }

    /// Like [`Self::nearest`], starting from a palette entry that is likely
    /// the answer. A close seed bounds the scan to a narrow green window.
    pub(crate) fn nearest_seeded(&self, r: u8, g: u8, b: u8, seed: u8, seed_color: u32) -> u8 {
        let red = i32::from(r) - ((seed_color >> 16) & 0xff) as i32;
        let green = i32::from(g) - ((seed_color >> 8) & 0xff) as i32;
        let blue = i32::from(b) - (seed_color & 0xff) as i32;
        let distance = red * red + green * green + blue * blue;
        self.nearest_from(r, g, b, (distance << 8) + i32::from(seed))
    }

    #[inline(never)]
    fn nearest_from(&self, r: u8, g: u8, b: u8, mut best: i32) -> u8 {
        let query = Query::new(r, g, b);
        let green = i32::from(g);
        let end = PAD + self.len;
        let mut up = PAD + usize::from(self.start[usize::from(g)]);
        let mut down = up;
        loop {
            // A direction continues while its next color's green gap alone
            // does not exceed the best distance. Not strictly greater: a color
            // at exactly the best distance could win the tie on palette index.
            let limit = best >> 8;
            let take_up = up < end && {
                let gap = i32::from(self.green[up]) - green;
                gap * gap <= limit
            };
            let take_down = down > PAD && {
                let gap = green - i32::from(self.green[down - 1]);
                gap * gap <= limit
            };
            if !take_up && !take_down {
                break;
            }
            // The two blocks are independent, so their latencies overlap.
            let mut minimum = i32::MAX;
            if take_up {
                minimum = minimum.min(self.block_minimum(up, &query));
                up += 8;
            }
            if take_down {
                down -= 8;
                minimum = minimum.min(self.block_minimum(down, &query));
            }
            best = best.min(minimum);
        }
        best as u8
    }

    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    #[inline(always)]
    fn block_minimum(&self, position: usize, query: &Query) -> i32 {
        use core::arch::wasm32::{
            i16x8_sub, i32x4_add, i32x4_extmul_high_i16x8, i32x4_extmul_low_i16x8,
            i32x4_extract_lane, i32x4_min, i32x4_shl, i32x4_shuffle, v128_load,
        };
        debug_assert!(position + 8 <= SLOTS);
        unsafe {
            let red = i16x8_sub(query.red, v128_load(self.red.as_ptr().add(position).cast()));
            let green = i16x8_sub(
                query.green,
                v128_load(self.green.as_ptr().add(position).cast()),
            );
            let blue = i16x8_sub(
                query.blue,
                v128_load(self.blue.as_ptr().add(position).cast()),
            );
            let low = i32x4_add(
                i32x4_add(
                    i32x4_extmul_low_i16x8(red, red),
                    i32x4_extmul_low_i16x8(green, green),
                ),
                i32x4_extmul_low_i16x8(blue, blue),
            );
            let high = i32x4_add(
                i32x4_add(
                    i32x4_extmul_high_i16x8(red, red),
                    i32x4_extmul_high_i16x8(green, green),
                ),
                i32x4_extmul_high_i16x8(blue, blue),
            );
            let low = i32x4_add(
                v128_load(self.index.as_ptr().add(position).cast()),
                i32x4_shl(low, 8),
            );
            let high = i32x4_add(
                v128_load(self.index.as_ptr().add(position + 4).cast()),
                i32x4_shl(high, 8),
            );
            let mut minimum = i32x4_min(low, high);
            minimum = i32x4_min(minimum, i32x4_shuffle::<2, 3, 0, 1>(minimum, minimum));
            minimum = i32x4_min(minimum, i32x4_shuffle::<1, 0, 3, 2>(minimum, minimum));
            i32x4_extract_lane::<0>(minimum)
        }
    }

    #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
    #[inline(always)]
    fn block_minimum(&self, position: usize, query: &Query) -> i32 {
        (position..position + 8)
            .map(|slot| {
                let red = i32::from(query.red) - i32::from(self.red[slot]);
                let green = i32::from(query.green) - i32::from(self.green[slot]);
                let blue = i32::from(query.blue) - i32::from(self.blue[slot]);
                ((red * red + green * green + blue * blue) << 8) + self.index[slot]
            })
            .min()
            .unwrap_or(i32::MAX)
    }
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
struct Query {
    red: core::arch::wasm32::v128,
    green: core::arch::wasm32::v128,
    blue: core::arch::wasm32::v128,
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
impl Query {
    #[inline(always)]
    fn new(r: u8, g: u8, b: u8) -> Self {
        use core::arch::wasm32::i16x8_splat;
        Self {
            red: i16x8_splat(i16::from(r)),
            green: i16x8_splat(i16::from(g)),
            blue: i16x8_splat(i16::from(b)),
        }
    }
}

#[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
struct Query {
    red: i16,
    green: i16,
    blue: i16,
}

#[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
impl Query {
    #[inline(always)]
    fn new(r: u8, g: u8, b: u8) -> Self {
        Self {
            red: i16::from(r),
            green: i16::from(g),
            blue: i16::from(b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brute_force(palette: &[u32], r: u8, g: u8, b: u8) -> u8 {
        palette
            .iter()
            .enumerate()
            .map(|(index, &color)| {
                let red = i32::from(r) - ((color >> 16) & 0xff) as i32;
                let green = i32::from(g) - ((color >> 8) & 0xff) as i32;
                let blue = i32::from(b) - (color & 0xff) as i32;
                ((red * red + green * green + blue * blue) << 8) + index as i32
            })
            .min()
            .unwrap() as u8
    }

    #[test]
    fn matches_a_full_scan_including_ties() {
        let mut state = 0x2545_f491u32;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        let mut search = SortedPaletteSearch::new();
        for trial in 0..300 {
            let len = 1 + (next() as usize % 256);
            // Few distinct channel values force many equal distances.
            let levels = if trial % 3 == 0 { 4 } else { 256 };
            let palette: Vec<u32> = (0..len)
                .map(|_| {
                    let channel = |value: u32| (value % levels) * (255 / (levels - 1).max(1));
                    (channel(next()) << 16) | (channel(next()) << 8) | channel(next())
                })
                .collect();
            search.rebuild(&palette);
            for _ in 0..200 {
                let (r, g, b) = (next() as u8, next() as u8, next() as u8);
                assert_eq!(search.nearest(r, g, b), brute_force(&palette, r, g, b));
            }
            for &color in &palette {
                let (r, g, b) = ((color >> 16) as u8, (color >> 8) as u8, color as u8);
                assert_eq!(search.nearest(r, g, b), brute_force(&palette, r, g, b));
            }
            // Any seed, good or bad, must give the same answer.
            for _ in 0..200 {
                let (r, g, b) = (next() as u8, next() as u8, next() as u8);
                let seed = next() as usize % len;
                assert_eq!(
                    search.nearest_seeded(r, g, b, seed as u8, palette[seed]),
                    brute_force(&palette, r, g, b)
                );
            }
        }
    }
}
