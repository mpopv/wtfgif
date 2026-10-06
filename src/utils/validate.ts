// Encoder options must already be integers in range. Coercing with `| 0`
// would turn 4.9 into 4, NaN into 0, and 2 ** 32 into 0 (a loop count of
// "forever"), so these checks reject instead of guessing.

function isIntegerInRange(value: unknown, min: number, max: number): boolean {
	return (
		typeof value === "number" &&
		Number.isInteger(value) &&
		value >= min &&
		value <= max
	);
}

export function checkedDimensions(width: number, height: number): void {
	if (
		!isIntegerInRange(width, 1, 65535) ||
		!isIntegerInRange(height, 1, 65535)
	) {
		throw new Error("Width/Height invalid.");
	}
}

export function checkedFrameCount(frameCount: number): number {
	if (!isIntegerInRange(frameCount, 1, Number.MAX_SAFE_INTEGER)) {
		throw new Error("Frame count must be a positive integer.");
	}
	return frameCount;
}

export function checkedU16(value: number, message: string): number {
	if (!isIntegerInRange(value, 0, 65535)) throw new Error(message);
	return value;
}

export function checkedU8(value: number, message: string): number {
	if (!isIntegerInRange(value, 0, 255)) throw new Error(message);
	return value;
}
