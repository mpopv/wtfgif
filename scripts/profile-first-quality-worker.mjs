import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { performance } from "node:perf_hooks";
import { loadBenchmarkCorpus } from "./benchmark/corpus.mjs";

const fixtureId = process.env.PROFILE_FIXTURE ?? "makeemoji-real";
const fixture = loadBenchmarkCorpus().find((value) => value.id === fixtureId);
if (!fixture) throw new Error(`Unknown PROFILE_FIXTURE: ${fixtureId}`);
const { rgba, width, height, frameCount } = fixture;
const wasmBytes = readFileSync(
	process.env.PROFILE_WASM_PATH ??
		new URL(
			"../crates/wtfgif-core/pkg-quality-simd/wtfgif_core_bg.wasm",
			import.meta.url,
		),
);
const moduleStarted = performance.now();
const module = new WebAssembly.Module(wasmBytes);
const moduleCompleted = performance.now();
let wasm;
const imports = {
	"./wtfgif_core_bg.js": {
		__wbindgen_init_externref_table() {
			const table = wasm.__wbindgen_externrefs;
			const offset = table.grow(4);
			table.set(0, undefined);
			table.set(offset, undefined);
			table.set(offset + 1, null);
			table.set(offset + 2, true);
			table.set(offset + 3, false);
		},
	},
};
wasm = new WebAssembly.Instance(module, imports).exports;
const instanceCompleted = performance.now();
wasm.__wbindgen_start();
const startCompleted = performance.now();
wasm.indexed_lzw_input_scratch_reserve(4_000_000);

const started = performance.now();
const inputPointer = wasm.indexed_lzw_input_scratch_reserve(rgba.length);
const reserved = performance.now();
new Uint8Array(wasm.memory.buffer, inputPointer, rgba.length).set(rgba);
const copiedInput = performance.now();
const outputLength = (
	rgba.length / 4 > 1_000_000 || process.env.PROFILE_GENERAL === "1"
		? wasm.encode_rgba_quality_gif_constant_delay_scratch_from_input
		: wasm.encode_rgba_quality_low_res_constant_delay_scratch_from_input
)(rgba.length, width, height, frameCount, 10, 0, 179);
const encoded = performance.now();
const output = new Uint8Array(
	wasm.memory.buffer,
	wasm.gif_output_scratch_ptr(),
	outputLength,
).slice();
const copiedOutput = performance.now();

process.stdout.write(
	JSON.stringify({
		moduleMs: moduleCompleted - moduleStarted,
		instanceMs: instanceCompleted - moduleCompleted,
		startMs: startCompleted - instanceCompleted,
		reserveMs: reserved - started,
		inputCopyMs: copiedInput - reserved,
		wasmMs: encoded - copiedInput,
		outputCopyMs: copiedOutput - encoded,
		totalMs: copiedOutput - started,
		outputBytes: output.length,
		outputSha256: createHash("sha256").update(output).digest("hex"),
	}),
);
