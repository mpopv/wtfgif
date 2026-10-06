import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { root } from "../lib/paths.mjs";
import { withTemporaryDirectorySync } from "../lib/temporary.mjs";

const coreDirectory = join(root, "crates", "wtfgif-core");
// FUZZ_SECONDS bounds each target by time, which the weekly campaign uses;
// otherwise FUZZ_RUNS bounds it by executions.
const seconds =
	process.env.FUZZ_SECONDS === undefined
		? null
		: Number(process.env.FUZZ_SECONDS);
const runs = Number(process.env.FUZZ_RUNS ?? 500);

if (seconds !== null && (!Number.isInteger(seconds) || seconds < 1)) {
	throw new Error("FUZZ_SECONDS must be a positive integer");
}
if (!Number.isInteger(runs) || runs < 1) {
	throw new Error("FUZZ_RUNS must be a positive integer");
}

const targets = [
	["decode", join(root, "test", "gifs")],
	["encode", join(coreDirectory, "fuzz", "corpus", "encode")],
	["roundtrip", join(coreDirectory, "fuzz", "corpus", "roundtrip")],
];

function copyCorpus(source, target) {
	mkdirSync(target, { recursive: true });
	for (const name of readdirSync(source)) {
		cpSync(join(source, name), join(target, name), { recursive: true });
	}
}

function run(target, corpus) {
	const limit =
		seconds === null ? `-runs=${runs}` : `-max_total_time=${seconds}`;
	const result = spawnSync(
		"cargo",
		["+nightly", "fuzz", "run", target, corpus, "--", limit],
		{ cwd: coreDirectory, stdio: "inherit" },
	);
	if (result.error) throw result.error;
	if (result.status !== 0) process.exitCode = result.status ?? 1;
}

withTemporaryDirectorySync("wtfgif-fuzz-smoke-", (temporaryDirectory) => {
	for (const [target, seeds] of targets) {
		const corpus = join(temporaryDirectory, target);
		copyCorpus(seeds, corpus);
		run(target, corpus);
		if (process.exitCode) return;
	}
});
