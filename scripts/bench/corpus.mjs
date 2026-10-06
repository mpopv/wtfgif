import { join } from "node:path";
import { root } from "../lib/paths.mjs";
import { corpusManifestEntry, loadBenchmarkCorpus } from "./lib/corpus.mjs";
import {
	benchmarkFixture,
	boundary,
	formatQuality,
	implementationManifest,
	receiptEnvironment,
	writeReceipt,
} from "./lib/run-corpus.mjs";

const iterations = Number(process.env.BENCH_ITERATIONS ?? 40);
const includeStress = process.env.BENCH_CORPUS_INCLUDE_STRESS === "1";
const fixtureFilter = process.env.BENCH_CORPUS_FIXTURE;
const shouldWriteReceipt = process.env.BENCH_WRITE_RECEIPT !== "0";
if (!Number.isInteger(iterations) || iterations < 1) {
	throw new Error("BENCH_ITERATIONS must be a positive integer");
}

let corpus = loadBenchmarkCorpus({ includeStress });
if (fixtureFilter)
	corpus = corpus.filter((value) => value.id === fixtureFilter);
if (corpus.length === 0)
	throw new Error("No benchmark fixture matched BENCH_CORPUS_FIXTURE");

// image-q + omggif is the JavaScript baseline. sharp's fastest undithered
// setting is the native baseline: libvips, cgif, and libimagequant.
const JAVASCRIPT_BASELINE = "image-q-rgbquant+omggif";
const NATIVE_BASELINE = "sharp-effort1";
const implementations = [
	"wtfgif",
	"wtfgif-smallest",
	JAVASCRIPT_BASELINE,
	NATIVE_BASELINE,
];
const runtimes = {};

const results = corpus.flatMap((value, fixtureIndex) => {
	const rows = benchmarkFixture({
		value,
		fixtureIndex,
		implementations,
		iterations,
		includeStress,
		runtimes,
	});
	const byId = new Map(rows.map((row) => [row.implementation, row]));
	const javascript = byId.get(JAVASCRIPT_BASELINE);
	const native = byId.get(NATIVE_BASELINE);
	for (const wtf of [byId.get("wtfgif"), byId.get("wtfgif-smallest")]) {
		wtf.speedupVsImageQOmggif = javascript.medianMs / wtf.medianMs;
		wtf.sizeRatioVsImageQOmggif = wtf.bytes / javascript.bytes;
		wtf.speedupVsSharp = native.medianMs / wtf.medianMs;
		wtf.sizeRatioVsSharp = wtf.bytes / native.bytes;
	}
	const smallest = byId.get("wtfgif-smallest");
	smallest.sizeRatioVsFastest = smallest.bytes / byId.get("wtfgif").bytes;
	smallest.timeRatioVsFastest = smallest.medianMs / byId.get("wtfgif").medianMs;
	return rows;
});

const receipt = {
	schemaVersion: 3,
	createdAt: new Date().toISOString(),
	benchmark: {
		...boundary,
		iterations,
		processesPerImplementation: iterations,
		baselines: { javascript: JAVASCRIPT_BASELINE, native: NATIVE_BASELINE },
	},
	environment: receiptEnvironment(runtimes),
	implementations: implementationManifest(implementations),
	corpus: corpus.map(corpusManifestEntry),
	results,
};

console.log(
	"fixture\timplementation\tmedian ms\tp95 ms\tbytes\tPSNR dB\tSSIM\tvs image-q\tvs sharp",
);
for (const row of results) {
	console.log(
		[
			row.fixtureId,
			row.implementation,
			row.medianMs.toFixed(3),
			row.p95Ms.toFixed(3),
			row.bytes,
			formatQuality(row),
			row.quality.ssimBlackComposite.toFixed(5),
			row.speedupVsImageQOmggif
				? `${row.speedupVsImageQOmggif.toFixed(2)}x`
				: "-",
			row.speedupVsSharp ? `${row.speedupVsSharp.toFixed(2)}x` : "-",
		].join("\t"),
	);
}

if (shouldWriteReceipt) {
	writeReceipt(join(root, "benchmarks", "corpus.json"), receipt);
}
