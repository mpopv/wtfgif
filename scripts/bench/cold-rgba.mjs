import { join } from "node:path";
import { performance } from "node:perf_hooks";
import { root } from "../lib/paths.mjs";
import { median } from "./lib/metrics.mjs";
import { alternatingOrder, runJsonWorker } from "./lib/process.mjs";

const worker = join(root, "scripts", "bench", "cold-rgba-worker.mjs");
const iterations = Number(process.env.BENCH_ITERATIONS ?? 10);
const fixture = process.env.BENCH_COLD_RGBA_FIXTURE ?? "real";

if (!Number.isInteger(iterations) || iterations < 1) {
	throw new Error("BENCH_ITERATIONS must be a positive integer");
}

function runWorker(implementation) {
	const wallStarted = performance.now();
	const result = runJsonWorker({
		worker,
		args: [implementation],
		cwd: root,
		env: {
			BENCH_COLD_RGBA_FIXTURE: fixture,
			WTFGIF_COLD_RGBA_WORKER: "1",
		},
		maxBuffer: 4 * 1024 * 1024,
		label: `cold RGBA ${implementation}`,
	});
	return {
		...result,
		wallMs: performance.now() - wallStarted,
	};
}

const samples = { baseline: [], wtfgif: [] };
const wallSamples = { baseline: [], wtfgif: [] };
const phaseSamples = {
	fixtureMs: [],
	importMs: [],
	initializeMs: [],
	encodeMs: [],
	totalMs: [],
};
let outputBytes = 0;
for (let iteration = 0; iteration < iterations; iteration += 1) {
	const order = alternatingOrder(["baseline", "wtfgif"], iteration);
	for (const implementation of order) {
		const result = runWorker(implementation);
		samples[implementation].push(result.elapsedMs);
		wallSamples[implementation].push(result.wallMs);
		if (implementation === "wtfgif") {
			outputBytes = result.outputBytes;
			if (result.phases) {
				for (const phase of Object.keys(phaseSamples)) {
					phaseSamples[phase].push(result.phases[phase]);
				}
			}
		}
	}
}

const baseline = median(samples.baseline);
const wtfgif = median(samples.wtfgif);
const baselineWall = median(wallSamples.baseline);
const wtfgifWall = median(wallSamples.wtfgif);
console.log(
	`Fresh-process arbitrary-RGBA encode (${fixture}): ${iterations} processes per implementation, zero warmups`,
);
console.log("boundary\tbaseline ms\twtfgif ms\tspeedup\twtfgif bytes");
for (const [boundary, baselineMs, wtfgifMs] of [
	["in-worker operation", baseline, wtfgif],
	["complete process wall clock", baselineWall, wtfgifWall],
]) {
	console.log(
		[
			boundary,
			baselineMs.toFixed(3),
			wtfgifMs.toFixed(3),
			`${(baselineMs / wtfgifMs).toFixed(2)}x`,
			outputBytes,
		].join("\t"),
	);
}
if (phaseSamples.totalMs.length > 0) {
	console.log(
		"phase\tfixture ms\timport ms\tinitialize ms\tencode ms\ttotal ms",
	);
	console.log(
		[
			"wtfgif median",
			...Object.values(phaseSamples).map((values) => median(values).toFixed(3)),
		].join("\t"),
	);
}
