import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { root } from "../lib/paths.mjs";
import { geometricMean } from "./lib/metrics.mjs";
import { renderCorpusSpeedupChart } from "./render-corpus-speedup-chart.mjs";
import { renderEncoderRaceChart } from "./render-encoder-race-chart.mjs";

const corpusPath = path.join(root, "benchmarks", "corpus.json");
const racePath = path.join(root, "benchmarks", "encoder-race.json");

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

function corpusRows(receipt) {
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

function corpusSummary(receipt) {
	const rows = corpusRows(receipt);
	const speedups = rows.map((row) => row.speedupVsImageQOmggif);
	const sizeRatios = rows.map((row) => row.sizeRatioVsImageQOmggif);
	const makeEmoji = rows.find((row) => row.fixtureId === "makeemoji-real");
	return {
		makeEmoji,
		maximumSize: Math.max(...sizeRatios),
		maximumSpeed: Math.max(...speedups),
		meanSize: geometricMean(sizeRatios),
		meanSpeed: geometricMean(speedups),
		minimumSize: Math.min(...sizeRatios),
		minimumSpeed: Math.min(...speedups),
		rows,
	};
}

function sizeRange(values) {
	return `${values.minimumSize.toFixed(2)}×–${values.maximumSize.toFixed(2)}× the size of image-q + omggif's, with a ${values.meanSize.toFixed(2)}× geometric mean`;
}

function readmeCorpus(receipt) {
	const values = corpusSummary(receipt);
	const makeEmoji = values.makeEmoji;
	return `On ${values.rows.length} arbitrary-RGBA workloads, wtfgif's first encode is **${values.minimumSpeed.toFixed(0)}×–${values.maximumSpeed.toFixed(0)}× faster**
than image-q + omggif, with a **${values.meanSpeed.toFixed(0)}× geometric mean**. The real ${makeEmoji.fixture.width}×${makeEmoji.fixture.height}×${makeEmoji.fixture.frameCount}
MakeEmoji animation takes **${makeEmoji.medianMs.toFixed(2)} ms** instead of ${makeEmoji.baseline.medianMs.toFixed(0)} ms. wtfgif's files are
${sizeRange(values)}.

![wtfgif speedup over image-q + omggif on ten RGBA workloads](docs/corpus-speedup.svg)

Measured with ${receipt.benchmark.processesPerImplementation} fresh Node ${receipt.environment.node} processes per workload on an ${receipt.environment.cpu}, wtfgif
${receipt.environment.packageVersion} (\`${receipt.environment.commit.slice(0, 7)}\`). The raw samples are in
[\`benchmarks/corpus.json\`](benchmarks/corpus.json).`;
}

function corpusReceipt(receipt) {
	const values = corpusSummary(receipt);
	const rows = values.rows.map(
		(row) =>
			`| ${row.fixture.label} | ${row.fixture.width}×${row.fixture.height}×${row.fixture.frameCount} | ${row.medianMs.toFixed(3)} ms | ${row.baseline.medianMs.toFixed(3)} ms | **${row.speedupVsImageQOmggif.toFixed(2)}×** | ${row.sizeRatioVsImageQOmggif.toFixed(2)}× |`,
	);
	const sorted = values.rows.toSorted(
		(left, right) => left.speedupVsImageQOmggif - right.speedupVsImageQOmggif,
	);
	return `The receipt records package version ${receipt.environment.packageVersion} at source commit
\`${receipt.environment.commit}\`. Its dirty flag is ${formatDirty(receipt.environment.dirty)}. The
benchmark command rebuilds both scalar and SIMD quality Wasm before bundling,
so the timed artifact comes from that recorded source commit.

| Fixture | Shape | wtfgif | image-q + omggif | Speedup | File-size ratio |
| --- | ---: | ---: | ---: | ---: | ---: |
${rows.join("\n")}

The observed range is ${values.minimumSpeed.toFixed(2)}×–${values.maximumSpeed.toFixed(2)}×, with a ${values.meanSpeed.toFixed(2)}× geometric-mean speedup.
wtfgif's files are ${sizeRange(values)}.
This is the library's intended tradeoff: encode latency takes priority over
compression ratio.

These results describe the ${values.rows.length} committed fixtures on the recorded ${receipt.environment.cpu}
and Node.js ${receipt.environment.node} runtime. They do not claim codec-only performance, equal
output size, or unmeasured hardware and runtimes.

${values.rows.every((row) => row.speedupVsImageQOmggif > 100) ? "Every category exceeds" : `${values.rows.filter((row) => row.speedupVsImageQOmggif > 100).length} of ${values.rows.length} categories exceed`} 100× on its first real encode after
initialization. The ${sorted[0].fixture.label.toLowerCase()} fixture has the narrowest margin at ${sorted[0].speedupVsImageQOmggif.toFixed(2)}×, followed by
${sorted[1].fixture.label.toLowerCase()} at ${sorted[1].speedupVsImageQOmggif.toFixed(2)}×. No result depends on a known palette, source cache,
previous result, or reduced-quality mode.

![wtfgif speedup across the arbitrary-RGBA corpus](docs/corpus-speedup.svg)

The chart comes from the same receipt as the table. Run \`npm run bench:charts\`
to generate it. Bar length is speedup over image-q + omggif. The labels give
both median encode times, the output file-size ratio, and source-relative RGB
quality for both encoders. "Lossless RGB" means every source-opaque RGB pixel
survived palette mapping exactly. Shape, frame timing, and binary alpha must be
exact on every fixture regardless of that RGB label.

The baseline uses image-q \`rgbquant\` palette generation and nearest-color
mapping followed by omggif LZW. wtfgif uses its global quality quantizer,
fixed-width run-aware LZW codes, and changed-rectangle frames. The algorithms
can select different indexed pixels, so the receipt
reports output bytes, opaque-source RGB PSNR, and per-frame SSIM after binary
alpha compositing against black. Every output is decoded and checked for shape,
frame count, delays, and exact binary alpha before it is accepted.

[\`benchmarks/corpus.json\`](benchmarks/corpus.json) records all raw samples,
medians, p95 values, output hashes, quality values, fixture hashes and
provenance, package-lock hash, commit, dirty state, and runtime environment.
The corpus covers real small images, photographic content, flat pixel art,
gradients, noise, transparency, disjoint frame palettes, similar adjacent
frames, tiny animations, and a one-megapixel workload. Add the optional
three-megapixel fixture with \`npm run bench:corpus:stress\`.`;
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
	const competitors = rows.filter((row) => row.id !== "wtfgif");
	const smallest = Math.min(...competitors.map((row) => row.bytes));
	const largest = Math.max(...competitors.map((row) => row.bytes));
	return `![Encode time in Chrome for six GIF encoders](docs/encoder-race.svg)

Encoding the same ${receipt.fixture.frames}-frame ${receipt.fixture.width}×${receipt.fixture.height} MakeEmoji animation through each library's
public API in ${receipt.environment.browser.replace(/^Google /, "")}, wtfgif took **${wtfgif.medianMs.toFixed(2)} ms**. The other five took
${Math.min(...competitors.map((row) => row.medianMs)).toFixed(0)}–${Math.max(...competitors.map((row) => row.medianMs)).toFixed(0)} ms (**${Math.min(...competitors.map((row) => row.slowerThanWtfgif)).toFixed(0)}×–${Math.max(...competitors.map((row) => row.slowerThanWtfgif)).toFixed(0)}× slower**). wtfgif's GIF was
${formatKilobytes(wtfgif.bytes)}; theirs were ${formatKilobytes(smallest)}–${formatKilobytes(largest)}. Medians of ${receipt.environment.iterations} fresh browser processes
per encoder; raw data in [\`benchmarks/encoder-race.json\`](benchmarks/encoder-race.json).`;
}

function browserVersion(row) {
	return row.id === "omggif" ? `2.1.2 + ${row.version}` : row.version;
}

function browserReceipt(receipt) {
	const rows = sortedRaceRows(receipt);
	const wtfgif = rows.find((row) => row.id === "wtfgif");
	const tableRows = rows.map((row) => {
		const name = row.id === "wtfgif" ? `**${row.label}**` : row.label;
		const median =
			row.id === "wtfgif"
				? `**${row.medianMs.toFixed(3)} ms**`
				: `${row.medianMs.toFixed(3)} ms`;
		const advantage =
			row.id === "wtfgif" ? "—" : `**${row.slowerThanWtfgif.toFixed(2)}×**`;
		return `| ${name} | ${browserVersion(row)} | ${median} | ${advantage} | ${row.bytes.toLocaleString("en-US")} | ${row.psnrDb.toFixed(2)} dB | ${Number(row.alphaAccuracyPercent.toFixed(2))}% |`;
	});
	return `This is public-API time-to-result. It is not a codec-kernel
microbenchmark or an equal-file-size comparison. The gif.js and
gif.js.optimized APIs create workers when \`render()\` begins, so worker creation
is inside their timed jobs. The README chart uses bar length for median encode
time. It marks 100× wtfgif latency with a dashed reference line. It also gives
each encoder's emitted GIF size, relative slowdown, PSNR, and alpha agreement.

| Implementation | Version | Median | wtfgif advantage | Bytes | PSNR | Alpha match |
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

const [corpus, race, readmeSource, benchmarksSource] = await Promise.all([
	readFile(corpusPath, "utf8").then(JSON.parse),
	readFile(racePath, "utf8").then(JSON.parse),
	readFile(path.join(root, "README.md"), "utf8"),
	readFile(path.join(root, "BENCHMARKS.md"), "utf8"),
]);

const readme = replaceBlock(
	replaceBlock(readmeSource, "readme-corpus", readmeCorpus(corpus)),
	"readme-browser",
	readmeBrowser(race),
);
const benchmarks = replaceBlock(
	replaceBlock(benchmarksSource, "corpus-receipt", corpusReceipt(corpus)),
	"browser-receipt",
	browserReceipt(race),
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
]);

console.log("Updated benchmark text and charts from committed receipts");
