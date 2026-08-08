import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const worker = join(root, "scripts", "profile-first-quality-worker.mjs");
const iterations = Number(process.env.PROFILE_ITERATIONS ?? 100);
const fixture = process.env.PROFILE_FIXTURE ?? "makeemoji-real";
const baselinePath = process.env.PROFILE_WASM_BASELINE;
const candidatePath = process.env.PROFILE_WASM_CANDIDATE;
if (!baselinePath || !candidatePath) {
	throw new Error(
		"PROFILE_WASM_BASELINE and PROFILE_WASM_CANDIDATE are required",
	);
}
if (!Number.isInteger(iterations) || iterations < 1) {
	throw new Error("PROFILE_ITERATIONS must be a positive integer");
}

const samples = {
	baseline: { totalMs: [], wasmMs: [] },
	candidate: { totalMs: [], wasmMs: [] },
};
const pairedRatios = { totalMs: [], wasmMs: [] };
let outputSha256;

function run(wasmPath) {
	const result = spawnSync(process.execPath, [worker], {
		cwd: root,
		encoding: "utf8",
		env: {
			...process.env,
			PROFILE_FIXTURE: fixture,
			PROFILE_WASM_PATH: wasmPath,
		},
	});
	if (result.status !== 0) {
		throw new Error([result.stdout, result.stderr].join("\n"));
	}
	const sample = JSON.parse(result.stdout);
	if (outputSha256 && outputSha256 !== sample.outputSha256) {
		throw new Error(
			`Profile output differs: ${outputSha256} != ${sample.outputSha256}`,
		);
	}
	outputSha256 = sample.outputSha256;
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
		const sample = run(wasmPath);
		pair[name] = sample;
		for (const phase of ["totalMs", "wasmMs"]) {
			samples[name][phase].push(sample[phase]);
		}
	}
	for (const phase of ["totalMs", "wasmMs"]) {
		pairedRatios[phase].push(pair.baseline[phase] / pair.candidate[phase]);
	}
}

console.log(`Paired first raw quality encode: ${fixture}, ${iterations} pairs`);
for (const phase of ["wasmMs", "totalMs"]) {
	const baseline = median(samples.baseline[phase]);
	const candidate = median(samples.candidate[phase]);
	const ratio = median(pairedRatios[phase]);
	console.log(
		`${phase}\t${baseline.toFixed(3)} ms baseline\t${candidate.toFixed(3)} ms candidate\t${ratio.toFixed(4)}x paired`,
	);
}
console.log(`outputSha256\t${outputSha256}`);
