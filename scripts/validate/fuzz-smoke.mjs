import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { root } from "../lib/paths.mjs";
import { withTemporaryDirectorySync } from "../lib/temporary.mjs";

const coreDirectory = join(root, "crates", "wtfgif-core");
const runs = Number(process.env.FUZZ_RUNS ?? 500);

if (!Number.isInteger(runs) || runs < 1) {
	throw new Error("FUZZ_RUNS must be a positive integer");
}

function copyCorpus(source, target) {
	mkdirSync(target, { recursive: true });
	for (const name of readdirSync(source)) {
		cpSync(join(source, name), join(target, name), { recursive: true });
	}
}

function run(target, corpus) {
	const result = spawnSync(
		"cargo",
		["+nightly", "fuzz", "run", target, corpus, "--", `-runs=${runs}`],
		{ cwd: coreDirectory, stdio: "inherit" },
	);
	if (result.error) throw result.error;
	if (result.status !== 0) process.exitCode = result.status ?? 1;
}

withTemporaryDirectorySync("wtfgif-fuzz-smoke-", (temporaryDirectory) => {
	const decodeCorpus = join(temporaryDirectory, "decode");
	const encodeCorpus = join(temporaryDirectory, "encode");
	copyCorpus(join(root, "test", "gifs"), decodeCorpus);
	copyCorpus(join(coreDirectory, "fuzz", "corpus", "encode"), encodeCorpus);
	run("decode", decodeCorpus);
	if (!process.exitCode) run("encode", encodeCorpus);
});
