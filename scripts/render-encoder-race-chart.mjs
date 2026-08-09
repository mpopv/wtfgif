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

function formatAlphaAccuracy(value) {
	return `${Number(value.toFixed(2))}% alpha match`;
}

export function renderEncoderRaceChart(receipt) {
	const rows = receipt.results.toSorted(
		(left, right) => left.medianMs - right.medianMs,
	);
	const wtfgif = rows.find((row) => row.id === "wtfgif");
	if (!wtfgif) throw new Error("Encoder race receipt has no wtfgif result");
	const width = 1200;
	const labelWidth = 220;
	const chartLeft = 250;
	// Leave enough room for the value label after the longest bar.
	const chartRight = 830;
	const chartWidth = chartRight - chartLeft;
	const rowHeight = 62;
	const firstRowY = 148;
	const height = firstRowY + rows.length * rowHeight + 112;
	const maximum = niceMaximum(Math.max(...rows.map((row) => row.medianMs)));
	const hundredTimesWtfgifMs = wtfgif.medianMs * 100;
	const hundredTimesWtfgifX = coordinate(
		chartLeft + (hundredTimesWtfgifMs / maximum) * chartWidth,
	);
	const tickCount = 5;
	const competitors = rows.filter((row) => row.id !== "wtfgif");
	const minimumSlowdown = Math.min(
		...competitors.map((row) => row.slowerThanWtfgif),
	);
	const maximumSlowdown = Math.max(
		...competitors.map((row) => row.slowerThanWtfgif),
	);
	const workload = `${receipt.fixture.frames} × ${receipt.fixture.width}×${receipt.fixture.height} RGBA frames · ${receipt.environment.iterations} fresh Chrome processes`;
	const elements = [];

	elements.push(
		`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="img" aria-labelledby="title desc">`,
		'<title id="title">Browser GIF encoder speed in milliseconds</title>',
		`<desc id="desc">Horizontal bar chart comparing ${rows.map((row) => row.label).join(", ")}. Bar length is median encode time in milliseconds, so shorter is faster. A dashed vertical line marks one hundred times wtfgif latency. Labels also report emitted GIF size, relative slowdown versus wtfgif, PSNR, and alpha agreement.</desc>`,
		'<rect width="100%" height="100%" fill="#ffffff"/>',
		'<g font-family="ui-sans-serif, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif" fill="#172026">',
		'<text x="40" y="48" font-size="25" font-weight="700">Public-API browser encode time</text>',
		`<text x="40" y="76" font-size="14" fill="#51606a">${escapeXml(`Median first real encode after initialization + 64 MiB eviction · ${workload}`)}</text>`,
		`<text x="40" y="100" font-size="13" fill="#51606a">${escapeXml(`wtfgif ${wtfgif.medianMs.toFixed(3)} ms · alternatives ${minimumSlowdown.toFixed(2)}×–${maximumSlowdown.toFixed(2)}× slower · lower is better`)}</text>`,
	);

	for (let tick = 0; tick <= tickCount; tick += 1) {
		const value = (maximum * tick) / tickCount;
		const x = coordinate(chartLeft + (chartWidth * tick) / tickCount);
		elements.push(
			`<line x1="${x}" y1="120" x2="${x}" y2="${firstRowY + rows.length * rowHeight - 12}" stroke="#e3e8eb" stroke-width="1"/>`,
			`<text x="${x}" y="118" text-anchor="middle" font-size="12" fill="#687780">${value.toFixed(0)} ms</text>`,
		);
	}

	if (hundredTimesWtfgifMs <= maximum) {
		elements.push(
			`<line x1="${hundredTimesWtfgifX}" y1="128" x2="${hundredTimesWtfgifX}" y2="${firstRowY + rows.length * rowHeight - 12}" stroke="#b34b3f" stroke-width="2" stroke-dasharray="5 4"/>`,
			`<text x="${hundredTimesWtfgifX + 7}" y="140" font-size="12" font-weight="700" fill="#9b3d33">100× wtfgif (${formatMilliseconds(hundredTimesWtfgifMs)})</text>`,
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
			`<text x="${chartLeft}" y="${y + 45}" font-size="12" fill="#687780">${row.psnrDb.toFixed(2)} dB PSNR · ${formatAlphaAccuracy(row.alphaAccuracyPercent)}</text>`,
		);
	}

	elements.push(
		`<text x="40" y="${height - 58}" font-size="12" fill="#687780">Time to complete GIF · dashed line = 100× wtfgif · labels include size and decoded quality · gif.js worker creation is timed</text>`,
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
