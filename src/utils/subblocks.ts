export function concatSubBlocks(
	buf: Uint8Array,
	offset: number,
): { bytes: Uint8Array; mcs: number } {
	const mcs = buf[offset]! | 0;
	let q = (offset + 1) | 0;
	const firstLength = buf[q]! | 0;
	const firstPayload = (q + 1) | 0;
	if (
		firstLength > 0 &&
		firstPayload + firstLength < buf.length &&
		buf[firstPayload + firstLength] === 0
	) {
		return {
			bytes: buf.subarray(firstPayload, firstPayload + firstLength),
			mcs,
		};
	}
	let total = 0;
	while (true) {
		const len = buf[q++]! | 0;
		if (len === 0) break;
		total += len;
		q += len;
	}
	const out = new Uint8Array(total);
	q = (offset + 1) | 0;
	let w = 0;
	while (true) {
		const len = buf[q++]! | 0;
		if (len === 0) break;
		out.set(buf.subarray(q, q + len), w);
		w += len;
		q += len;
	}
	return { bytes: out, mcs };
}
