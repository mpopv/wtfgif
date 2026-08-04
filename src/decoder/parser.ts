import { GIF } from "../constants/gif";
import type { FrameInfo } from "../types";

export interface ParsedGif {
	width: number;
	height: number;
	globalPaletteOffset: number | null;
	globalPaletteSize: number | null;
	frames: FrameInfo[];
	loopCount: number | null;
}

function requireBytes(
	data: Uint8Array,
	offset: number,
	length: number,
	context: string,
): void {
	if (offset < 0 || length < 0 || offset + length > data.length) {
		throw new Error(`Truncated GIF ${context}.`);
	}
}

function readUint16(data: Uint8Array, offset: number, context: string): number {
	requireBytes(data, offset, 2, context);
	return (data[offset]! | (data[offset + 1]! << 8)) >>> 0;
}

function skipSubBlocks(
	data: Uint8Array,
	offset: number,
	context: string,
): number {
	let position = offset;
	while (true) {
		requireBytes(data, position, 1, context);
		const length = data[position++]!;
		if (length === 0) return position;
		requireBytes(data, position, length, context);
		position += length;
	}
}

function parseApplicationExtension(
	data: Uint8Array,
	offset: number,
): { nextOffset: number; loopCount: number | null } {
	requireBytes(data, offset, 1, "application extension header");
	const identifierLength = data[offset++]!;
	requireBytes(data, offset, identifierLength, "application identifier");
	const identifier = data.subarray(offset, offset + identifierLength);
	offset += identifierLength;
	const isNetscape =
		identifierLength === 11 &&
		String.fromCharCode(...identifier) === "NETSCAPE2.0";
	let loopCount: number | null = null;
	while (true) {
		requireBytes(data, offset, 1, "application extension data");
		const length = data[offset++]!;
		if (length === 0) break;
		requireBytes(data, offset, length, "application extension data");
		if (isNetscape && length >= 3 && data[offset] === 1) {
			loopCount = readUint16(data, offset + 1, "NETSCAPE loop count");
		}
		offset += length;
	}
	return { nextOffset: offset, loopCount };
}

/** Parse one complete, structurally valid GIF87a/GIF89a stream. */
export function parseGif(data: Uint8Array): ParsedGif {
	if (data.length < 13) {
		throw new Error("Invalid GIF: data is too short for a logical screen.");
	}
	if (
		data[0] !== GIF.G ||
		data[1] !== GIF.I ||
		data[2] !== GIF.F ||
		data[3] !== GIF._8 ||
		(data[4] !== GIF._7 && data[4] !== GIF._9) ||
		data[5] !== GIF.A
	) {
		throw new Error("Invalid GIF 87a/89a header.");
	}

	const width = readUint16(data, 6, "logical screen width");
	const height = readUint16(data, 8, "logical screen height");
	if (width === 0 || height === 0) {
		throw new Error("Invalid GIF logical screen dimensions.");
	}
	const packed = data[10]!;
	let position = 13;
	let globalPaletteOffset: number | null = null;
	let globalPaletteSize: number | null = null;
	if ((packed & 0x80) !== 0) {
		globalPaletteSize = 2 << (packed & 0x07);
		globalPaletteOffset = position;
		const paletteBytes = globalPaletteSize * 3;
		requireBytes(data, position, paletteBytes, "global color table");
		position += paletteBytes;
	}

	const frames: FrameInfo[] = [];
	let loopCount: number | null = null;
	let delay = 0;
	let disposal = 0;
	let transparentIndex: number | null = null;
	let foundTrailer = false;

	while (position < data.length) {
		const block = data[position++]!;
		if (block === GIF.TRAILER) {
			foundTrailer = true;
			break;
		}

		if (block === GIF.EXT) {
			requireBytes(data, position, 1, "extension label");
			const label = data[position++]!;
			if (label === GIF.APPLICATION) {
				const application = parseApplicationExtension(data, position);
				position = application.nextOffset;
				loopCount = application.loopCount ?? loopCount;
				continue;
			}
			if (label === GIF.GCE) {
				requireBytes(data, position, 6, "graphics control extension");
				if (data[position++] !== 4) {
					throw new Error("Invalid GIF graphics control extension size.");
				}
				const control = data[position++]!;
				delay = readUint16(data, position, "graphics control delay");
				position += 2;
				const candidate = data[position++]!;
				if (data[position++] !== 0) {
					throw new Error("Invalid GIF graphics control extension terminator.");
				}
				transparentIndex = (control & 1) !== 0 ? candidate : null;
				disposal = (control >>> 2) & 0x07;
				continue;
			}
			if (label === GIF.PLAINTEXT || label === GIF.COMMENT) {
				position = skipSubBlocks(data, position, "extension data");
				continue;
			}
			throw new Error(`Unknown GIF extension label: 0x${label.toString(16)}.`);
		}

		if (block !== GIF.IMG) {
			throw new Error(`Unknown GIF block: 0x${block.toString(16)}.`);
		}
		requireBytes(data, position, 9, "image descriptor");
		const x = readUint16(data, position, "frame x");
		const y = readUint16(data, position + 2, "frame y");
		const frameWidth = readUint16(data, position + 4, "frame width");
		const frameHeight = readUint16(data, position + 6, "frame height");
		const framePacked = data[position + 8]!;
		position += 9;
		if (
			frameWidth === 0 ||
			frameHeight === 0 ||
			x + frameWidth > width ||
			y + frameHeight > height
		) {
			throw new Error("Invalid GIF frame dimensions.");
		}

		let paletteOffset = globalPaletteOffset;
		let paletteSize = globalPaletteSize;
		const hasLocalPalette = (framePacked & 0x80) !== 0;
		if (hasLocalPalette) {
			paletteSize = 2 << (framePacked & 0x07);
			paletteOffset = position;
			const paletteBytes = paletteSize * 3;
			requireBytes(data, position, paletteBytes, "local color table");
			position += paletteBytes;
		}
		if (paletteOffset === null || paletteSize === null) {
			throw new Error("GIF frame has no color palette.");
		}

		const dataOffset = position;
		requireBytes(data, position, 1, "image LZW code size");
		const minCodeSize = data[position++]!;
		if (minCodeSize < 2 || minCodeSize > 8) {
			throw new Error(`Invalid GIF LZW minimum code size ${minCodeSize}.`);
		}
		position = skipSubBlocks(data, position, "image data");
		frames.push({
			x,
			y,
			width: frameWidth,
			height: frameHeight,
			has_local_palette: hasLocalPalette,
			palette_offset: paletteOffset,
			palette_size: paletteSize,
			data_offset: dataOffset,
			data_length: position - dataOffset,
			transparent_index: transparentIndex,
			interlaced: (framePacked & 0x40) !== 0,
			delay,
			disposal,
			min_code_size: minCodeSize,
		});
		delay = 0;
		disposal = 0;
		transparentIndex = null;
	}

	if (!foundTrailer) throw new Error("Truncated GIF: missing trailer.");
	if (frames.length === 0) throw new Error("Invalid GIF: no image frames.");
	if (position !== data.length) {
		throw new Error("Invalid GIF: trailing bytes after trailer.");
	}

	return {
		width,
		height,
		globalPaletteOffset,
		globalPaletteSize,
		frames,
		loopCount,
	};
}
