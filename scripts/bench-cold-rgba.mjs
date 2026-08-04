import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const worker = join(root, "scripts", "bench-cold-rgba-worker.mjs");
const iterations = Number(process.env.BENCH_ITERATIONS ?? 10);
const fixture = process.env.BENCH_COLD_RGBA_FIXTURE ?? "real";

if (!Number.isInteger(iterations) || iterations < 1) {
	throw new Error("BENCH_ITERATIONS must be a positive integer");
}

function runWorker(implementation) {
	const result = spawnSync(process.execPath, [worker, implementation], {
		cwd: root,
		encoding: "utf8",
		env: {
			...process.env,
			BENCH_COLD_RGBA_FIXTURE: fixture,
			WTFGIF_COLD_RGBA_WORKER: "1",
		},
		maxBuffer: 4 * 1024 * 1024,
	});
	if (result.status !== 0) {
		throw new Error(
			[
				`Cold RGBA worker failed: ${implementation}`,
				result.stdout,
				result.stderr,
			].join("\n"),
		);
	}
	return JSON.parse(result.stdout);
}

function median(values) {
	const sorted = values.toSorted((left, right) => left - right);
	return sorted[Math.floor(sorted.length / 2)];
}

const samples = { baseline: [], wtfgif: [] };
let outputBytes = 0;
for (let iteration = 0; iteration < iterations; iteration += 1) {
	const order =
		iteration % 2 === 0 ? ["baseline", "wtfgif"] : ["wtfgif", "baseline"];
	for (const implementation of order) {
		const result = runWorker(implementation);
		samples[implementation].push(result.elapsedMs);
		if (implementation === "wtfgif") outputBytes = result.outputBytes;
	}
}

const baseline = median(samples.baseline);
const wtfgif = median(samples.wtfgif);
console.log(
	`True cold arbitrary-RGBA encode (${fixture}): ${iterations} fresh Node processes per implementation, zero warmups`,
);
console.log("contract\tbaseline ms\twtfgif ms\tspeedup\twtfgif bytes");
console.log(
	[
		fixture === "stress"
			? "10 synthetic frames / 512x512 / quality global palette"
			: "8 real MakeEmoji images / 128x128 / quality global palette",
		baseline.toFixed(3),
		wtfgif.toFixed(3),
		`${(baseline / wtfgif).toFixed(2)}x`,
		outputBytes,
	].join("\t"),
);
