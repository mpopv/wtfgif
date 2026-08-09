import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const worker = join(root, "scripts", "profile-first-quality-worker.mjs");
const iterations = Number(process.env.PROFILE_ITERATIONS ?? 100);
const fixture = process.env.PROFILE_FIXTURE ?? "makeemoji-real";
const baselinePath = process.env.PROFILE_WASM_BASELINE;
const candidatePath = process.env.PROFILE_WASM_CANDIDATE;
const allowOutputDifference =
	process.env.PROFILE_ALLOW_OUTPUT_DIFFERENCE === "1";
if (!baselinePath || !candidatePath) {
	throw new Error(
		"PROFILE_WASM_BASELINE and PROFILE_WASM_CANDIDATE are required",
	);
}
if (!Number.isInteger(iterations) || iterations < 1) {
	throw new Error("PROFILE_ITERATIONS must be a positive integer");
}

const samples = {
	baseline: { startMs: [], tierMs: [], totalMs: [], wasmMs: [] },
	candidate: { startMs: [], tierMs: [], totalMs: [], wasmMs: [] },
};
const pairedRatios = { totalMs: [], wasmMs: [] };
const outputSha256 = {};

function run(name, wasmPath) {
	const nodeArguments =
		process.env.PROFILE_EAGER === "1"
			? ["--no-wasm-lazy-compilation", worker]
			: [worker];
	const result = spawnSync(process.execPath, nodeArguments, {
		cwd: root,
		encoding: "utf8",
		env: {
			...process.env,
			PROFILE_FIXTURE: fixture,
			PROFILE_WASM_PATH: wasmPath,
			PROFILE_JS_TIER:
				(name === "candidate" &&
					process.env.PROFILE_CANDIDATE_JS_TIER === "1") ||
				(name === "baseline" && process.env.PROFILE_BASELINE_JS_TIER === "1")
					? "1"
					: "0",
		},
	});
	if (result.status !== 0) {
		throw new Error([result.stdout, result.stderr].join("\n"));
	}
	const sample = JSON.parse(result.stdout);
	if (
		!allowOutputDifference &&
		outputSha256.baseline &&
		outputSha256.baseline !== sample.outputSha256
	) {
		throw new Error(
			`Profile output differs: ${outputSha256.baseline} != ${sample.outputSha256}`,
		);
	}
	outputSha256[name] = sample.outputSha256;
	return sample;
}

function median(values) {
	const sorted = values.toSorted((left, right) => left - right);
	return sorted[Math.floor(sorted.length / 2)];
}

for (let iteration = 0; iteration < iterations; iteration += 1) {
	const order =
		iteration & 1
			? [
					["candidate", candidatePath],
					["baseline", baselinePath],
				]
			: [
					["baseline", baselinePath],
					["candidate", candidatePath],
				];
	const pair = {};
	for (const [name, wasmPath] of order) {
		const sample = run(name, wasmPath);
		pair[name] = sample;
		for (const phase of ["startMs", "tierMs", "totalMs", "wasmMs"]) {
			samples[name][phase].push(sample[phase]);
		}
	}
	for (const phase of ["totalMs", "wasmMs"]) {
		pairedRatios[phase].push(pair.baseline[phase] / pair.candidate[phase]);
	}
}

console.log(`Paired first raw quality encode: ${fixture}, ${iterations} pairs`);
console.log(
	`startMs\t${median(samples.baseline.startMs).toFixed(3)} ms baseline\t${median(samples.candidate.startMs).toFixed(3)} ms candidate`,
);
console.log(
	`tierMs\t${median(samples.baseline.tierMs).toFixed(3)} ms baseline\t${median(samples.candidate.tierMs).toFixed(3)} ms candidate`,
);
for (const phase of ["wasmMs", "totalMs"]) {
	const baseline = median(samples.baseline[phase]);
	const candidate = median(samples.candidate[phase]);
	const ratio = median(pairedRatios[phase]);
	console.log(
		`${phase}\t${baseline.toFixed(3)} ms baseline\t${candidate.toFixed(3)} ms candidate\t${ratio.toFixed(4)}x paired`,
	);
}
for (const name of ["baseline", "candidate"]) {
	console.log(`outputSha256.${name}\t${outputSha256[name]}`);
}
