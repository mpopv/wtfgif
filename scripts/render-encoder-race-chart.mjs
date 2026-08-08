import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function escapeXml(value) {
	return String(value)
		.replaceAll("&", "&amp;")
		.replaceAll("<", "&lt;")
		.replaceAll(">", "&gt;")
		.replaceAll('"', "&quot;");
}

function niceMaximum(value) {
	const exponent = 10 ** Math.floor(Math.log10(value));
	const normalized = value / exponent;
	const rounded = [1, 1.25, 1.5, 2, 2.5, 5, 10].find(
		(candidate) => candidate >= normalized,
	);
	return rounded * exponent;
}

function coordinate(value) {
	return Number(value.toFixed(2));
}

function formatMilliseconds(value) {
	if (value < 10) return `${value.toFixed(2)} ms`;
	return `${value.toFixed(1)} ms`;
}

function formatBytes(value) {
	return `${(value / 1024).toFixed(1)} KiB`;
}

function formatComparison(row) {
	if (row.id === "wtfgif") return "baseline";
	return `${row.slowerThanWtfgif.toFixed(2)}× slower`;
}

export function renderEncoderRaceChart(receipt) {
	const rows = receipt.results.toSorted(
		(left, right) => left.medianMs - right.medianMs,
	);
	const width = 1060;
	const labelWidth = 220;
	const chartLeft = 250;
	// Leave enough room for the value label after the longest bar.
	const chartRight = 790;
	const chartWidth = chartRight - chartLeft;
	const rowHeight = 50;
	const firstRowY = 130;
	const height = firstRowY + rows.length * rowHeight + 108;
	const maximum = niceMaximum(Math.max(...rows.map((row) => row.medianMs)));
	const tickCount = 5;
	const subtitle = `Median first real encode · ${receipt.fixture.frames} × ${receipt.fixture.width}×${receipt.fixture.height} RGBA frames · ${receipt.environment.iterations} fresh Chrome processes · lower is better`;
	const elements = [];

	elements.push(
		`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="img" aria-labelledby="title desc">`,
		'<title id="title">Browser GIF encoder speed in milliseconds</title>',
		`<desc id="desc">Horizontal bar chart comparing ${rows.map((row) => row.label).join(", ")}. Bar length is median encode time in milliseconds, so shorter is faster. Labels also report emitted GIF size and relative slowdown versus wtfgif.</desc>`,
		'<rect width="100%" height="100%" fill="#ffffff"/>',
		'<g font-family="ui-sans-serif, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif" fill="#172026">',
		'<text x="40" y="48" font-size="25" font-weight="700">Browser encode time: arbitrary RGBA images → GIF</text>',
		`<text x="40" y="76" font-size="14" fill="#51606a">${escapeXml(subtitle)}</text>`,
	);

	for (let tick = 0; tick <= tickCount; tick += 1) {
		const value = (maximum * tick) / tickCount;
		const x = coordinate(chartLeft + (chartWidth * tick) / tickCount);
		elements.push(
			`<line x1="${x}" y1="102" x2="${x}" y2="${firstRowY + rows.length * rowHeight - 12}" stroke="#e3e8eb" stroke-width="1"/>`,
			`<text x="${x}" y="98" text-anchor="middle" font-size="12" fill="#687780">${value.toFixed(0)} ms</text>`,
		);
	}

	for (const [index, row] of rows.entries()) {
		const y = firstRowY + index * rowHeight;
		const barWidth = coordinate(
			Math.max(2, (row.medianMs / maximum) * chartWidth),
		);
		const color = row.id === "wtfgif" ? "#0d8f6f" : "#aab5bb";
		const valueX = coordinate(
			Math.min(chartRight - 4, chartLeft + barWidth + 9),
		);
		const anchor = valueX >= chartRight - 4 ? "end" : "start";
		elements.push(
			`<text x="${labelWidth}" y="${y + 21}" text-anchor="end" font-size="15" font-weight="${row.id === "wtfgif" ? 700 : 500}">${escapeXml(row.label)}</text>`,
			`<rect x="${chartLeft}" y="${y}" width="${barWidth}" height="28" rx="3" fill="${color}"/>`,
			`<text x="${valueX}" y="${y + 20}" text-anchor="${anchor}" font-size="14" font-weight="700" fill="#172026">${formatMilliseconds(row.medianMs)} · ${formatBytes(row.bytes)} · ${formatComparison(row)}</text>`,
		);
	}

	elements.push(
		`<text x="40" y="${height - 56}" font-size="12" fill="#687780">Bar length = encode time · labels include emitted GIF size and relative slowdown · gif.js worker creation is timed</text>`,
		`<text x="40" y="${height - 34}" font-size="12" fill="#687780">${escapeXml(receipt.environment.browser)} · ${escapeXml(receipt.environment.cpu)} · every output decoded and checked</text>`,
		"</g>",
		"</svg>",
	);
	return `${elements.join("\n")}\n`;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
	const input =
		process.argv[2] ?? path.join(root, "benchmarks", "encoder-race.json");
	const output = process.argv[3] ?? path.join(root, "docs", "encoder-race.svg");
	const receipt = JSON.parse(await readFile(input, "utf8"));
	await writeFile(output, renderEncoderRaceChart(receipt));
	console.log(`Rendered ${path.relative(root, output)}`);
}
