import { readdirSync } from "node:fs";
import { basename, join } from "node:path";
import { root } from "../lib/paths.mjs";
import {
	formatFixed,
	geometricMean,
	median,
	percentile,
} from "./lib/metrics.mjs";
import { alternatingOrder, runJsonWorker } from "./lib/process.mjs";

const worker = join(root, "scripts", "bench", "cold-worker.mjs");
const gifsDir = join(root, "test", "gifs");
const iterations = Number(process.env.BENCH_ITERATIONS ?? 15);
const fixtureFilter = process.env.BENCH_FILTER ?? "";
const decodeOnly = process.env.BENCH_DECODE_ONLY === "1";
const reencodeOnly = process.env.BENCH_REENCODE_ONLY === "1";
const encodeOnly = process.env.BENCH_ENCODE_ONLY === "1";
const pageLoadPrepared = process.env.WTFGIF_PREPARE_WASM_AT_PAGE_LOAD === "1";
const losslessRemux = process.env.WTFGIF_REENCODE_MODE === "remux";

if (!Number.isInteger(iterations) || iterations < 1) {
	throw new Error("BENCH_ITERATIONS must be a positive integer");
}
if (
	process.env.BENCH_WARMUP_ITERATIONS !== undefined &&
	process.env.BENCH_WARMUP_ITERATIONS !== "0"
) {
	throw new Error(
		"Cold one-off benchmarks prohibit warmups; remove BENCH_WARMUP_ITERATIONS",
	);
}

function runWorker(operation, implementation, fixture) {
	return runJsonWorker({
		worker,
		args: [operation, implementation, fixture],
		cwd: root,
		env: { WTFGIF_BENCH_FRESH_PROCESS: "1" },
		label: `${operation}/${implementation}/${fixture}`,
	});
}

function measurePair(operation, fixture) {
	const samples = { omggif: [], wtfgif: [] };
	let expectedSignature;
	let outputBytes;
	for (let sample = 0; sample < iterations; sample++) {
		const order = alternatingOrder(["omggif", "wtfgif"], sample);
		for (const implementation of order) {
			const result = runWorker(operation, implementation, fixture);
			if (
				result.sourceSignature !== undefined &&
				result.signature !== result.sourceSignature
			) {
				throw new Error(
					`${operation}/${fixture}: ${implementation} output pixels differ from the source GIF`,
				);
			}
			if (expectedSignature === undefined) {
				expectedSignature = result.signature;
			} else if (result.signature !== expectedSignature) {
				throw new Error(
					`${operation}/${fixture}: decoded pixels differ for ${implementation}`,
				);
			}
			samples[implementation].push(result.elapsedMs);
			if (implementation === "wtfgif") {
				outputBytes = result.outputBytes;
			}
		}
	}
	return {
		omggif: median(samples.omggif),
		wtfgif: median(samples.wtfgif),
		wtfgifP95: percentile(samples.wtfgif, 0.95),
		outputBytes,
	};
}

function formatMs(value) {
	return formatFixed(value, 3, 9);
}

function formatRatio(value) {
	return formatFixed(value, 2, 9, "x");
}

function printRows(title, rows, includeBytes = false) {
	console.log(`\n${title}`);
	console.log(
		"fixture".padEnd(42),
		"omggif".padStart(9),
		"wtfgif".padStart(9),
		"faster".padStart(9),
		"wtf p95".padStart(9),
		...(includeBytes ? ["bytes".padStart(10)] : []),
	);
	for (const row of rows) {
		console.log(
			row.name.padEnd(42),
			formatMs(row.omggif),
			formatMs(row.wtfgif),
			formatRatio(row.omggif / row.wtfgif),
			formatMs(row.wtfgifP95),
			...(includeBytes ? [String(row.outputBytes).padStart(10)] : []),
		);
	}
	const geomean = geometricMean(rows.map((row) => row.omggif / row.wtfgif));
	console.log(
		"geomean".padEnd(42),
		"".padStart(9),
		"".padStart(9),
		formatRatio(geomean),
	);
}

console.log(
	[
		pageLoadPrepared
			? "Page-load-prepared one-off benchmark"
			: "True cold one-off benchmark",
		`${iterations} fresh Node processes per implementation and fixture`,
		pageLoadPrepared
			? "Wasm engine prepared with unrelated generated GIFs before the clock"
			: "zero warmups",
		"exactly one timed operation per process",
		"imports and input reads occur before the clock",
	].join(" | "),
);

const gifFixtures = readdirSync(gifsDir)
	.filter((file) => file.endsWith(".gif"))
	.filter((file) => file.includes(fixtureFilter))
	.toSorted()
	.map((file) => join(gifsDir, file));

if (gifFixtures.length === 0) {
	throw new Error(`No GIF fixtures matched BENCH_FILTER=${fixtureFilter}`);
}

if (!reencodeOnly && !encodeOnly) {
	const rows = gifFixtures.map((fixture) => ({
		name: basename(fixture),
		...measurePair("decode", fixture),
	}));
	printRows(
		"GIF -> all owned composited RGBA frames (pixel-perfect cold job)",
		rows,
	);
}

if (!decodeOnly && !encodeOnly) {
	const rows = gifFixtures.map((fixture) => ({
		name: basename(fixture),
		...measurePair("reencode", fixture),
	}));
	printRows(
		losslessRemux
			? "GIF -> losslessly remuxed GIF (original LZW payloads; decoded pixels are identical)"
			: "GIF -> freshly LZW-reencoded GIF (decoded pixels are identical)",
		rows,
		true,
	);
}

if (!decodeOnly && !reencodeOnly) {
	const indexedFixtures = [
		{ name: "4 colors", value: "4:full" },
		{ name: "16 colors", value: "16:full" },
		{ name: "256 colors", value: "256:full" },
		{ name: "4 colors, changed rectangles", value: "4:delta" },
	];
	printRows(
		"Indexed frames -> GIF (decoded pixels are identical)",
		indexedFixtures.map((fixture) => ({
			name: fixture.name,
			...measurePair("encode-indexed", fixture.value),
		})),
		true,
	);
	printRows(
		"RGBA frames -> GIF (decoded pixels are identical)",
		indexedFixtures.map((fixture) => ({
			name: fixture.name,
			...measurePair("encode-rgba", fixture.value),
		})),
		true,
	);
}
