import { createHash } from "node:crypto";
import { performance } from "node:perf_hooks";
import {
	encodeImageQOmggif,
	encodeWtfgif,
	initializeAdapter,
} from "./benchmark/adapters.mjs";
import { ALPHA_THRESHOLD, loadBenchmarkCorpus } from "./benchmark/corpus.mjs";

if (process.env.WTFGIF_CORPUS_WORKER !== "1") {
	throw new Error(
		"bench-corpus-worker.mjs must be launched by bench-corpus.mjs",
	);
}

const [implementation, fixtureId] = process.argv.slice(2);
const value = loadBenchmarkCorpus({
	includeStress: process.env.BENCH_CORPUS_INCLUDE_STRESS === "1",
}).find((fixture) => fixture.id === fixtureId);
if (!value) throw new Error(`Unknown corpus fixture: ${fixtureId}`);

const wasmStatus = await initializeAdapter(implementation);
const encode =
	implementation === "wtfgif"
		? encodeWtfgif
		: implementation === "image-q-rgbquant+omggif"
			? encodeImageQOmggif
			: null;
if (!encode) throw new Error(`Unknown implementation: ${implementation}`);

const started = performance.now();
const bytes = encode(value, ALPHA_THRESHOLD);
const milliseconds = performance.now() - started;
const outputSha256 = createHash("sha256").update(bytes).digest("hex");

process.stdout.write(
	JSON.stringify({
		milliseconds,
		bytes: bytes.length,
		outputSha256,
		...(implementation === "wtfgif" ? { wasmStatus } : {}),
		...(process.env.BENCH_RETURN_OUTPUT === "1"
			? { outputBase64: Buffer.from(bytes).toString("base64") }
			: {}),
	}),
);
