import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const worker = join(root, "scripts", "profile-first-quality-worker.mjs");
const iterations = Number(process.env.PROFILE_ITERATIONS ?? 100);
const phases = [
	"reserveMs",
	"inputCopyMs",
	"wasmMs",
	"outputCopyMs",
	"totalMs",
];
const samples = Object.fromEntries(phases.map((phase) => [phase, []]));
let outputBytes = 0;

for (let iteration = 0; iteration < iterations; iteration += 1) {
	const nodeArguments =
		process.env.PROFILE_EAGER === "1"
			? ["--no-wasm-lazy-compilation", worker]
			: [worker];
	const result = spawnSync(process.execPath, nodeArguments, {
		cwd: root,
		encoding: "utf8",
	});
	if (result.status !== 0) {
		throw new Error([result.stdout, result.stderr].join("\n"));
	}
	const sample = JSON.parse(result.stdout);
	outputBytes = sample.outputBytes;
	for (const phase of phases) samples[phase].push(sample[phase]);
}

function median(values) {
	const sorted = values.toSorted((left, right) => left - right);
	return sorted[Math.floor(sorted.length / 2)];
}

console.log(`First raw quality encode: ${iterations} fresh processes`);
for (const phase of phases) {
	console.log(`${phase}\t${median(samples[phase]).toFixed(3)} ms`);
}
console.log(`outputBytes\t${outputBytes}`);
