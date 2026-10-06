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

// Speed against file size for every encoder that runs in Node. gif.js and
// gif.js.optimized need browser Web Workers, so they appear only in the
// browser race.
const iterations = Number(process.env.BENCH_ITERATIONS ?? 40);
const fixtureId = process.env.BENCH_PARETO_FIXTURE ?? "makeemoji-real";
const shouldWriteReceipt = process.env.BENCH_WRITE_RECEIPT !== "0";
if (!Number.isInteger(iterations) || iterations < 1) {
	throw new Error("BENCH_ITERATIONS must be a positive integer");
}

const value = loadBenchmarkCorpus().find((fixture) => fixture.id === fixtureId);
if (!value) throw new Error(`Unknown corpus fixture: ${fixtureId}`);

const implementations = [
	"wtfgif",
	"wtfgif-independent",
	"image-q-rgbquant+omggif",
	"gifenc",
	"modern-gif",
	"sharp-effort1",
	"sharp-effort4",
	"sharp-effort7",
	"sharp-effort10",
	"sharp-default",
];
const runtimes = {};
const results = benchmarkFixture({
	value,
	fixtureIndex: 0,
	implementations,
	iterations,
	includeStress: false,
	runtimes,
});

// A result is on the frontier when no other result is both at least as fast
// and at least as small, and strictly better in one of the two.
for (const row of results) {
	row.paretoOptimal = !results.some(
		(other) =>
			other !== row &&
			other.medianMs <= row.medianMs &&
			other.bytes <= row.bytes &&
			(other.medianMs < row.medianMs || other.bytes < row.bytes),
	);
}

const receipt = {
	schemaVersion: 1,
	createdAt: new Date().toISOString(),
	benchmark: {
		...boundary,
		iterations,
		processesPerImplementation: iterations,
		excluded:
			"gif.js and gif.js.optimized render in browser Web Workers and do not run in Node.",
	},
	environment: receiptEnvironment(runtimes),
	implementations: implementationManifest(implementations),
	fixture: corpusManifestEntry(value),
	results,
};

console.log("implementation\tmedian ms\tp95 ms\tbytes\tPSNR dB\tfrontier");
for (const row of results.toSorted((a, b) => a.medianMs - b.medianMs)) {
	console.log(
		[
			row.implementation,
			row.medianMs.toFixed(3),
			row.p95Ms.toFixed(3),
			row.bytes,
			formatQuality(row),
			row.paretoOptimal ? "yes" : "",
		].join("\t"),
	);
}

if (shouldWriteReceipt) {
	writeReceipt(join(root, "benchmarks", "pareto.json"), receipt);
}
