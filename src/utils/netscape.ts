import { GIF } from "../constants/gif";
import type { GifBinary } from "../types";

// Identifier for the Netscape application extension: "NETSCAPE2.0"
const NETSCAPE_APPLICATION_ID = new Uint8Array([
	0x4e, 0x45, 0x54, 0x53, 0x43, 0x41, 0x50, 0x45, 0x32, 0x2e, 0x30,
]);

/**
 * Write a Netscape loop count application extension to the buffer.
 * Returns the new buffer position after the extension.
 */
export function writeNetscapeLoopCount(
	buf: GifBinary,
	p: number,
	loopCount: number,
): number {
	buf[p++] = GIF.EXT;
	buf[p++] = GIF.APPLICATION;
	buf[p++] = GIF.NETSCAPE_LEN;
	for (let i = 0; i < NETSCAPE_APPLICATION_ID.length; i++) {
		buf[p++] = NETSCAPE_APPLICATION_ID[i]!;
	}
	buf[p++] = 0x03;
	buf[p++] = 0x01;
	buf[p++] = loopCount & 0xff;
	buf[p++] = (loopCount >> 8) & 0xff;
	buf[p++] = 0x00;
	return p;
}
