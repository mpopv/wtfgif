export interface PixelRect {
	x: number;
	y: number;
	width: number;
	height: number;
}

export function findChangedRect(
	previous: ArrayLike<number>,
	current: ArrayLike<number>,
	width: number,
	height: number,
): PixelRect | null {
	let top = 0;
	let bottom = height - 1;

	while (top < height) {
		const row = top * width;
		let changed = false;
		for (let x = 0; x < width; x++) {
			if (previous[row + x] !== current[row + x]) {
				changed = true;
				break;
			}
		}
		if (changed) break;
		top++;
	}

	if (top === height) return null;

	while (bottom > top) {
		const row = bottom * width;
		let changed = false;
		for (let x = 0; x < width; x++) {
			if (previous[row + x] !== current[row + x]) {
				changed = true;
				break;
			}
		}
		if (changed) break;
		bottom--;
	}

	let left = width - 1;
	let right = 0;
	for (let y = top; y <= bottom; y++) {
		const row = y * width;
		for (let x = 0; x < width; x++) {
			if (previous[row + x] !== current[row + x]) {
				if (x < left) left = x;
				if (x > right) right = x;
			}
		}
	}

	return {
		x: left,
		y: top,
		width: right - left + 1,
		height: bottom - top + 1,
	};
}

export function blitRectPixels(
	source: Uint32Array,
	target: Uint32Array,
	targetWidth: number,
	x: number,
	y: number,
	width: number,
	height: number,
): void {
	for (let row = 0; row < height; row++) {
		const src = row * width;
		const dst = (y + row) * targetWidth + x;
		target.set(source.subarray(src, src + width), dst);
	}
}

export function hashPixels(pixels: Uint32Array): number {
	let hash = 2166136261;
	for (let i = 0; i < pixels.length; i++) {
		hash ^= pixels[i]!;
		hash = Math.imul(hash, 16777619);
	}
	return hash >>> 0;
}

export function pixelsEqual(a: Uint32Array, b: Uint32Array): boolean {
	if (a.length !== b.length) return false;
	for (let i = 0; i < a.length; i++) {
		if (a[i] !== b[i]) return false;
	}
	return true;
}

export function findMatchingPixels(
	pixels: Uint32Array,
	bucket: Uint32Array[] | undefined,
): Uint32Array | null {
	if (!bucket) return null;
	for (const candidate of bucket) {
		if (pixelsEqual(candidate, pixels)) return candidate;
	}
	return null;
}
