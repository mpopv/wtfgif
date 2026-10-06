//! A k-d tree for nearest-palette-color search.

use super::*;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub(crate) struct PaletteKdNode {
    pub(crate) red: u8,
    pub(crate) green: u8,
    pub(crate) blue: u8,
    pub(crate) palette_index: u8,
    pub(crate) min_red: u8,
    pub(crate) min_green: u8,
    pub(crate) min_blue: u8,
    pub(crate) max_red: u8,
    pub(crate) max_green: u8,
    pub(crate) max_blue: u8,
    pub(crate) parent: u16,
    pub(crate) escape: u16,
}

pub(crate) const PALETTE_KD_EMPTY: u16 = u16::MAX;

pub(crate) struct PaletteKdTree {
    pub(crate) nodes: Vec<PaletteKdNode>,
    pub(crate) root: u16,
}

impl PaletteKdTree {
    pub(crate) fn new(palette_rgb: &[u32]) -> Self {
        #[inline(always)]
        fn component(color: u32, axis: u8) -> u8 {
            match axis {
                0 => (color >> 16) as u8,
                1 => (color >> 8) as u8,
                _ => color as u8,
            }
        }

        fn select_axis_median(indices: &mut [usize], palette_rgb: &[u32], axis: u8, target: usize) {
            let mut start = 0usize;
            let mut end = indices.len();
            while end - start > 1 {
                let first = component(palette_rgb[indices[start]], axis);
                let middle = component(palette_rgb[indices[start + (end - start) / 2]], axis);
                let last = component(palette_rgb[indices[end - 1]], axis);
                let pivot = first.max(middle).min(first.min(middle).max(last));
                let mut lower = start;
                let mut current = start;
                let mut upper = end;
                while current < upper {
                    let value = component(palette_rgb[indices[current]], axis);
                    match value.cmp(&pivot) {
                        std::cmp::Ordering::Less => {
                            indices.swap(lower, current);
                            lower += 1;
                            current += 1;
                        }
                        std::cmp::Ordering::Greater => {
                            upper -= 1;
                            indices.swap(current, upper);
                        }
                        std::cmp::Ordering::Equal => current += 1,
                    }
                }
                if target < lower {
                    end = lower;
                } else if target >= upper {
                    start = upper;
                } else {
                    return;
                }
            }
        }

        fn build(
            indices: &mut [usize],
            palette_rgb: &[u32],
            nodes: &mut Vec<PaletteKdNode>,
            parent: u16,
        ) -> u16 {
            if indices.is_empty() {
                return PALETTE_KD_EMPTY;
            }
            let mut min_red = u8::MAX;
            let mut min_green = u8::MAX;
            let mut min_blue = u8::MAX;
            let mut max_red = 0u8;
            let mut max_green = 0u8;
            let mut max_blue = 0u8;
            for &index in indices.iter() {
                let color = palette_rgb[index];
                let red = (color >> 16) as u8;
                let green = (color >> 8) as u8;
                let blue = color as u8;
                min_red = min_red.min(red);
                min_green = min_green.min(green);
                min_blue = min_blue.min(blue);
                max_red = max_red.max(red);
                max_green = max_green.max(green);
                max_blue = max_blue.max(blue);
            }
            let red_range = max_red - min_red;
            let green_range = max_green - min_green;
            let blue_range = max_blue - min_blue;
            let axis = if red_range >= green_range && red_range >= blue_range {
                0
            } else if green_range >= blue_range {
                1
            } else {
                2
            };
            let midpoint = indices.len() / 2;
            select_axis_median(indices, palette_rgb, axis, midpoint);
            let palette_index = indices[midpoint];
            let node_index = nodes.len() as u16;
            let color = palette_rgb[palette_index];
            nodes.push(PaletteKdNode {
                red: (color >> 16) as u8,
                green: (color >> 8) as u8,
                blue: color as u8,
                palette_index: palette_index as u8,
                min_red,
                min_green,
                min_blue,
                max_red,
                max_green,
                max_blue,
                parent,
                escape: PALETTE_KD_EMPTY,
            });
            let left = build(&mut indices[..midpoint], palette_rgb, nodes, node_index);
            debug_assert!(left == PALETTE_KD_EMPTY || left == node_index + 1);
            build(&mut indices[midpoint + 1..], palette_rgb, nodes, node_index);
            let escape = nodes.len() as u16;
            nodes[usize::from(node_index)].escape = escape;
            node_index
        }

        debug_assert!(palette_rgb.len() <= 256);
        let mut indices = [0usize; 256];
        for (index, value) in indices.iter_mut().take(palette_rgb.len()).enumerate() {
            *value = index;
        }
        let mut nodes = REUSABLE_PALETTE_KD_NODES.with(|scratch| {
            let mut nodes = std::mem::take(&mut *scratch.borrow_mut());
            nodes.clear();
            if nodes.capacity() < palette_rgb.len() {
                nodes.reserve(palette_rgb.len() - nodes.capacity());
            }
            nodes
        });
        let root = build(
            &mut indices[..palette_rgb.len()],
            palette_rgb,
            &mut nodes,
            PALETTE_KD_EMPTY,
        );
        Self { nodes, root }
    }

    /// Update the colors and exact subtree bounds without rebuilding the
    /// median topology. Palette representatives may move after the weighted
    /// assignment pass, but node membership and palette indices stay fixed.
    /// The retained topology supplies traversal order; bounds are recomputed
    /// from the current colors so pruning remains exact even when a
    /// representative crosses an old split plane.
    pub(crate) fn recolor(&mut self, palette_rgb: &[u32]) {
        for node in &mut self.nodes {
            let color = palette_rgb[usize::from(node.palette_index)];
            node.red = (color >> 16) as u8;
            node.green = (color >> 8) as u8;
            node.blue = color as u8;
            node.min_red = node.red;
            node.min_green = node.green;
            node.min_blue = node.blue;
            node.max_red = node.red;
            node.max_green = node.green;
            node.max_blue = node.blue;
        }
        for index in (0..self.nodes.len()).rev() {
            let child = self.nodes[index];
            if child.parent == PALETTE_KD_EMPTY {
                continue;
            }
            let parent = &mut self.nodes[usize::from(child.parent)];
            parent.min_red = parent.min_red.min(child.min_red);
            parent.min_green = parent.min_green.min(child.min_green);
            parent.min_blue = parent.min_blue.min(child.min_blue);
            parent.max_red = parent.max_red.max(child.max_red);
            parent.max_green = parent.max_green.max(child.max_green);
            parent.max_blue = parent.max_blue.max(child.max_blue);
        }
    }

    /// Build the exact 4-bit/channel nearest-color table used by the median
    /// cut mapper. Each cell starts its KD search from the result for an
    /// adjacent cell, which is a much tighter seed than the coarser 3-bit
    /// lookup. Duplicate palette colors keep the original coarse seed so the
    /// zero-distance early exit cannot change the existing lowest-index tie.
    #[cfg(all(test, not(feature = "encode-only")))]
    pub(crate) fn coarse_nearest_table(&self, palette_rgb: &[u32]) -> [u8; 1 << 12] {
        self.coarse_nearest_table_for_cells(palette_rgb, &[true; 1 << 12], &[0; 1 << 12], 1 << 12)
    }

    #[inline(never)]
    pub(crate) fn coarse_nearest_table_for_cells(
        &self,
        palette_rgb: &[u32],
        requested_cells: &[bool; 1 << 12],
        initial_hints: &[u8; 1 << 12],
        _requested_cell_count: usize,
    ) -> [u8; 1 << 12] {
        #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
        if _requested_cell_count >= 2_048 {
            return if _requested_cell_count == 1 << 12 {
                dense_coarse_nearest_table_simd::<false>(
                    palette_rgb,
                    requested_cells,
                    initial_hints,
                )
            } else {
                dense_coarse_nearest_table_simd::<true>(palette_rgb, requested_cells, initial_hints)
            };
        }
        let mut sorted = true;
        let mut has_duplicate = false;
        for pair in palette_rgb.windows(2) {
            sorted &= pair[0] <= pair[1];
            has_duplicate |= pair[0] == pair[1];
        }
        if !sorted && !has_duplicate {
            'duplicate_search: for (index, &color) in palette_rgb.iter().enumerate() {
                if palette_rgb[..index].contains(&color) {
                    has_duplicate = true;
                    break 'duplicate_search;
                }
            }
        }
        let use_neighbor_hints = !has_duplicate;
        let mut table = [0u8; 1 << 12];
        for red in 0..16u8 {
            for green in 0..16u8 {
                for blue in 0..16u8 {
                    let index =
                        (usize::from(red) << 8) | (usize::from(green) << 4) | usize::from(blue);
                    if !requested_cells[index] {
                        continue;
                    }
                    let mut hint_index = initial_hints[index];
                    let neighbor = if use_neighbor_hints {
                        if blue != 0 && requested_cells[index - 1] {
                            Some(table[index - 1])
                        } else if green != 0 && requested_cells[index - (1 << 4)] {
                            Some(table[index - (1 << 4)])
                        } else if red != 0 && requested_cells[index - (1 << 8)] {
                            Some(table[index - (1 << 8)])
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let red = (red << 4) | 8;
                    let green = (green << 4) | 8;
                    let blue = (blue << 4) | 8;
                    let mut hint_distance = palette_color_distance(
                        palette_rgb[usize::from(hint_index)],
                        red,
                        green,
                        blue,
                    );
                    if let Some(neighbor) = neighbor {
                        let neighbor_distance = palette_color_distance(
                            palette_rgb[usize::from(neighbor)],
                            red,
                            green,
                            blue,
                        );
                        if neighbor_distance < hint_distance
                            || (neighbor_distance == hint_distance && neighbor < hint_index)
                        {
                            hint_index = neighbor;
                            hint_distance = neighbor_distance;
                        }
                    }
                    table[index] =
                        self.nearest_with_seed(red, green, blue, hint_index, hint_distance);
                }
            }
        }
        table
    }

    #[cfg(all(test, not(feature = "encode-only")))]
    #[inline]
    pub(crate) fn nearest(&self, r: u8, g: u8, b: u8) -> u8 {
        self.nearest_with_hint(r, g, b, None)
    }

    #[cfg(all(test, not(feature = "encode-only")))]
    #[inline]
    pub(crate) fn nearest_with_hint(&self, r: u8, g: u8, b: u8, hint: Option<(u8, u32)>) -> u8 {
        self.nearest_with_hint_impl(r, g, b, hint)
    }

    #[inline(always)]
    pub(crate) fn nearest_with_hint_split(
        &self,
        r: u8,
        g: u8,
        b: u8,
        hint: Option<(u8, u32)>,
    ) -> u8 {
        self.nearest_with_hint_impl(r, g, b, hint)
    }

    #[inline(always)]
    pub(crate) fn nearest_with_seed_bounds(
        &self,
        r: u8,
        g: u8,
        b: u8,
        best_index: u8,
        best_distance: u32,
    ) -> u8 {
        self.nearest_with_seed(r, g, b, best_index, best_distance)
    }

    #[inline(always)]
    pub(crate) fn nearest_with_hint_impl(
        &self,
        r: u8,
        g: u8,
        b: u8,
        hint: Option<(u8, u32)>,
    ) -> u8 {
        if self.root == PALETTE_KD_EMPTY {
            return 0;
        }
        let (best_index, best_distance) = hint
            .map(|(index, color)| (index, palette_color_distance(color, r, g, b)))
            .unwrap_or((0, u32::MAX));
        self.nearest_with_seed(r, g, b, best_index, best_distance)
    }

    #[inline(never)]
    pub(crate) fn nearest_with_seed(
        &self,
        r: u8,
        g: u8,
        b: u8,
        mut best_index: u8,
        mut best_distance: u32,
    ) -> u8 {
        if self.root == PALETTE_KD_EMPTY || best_distance == 0 {
            return best_index;
        }
        let r = i32::from(r);
        let g = i32::from(g);
        let b = i32::from(b);
        // Nodes are in preorder and every node records the first index after
        // its subtree. Exact RGB bounds can therefore skip an entire branch
        // without writing a pending-branch stack for every histogram cell.
        let mut next = usize::from(self.root);
        while next < self.nodes.len() {
            let node_index = next;
            let node = unsafe { *self.nodes.get_unchecked(node_index) };
            let min_red = i32::from(node.min_red);
            let min_green = i32::from(node.min_green);
            let min_blue = i32::from(node.min_blue);
            let max_red = i32::from(node.max_red);
            let max_green = i32::from(node.max_green);
            let max_blue = i32::from(node.max_blue);
            let red_bound = (min_red - r).max(r - max_red).max(0);
            let green_bound = (min_green - g).max(g - max_green).max(0);
            let blue_bound = (min_blue - b).max(b - max_blue).max(0);
            let subtree_distance = (red_bound * red_bound
                + green_bound * green_bound
                + blue_bound * blue_bound) as u32;
            if subtree_distance > best_distance {
                next = usize::from(node.escape);
                continue;
            }
            let dr = r - i32::from(node.red);
            let dg = g - i32::from(node.green);
            let db = b - i32::from(node.blue);
            let node_distance = (dr * dr + dg * dg + db * db) as u32;
            if node_distance < best_distance
                || (node_distance == best_distance && node.palette_index < best_index)
            {
                best_distance = node_distance;
                best_index = node.palette_index;
            }
            next += 1;
        }
        best_index
    }
}

impl Drop for PaletteKdTree {
    fn drop(&mut self) {
        let nodes = std::mem::take(&mut self.nodes);
        REUSABLE_PALETTE_KD_NODES.with(|scratch| {
            *scratch.borrow_mut() = nodes;
        });
    }
}
