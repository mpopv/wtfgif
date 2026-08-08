import { execFileSync, spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { cpus } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
	ALPHA_THRESHOLD,
	corpusManifestEntry,
	loadBenchmarkCorpus,
	sha256,
} from "./benchmark/corpus.mjs";
import { percentile, validateAndMeasure } from "./benchmark/metrics.mjs";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const require = createRequire(import.meta.url);
const worker = join(root, "scripts", "bench-corpus-worker.mjs");
const iterations = Number(process.env.BENCH_ITERATIONS ?? 40);
const includeStress = process.env.BENCH_CORPUS_INCLUDE_STRESS === "1";
const fixtureFilter = process.env.BENCH_CORPUS_FIXTURE;
const writeReceipt = process.env.BENCH_WRITE_RECEIPT !== "0";
if (!Number.isInteger(iterations) || iterations < 1) {
	throw new Error("BENCH_ITERATIONS must be a positive integer");
}

let corpus = loadBenchmarkCorpus({ includeStress });
if (fixtureFilter)
	corpus = corpus.filter((value) => value.id === fixtureFilter);
if (corpus.length === 0)
	throw new Error("No benchmark fixture matched BENCH_CORPUS_FIXTURE");

const implementations = ["wtfgif", "image-q-rgbquant+omggif"];
let sink = 0;
let wasmStatus;

function runWorker(implementation, value, returnOutput) {
	const result = spawnSync(
		process.execPath,
		[worker, implementation, value.id],
		{
			cwd: root,
			encoding: "utf8",
			env: {
				...process.env,
				BENCH_CORPUS_INCLUDE_STRESS: includeStress ? "1" : "0",
				BENCH_RETURN_OUTPUT: returnOutput ? "1" : "0",
				WTFGIF_CORPUS_WORKER: "1",
			},
			maxBuffer: 16 * 1024 * 1024,
		},
	);
	if (result.status !== 0) {
		throw new Error(
			[
				`Corpus worker failed: ${value.id} / ${implementation}`,
				result.stdout,
				result.stderr,
			].join("\n"),
		);
	}
	const row = JSON.parse(result.stdout);
	if (row.wasmStatus) wasmStatus = row.wasmStatus;
	sink ^= row.bytes ^ Number.parseInt(row.outputSha256.slice(0, 8), 16);
	return row;
}

function benchmarkFixture(value, fixtureIndex) {
	const samples = new Map(implementations.map((id) => [id, []]));
	const first = new Map();
	for (let iteration = 0; iteration < iterations; iteration += 1) {
		const order =
			(iteration + fixtureIndex) & 1
				? implementations.toReversed()
				: implementations;
		for (const implementation of order) {
			const row = runWorker(implementation, value, iteration === 0);
			const initial = first.get(implementation);
			if (initial) {
				if (
					row.bytes !== initial.bytes ||
					row.outputSha256 !== initial.outputSha256
				) {
					throw new Error(
						`${value.id}: ${implementation} output changed between fresh processes`,
					);
				}
			} else {
				first.set(implementation, row);
			}
			samples.get(implementation).push(row.milliseconds);
		}
	}

	const rows = implementations.map((implementation) => {
		const firstRun = first.get(implementation);
		const output = Buffer.from(firstRun.outputBase64, "base64");
		if (output.length !== firstRun.bytes) {
			throw new Error(`${value.id}: worker output length is inconsistent`);
		}
		const quality = validateAndMeasure(output, value, ALPHA_THRESHOLD);
		const rawSamples = samples.get(implementation);
		return {
			fixtureId: value.id,
			implementation,
			firstObservedMs: rawSamples[0],
			samplesMs: rawSamples,
			medianMs: percentile(rawSamples, 0.5),
			p95Ms: percentile(rawSamples, 0.95),
			bytes: firstRun.bytes,
			outputSha256: firstRun.outputSha256,
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
	schemaVersion: 2,
	createdAt: new Date().toISOString(),
	benchmark: {
		boundary:
			"First user-input RGBA-to-complete-GIF call in a fresh process after package loading and source-independent wtfgif Wasm runtime preparation; zero fixture-derived warmups.",
		alphaThreshold: ALPHA_THRESHOLD,
		iterations,
		warmups: 0,
		processesPerImplementation: iterations,
		order: "Alternated by fixture and fresh process",
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
				"global quality quantization and literal LZW through the wtfgif/encode entry point",
		},
		{
			id: "image-q-rgbquant+omggif",
			configuration:
				"global image-q rgbquant palette, nearest mapping, and omggif LZW",
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
