import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { root } from "../lib/paths.mjs";
import { geometricMean, median } from "./lib/metrics.mjs";
import {
	corpusChartRows,
	renderCorpusSpeedupChart,
} from "./render-corpus-speedup-chart.mjs";
import { renderEncoderRaceChart } from "./render-encoder-race-chart.mjs";
import { renderParetoChart } from "./render-pareto-chart.mjs";

const corpusPath = path.join(root, "benchmarks", "corpus.json");
const racePath = path.join(root, "benchmarks", "encoder-race.json");
const paretoPath = path.join(root, "benchmarks", "pareto.json");

function replaceBlock(source, id, content) {
	const start = `<!-- benchmark:${id}:start -->`;
	const end = `<!-- benchmark:${id}:end -->`;
	const startIndex = source.indexOf(start);
	const endIndex = source.indexOf(end, startIndex);
	if (startIndex < 0 || endIndex < 0) {
		throw new Error(`Document does not contain the ${id} benchmark block`);
	}
	return `${source.slice(0, startIndex)}${start}\n${content.trim()}\n${end}${source.slice(endIndex + end.length)}`;
}

function formatDirty(value) {
	return value ? "true" : "false";
}

function corpusSummary(receipt) {
	const rows = corpusChartRows(receipt);
	const stats = (values) => ({
		minimum: Math.min(...values),
		maximum: Math.max(...values),
		mean: geometricMean(values),
	});
	const wtfgifInitialization = receipt.results
		.filter((result) => result.implementation === "wtfgif")
		.map((result) => result.initializeMedianMs);
	const smallestRows = rows.map((row) => row.smallest);
	return {
		smallestSize: stats(smallestRows.map((row) => row.sizeRatioVsFastest)),
		smallestTime: stats(smallestRows.map((row) => row.timeRatioVsFastest)),
		smallestJavascriptSpeed: stats(
			smallestRows.map((row) => row.speedupVsImageQOmggif),
		),
		smallestNativeSpeed: stats(smallestRows.map((row) => row.speedupVsSharp)),
		smallestJavascriptSize: stats(
			smallestRows.map((row) => row.sizeRatioVsImageQOmggif),
		),
		smallestNativeSize: stats(smallestRows.map((row) => row.sizeRatioVsSharp)),
		makeEmoji: rows.find((row) => row.fixtureId === "makeemoji-real"),
		javascriptSpeed: stats(rows.map((row) => row.speedupVsImageQOmggif)),
		javascriptSize: stats(rows.map((row) => row.sizeRatioVsImageQOmggif)),
		nativeSpeed: stats(rows.map((row) => row.speedupVsSharp)),
		nativeSize: stats(rows.map((row) => row.sizeRatioVsSharp)),
		nativeHigherPsnr: rows.filter((row) => psnrOf(row.native) > psnrOf(row))
			.length,
		initializationMs: median(wtfgifInitialization),
		rows,
	};
}

function psnrOf(row) {
	return row.quality.psnrDb ?? Number.POSITIVE_INFINITY;
}

function times(value, digits = 0) {
	return `${value.toFixed(digits)}×`;
}

function readmeCorpus(receipt) {
	const values = corpusSummary(receipt);
	const {
		makeEmoji,
		javascriptSpeed,
		javascriptSize,
		nativeSpeed,
		nativeSize,
	} = values;
	return `On ${values.rows.length} arbitrary-RGBA workloads, wtfgif's first encode is **${times(javascriptSpeed.minimum)}–${times(javascriptSpeed.maximum)} faster**
than image-q + omggif (**${times(javascriptSpeed.mean)} geometric mean**) and **${times(nativeSpeed.minimum)}–${times(nativeSpeed.maximum)} faster** than
[sharp](https://sharp.pixelplumbing.com/), the native libvips encoder, at its fastest setting (**${times(nativeSpeed.mean)} geometric mean**).
The real ${makeEmoji.fixture.width}×${makeEmoji.fixture.height}×${makeEmoji.fixture.frameCount} MakeEmoji animation takes **${makeEmoji.medianMs.toFixed(2)} ms**, against
${makeEmoji.javascript.medianMs.toFixed(0)} ms for image-q + omggif and ${makeEmoji.native.medianMs.toFixed(0)} ms for sharp. wtfgif's files are ${javascriptSize.minimum.toFixed(2)}×–${javascriptSize.maximum.toFixed(2)}× the size of
image-q + omggif's (${javascriptSize.mean.toFixed(2)}× geometric mean) and ${nativeSize.minimum.toFixed(2)}×–${nativeSize.maximum.toFixed(2)}× the size of sharp's
(${nativeSize.mean.toFixed(2)}×). sharp, which can choose a palette for each frame, measures higher
RGB quality on ${values.nativeHigherPsnr} of the ${values.rows.length} workloads.

With \`mode: "smallest"\`, the same pixels take ${values.smallestSize.minimum.toFixed(2)}×–${values.smallestSize.maximum.toFixed(2)}× the bytes of the fastest
mode (**${values.smallestSize.mean.toFixed(2)}× geometric mean**) and ${values.smallestTime.mean.toFixed(1)}× its time. That is still **${times(values.smallestJavascriptSpeed.minimum)}–${times(values.smallestJavascriptSpeed.maximum)}**
faster than image-q + omggif and **${times(values.smallestNativeSpeed.minimum)}–${times(values.smallestNativeSpeed.maximum)}** faster than sharp, with files
${values.smallestJavascriptSize.mean.toFixed(2)}× and ${values.smallestNativeSize.mean.toFixed(2)}× their size (geometric means). MakeEmoji takes
${makeEmoji.smallest.medianMs.toFixed(2)} ms and ${(makeEmoji.smallest.bytes / 1024).toFixed(0)} KiB.

![wtfgif speedup over image-q + omggif and sharp on ten RGBA workloads](docs/corpus-speedup.svg)

Measured on an ${receipt.environment.cpu} with Node.js ${receipt.environment.node.replace(/^v/, "")}: ${receipt.benchmark.processesPerImplementation} fresh processes per
workload and library, wtfgif ${receipt.environment.packageVersion} at \`${receipt.environment.commit.slice(0, 7)}\`. Raw samples are in
[\`benchmarks/corpus.json\`](benchmarks/corpus.json).`;
}

function readmeStartup(receipt) {
	const values = corpusSummary(receipt);
	const { makeEmoji } = values;
	const initialization = values.initializationMs;
	return `- **Startup work.** \`initializeWasmGlobally()\` loads WebAssembly and runs fixed
  synthetic encodes so that your first real encode is fast. Loading the package
  and initializing took **${initialization.toFixed(0)} ms** (median across ${values.rows.length * receipt.benchmark.processesPerImplementation} fresh processes),
  once per process and outside the timed encodes above. Do it at startup, not
  right before your first GIF. If each process encodes a single GIF, as in a
  cold serverless start, compare ${(initialization + makeEmoji.medianMs).toFixed(0)} ms with image-q + omggif's
  ${makeEmoji.javascript.medianMs.toFixed(0)} ms for MakeEmoji, not ${makeEmoji.medianMs.toFixed(2)} ms.`;
}

function corpusReceipt(receipt) {
	const values = corpusSummary(receipt);
	const rows = values.rows.map(
		(row) =>
			`| ${row.fixture.label} | ${row.fixture.width}×${row.fixture.height}×${row.fixture.frameCount} | ${row.medianMs.toFixed(3)} ms | ${row.javascript.medianMs.toFixed(3)} ms | ${row.native.medianMs.toFixed(3)} ms | **${row.speedupVsImageQOmggif.toFixed(2)}×** | **${row.speedupVsSharp.toFixed(2)}×** | ${row.sizeRatioVsImageQOmggif.toFixed(2)}× | ${row.sizeRatioVsSharp.toFixed(2)}× |`,
	);
	const sorted = values.rows.toSorted(
		(left, right) => left.speedupVsImageQOmggif - right.speedupVsImageQOmggif,
	);
	const quality = values.rows.map(
		(row) =>
			`| ${row.fixture.label} | ${formatPsnr(row)} | ${formatPsnr(row.javascript)} | ${formatPsnr(row.native)} | ${row.quality.ssimBlackComposite.toFixed(4)} | ${row.javascript.quality.ssimBlackComposite.toFixed(4)} | ${row.native.quality.ssimBlackComposite.toFixed(4)} |`,
	);
	const { javascriptSpeed, javascriptSize, nativeSpeed, nativeSize } = values;
	return `The receipt records package version ${receipt.environment.packageVersion} at source commit
\`${receipt.environment.commit}\`. Its dirty flag is ${formatDirty(receipt.environment.dirty)}. The
benchmark command rebuilds both scalar and SIMD quality Wasm before bundling,
so the timed artifact comes from that recorded source commit.

| Fixture | Shape | wtfgif | image-q + omggif | sharp | vs image-q + omggif | vs sharp | Size vs image-q + omggif | Size vs sharp |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
${rows.join("\n")}

Against image-q + omggif the observed range is ${javascriptSpeed.minimum.toFixed(2)}×–${javascriptSpeed.maximum.toFixed(2)}×, with a ${javascriptSpeed.mean.toFixed(2)}× geometric-mean
speedup, and wtfgif's files are ${javascriptSize.minimum.toFixed(2)}×–${javascriptSize.maximum.toFixed(2)}× the size (${javascriptSize.mean.toFixed(2)}× geometric mean). Against
sharp the range is ${nativeSpeed.minimum.toFixed(2)}×–${nativeSpeed.maximum.toFixed(2)}× (${nativeSpeed.mean.toFixed(2)}× geometric mean), and wtfgif's files are
${nativeSize.minimum.toFixed(2)}×–${nativeSize.maximum.toFixed(2)}× the size (${nativeSize.mean.toFixed(2)}× geometric mean). This is the library's intended
tradeoff: encode latency takes priority over compression ratio and palette
quality.

### Smallest mode

| Fixture | Smallest | Bytes | Size vs fastest | Time vs fastest | vs image-q + omggif | vs sharp | Size vs image-q + omggif | Size vs sharp |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
${values.rows
	.map(
		(row) =>
			`| ${row.fixture.label} | ${row.smallest.medianMs.toFixed(3)} ms | ${row.smallest.bytes.toLocaleString("en-US")} | ${row.smallest.sizeRatioVsFastest.toFixed(2)}× | ${row.smallest.timeRatioVsFastest.toFixed(2)}× | **${row.smallest.speedupVsImageQOmggif.toFixed(2)}×** | **${row.smallest.speedupVsSharp.toFixed(2)}×** | ${row.smallest.sizeRatioVsImageQOmggif.toFixed(2)}× | ${row.smallest.sizeRatioVsSharp.toFixed(2)}× |`,
	)
	.join("\n")}

The smallest mode decodes to the same pixels as the fastest mode; it also codes
each image with full-dictionary LZW and keeps the shorter stream, so it is
never larger. Its files are ${values.smallestSize.minimum.toFixed(2)}×–${values.smallestSize.maximum.toFixed(2)}× the fastest mode's (${values.smallestSize.mean.toFixed(2)}× geometric
mean) at ${values.smallestTime.minimum.toFixed(2)}×–${values.smallestTime.maximum.toFixed(2)}× its time (${values.smallestTime.mean.toFixed(2)}×).

### Quality

| Fixture | wtfgif PSNR | image-q + omggif PSNR | sharp PSNR | wtfgif SSIM | image-q + omggif SSIM | sharp SSIM |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
${quality.join("\n")}

sharp measures higher RGB PSNR than wtfgif on ${values.nativeHigherPsnr} of the ${values.rows.length} workloads. It can
choose a separate palette for each frame, while wtfgif builds one palette for
the whole animation. wtfgif keeps every source-opaque color exactly when the
animation fits in one palette ("lossless" above).

These results describe the ${values.rows.length} committed fixtures on the recorded ${receipt.environment.cpu}
and Node.js ${receipt.environment.node} runtime. They do not claim codec-only performance, equal
output size, or unmeasured hardware and runtimes.

${values.rows.every((row) => row.speedupVsImageQOmggif > 100) ? "Every category exceeds" : `${values.rows.filter((row) => row.speedupVsImageQOmggif > 100).length} of ${values.rows.length} categories exceed`} 100× over image-q + omggif on its first real encode after
initialization. The ${sorted[0].fixture.label.toLowerCase()} fixture has the narrowest margin at ${sorted[0].speedupVsImageQOmggif.toFixed(2)}×, followed by
${sorted[1].fixture.label.toLowerCase()} at ${sorted[1].speedupVsImageQOmggif.toFixed(2)}×. No result depends on a known palette, source cache,
previous result, or reduced-quality mode.

![wtfgif speedup across the arbitrary-RGBA corpus](docs/corpus-speedup.svg)

The chart comes from the same receipt as the tables. Run \`npm run bench:charts\`
to generate it. Bar length is speedup on a logarithmic axis. The labels give all
three median encode times, wtfgif's output size relative to each baseline, and
source-relative RGB quality for every encoder. "Lossless" means every
source-opaque RGB pixel survived palette mapping exactly. Shape, frame timing,
and binary alpha must be exact on every fixture regardless of that RGB label.

The JavaScript baseline uses image-q \`rgbquant\` palette generation and
nearest-color mapping followed by omggif LZW. The native baseline is sharp
${receipt.environment.runtimes["sharp-effort1"]?.sharp ?? ""} with libvips ${receipt.environment.runtimes["sharp-effort1"]?.vips ?? ""} at \`gif({ effort: 1, dither: 0 })\`, its
fastest setting, without dithering to match wtfgif. sharp receives RGBA with
alpha already thresholded, because it has no threshold option; that pass is
inside its timed call. Each sharp process first encodes a fixed 2×2 animation
so that libvips' one-time saver and thread-pool setup stays outside the clock,
as wtfgif's initialization does. wtfgif uses its global quality quantizer,
run-aware LZW codes, and changed-rectangle frames. The algorithms can select
different indexed pixels, so the receipt reports output bytes, opaque-source
RGB PSNR, and per-frame SSIM after binary alpha compositing against black.
Every output is decoded and checked for shape, frame count, delays, and exact
binary alpha before it is accepted.

[\`benchmarks/corpus.json\`](benchmarks/corpus.json) records all raw samples,
medians, p95 values, initialization medians, output hashes, quality values,
fixture hashes and provenance, package-lock hash, commit, dirty state, and
runtime environment. The corpus covers real small images, photographic content,
flat pixel art, gradients, noise, transparency, disjoint frame palettes, similar
adjacent frames, tiny animations, and a one-megapixel workload. Add the
optional three-megapixel fixture with \`npm run bench:corpus:stress\`.`;
}

function formatPsnr(row) {
	return row.quality.psnrDb === null
		? "lossless"
		: `${row.quality.psnrDb.toFixed(2)} dB`;
}

function paretoRows(receipt) {
	const labels = new Map(
		receipt.implementations.map((value) => [value.id, value.label]),
	);
	return receipt.results
		.toSorted((left, right) => left.medianMs - right.medianMs)
		.map((row) => ({ ...row, label: labels.get(row.implementation) }));
}

function readmePareto(receipt) {
	const rows = paretoRows(receipt);
	const frontier = rows.filter((row) => row.paretoOptimal);
	const inexact = rows.filter(
		(row) => (row.quality.alphaAgreementPercent ?? 100) < 99.95,
	);
	const list = (values) =>
		new Intl.ListFormat("en", { type: "conjunction" }).format(values);
	return `![Encode time against file size for GIF encoders that run in Node](docs/encoder-pareto.svg)

The same MakeEmoji animation through every encoder that runs in Node: pure
JavaScript libraries, sharp at four effort levels, and Wasm builds of gifski,
libvips, ImageMagick, and FFmpeg. ${list(inexact.map((row) => `${row.label} (${row.quality.alphaAgreementPercent.toFixed(0)}%)`))} get some pixels' transparency wrong. On the frontier, where no other encoder is both
faster and smaller: ${new Intl.ListFormat("en", { type: "conjunction" }).format(frontier.map((row) => row.label))}. Labels give RGB
PSNR, since size alone does not show palette quality. Medians of ${receipt.benchmark.processesPerImplementation} fresh processes per
encoder; raw data in [\`benchmarks/pareto.json\`](benchmarks/pareto.json). gif.js
needs browser workers, so it appears only in the Chrome comparison below.`;
}

function paretoReceipt(receipt) {
	const rows = paretoRows(receipt);
	const table = rows.map(
		(row) =>
			`| ${row.paretoOptimal ? `**${row.label}**` : row.label} | ${row.medianMs.toFixed(3)} ms | ${row.bytes.toLocaleString("en-US")} | ${formatPsnr(row)} | ${row.quality.ssimBlackComposite.toFixed(4)} | ${Number((row.quality.alphaAgreementPercent ?? 100).toFixed(2))}% | ${row.paretoOptimal ? "yes" : ""} |`,
	);
	const configurations = receipt.implementations.map(
		(value) => `- **${value.label}**: ${value.configuration}.`,
	);
	return `The receipt records wtfgif ${receipt.environment.packageVersion} at ${receipt.environment.dirty ? "dirty" : "clean"} commit
\`${receipt.environment.commit}\` and uses the corpus timing boundary above: one
first encode of the ${receipt.fixture.label} (${receipt.fixture.width}×${receipt.fixture.height}×${receipt.fixture.frameCount}) per fresh Node process,
${receipt.benchmark.processesPerImplementation} processes per configuration.

| Configuration | Median | Bytes | PSNR | SSIM | Alpha match | Frontier |
| --- | ---: | ---: | ---: | ---: | ---: | :---: |
${table.join("\n")}

${configurations.join("\n")}

A configuration is on the frontier when no other is at least as fast and at
least as small, and strictly better in one of them. Libraries without an
alpha-threshold option receive alpha already thresholded inside their timed
call, and gif-encoder-2 and gifencoder, which cannot store alpha, mark
transparent pixels with a color key. Each Wasm library encodes a fixed 2×2
animation during initialization, as wtfgif and sharp do. The PSNR here counts
a pixel decoded transparent where the source is opaque as black.
${receipt.benchmark.excluded} FFmpeg (GPL-2.0) and gifski (AGPL-3.0) are
development dependencies of this benchmark only; the published package does
not include or link them.
Run \`npm run bench:pareto\` to measure it and \`npm run bench:charts\` to render
[\`docs/encoder-pareto.svg\`](docs/encoder-pareto.svg).`;
}

function sortedRaceRows(receipt) {
	return receipt.results.toSorted(
		(left, right) => left.medianMs - right.medianMs,
	);
}

function formatKilobytes(bytes) {
	return `${(bytes / 1024).toFixed(0)} KiB`;
}

function readmeBrowser(receipt) {
	const rows = sortedRaceRows(receipt);
	const wtfgif = rows.find((row) => row.id === "wtfgif");
	const wtfgifSmallest = rows.find((row) => row.id === "wtfgifSmallest");
	const competitors = rows.filter((row) => !row.id.startsWith("wtfgif"));
	const smallest = Math.min(...competitors.map((row) => row.bytes));
	const largest = Math.max(...competitors.map((row) => row.bytes));
	return `![Encode time in Chrome for wtfgif's two modes and five other GIF encoders](docs/encoder-race.svg)

Encoding the same ${receipt.fixture.frames}-frame ${receipt.fixture.width}×${receipt.fixture.height} MakeEmoji animation through each library's
public API in ${receipt.environment.browser.replace(/^Google /, "")}, wtfgif took **${wtfgif.medianMs.toFixed(2)} ms** (${formatKilobytes(wtfgif.bytes)}) in its fastest mode and
**${wtfgifSmallest.medianMs.toFixed(2)} ms** (${formatKilobytes(wtfgifSmallest.bytes)}) in its smallest mode. The other five took
${Math.min(...competitors.map((row) => row.medianMs)).toFixed(0)}–${Math.max(...competitors.map((row) => row.medianMs)).toFixed(0)} ms (**${Math.min(...competitors.map((row) => row.slowerThanWtfgif)).toFixed(0)}×–${Math.max(...competitors.map((row) => row.slowerThanWtfgif)).toFixed(0)}× slower than the fastest mode**)
and wrote ${formatKilobytes(smallest)}–${formatKilobytes(largest)}. Medians of ${receipt.environment.iterations} fresh browser processes
per encoder; raw data in [\`benchmarks/encoder-race.json\`](benchmarks/encoder-race.json).`;
}

function browserVersion(row) {
	return row.id === "omggif" ? `2.1.2 + ${row.version}` : row.version;
}

function browserReceipt(receipt) {
	const rows = sortedRaceRows(receipt);
	const wtfgif = rows.find((row) => row.id === "wtfgif");
	const tableRows = rows.map((row) => {
		const ours = row.id.startsWith("wtfgif");
		const name = ours ? `**${row.label}**` : row.label;
		const median = ours
			? `**${row.medianMs.toFixed(3)} ms**`
			: `${row.medianMs.toFixed(3)} ms`;
		const advantage = ours ? "—" : `**${row.slowerThanWtfgif.toFixed(2)}×**`;
		return `| ${name} | ${browserVersion(row)} | ${median} | ${advantage} | ${row.bytes.toLocaleString("en-US")} | ${row.psnrDb.toFixed(2)} dB | ${Number(row.alphaAccuracyPercent.toFixed(2))}% |`;
	});
	return `This is public-API time-to-result. It is not a codec-kernel
microbenchmark or an equal-file-size comparison. The gif.js and
gif.js.optimized APIs create workers when \`render()\` begins, so worker creation
is inside their timed jobs. The README chart uses bar length for median encode
time. It marks 100× wtfgif latency with a dashed reference line. It also gives
each encoder's emitted GIF size, relative slowdown, PSNR, and alpha agreement.

| Implementation | Version | Median | Slower than wtfgif fastest | Bytes | PSNR | Alpha match |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
${tableRows.join("\n")}

The browser receipt records wtfgif ${receipt.environment.packageVersion} at ${receipt.environment.dirty ? "dirty" : "clean"} commit
\`${receipt.environment.commit}\`, together with the package-lock
hash and complete runtime environment. wtfgif's median is ${wtfgif.medianMs.toFixed(3)} ms.

Every output must parse as an ${receipt.fixture.frames}-frame ${receipt.fixture.width}×${receipt.fixture.height} animation with exact 100 ms
delays before its sample is accepted. The validator composites all frames,
measures RGB PSNR on source-opaque pixels, and measures binary alpha agreement
over every pixel. Raw timings and interquartile values are committed in
[\`benchmarks/encoder-race.json\`](benchmarks/encoder-race.json). The README chart
comes from that receipt through \`scripts/bench/render-encoder-race-chart.mjs\`.`;
}

const [corpus, race, pareto, readmeSource, benchmarksSource] =
	await Promise.all([
		readFile(corpusPath, "utf8").then(JSON.parse),
		readFile(racePath, "utf8").then(JSON.parse),
		readFile(paretoPath, "utf8").then(JSON.parse),
		readFile(path.join(root, "README.md"), "utf8"),
		readFile(path.join(root, "BENCHMARKS.md"), "utf8"),
	]);

const readmeBlocks = [
	["readme-corpus", readmeCorpus(corpus)],
	["readme-startup", readmeStartup(corpus)],
	["readme-pareto", readmePareto(pareto)],
	["readme-browser", readmeBrowser(race)],
];
const benchmarkBlocks = [
	["corpus-receipt", corpusReceipt(corpus)],
	["pareto-receipt", paretoReceipt(pareto)],
	["browser-receipt", browserReceipt(race)],
];
const readme = readmeBlocks.reduce(
	(source, [id, content]) => replaceBlock(source, id, content),
	readmeSource,
);
const benchmarks = benchmarkBlocks.reduce(
	(source, [id, content]) => replaceBlock(source, id, content),
	benchmarksSource,
);

await Promise.all([
	writeFile(path.join(root, "README.md"), readme),
	writeFile(path.join(root, "BENCHMARKS.md"), benchmarks),
	writeFile(
		path.join(root, "docs", "corpus-speedup.svg"),
		renderCorpusSpeedupChart(corpus),
	),
	writeFile(
		path.join(root, "docs", "encoder-race.svg"),
		renderEncoderRaceChart(race),
	),
	writeFile(
		path.join(root, "docs", "encoder-pareto.svg"),
		renderParetoChart(pareto),
	),
]);

console.log("Updated benchmark text and charts from committed receipts");
