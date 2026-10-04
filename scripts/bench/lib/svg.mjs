export function escapeXml(value) {
	return String(value)
		.replaceAll("&", "&amp;")
		.replaceAll("<", "&lt;")
		.replaceAll(">", "&gt;")
		.replaceAll('"', "&quot;");
}

export function coordinate(value) {
	return Number(value.toFixed(2));
}

export function niceMaximum(value) {
	const exponent = 10 ** Math.floor(Math.log10(value));
	const normalized = value / exponent;
	const rounded = [1, 1.25, 1.5, 2, 2.5, 5, 10].find(
		(candidate) => candidate >= normalized,
	);
	return rounded * exponent;
}
