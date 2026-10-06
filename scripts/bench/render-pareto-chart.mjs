import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { root } from "../lib/paths.mjs";
import { coordinate, escapeXml, niceMaximum } from "./lib/svg.mjs";

const COLORS = { wtfgif: "#0d8f6f", sharp: "#3b6fd6", other: "#5c6b75" };
const FRONTIER_COLOR = "#b34b3f";

function family(id) {
	if (id.startsWith("wtfgif")) return "wtfgif";
	if (id.startsWith("sharp")) return "sharp";
	return "other";
}

function formatMs(value) {
	if (value < 10) return `${value.toFixed(2)} ms`;
	return `${value.toFixed(0)} ms`;
}

function formatQuality(row) {
	return row.quality.psnrDb === null
		? "lossless"
		: `${row.quality.psnrDb.toFixed(1)} dB`;
}

function overlaps(a, b) {
	return !(
		a.right <= b.left ||
		b.right <= a.left ||
		a.bottom <= b.top ||
		b.bottom <= a.top
	);
}

/**
 * Place each label in the first nearby slot that stays inside the plot and
 * clear of earlier labels, every marker, and the given line segments, trying
 * progressively farther offsets. Labels placed away from their marker get a
 * leader line.
 */
function placeLabels(points, bounds, segments) {
	const markers = points.map((point) => ({
		left: point.x - 8,
		right: point.x + 8,
		top: point.y - 8,
		bottom: point.y + 8,
	}));
	const obstacles = [
		...markers,
		...segments.map(([x1, y1, x2, y2]) => ({
			left: Math.min(x1, x2) - 2,
			right: Math.max(x1, x2) + 2,
			top: Math.min(y1, y2) - 2,
			bottom: Math.max(y1, y2) + 2,
		})),
	];
	const placed = [];
	const offsets = [];
	for (const distance of [12, 30, 52, 76, 102]) {
		for (const dy of [0, -1, 1, -2, 2]) {
			offsets.push([distance, dy * 20], [-distance, dy * 20]);
		}
		offsets.push([0, -distance - 10], [0, distance + 10]);
	}
	for (const point of points) {
		const width = Math.max(point.name.length * 7.4, point.detail.length * 5.9);
		const height = 30;
		for (const [dx, dy] of offsets) {
			const left =
				dx > 0
					? point.x + dx
					: dx < 0
						? point.x + dx - width
						: point.x - width / 2;
			const top = point.y + dy - height / 2;
			const box = { left, right: left + width, top, bottom: top + height };
			if (
				box.left < bounds.left ||
				box.right > bounds.right ||
				box.top < bounds.top ||
				box.bottom > bounds.bottom
			) {
				continue;
			}
			if (placed.some((other) => overlaps(box, other.box))) continue;
			if (obstacles.some((obstacle) => overlaps(box, obstacle))) continue;
			const gapX = Math.max(box.left - point.x, 0, point.x - box.right);
			const gapY = Math.max(box.top - point.y, 0, point.y - box.bottom);
			placed.push({ point, box, leader: Math.hypot(gapX, gapY) > 10 });
			break;
		}
	}
	return placed;
}

/** One sentence comparing wtfgif with the next-fastest and smallest encoders. */
function findings(rows, labels) {
	const wtfgif = rows.find((row) => row.implementation === "wtfgif");
	const others = rows.filter((row) => !row.implementation.startsWith("wtfgif"));
	const nextFastest = others.toSorted((a, b) => a.medianMs - b.medianMs)[0];
	const smallest = others.toSorted((a, b) => a.bytes - b.bytes)[0];
	const psnr = (row) => row.quality.psnrDb ?? Number.POSITIVE_INFINITY;
	const psnrGap = psnr(nextFastest) - psnr(wtfgif);
	const quality =
		Math.abs(psnrGap) < 0.05
			? "equal PSNR"
			: `${Math.abs(psnrGap).toFixed(1)} dB ${psnrGap > 0 ? "higher" : "lower"} PSNR`;
	return `wtfgif is ${(nextFastest.medianMs / wtfgif.medianMs).toFixed(0)}× faster than the next encoder, ${labels.get(nextFastest.implementation)}, whose GIF is ${(nextFastest.bytes / wtfgif.bytes).toFixed(2)}× the size with ${quality}. The smallest, ${labels.get(smallest.implementation)}, is ${(smallest.bytes / wtfgif.bytes).toFixed(2)}× the size at ${(smallest.medianMs / wtfgif.medianMs).toFixed(0)}× the time.`;
}

export function renderParetoChart(receipt) {
	const width = 1120;
	const height = 740;
	const plot = { left: 96, right: 1080, top: 140, bottom: 610 };
	const rows = receipt.results;
	const labels = new Map(
		receipt.implementations.map((value) => [value.id, value.label]),
	);
	const minimumDecade = Math.floor(
		Math.log10(Math.min(...rows.map((row) => row.medianMs))),
	);
	const maximumDecade = Math.ceil(
		Math.log10(Math.max(...rows.map((row) => row.medianMs))),
	);
	const maximumKiB = niceMaximum(
		Math.max(...rows.map((row) => row.bytes / 1024)) * 1.08,
	);
	const x = (ms) =>
		coordinate(
			plot.left +
				((Math.log10(ms) - minimumDecade) / (maximumDecade - minimumDecade)) *
					(plot.right - plot.left),
		);
	const y = (bytes) =>
		coordinate(
			plot.bottom - (bytes / 1024 / maximumKiB) * (plot.bottom - plot.top),
		);
	const points = rows.map((row) => ({
		row,
		x: x(row.medianMs),
		y: y(row.bytes),
		family: family(row.implementation),
		name: labels.get(row.implementation) ?? row.implementation,
		detail: `${formatMs(row.medianMs)} · ${(row.bytes / 1024).toFixed(1)} KiB · ${formatQuality(row)}`,
	}));
	const frontier = points
		.filter((point) => point.row.paretoOptimal)
		.toSorted((left, right) => left.row.medianMs - right.row.medianMs);
	const fixture = receipt.fixture;
	const elements = [];

	elements.push(
		`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="img" aria-labelledby="title desc">`,
		'<title id="title">GIF encoder speed against file size</title>',
		`<desc id="desc">${escapeXml(`Scatter plot of median first-encode time on a logarithmic axis against output size for ${rows.length} encoder configurations on the ${fixture.width}×${fixture.height}×${fixture.frameCount} MakeEmoji animation. A dashed step line marks the Pareto frontier: ${frontier.map((point) => point.name).join(", ")}. Labels give each configuration's time, size, and RGB PSNR.`)}</desc>`,
		'<rect width="100%" height="100%" fill="#ffffff"/>',
		'<g font-family="ui-sans-serif, -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif" fill="#172026">',
		'<text x="40" y="46" font-size="25" font-weight="700">Encode time against file size</text>',
		`<text x="40" y="74" font-size="14" fill="#51606a">${escapeXml(`${fixture.label}, ${fixture.width}×${fixture.height}×${fixture.frameCount} · first encode in ${receipt.benchmark.processesPerImplementation} fresh Node processes per encoder · down and left is better`)}</text>`,
		`<text x="40" y="96" font-size="13" fill="#172026">${escapeXml(findings(rows, labels))}</text>`,
		`<text x="40" y="116" font-size="12" fill="${FRONTIER_COLOR}">${escapeXml("Dashed line: Pareto frontier, where no other encoder is both faster and smaller · PSNR is RGB quality on opaque pixels, higher is better")}</text>`,
	);

	for (let decade = minimumDecade; decade <= maximumDecade; decade += 1) {
		const gridX = x(10 ** decade);
		const label =
			10 ** decade >= 1000 ? `${10 ** (decade - 3)} s` : `${10 ** decade} ms`;
		elements.push(
			`<line x1="${gridX}" y1="${plot.top}" x2="${gridX}" y2="${plot.bottom}" stroke="#e3e8eb"/>`,
			`<text x="${gridX}" y="${plot.bottom + 20}" text-anchor="middle" font-size="12" fill="#687780">${label}</text>`,
		);
	}
	const tickStep = maximumKiB / 5;
	for (let tick = 0; tick <= 5; tick += 1) {
		const gridY = y(tick * tickStep * 1024);
		elements.push(
			`<line x1="${plot.left}" y1="${gridY}" x2="${plot.right}" y2="${gridY}" stroke="#e3e8eb"/>`,
			`<text x="${plot.left - 10}" y="${coordinate(gridY + 4)}" text-anchor="end" font-size="12" fill="#687780">${Number((tick * tickStep).toFixed(1))} KiB</text>`,
		);
	}
	elements.push(
		`<text x="${coordinate((plot.left + plot.right) / 2)}" y="${plot.bottom + 44}" text-anchor="middle" font-size="13" fill="#51606a">Median first-encode time (logarithmic)</text>`,
		`<text x="30" y="${coordinate((plot.top + plot.bottom) / 2)}" text-anchor="middle" font-size="13" fill="#51606a" transform="rotate(-90 30 ${coordinate((plot.top + plot.bottom) / 2)})">Output size</text>`,
	);

	const sharpCurve = points
		.filter((point) => /^sharp-effort\d+$/.test(point.row.implementation))
		.toSorted(
			(left, right) =>
				Number(left.row.implementation.slice(12)) -
				Number(right.row.implementation.slice(12)),
		);
	if (sharpCurve.length > 1) {
		elements.push(
			`<polyline points="${sharpCurve.map((point) => `${point.x},${point.y}`).join(" ")}" fill="none" stroke="${COLORS.sharp}" stroke-width="1.5" stroke-opacity="0.45"/>`,
		);
	}
	// Step from each frontier point right to the next one's time, then down
	// to its size.
	const steps = frontier.length > 0 ? [[frontier[0].x, frontier[0].y]] : [];
	for (const point of frontier.slice(1)) {
		steps.push([point.x, steps.at(-1)[1]], [point.x, point.y]);
	}
	const frontierSegments = steps
		.slice(1)
		.map((step, index) => [...steps[index], ...step]);
	if (steps.length > 1) {
		elements.push(
			`<polyline points="${steps.map((step) => step.join(",")).join(" ")}" fill="none" stroke="${FRONTIER_COLOR}" stroke-width="2" stroke-dasharray="6 4"/>`,
		);
	}

	const placementOrder = points.toSorted(
		(left, right) =>
			Number(right.row.paretoOptimal) - Number(left.row.paretoOptimal) ||
			left.x - right.x,
	);
	const placed = placeLabels(
		placementOrder,
		{
			left: plot.left + 4,
			right: width - 12,
			top: plot.top + 2,
			bottom: plot.bottom - 2,
		},
		frontierSegments,
	);
	for (const { point, box, leader } of placed) {
		if (leader) {
			const anchorX = Math.min(Math.max(point.x, box.left), box.right);
			const anchorY = Math.min(Math.max(point.y, box.top), box.bottom);
			elements.push(
				`<line x1="${point.x}" y1="${point.y}" x2="${coordinate(anchorX)}" y2="${coordinate(anchorY)}" stroke="#9aa6ad" stroke-width="1"/>`,
			);
		}
	}
	for (const point of points) {
		const color = COLORS[point.family];
		elements.push(
			`<circle cx="${point.x}" cy="${point.y}" r="${point.family === "wtfgif" ? 7 : 6}" fill="${color}" stroke="#ffffff" stroke-width="1.5"/>`,
		);
	}
	for (const { point, box } of placed) {
		elements.push(
			`<text x="${coordinate(box.left)}" y="${coordinate(box.top + 12)}" font-size="13" font-weight="700" fill="${COLORS[point.family]}">${escapeXml(point.name)}</text>`,
			`<text x="${coordinate(box.left)}" y="${coordinate(box.top + 26)}" font-size="11" fill="#51606a">${escapeXml(point.detail)}</text>`,
		);
	}
	if (placed.length !== points.length) {
		throw new Error("Pareto chart could not place every label");
	}

	elements.push(
		`<text x="40" y="${height - 42}" font-size="12" fill="#687780">${escapeXml(`sharp may use a palette per frame; wtfgif uses one palette without dithering. ${receipt.benchmark.excluded}`)}</text>`,
		`<text x="40" y="${height - 22}" font-size="12" fill="#687780">${escapeXml(`${receipt.environment.cpu} · Node ${receipt.environment.node} · wtfgif ${receipt.environment.packageVersion} · raw samples in benchmarks/pareto.json`)}</text>`,
		"</g>",
		"</svg>",
	);
	return `${elements.join("\n")}\n`;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
	const input = process.argv[2] ?? path.join(root, "benchmarks", "pareto.json");
	const output =
		process.argv[3] ?? path.join(root, "docs", "encoder-pareto.svg");
	const receipt = JSON.parse(await readFile(input, "utf8"));
	await writeFile(output, renderParetoChart(receipt));
	console.log(`Rendered ${path.relative(root, output)}`);
}
