import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { root } from "../lib/paths.mjs";
import { geometricMean } from "./lib/metrics.mjs";
import { coordinate, escapeXml } from "./lib/svg.mjs";

const JAVASCRIPT_COLOR = "#0d8f6f";
const NATIVE_COLOR = "#3b6fd6";

export function corpusChartRows(receipt) {
	const fixtures = new Map(
		receipt.corpus.map((fixture) => [fixture.id, fixture]),
	);
	const { javascript, native } = receipt.benchmark.baselines;
	const resultFor = (implementation, fixtureId) =>
		receipt.results.find(
			(result) =>
				result.implementation === implementation &&
				result.fixtureId === fixtureId,
		);
	return receipt.results
		.filter((result) => result.implementation === "wtfgif")
		.map((result) => ({
			...result,
			javascript: resultFor(javascript, result.fixtureId),
			native: resultFor(native, result.fixtureId),
			smallest: resultFor("wtfgif-smallest", result.fixtureId),
			fixture: fixtures.get(result.fixtureId),
		}));
}

function formatRgbQuality(result) {
	if (result.quality.losslessOpaqueRgb) return "lossless";
	return `${result.quality.psnrDb.toFixed(1)} dB`;
}

function formatMs(value) {
	return value < 10 ? `${value.toFixed(3)} ms` : `${value.toFixed(1)} ms`;
}

function range(values) {
	return `${Math.min(...values).toFixed(0)}×–${Math.max(...values).toFixed(0)}× (${geometricMean(values).toFixed(0)}× geometric mean)`;
}

export function renderCorpusSpeedupChart(receipt) {
	const rows = corpusChartRows(receipt);
	const width = 1120;
	const labelWidth = 220;
	const chartLeft = 250;
	const chartRight = 1000;
	const chartWidth = chartRight - chartLeft;
	const rowHeight = 108;
	const firstRowY = 170;
	const height = firstRowY + rows.length * rowHeight + 80;
	const javascriptSpeedups = rows.map((row) => row.speedupVsImageQOmggif);
	const nativeSpeedups = rows.map((row) => row.speedupVsSharp);
	// Log axis from 1× to the next power of ten above the largest speedup.
	const decades = Math.ceil(Math.log10(Math.max(...javascriptSpeedups)));
	const x = (speedup) =>
		coordinate(chartLeft + (Math.log10(speedup) / decades) * chartWidth);
	const javascriptLabel = "image-q + omggif";
	const nativeLabel = "sharp (libvips)";
	const elements = [];

	elements.push(
		`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="img" aria-labelledby="title desc">`,
		'<title id="title">wtfgif speedup over a JavaScript and a native GIF encoder</title>',
		`<desc id="desc">${escapeXml(`For ${rows.length} RGBA workloads, paired bars on a logarithmic axis show how many times faster wtfgif's first encode in its fastest mode is than image-q plus omggif and than sharp. Labels give all three median encode times, wtfgif's output size relative to each, each encoder's RGB quality, and wtfgif's smallest mode time and size.`)}</desc>`,
		'<rect width="100%" height="100%" fill="#ffffff"/>',
		'<g font-family="ui-sans-serif, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif" fill="#172026">',
		'<text x="40" y="46" font-size="25" font-weight="700">wtfgif fastest-mode speedup on 10 RGBA workloads</text>',
		`<text x="40" y="74" font-size="14" fill="#51606a">${escapeXml(`Cold-cache first real encode after initialization · ${receipt.benchmark.processesPerImplementation} fresh Node processes per encoder · logarithmic axis, longer is faster`)}</text>`,
		`<rect x="40" y="92" width="14" height="14" rx="2" fill="${JAVASCRIPT_COLOR}"/>`,
		`<text x="62" y="104" font-size="13">${escapeXml(`vs ${javascriptLabel} (JavaScript): ${range(javascriptSpeedups)}`)}</text>`,
		`<rect x="40" y="114" width="14" height="14" rx="2" fill="${NATIVE_COLOR}"/>`,
		`<text x="62" y="126" font-size="13">${escapeXml(`vs ${nativeLabel}, effort 1, no dither (native): ${range(nativeSpeedups)}`)}</text>`,
	);

	for (let decade = 0; decade <= decades; decade += 1) {
		const gridX = x(10 ** decade);
		elements.push(
			`<line x1="${gridX}" y1="${firstRowY - 18}" x2="${gridX}" y2="${firstRowY + rows.length * rowHeight - 14}" stroke="#e3e8eb" stroke-width="1"/>`,
			`<text x="${gridX}" y="${firstRowY - 24}" text-anchor="middle" font-size="11" fill="#687780">${10 ** decade}×</text>`,
		);
	}

	for (const [index, row] of rows.entries()) {
		const y = firstRowY + index * rowHeight;
		const label = row.fixture?.label ?? row.fixtureId;
		const shape = row.fixture
			? `${row.fixture.width}×${row.fixture.height}×${row.fixture.frameCount}`
			: "";
		const bars = [
			[row.speedupVsImageQOmggif, JAVASCRIPT_COLOR, y],
			[row.speedupVsSharp, NATIVE_COLOR, y + 22],
		];
		elements.push(
			`<text x="${labelWidth}" y="${y + 18}" text-anchor="end" font-size="14" font-weight="600">${escapeXml(label)}</text>`,
			`<text x="${labelWidth}" y="${y + 34}" text-anchor="end" font-size="11" fill="#687780">${escapeXml(shape)}</text>`,
		);
		for (const [speedup, color, barY] of bars) {
			const barWidth = Math.max(2, x(speedup) - chartLeft);
			elements.push(
				`<rect x="${chartLeft}" y="${barY}" width="${coordinate(barWidth)}" height="18" rx="3" fill="${color}"/>`,
				`<text x="${coordinate(chartLeft + barWidth + 8)}" y="${barY + 14}" font-size="12" font-weight="700">${speedup.toFixed(1)}×</text>`,
			);
		}
		elements.push(
			`<text x="${chartLeft}" y="${y + 58}" font-size="11" fill="#51606a">${escapeXml(`${formatMs(row.medianMs)} wtfgif fastest · ${formatMs(row.javascript.medianMs)} ${javascriptLabel} · ${formatMs(row.native.medianMs)} sharp`)}</text>`,
			`<text x="${chartLeft}" y="${y + 73}" font-size="11" fill="#687780">${escapeXml(`fastest size ${row.sizeRatioVsImageQOmggif.toFixed(2)}× ${javascriptLabel}, ${row.sizeRatioVsSharp.toFixed(2)}× sharp · RGB ${formatRgbQuality(row)} wtfgif, ${formatRgbQuality(row.javascript)} ${javascriptLabel}, ${formatRgbQuality(row.native)} sharp`)}</text>`,
			`<text x="${chartLeft}" y="${y + 88}" font-size="11" fill="#0b6e55">${escapeXml(`smallest mode ${formatMs(row.smallest.medianMs)}, ${row.smallest.sizeRatioVsFastest.toFixed(2)}× fastest's size · ${row.smallest.sizeRatioVsImageQOmggif.toFixed(2)}× ${javascriptLabel}, ${row.smallest.sizeRatioVsSharp.toFixed(2)}× sharp · same pixels`)}</text>`,
		);
	}

	elements.push(
		`<text x="40" y="${height - 49}" font-size="12" fill="#687780">Latency includes palette creation, mapping, LZW, and GIF assembly · every output has exact shape, timing, and binary alpha · sharp may use a palette per frame.</text>`,
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
