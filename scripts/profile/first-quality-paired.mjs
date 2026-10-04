import { join } from "node:path";
import { median } from "../bench/lib/metrics.mjs";
import { runJsonWorker } from "../bench/lib/process.mjs";
import { root } from "../lib/paths.mjs";

const worker = join(root, "scripts", "profile", "first-quality-worker.mjs");
const iterations = Number(process.env.PROFILE_ITERATIONS ?? 100);
const fixture = process.env.PROFILE_FIXTURE ?? "makeemoji-real";
const baselinePath = process.env.PROFILE_WASM_BASELINE;
const candidatePath = process.env.PROFILE_WASM_CANDIDATE;
// Candidates are accepted by what they decode to, not by their bytes:
//   pixels  (default) composited frames and delays must match the baseline
//   quality RGB PSNR and SSIM may fall by at most the configured tolerances
//   none    no output check, for deliberate diagnostic comparisons
const gate =
	process.env.PROFILE_ALLOW_OUTPUT_DIFFERENCE === "1"
		? "none"
		: (process.env.PROFILE_GATE ?? "pixels");
if (!["pixels", "quality", "none"].includes(gate)) {
	throw new Error('PROFILE_GATE must be "pixels", "quality", or "none"');
}
const maxPsnrLoss = Number(process.env.PROFILE_MAX_PSNR_LOSS ?? 0.05);
const maxSsimLoss = Number(process.env.PROFILE_MAX_SSIM_LOSS ?? 0.0005);
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
const decodedSha256 = {};
const outputBytes = {};
const measured = {};

function run(name, wasmPath, measureQuality = false) {
	const nodeArguments =
		process.env.PROFILE_EAGER === "1"
			? ["--no-wasm-lazy-compilation", worker]
			: [worker];
	const sample = runJsonWorker({
		worker,
		nodeArgs: nodeArguments.slice(0, -1),
		cwd: root,
		env: {
			PROFILE_FIXTURE: fixture,
			PROFILE_WASM_PATH: wasmPath,
			PROFILE_JS_TIER:
				(name === "candidate" &&
					process.env.PROFILE_CANDIDATE_JS_TIER === "1") ||
				(name === "baseline" && process.env.PROFILE_BASELINE_JS_TIER === "1")
					? "1"
					: "0",
			PROFILE_SPLIT_TIER:
				name === "candidate"
					? (process.env.PROFILE_CANDIDATE_SPLIT_TIER ?? "1")
					: (process.env.PROFILE_BASELINE_SPLIT_TIER ?? "1"),
			PROFILE_SPLIT_TIER_ITERATIONS:
				name === "candidate"
					? (process.env.PROFILE_CANDIDATE_SPLIT_TIER_ITERATIONS ?? "1")
					: (process.env.PROFILE_BASELINE_SPLIT_TIER_ITERATIONS ?? "1"),
			PROFILE_MIXED_TIER_ITERATIONS:
				name === "candidate"
					? (process.env.PROFILE_CANDIDATE_MIXED_TIER_ITERATIONS ?? "1")
					: (process.env.PROFILE_BASELINE_MIXED_TIER_ITERATIONS ?? "1"),
			PROFILE_INDEPENDENT_FRAMES:
				name === "candidate"
					? (process.env.PROFILE_CANDIDATE_INDEPENDENT_FRAMES ?? "0")
					: "0",
			PROFILE_MEASURE_QUALITY: measureQuality ? "1" : "0",
		},
		label: `${name}/${fixture}`,
	});
	if (outputSha256[name] && outputSha256[name] !== sample.outputSha256) {
		throw new Error(`${name} output changed between fresh processes`);
	}
	if (
		gate === "pixels" &&
		decodedSha256.baseline &&
		decodedSha256.baseline !== sample.decodedSha256
	) {
		throw new Error(
			`Decoded frames differ from the baseline: ${decodedSha256.baseline} != ${sample.decodedSha256}`,
		);
	}
	outputSha256[name] = sample.outputSha256;
	decodedSha256[name] = sample.decodedSha256;
	outputBytes[name] = sample.outputBytes;
	return sample;
}

if (gate === "quality") {
	for (const [name, wasmPath] of [
		["baseline", baselinePath],
		["candidate", candidatePath],
	]) {
		measured[name] = run(name, wasmPath, true);
	}
	const psnrLoss = measured.baseline.psnrDb - measured.candidate.psnrDb;
	const ssimLoss = measured.baseline.ssim - measured.candidate.ssim;
	if (!(psnrLoss <= maxPsnrLoss) || !(ssimLoss <= maxSsimLoss)) {
		throw new Error(
			`Quality gate failed: PSNR fell ${psnrLoss.toFixed(3)} dB and SSIM fell ${ssimLoss.toFixed(5)}`,
		);
	}
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
	console.log(
		`output.${name}\t${outputBytes[name]} bytes\t${outputSha256[name]}`,
	);
}
console.log(
	`gate\t${gate}\t${
		gate === "pixels"
			? "decoded frames identical"
			: gate === "quality"
				? `PSNR ${measured.baseline.psnrDb.toFixed(2)} -> ${measured.candidate.psnrDb.toFixed(2)} dB, SSIM ${measured.baseline.ssim.toFixed(4)} -> ${measured.candidate.ssim.toFixed(4)}`
				: "not checked"
	}`,
);
