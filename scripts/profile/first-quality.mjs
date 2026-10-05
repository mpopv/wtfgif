import { join } from "node:path";
import { median } from "../bench/lib/metrics.mjs";
import { runJsonWorker } from "../bench/lib/process.mjs";
import { root } from "../lib/paths.mjs";

const worker = join(root, "scripts", "profile", "first-quality-worker.mjs");
const iterations = Number(process.env.PROFILE_ITERATIONS ?? 100);
const fixture = process.env.PROFILE_FIXTURE ?? "makeemoji-real";
const phases = [
	"moduleMs",
	"instanceMs",
	"startMs",
	"reserveMs",
	"inputCopyMs",
	"wasmMs",
	"outputCopyMs",
	"totalMs",
];
const samples = Object.fromEntries(phases.map((phase) => [phase, []]));
let outputBytes = 0;
let outputSha256 = "";

for (let iteration = 0; iteration < iterations; iteration += 1) {
	const nodeArguments =
		process.env.PROFILE_EAGER === "1"
			? ["--no-wasm-lazy-compilation", worker]
			: [worker];
	const sample = runJsonWorker({
		worker,
		nodeArgs: nodeArguments.slice(0, -1),
		cwd: root,
		label: `first quality ${fixture}`,
	});
	outputBytes = sample.outputBytes;
	if (outputSha256 && outputSha256 !== sample.outputSha256) {
		throw new Error("Profile output changed between fresh processes");
	}
	outputSha256 = sample.outputSha256;
	for (const phase of phases) samples[phase].push(sample[phase]);
}

console.log(
	`First raw quality encode: ${fixture}, ${iterations} fresh processes${
		process.env.PROFILE_EAGER === "1" ? ", eager Wasm compilation" : ""
	}`,
);
for (const phase of phases) {
	console.log(`${phase}\t${median(samples[phase]).toFixed(3)} ms`);
}
console.log(`outputBytes\t${outputBytes}`);
console.log(`outputSha256\t${outputSha256}`);
