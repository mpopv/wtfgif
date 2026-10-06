import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { cpus } from "node:os";
import { join } from "node:path";
import { root } from "../../lib/paths.mjs";
import { implementations as adapters } from "./adapters.mjs";
import { ALPHA_THRESHOLD, sha256 } from "./corpus.mjs";
import { percentile, validateAndMeasure } from "./metrics.mjs";
import {
	alternatingOrder,
	assertStableOutput,
	runJsonWorker,
} from "./process.mjs";

const require = createRequire(import.meta.url);
const worker = join(root, "scripts", "bench", "corpus-worker.mjs");

export const boundary = {
	boundary:
		"First user-input RGBA-to-complete-GIF call in a fresh process after package loading, each library's source-independent initialization, and 64 MiB unrelated-memory cache eviction; zero fixture-derived warmups.",
	inputLayout:
		"wtfgif receives one independently allocated RGBA typed array per frame; the other libraries receive contiguous RGBA. Inputs are created before cache eviction and timing.",
	cacheEvictionBytes: 64 * 1024 * 1024,
	cacheQuiescence:
		"One zero-delay event-loop turn after eviction; no encoder code or fixture bytes are touched.",
	alphaThreshold: ALPHA_THRESHOLD,
	warmups: 0,
	order: "Alternated by fixture and fresh process",
	quality:
		"Opaque-source RGB PSNR plus per-frame SSIM after binary-alpha compositing against black.",
};

/**
 * Time each implementation's first encode of one fixture in fresh processes,
 * then validate and measure the first output of each.
 */
export function benchmarkFixture({
	value,
	fixtureIndex,
	implementations,
	iterations,
	includeStress,
	runtimes,
}) {
	const samples = new Map(implementations.map((id) => [id, []]));
	const initializeSamples = new Map(implementations.map((id) => [id, []]));
	const first = new Map();
	for (let iteration = 0; iteration < iterations; iteration += 1) {
		const order = alternatingOrder(implementations, iteration, fixtureIndex);
		for (const implementation of order) {
			const row = runJsonWorker({
				worker,
				args: [implementation, value.id],
				cwd: root,
				env: {
					BENCH_CORPUS_INCLUDE_STRESS: includeStress ? "1" : "0",
					BENCH_RETURN_OUTPUT: iteration === 0 ? "1" : "0",
					WTFGIF_CORPUS_WORKER: "1",
				},
				label: `${value.id}/${implementation}`,
			});
			if (row.runtime) runtimes[implementation] = row.runtime;
			const initial = first.get(implementation);
			if (initial) {
				assertStableOutput(initial, row, `${value.id}/${implementation}`);
			} else {
				first.set(implementation, row);
			}
			samples.get(implementation).push(row.milliseconds);
			initializeSamples.get(implementation).push(row.initializeMs);
		}
	}

	return implementations.map((implementation) => {
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
			initializeMedianMs: percentile(
				initializeSamples.get(implementation),
				0.5,
			),
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
}

export function receiptEnvironment(runtimes) {
	const packageJson = JSON.parse(
		readFileSync(join(root, "package.json"), "utf8"),
	);
	const git = (...args) =>
		execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
	return {
		platform: process.platform,
		arch: process.arch,
		node: process.version,
		cpu: cpus()[0]?.model ?? "unknown",
		packageVersion: packageJson.version,
		commit: git("rev-parse", "HEAD"),
		// Receipts are outputs; one benchmark writing its receipt must not mark
		// the next one, run from the same commit, as dirty.
		dirty:
			git("status", "--porcelain", "--", ".", ":(exclude)benchmarks") !== "",
		packageLockSha256: sha256(readFileSync(join(root, "package-lock.json"))),
		runtimes,
	};
}

export function implementationManifest(implementations) {
	return implementations.map((id) => ({
		id,
		label: adapters[id].label,
		configuration: adapters[id].configuration,
	}));
}

export function writeReceipt(path, receipt) {
	writeFileSync(path, `${JSON.stringify(receipt, null, "\t")}\n`);
	execFileSync(
		process.execPath,
		[require.resolve("@biomejs/biome/bin/biome"), "format", "--write", path],
		{ cwd: root, stdio: "inherit" },
	);
	console.log(`wrote ${path}`);
}

export function formatQuality(row) {
	return row.quality.psnrDb === null
		? "lossless"
		: row.quality.psnrDb.toFixed(2);
}
