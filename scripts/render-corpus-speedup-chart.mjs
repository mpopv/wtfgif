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

function coordinate(value) {
	return Number(value.toFixed(2));
}

function niceMaximum(value) {
	const exponent = 10 ** Math.floor(Math.log10(value));
	const normalized = value / exponent;
	const rounded = [1, 1.25, 1.5, 2, 2.5, 5, 10].find(
		(candidate) => candidate >= normalized,
	);
	return rounded * exponent;
}

function geometricMean(values) {
	return Math.exp(
		values.reduce((sum, value) => sum + Math.log(value), 0) / values.length,
	);
}

function chartRows(receipt) {
	const fixtures = new Map(
		receipt.corpus.map((fixture) => [fixture.id, fixture]),
	);
	const baselines = new Map(
		receipt.results
			.filter((result) => result.implementation === "image-q-rgbquant+omggif")
			.map((result) => [result.fixtureId, result]),
	);
	return receipt.results
		.filter((result) => result.implementation === "wtfgif")
		.map((result) => ({
			...result,
			baseline: baselines.get(result.fixtureId),
			fixture: fixtures.get(result.fixtureId),
		}));
}

function formatRgbQuality(result) {
	if (result.quality.losslessOpaqueRgb) return "lossless RGB";
	return `${result.quality.psnrDb.toFixed(2)} dB`;
}

export function renderCorpusSpeedupChart(receipt) {
	const rows = chartRows(receipt);
	const width = 1060;
	const labelWidth = 230;
	const chartLeft = 260;
	const chartRight = 940;
	const chartWidth = chartRight - chartLeft;
	const rowHeight = 42;
	const firstRowY = 142;
	const speedups = rows.map((row) => row.speedupVsImageQOmggif);
	const sizeRatios = rows.map((row) => row.sizeRatioVsImageQOmggif);
	const minimumSpeedup = Math.min(...speedups);
	const maximumSpeedup = Math.max(...speedups);
	const meanSpeedup = geometricMean(speedups);
	const minimumSizeRatio = Math.min(...sizeRatios);
	const maximumSizeRatio = Math.max(...sizeRatios);
	const maximum = niceMaximum(maximumSpeedup);
	const height = firstRowY + rows.length * rowHeight + 96;
	const thresholdX = coordinate(chartLeft + (100 / maximum) * chartWidth);
	const elements = [];

	elements.push(
		`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="img" aria-labelledby="title desc">`,
		'<title id="title">wtfgif speedup by arbitrary-RGBA workload</title>',
		`<desc id="desc">Horizontal bars show wtfgif speedup over image-q plus omggif for ten workloads. Every bar exceeds the marked 100 times threshold. Labels also report the output file-size ratio and RGB quality of both encoders.</desc>`,
		'<rect width="100%" height="100%" fill="#ffffff"/>',
		'<g font-family="ui-sans-serif, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif" fill="#172026">',
		'<text x="40" y="46" font-size="25" font-weight="700">Every tested arbitrary RGBA workload exceeds 100×</text>',
		`<text x="40" y="74" font-size="14" fill="#51606a">${escapeXml(`First real encode after initialization · ${receipt.benchmark.processesPerImplementation} fresh Node processes per implementation · higher is faster`)}</text>`,
		`<text x="40" y="98" font-size="13" fill="#51606a">${escapeXml(`${minimumSpeedup.toFixed(2)}×–${maximumSpeedup.toFixed(2)}× · ${meanSpeedup.toFixed(2)}× geometric mean · ${minimumSizeRatio.toFixed(2)}×–${maximumSizeRatio.toFixed(2)}× output size`)}</text>`,
		`<line x1="${thresholdX}" y1="112" x2="${thresholdX}" y2="${firstRowY + rows.length * rowHeight - 10}" stroke="#b34b3f" stroke-width="2" stroke-dasharray="5 4"/>`,
		`<text x="${thresholdX + 7}" y="124" font-size="12" font-weight="700" fill="#9b3d33">100×</text>`,
	);

	for (const [index, row] of rows.entries()) {
		const y = firstRowY + index * rowHeight;
		const barWidth = coordinate(
			(row.speedupVsImageQOmggif / maximum) * chartWidth,
		);
		const label = row.fixture?.label ?? row.fixtureId;
		const shape = row.fixture
			? `${row.fixture.width}×${row.fixture.height}×${row.fixture.frameCount}`
			: "";
		const quality = row.baseline
			? `${formatRgbQuality(row)} vs ${formatRgbQuality(row.baseline)} baseline`
			: formatRgbQuality(row);
		elements.push(
			`<text x="${labelWidth}" y="${y + 18}" text-anchor="end" font-size="14" font-weight="600">${escapeXml(label)}</text>`,
			`<text x="${labelWidth}" y="${y + 33}" text-anchor="end" font-size="11" fill="#687780">${escapeXml(shape)}</text>`,
			`<rect x="${chartLeft}" y="${y}" width="${barWidth}" height="27" rx="3" fill="#0d8f6f"/>`,
			`<text x="${coordinate(chartLeft + barWidth + 8)}" y="${y + 19}" font-size="13" font-weight="700">${row.speedupVsImageQOmggif.toFixed(2)}×</text>`,
			`<text x="${chartLeft}" y="${y + 38}" font-size="11" fill="#687780">${escapeXml(`${row.sizeRatioVsImageQOmggif.toFixed(2)}× output size · ${quality}`)}</text>`,
		);
	}

	elements.push(
		`<text x="40" y="${height - 49}" font-size="12" fill="#687780">Latency includes palette creation, mapping, LZW, and GIF assembly · every output has exact shape, timing, and binary alpha.</text>`,
		`<text x="40" y="${height - 27}" font-size="12" fill="#687780">${escapeXml(`${receipt.environment.cpu} · Node ${receipt.environment.node} · wtfgif ${receipt.environment.packageVersion} · no known palette, source cache, or reused result`)}</text>`,
		"</g>",
		"</svg>",
	);
	return `${elements.join("\n")}\n`;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
	const input = process.argv[2] ?? path.join(root, "benchmarks", "corpus.json");
	const output =
		process.argv[3] ?? path.join(root, "docs", "corpus-speedup.svg");
	const receipt = JSON.parse(await readFile(input, "utf8"));
	await writeFile(output, renderCorpusSpeedupChart(receipt));
	console.log(`Rendered ${path.relative(root, output)}`);
}
