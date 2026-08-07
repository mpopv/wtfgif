import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { cpus } from "node:os";
import { dirname, join } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import {
	encodeImageQOmggif,
	encodeWtfgif,
	initializeAdapters,
} from "./benchmark/adapters.mjs";
import {
	ALPHA_THRESHOLD,
	corpusManifestEntry,
	loadBenchmarkCorpus,
	sha256,
} from "./benchmark/corpus.mjs";
import { percentile, validateAndMeasure } from "./benchmark/metrics.mjs";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const require = createRequire(import.meta.url);
const iterations = Number(process.env.BENCH_ITERATIONS ?? 15);
const warmups = Number(process.env.BENCH_WARMUP_ITERATIONS ?? 3);
const includeStress = process.env.BENCH_CORPUS_INCLUDE_STRESS === "1";
const fixtureFilter = process.env.BENCH_CORPUS_FIXTURE;
const writeReceipt = process.env.BENCH_WRITE_RECEIPT !== "0";
if (
	!Number.isInteger(iterations) ||
	iterations < 1 ||
	!Number.isInteger(warmups) ||
	warmups < 0
) {
	throw new Error(
		"BENCH_ITERATIONS must be positive and BENCH_WARMUP_ITERATIONS non-negative",
	);
}

const wasmStatus = await initializeAdapters();
let corpus = loadBenchmarkCorpus({ includeStress });
if (fixtureFilter)
	corpus = corpus.filter((value) => value.id === fixtureFilter);
if (corpus.length === 0)
	throw new Error("No benchmark fixture matched BENCH_CORPUS_FIXTURE");

const implementations = [
	{ id: "wtfgif", encode: encodeWtfgif },
	{ id: "image-q-rgbquant+omggif", encode: encodeImageQOmggif },
];
let sink = 0;

function runEncode(implementation, value) {
	const started = performance.now();
	const bytes = implementation.encode(value, ALPHA_THRESHOLD);
	const milliseconds = performance.now() - started;
	sink ^= bytes[0] ^ bytes[Math.floor(bytes.length / 2)] ^ bytes.at(-1);
	return { bytes, milliseconds };
}

function benchmarkFixture(value, fixtureIndex) {
	const first = new Map();
	for (const implementation of implementations) {
		first.set(implementation.id, runEncode(implementation, value));
	}
	for (let warmup = 0; warmup < warmups; warmup += 1) {
		const order =
			(warmup + fixtureIndex) & 1
				? implementations.toReversed()
				: implementations;
		for (const implementation of order) runEncode(implementation, value);
	}

	const samples = new Map(implementations.map(({ id }) => [id, []]));
	for (let iteration = 0; iteration < iterations; iteration += 1) {
		const order =
			(iteration + fixtureIndex) & 1
				? implementations.toReversed()
				: implementations;
		for (const implementation of order) {
			samples
				.get(implementation.id)
				.push(runEncode(implementation, value).milliseconds);
		}
	}

	const rows = implementations.map((implementation) => {
		const firstRun = first.get(implementation.id);
		const rawSamples = samples.get(implementation.id);
		const quality = validateAndMeasure(firstRun.bytes, value, ALPHA_THRESHOLD);
		return {
			fixtureId: value.id,
			implementation: implementation.id,
			firstObservedMs: firstRun.milliseconds,
			samplesMs: rawSamples,
			medianMs: percentile(rawSamples, 0.5),
			p95Ms: percentile(rawSamples, 0.95),
			bytes: firstRun.bytes.length,
			outputSha256: sha256(firstRun.bytes),
			quality: {
				opaquePixels: quality.opaquePixels,
				psnrDb: Number.isFinite(quality.psnrDb) ? quality.psnrDb : null,
				losslessOpaqueRgb: !Number.isFinite(quality.psnrDb),
				ssimBlackComposite: quality.ssim,
				frameSsimBlackComposite: quality.frameSsim,
			},
		};
	});
	const wtf = rows.find((row) => row.implementation === "wtfgif");
	const baseline = rows.find(
		(row) => row.implementation === "image-q-rgbquant+omggif",
	);
	wtf.speedupVsImageQOmggif = baseline.medianMs / wtf.medianMs;
	wtf.sizeRatioVsImageQOmggif = wtf.bytes / baseline.bytes;
	return rows;
}

const results = corpus.flatMap(benchmarkFixture);
const commit = execFileSync("git", ["rev-parse", "HEAD"], {
	cwd: root,
	encoding: "utf8",
}).trim();
const dirty =
	execFileSync("git", ["status", "--porcelain"], {
		cwd: root,
		encoding: "utf8",
	}).trim() !== "";
const packageJson = JSON.parse(
	readFileSync(join(root, "package.json"), "utf8"),
);
const receipt = {
	schemaVersion: 1,
	createdAt: new Date().toISOString(),
	benchmark: {
		boundary:
			"Synchronous RGBA-to-complete-GIF calls after package loading and Wasm initialization; warmup calls are excluded from samples.",
		firstObservedBoundary:
			"The first call for each implementation in this initialized corpus process; only the first fixture is also the process's first encode.",
		alphaThreshold: ALPHA_THRESHOLD,
		iterations,
		warmups,
		order: "Alternated by fixture and sample",
		quality:
			"Opaque-source RGB PSNR plus per-frame SSIM after binary-alpha compositing against black.",
	},
	environment: {
		platform: process.platform,
		arch: process.arch,
		node: process.version,
		cpu: cpus()[0]?.model ?? "unknown",
		packageVersion: packageJson.version,
		commit,
		dirty,
		packageLockSha256: sha256(readFileSync(join(root, "package-lock.json"))),
		wasmStatus,
	},
	implementations: [
		{
			id: "wtfgif",
			configuration:
				"global quality quantization, literal LZW, delta rectangles only for the nearly-static fixture",
		},
		{
			id: "image-q-rgbquant+omggif",
			configuration:
				"global image-q rgbquant palette, nearest mapping, omggif LZW, equivalent delta rectangles for the nearly-static fixture",
		},
	],
	corpus: corpus.map(corpusManifestEntry),
	results,
};

console.log(
	"fixture\timplementation\tmedian ms\tp95 ms\tbytes\tPSNR dB\tSSIM\tspeedup\tsize ratio",
);
for (const row of results) {
	const psnr =
		row.quality.psnrDb === null ? "lossless" : row.quality.psnrDb.toFixed(2);
	console.log(
		[
			row.fixtureId,
			row.implementation,
			row.medianMs.toFixed(3),
			row.p95Ms.toFixed(3),
			row.bytes,
			psnr,
			row.quality.ssimBlackComposite.toFixed(5),
			row.speedupVsImageQOmggif
				? `${row.speedupVsImageQOmggif.toFixed(2)}x`
				: "-",
			row.sizeRatioVsImageQOmggif
				? `${row.sizeRatioVsImageQOmggif.toFixed(2)}x`
				: "-",
		].join("\t"),
	);
}

if (writeReceipt) {
	const path = join(root, "benchmarks", "corpus.json");
	writeFileSync(path, `${JSON.stringify(receipt, null, "\t")}\n`);
	execFileSync(
		process.execPath,
		[require.resolve("@biomejs/biome/bin/biome"), "format", "--write", path],
		{ cwd: root, stdio: "inherit" },
	);
	console.log(`wrote ${path}`);
}
console.log(`sink=${sink}`);
